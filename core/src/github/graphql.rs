// Adapted from diffity (MIT License, Copyright (c) 2026 Kamran Ahmed).
// The license text is in core/THIRD_PARTY_NOTICES.

use super::error::GithubError;
use super::types::{ChecksState, PrState, PullRequest, ReviewDecision};
use serde::Deserialize;
use serde::de::DeserializeOwned;

#[derive(Deserialize)]
struct Envelope<T> {
    data: Option<T>,
    errors: Option<Vec<GqlError>>,
}

#[derive(Deserialize)]
struct GqlError {
    message: String,
}

pub fn parse_response<T: DeserializeOwned>(text: &str) -> Result<T, GithubError> {
    let envelope: Envelope<T> = serde_json::from_str(text).map_err(|error| GithubError::Response(error.to_string()))?;
    if let Some(errors) = envelope.errors.filter(|errors| !errors.is_empty()) {
        let joined = errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(GithubError::Api(joined));
    }
    envelope
        .data
        .ok_or_else(|| GithubError::Response("empty GitHub response".into()))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Connection<T> {
    #[serde(default = "Vec::new")]
    pub nodes: Vec<Option<T>>,
    pub page_info: Option<PageInfo>,
}

impl<T> Connection<T> {
    pub fn into_items(self) -> Vec<T> {
        self.nodes.into_iter().flatten().collect()
    }

    pub fn next_cursor(&self) -> Option<String> {
        let info = self.page_info.as_ref()?;
        if !info.has_next_page {
            return None;
        }
        info.end_cursor.clone()
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Actor {
    pub login: String,
    #[serde(rename = "__typename")]
    pub typename: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RepoRef {
    pub name_with_owner: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TotalCount {
    pub total_count: u32,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrNode {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: PrState,
    pub is_draft: bool,
    pub author: Option<Actor>,
    pub base_ref_name: String,
    pub head_ref_name: String,
    pub head_ref_oid: String,
    pub review_decision: Option<ReviewDecision>,
    #[serde(default)]
    pub review_requests: Option<Connection<ReviewRequest>>,
    pub body: Option<String>,
    pub head_repository: Option<RepoRef>,
    pub commits: Option<Connection<CommitNode>>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub review_threads: Option<TotalCount>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub additions: Option<u32>,
    #[serde(default)]
    pub deletions: Option<u32>,
    #[serde(default)]
    pub changed_files: Option<u32>,
    #[serde(default)]
    pub is_cross_repository: Option<bool>,
}

#[derive(Deserialize, Debug)]
pub struct ReviewRequest {
    #[serde(rename = "requestedReviewer")]
    pub requested_reviewer: Option<RequestedReviewer>,
}

/// Only users have a login. A team request deserializes with `login: None`, because the API cannot say
/// cheaply whether the viewer is in the team.
#[derive(Deserialize, Debug)]
pub struct RequestedReviewer {
    pub login: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct Viewer {
    pub login: String,
}

#[derive(Deserialize, Debug)]
pub struct CommitNode {
    pub commit: Option<CommitInner>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitInner {
    pub status_check_rollup: Option<Rollup>,
}

#[derive(Deserialize, Debug)]
pub struct Rollup {
    pub state: ChecksState,
}

impl PrNode {
    fn checks(&self) -> Option<ChecksState> {
        self.commits
            .as_ref()?
            .nodes
            .iter()
            .flatten()
            .next_back()?
            .commit
            .as_ref()?
            .status_check_rollup
            .as_ref()
            .map(|rollup| rollup.state)
    }

    fn requested_from(&self, viewer: &str) -> bool {
        self.review_requests
            .iter()
            .flat_map(|requests| &requests.nodes)
            .flatten()
            .any(|request| {
                request
                    .requested_reviewer
                    .as_ref()
                    .and_then(|reviewer| reviewer.login.as_deref())
                    .is_some_and(|login| login.eq_ignore_ascii_case(viewer))
            })
    }

    pub fn into_pull_request(self, viewer: &str) -> PullRequest {
        let checks = self.checks();
        let requested_from_you = self.requested_from(viewer);
        PullRequest {
            node_id: self.id,
            number: self.number,
            title: self.title,
            url: self.url,
            state: self.state,
            is_draft: self.is_draft,
            author: self.author.map_or_else(|| "ghost".into(), |actor| actor.login),
            base_ref: self.base_ref_name,
            head_ref: self.head_ref_name,
            head_sha: self.head_ref_oid,
            review_decision: self.review_decision,
            requested_from_you,
            checks,
            body: self.body.unwrap_or_default(),
            created_at: self.created_at.unwrap_or_default(),
            updated_at: self.updated_at.unwrap_or_default(),
            additions: self.additions.unwrap_or(0),
            deletions: self.deletions.unwrap_or(0),
            changed_files: self.changed_files.unwrap_or(0),
            review_thread_count: self.review_threads.map_or(0, |threads| threads.total_count),
            head_repo: self.head_repository.map(|repo| repo.name_with_owner),
            is_cross_repository: self.is_cross_repository.unwrap_or(false),
        }
    }
}

fn pr_fields() -> &'static str {
    "id number title url state isDraft author{login} baseRefName headRefName headRefOid reviewDecision reviewRequests(first:20){nodes{requestedReviewer{... on User{login}}}} body \
     headRepository{nameWithOwner} commits(last:1){nodes{commit{statusCheckRollup{state}}}} \
     createdAt updatedAt additions deletions changedFiles isCrossRepository reviewThreads(first:1){totalCount}"
}

pub fn find_pr_query() -> String {
    format!(
        "query($owner:String!,$name:String!,$branch:String!){{viewer{{login}}repository(owner:$owner,name:$name){{\
         pullRequests(states:[OPEN,MERGED,CLOSED],headRefName:$branch,first:20,orderBy:{{field:UPDATED_AT,direction:DESC}}){{nodes{{{}}}}}}}}}",
        pr_fields()
    )
}

pub fn list_prs_query() -> String {
    format!(
        "query($owner:String!,$name:String!){{viewer{{login}}repository(owner:$owner,name:$name){{\
         pullRequests(states:OPEN,first:50,orderBy:{{field:UPDATED_AT,direction:DESC}}){{nodes{{{}}}}}}}}}",
        pr_fields()
    )
}

pub fn get_pr_query() -> String {
    format!(
        "query($owner:String!,$name:String!,$number:Int!){{viewer{{login}}repository(owner:$owner,name:$name){{\
         pullRequest(number:$number){{{}}}}}}}",
        pr_fields()
    )
}

#[derive(Deserialize, Debug)]
pub struct RepositoryData<T> {
    pub repository: Option<T>,
    #[serde(default)]
    pub viewer: Option<Viewer>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestsField {
    pub pull_requests: Connection<PrNode>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestField<T> {
    pub pull_request: Option<T>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRS: &str = include_str!("../../tests/fixtures/github/list_prs.json");

    #[test]
    fn parses_pull_requests() {
        let data: RepositoryData<PullRequestsField> = parse_response(PRS).unwrap();
        assert_eq!(data.viewer.as_ref().map(|viewer| viewer.login.as_str()), Some("bob"));
        let prs: Vec<_> = data
            .repository
            .unwrap()
            .pull_requests
            .into_items()
            .into_iter()
            .map(|node| node.into_pull_request("Bob"))
            .collect();
        assert_eq!(prs.len(), 3);
        let first = &prs[0];
        assert_eq!((first.number, first.author.as_str()), (12, "alice"));
        assert_eq!(first.checks, Some(ChecksState::Success));
        assert_eq!(first.review_decision, Some(ReviewDecision::ChangesRequested));
        assert_eq!(first.state, PrState::Open);
        assert_eq!(
            (
                first.base_ref.as_str(),
                first.head_ref.as_str(),
                first.head_sha.as_str()
            ),
            ("main", "feat", "abc123")
        );
        assert_eq!(first.body, "");
        assert_eq!(first.head_repo.as_deref(), Some("o/r"));
        assert_eq!((first.additions, first.deletions, first.changed_files), (5, 2, 3));
        assert!(first.is_cross_repository);
        assert_eq!(first.review_thread_count, 2);

        let second = &prs[1];
        assert_eq!(second.author, "ghost");
        assert_eq!((second.checks, second.review_decision), (None, None));
        assert!(second.is_draft && !second.is_cross_repository);
        assert_eq!(second.head_repo, None);
        assert_eq!(second.updated_at, "");

        assert_eq!(
            prs.iter().map(|pr| pr.requested_from_you).collect::<Vec<_>>(),
            [true, false, false],
            "only a user request naming the viewer counts, not another user or a team"
        );

        assert_eq!(prs[2].checks, Some(ChecksState::Pending));
        assert_eq!(prs[2].state, PrState::Merged);
    }

    #[test]
    fn unknown_states_do_not_break_parsing() {
        let text = r#"{"data":{"repository":{"pullRequest":{"id":"P","number":1,"title":"t","url":"u","state":"SOMETHING_NEW",
            "isDraft":false,"author":null,"baseRefName":"main","headRefName":"x","headRefOid":"s","reviewDecision":"FUTURE",
            "body":null,"headRepository":null,"commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"NEW_STATE"}}}]}}}}}"#;
        let data: RepositoryData<PullRequestField<PrNode>> = parse_response(text).unwrap();
        let pr = data.repository.unwrap().pull_request.unwrap().into_pull_request("me");
        assert_eq!(pr.state, PrState::Unknown);
        assert_eq!(pr.review_decision, Some(ReviewDecision::Unknown));
        assert_eq!(pr.checks, Some(ChecksState::Unknown));
        assert!(!pr.requested_from_you);
    }

    #[test]
    fn surfaces_graphql_errors() {
        let text = r#"{"data":null,"errors":[{"message":"Could not resolve"},{"message":"other"}]}"#;
        assert_eq!(
            parse_response::<RepositoryData<PullRequestsField>>(text).unwrap_err(),
            GithubError::Api("Could not resolve; other".into())
        );
    }

    #[test]
    fn rejects_bodies_that_are_not_graphql() {
        assert!(matches!(
            parse_response::<RepositoryData<PullRequestsField>>("<html>"),
            Err(GithubError::Response(_))
        ));
        assert!(matches!(
            parse_response::<RepositoryData<PullRequestsField>>(r#"{"data":null}"#),
            Err(GithubError::Response(_))
        ));
    }

    #[test]
    fn queries_have_balanced_braces() {
        for query in [find_pr_query(), list_prs_query(), get_pr_query()] {
            assert_eq!(query.matches('{').count(), query.matches('}').count(), "{query}");
        }
    }

    #[test]
    fn pages_follow_the_cursor() {
        let page: Connection<TotalCount> = serde_json::from_str(
            r#"{"nodes":[{"totalCount":1},null],"pageInfo":{"hasNextPage":true,"endCursor":"C"}}"#,
        )
        .unwrap();
        assert_eq!(page.next_cursor().as_deref(), Some("C"));
        assert_eq!(page.into_items().len(), 1);
    }
}
