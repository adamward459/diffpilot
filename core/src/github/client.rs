// Adapted from diffity (MIT License, Copyright (c) 2026 Kamran Ahmed).
// The license text is in core/THIRD_PARTY_NOTICES.

use super::auth::{Credentials, TokenSource};
use super::error::GithubError;
use super::graphql::{
    PrNode, PullRequestField, PullRequestsField, RepositoryData, find_pr_query, get_pr_query, list_prs_query,
    parse_response,
};
use super::http::{Http, Method, Request, Response};
use super::remote::{Endpoint, RepoSlug};
use super::types::PullRequest;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub const DRY_RUN_ENV: &str = "DIFFPILOT_GITHUB_DRY_RUN";

#[derive(Debug, Clone, PartialEq)]
pub struct DryRunMutation {
    pub name: String,
    pub payload: Value,
}

#[derive(Default)]
pub struct DryRunLog(Mutex<Vec<DryRunMutation>>);

impl DryRunLog {
    pub fn mutations(&self) -> Vec<DryRunMutation> {
        self.0.lock().unwrap().clone()
    }

    fn record(&self, name: &str, payload: Value) {
        self.0.lock().unwrap().push(DryRunMutation {
            name: name.to_string(),
            payload,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthStatus {
    pub login: Option<String>,
    pub source: Option<TokenSource>,
}

/// With the dry-run log set, mutations are recorded and never sent.
#[derive(Clone)]
pub struct Github {
    http: Arc<dyn Http>,
    endpoint: Endpoint,
    token: String,
    dry_run: Option<Arc<DryRunLog>>,
}

impl Github {
    /// Dry-run mode turns on when `DIFFPILOT_GITHUB_DRY_RUN=1`.
    pub fn new(http: Arc<dyn Http>, host: &str, token: &str) -> Self {
        let dry_run = (std::env::var(DRY_RUN_ENV).as_deref() == Ok("1")).then(|| Arc::new(DryRunLog::default()));
        Self {
            http,
            endpoint: Endpoint::for_host(host),
            token: token.to_string(),
            dry_run,
        }
    }

    pub fn connect(http: Arc<dyn Http>, credentials: &Credentials, host: &str) -> Result<Self, GithubError> {
        let (token, _) = credentials
            .token(host)?
            .ok_or_else(|| GithubError::NoToken(host.to_string()))?;
        Ok(Self::new(http, host, &token))
    }

    pub fn with_dry_run(mut self, log: Arc<DryRunLog>) -> Self {
        self.dry_run = Some(log);
        self
    }

    pub fn dry_run_log(&self) -> Option<&Arc<DryRunLog>> {
        self.dry_run.as_ref()
    }

    pub(crate) fn graphql<T: DeserializeOwned>(&self, query: &str, variables: Value) -> Result<T, GithubError> {
        let request = self.request(
            Method::Post,
            self.endpoint.graphql.clone(),
            Some(json!({ "query": query, "variables": variables }).to_string()),
        );
        let response = self.send(&request)?;
        match response.status {
            401 => Err(GithubError::Unauthorized),
            403 if serde_json::from_str::<Value>(&response.body).is_err() => Err(forbidden(&response)),
            status if !(200..300).contains(&status) && serde_json::from_str::<Value>(&response.body).is_err() => {
                Err(GithubError::Api(format!("GitHub returned {status}")))
            }
            _ => parse_response(&response.body),
        }
    }

    /// `None` in dry-run mode, where the mutation is recorded instead.
    pub fn graphql_mutation<T: DeserializeOwned>(
        &self,
        name: &str,
        query: &str,
        variables: Value,
    ) -> Result<Option<T>, GithubError> {
        if let Some(log) = &self.dry_run {
            log.record(name, variables);
            return Ok(None);
        }
        self.graphql(query, variables).map(Some)
    }

    pub(crate) fn rest_get(&self, path: &str) -> Result<String, GithubError> {
        let request = self.request(Method::Get, format!("{}{path}", self.endpoint.rest), None);
        let response = self.send(&request)?;
        rest_result(response, path)
    }

    /// Returns whether the request was sent; dry-run mode records it instead.
    pub fn rest_post(&self, name: &str, path: &str, body: Option<Value>) -> Result<bool, GithubError> {
        if let Some(log) = &self.dry_run {
            log.record(name, json!({ "path": path, "body": body }));
            return Ok(false);
        }
        let request = self.request(
            Method::Post,
            format!("{}{path}", self.endpoint.rest),
            Some(body.map_or_else(|| "{}".to_string(), |body| body.to_string())),
        );
        rest_result(self.send(&request)?, path).map(|_| true)
    }

    fn request(&self, method: Method, url: String, body: Option<String>) -> Request {
        let mut headers = vec![
            ("Authorization".to_string(), format!("Bearer {}", self.token)),
            ("Accept".to_string(), "application/vnd.github+json".to_string()),
            ("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string()),
        ];
        if body.is_some() {
            headers.push(("Content-Type".to_string(), "application/json".to_string()));
        }
        Request {
            method,
            url,
            headers,
            body,
        }
    }

    fn send(&self, request: &Request) -> Result<Response, GithubError> {
        self.http
            .send(request)
            .map_err(|error| GithubError::Network(error.to_string()))
    }

    pub fn viewer_login(&self) -> Result<String, GithubError> {
        let body = self.rest_get("/user")?;
        let value: Value = serde_json::from_str(&body).map_err(|error| GithubError::Response(error.to_string()))?;
        value
            .get("login")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| GithubError::Response("missing login in /user".into()))
    }

    pub fn list_pull_requests(&self, slug: &RepoSlug) -> Result<Vec<PullRequest>, GithubError> {
        let data: RepositoryData<PullRequestsField> =
            self.graphql(&list_prs_query(), json!({ "owner": slug.owner, "name": slug.name }))?;
        Ok(data
            .repository
            .ok_or_else(|| GithubError::NotFound(slug.full_name()))?
            .pull_requests
            .into_items()
            .into_iter()
            .map(PrNode::into_pull_request)
            .collect())
    }

    pub fn pull_request(&self, slug: &RepoSlug, number: u64) -> Result<PullRequest, GithubError> {
        let data: RepositoryData<PullRequestField<PrNode>> = self.graphql(
            &get_pr_query(),
            json!({ "owner": slug.owner, "name": slug.name, "number": number }),
        )?;
        data.repository
            .and_then(|repository| repository.pull_request)
            .map(PrNode::into_pull_request)
            .ok_or_else(|| GithubError::NotFound(format!("PR #{number} in {}", slug.full_name())))
    }

    /// Pull requests whose head is `branch` in one of `head_repos`, open ones first, newest first.
    pub fn pull_requests_for_branch(
        &self,
        slug: &RepoSlug,
        branch: &str,
        head_repos: &[RepoSlug],
    ) -> Result<Vec<PullRequest>, GithubError> {
        let data: RepositoryData<PullRequestsField> = self.graphql(
            &find_pr_query(),
            json!({ "owner": slug.owner, "name": slug.name, "branch": branch }),
        )?;
        let mut found: Vec<PullRequest> = data
            .repository
            .ok_or_else(|| GithubError::NotFound(slug.full_name()))?
            .pull_requests
            .into_items()
            .into_iter()
            .map(PrNode::into_pull_request)
            .filter(|pr| {
                pr.head_ref == branch
                    && pr.head_repo.as_deref().is_some_and(|name| {
                        head_repos
                            .iter()
                            .any(|repo| repo.full_name().eq_ignore_ascii_case(name))
                    })
            })
            .collect();
        found.sort_by_key(|pr| !pr.is_open());
        Ok(found)
    }

    pub fn auth_status(http: Arc<dyn Http>, credentials: &Credentials, host: &str) -> AuthStatus {
        let Ok(Some((token, source))) = credentials.token(host) else {
            return AuthStatus {
                login: None,
                source: None,
            };
        };
        let login = Self::new(http, host, &token).viewer_login().ok();
        AuthStatus {
            source: login.is_some().then_some(source),
            login,
        }
    }
}

fn forbidden(response: &Response) -> GithubError {
    let message = serde_json::from_str::<Value>(&response.body)
        .ok()
        .and_then(|value| value.get("message").and_then(Value::as_str).map(String::from))
        .unwrap_or_else(|| "forbidden".to_string());
    GithubError::Forbidden(message)
}

fn rest_result(response: Response, path: &str) -> Result<String, GithubError> {
    match response.status {
        200..=299 => Ok(response.body),
        401 => Err(GithubError::Unauthorized),
        403 => Err(forbidden(&response)),
        404 => Err(GithubError::NotFound(path.to_string())),
        status => Err(GithubError::Api(format!("GitHub returned {status} for {path}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::auth::{GhCli, MemoryStore};
    use crate::github::http::fake::FakeHttp;
    use crate::github::types::{ChecksState, PrState};

    const LIST: &str = include_str!("../../tests/fixtures/github/list_prs.json");
    const ONE: &str = include_str!("../../tests/fixtures/github/get_pr.json");
    const BRANCH: &str = include_str!("../../tests/fixtures/github/find_branch_prs.json");

    fn slug() -> RepoSlug {
        RepoSlug {
            host: "github.com".into(),
            owner: "o".into(),
            name: "r".into(),
        }
    }

    fn client(http: &Arc<FakeHttp>) -> Github {
        Github::new(http.clone(), "github.com", "tok")
    }

    #[test]
    fn lists_open_pull_requests_with_checks() {
        let http = Arc::new(FakeHttp::new().reply("graphql", 200, LIST));
        let prs = client(&http).list_pull_requests(&slug()).unwrap();
        assert_eq!(prs.len(), 3);
        assert_eq!(prs[0].checks, Some(ChecksState::Success));
        let sent = &http.requests()[0];
        assert_eq!(sent.url, "https://api.github.com/graphql");
        assert!(sent.headers.contains(&("Authorization".into(), "Bearer tok".into())));
        let body: Value = serde_json::from_str(sent.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["variables"], json!({"owner": "o", "name": "r"}));
        assert!(body["query"].as_str().unwrap().contains("pullRequests(states:OPEN"));
        http.assert_done();
    }

    #[test]
    fn reads_one_pull_request() {
        let http = Arc::new(FakeHttp::new().reply("graphql", 200, ONE));
        let pr = client(&http).pull_request(&slug(), 482).unwrap();
        assert_eq!(
            (pr.number, pr.base_ref.as_str(), pr.head_ref.as_str()),
            (482, "main", "feature/undo")
        );
        assert_eq!(pr.checks, Some(ChecksState::Failure));
        assert_eq!(pr.head_sha, "3f2a91c0d4e5b6a7988776655443322110ffeedd");
    }

    #[test]
    fn a_missing_pull_request_or_repository_is_not_found() {
        let missing = r#"{"data":{"repository":{"pullRequest":null}}}"#;
        let http = Arc::new(FakeHttp::new().reply("graphql", 200, missing));
        assert!(matches!(
            client(&http).pull_request(&slug(), 9),
            Err(GithubError::NotFound(_))
        ));
        let no_repo = r#"{"data":{"repository":null}}"#;
        let http = Arc::new(FakeHttp::new().reply("graphql", 200, no_repo));
        assert!(matches!(
            client(&http).list_pull_requests(&slug()),
            Err(GithubError::NotFound(_))
        ));
    }

    #[test]
    fn finds_the_pull_requests_of_a_branch_open_first() {
        let http = Arc::new(FakeHttp::new().reply("headRefName:$branch", 200, BRANCH));
        let found = client(&http)
            .pull_requests_for_branch(&slug(), "feature/undo", &[slug()])
            .unwrap();
        let numbers: Vec<(u64, PrState)> = found.iter().map(|pr| (pr.number, pr.state)).collect();
        assert_eq!(numbers, [(482, PrState::Open), (301, PrState::Merged)]);
        let sent: Value = serde_json::from_str(http.requests()[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(sent["variables"]["branch"], "feature/undo");
    }

    #[test]
    fn a_pull_request_from_another_repository_is_not_the_branchs() {
        let http = Arc::new(FakeHttp::new().reply("graphql", 200, BRANCH));
        let other = RepoSlug {
            host: "github.com".into(),
            owner: "someone".into(),
            name: "fork".into(),
        };
        assert!(
            client(&http)
                .pull_requests_for_branch(&slug(), "feature/undo", &[other])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn maps_http_failures_to_typed_errors() {
        let cases = [
            (401, r#"{"message":"Bad credentials"}"#, GithubError::Unauthorized),
            (403, "not json", GithubError::Forbidden("forbidden".into())),
            (502, "<html>", GithubError::Api("GitHub returned 502".into())),
        ];
        for (status, body, expected) in cases {
            let http = Arc::new(FakeHttp::new().reply("graphql", status, body));
            assert_eq!(
                client(&http).list_pull_requests(&slug()).unwrap_err(),
                expected,
                "{status}"
            );
        }
        let http = Arc::new(FakeHttp::new().reply("/user", 403, r#"{"message":"rate limit exceeded"}"#));
        assert_eq!(
            client(&http).viewer_login().unwrap_err(),
            GithubError::Forbidden("rate limit exceeded".into())
        );
        let http = Arc::new(FakeHttp::new().reply("/user", 404, "{}"));
        assert!(matches!(client(&http).viewer_login(), Err(GithubError::NotFound(_))));
    }

    #[test]
    fn enterprise_hosts_use_their_own_api() {
        let http = Arc::new(FakeHttp::new().reply("/api/v3/user", 200, r#"{"login":"me"}"#));
        let github = Github::new(http.clone(), "git.corp.example", "tok");
        assert_eq!(github.viewer_login().unwrap(), "me");
        assert_eq!(http.requests()[0].url, "https://git.corp.example/api/v3/user");
    }

    struct FakeGh;

    impl GhCli for FakeGh {
        fn token(&self, _host: &str) -> Option<String> {
            Some("gho_cli".into())
        }
    }

    #[test]
    fn reports_who_is_signed_in_and_where_the_token_came_from() {
        let credentials = Credentials::new(Arc::new(MemoryStore::default()), Arc::new(FakeGh));
        let http = Arc::new(FakeHttp::new().reply("/user", 200, r#"{"login":"adam"}"#));
        let status = Github::auth_status(http.clone(), &credentials, "github.com");
        assert_eq!(
            status,
            AuthStatus {
                login: Some("adam".into()),
                source: Some(TokenSource::Gh)
            }
        );
        assert!(
            http.requests()[0]
                .headers
                .contains(&("Authorization".into(), "Bearer gho_cli".into()))
        );

        let rejected = Arc::new(FakeHttp::new().reply("/user", 401, "{}"));
        let status = Github::auth_status(rejected, &credentials, "github.com");
        assert_eq!((status.login, status.source), (None, None));
    }

    #[test]
    fn connecting_without_a_token_fails() {
        struct NoGh;
        impl GhCli for NoGh {
            fn token(&self, _host: &str) -> Option<String> {
                None
            }
        }
        let credentials = Credentials::new(Arc::new(MemoryStore::default()), Arc::new(NoGh));
        let http: Arc<dyn Http> = Arc::new(FakeHttp::new());
        assert!(matches!(
            Github::connect(http, &credentials, "github.com"),
            Err(GithubError::NoToken(host)) if host == "github.com"
        ));
    }

    #[test]
    fn dry_run_records_a_mutation_and_sends_nothing() {
        let http = Arc::new(FakeHttp::new());
        let log = Arc::new(DryRunLog::default());
        let github = client(&http).with_dry_run(log.clone());
        let sent: Option<Value> = github
            .graphql_mutation(
                "addPullRequestReview",
                "mutation { x }",
                json!({"input": {"event": "COMMENT"}}),
            )
            .unwrap();
        assert_eq!(sent, None);
        assert!(
            !github
                .rest_post("rerun", "/repos/o/r/actions/runs/1/rerun", None)
                .unwrap()
        );
        assert!(http.requests().is_empty());
        let names: Vec<String> = log.mutations().into_iter().map(|mutation| mutation.name).collect();
        assert_eq!(names, ["addPullRequestReview", "rerun"]);
    }
}
