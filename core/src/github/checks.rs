use super::client::Github;
use super::error::GithubError;
use super::log::{ParsedLog, parse_log};
use super::remote::RepoSlug;
use serde::Deserialize;

const GITHUB_ACTIONS_SLUG: &str = "github-actions";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    InProgress,
    Completed,
    Waiting,
    Requested,
    Pending,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    Skipped,
    TimedOut,
    ActionRequired,
    Stale,
    StartupFailure,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub id: u64,
    pub name: String,
    pub status: RunStatus,
    pub conclusion: Option<Conclusion>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub html_url: Option<String>,
    pub details_url: Option<String>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    /// Set for GitHub Actions jobs, which a log download and a re-run need.
    pub run_id: Option<u64>,
    pub job_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
    Success,
    Failure,
    Error,
    Pending,
    #[serde(other)]
    Unknown,
}

/// A commit status from the older status API, which some CI systems still use.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CommitStatus {
    pub context: String,
    pub state: StatusState,
    pub description: Option<String>,
    pub target_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksSnapshot {
    pub sha: String,
    pub runs: Vec<CheckRun>,
    pub statuses: Vec<CommitStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub name: Option<String>,
    pub run_number: u64,
    pub status: RunStatus,
    pub conclusion: Option<Conclusion>,
    pub html_url: String,
    pub head_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct JobStep {
    pub number: u32,
    pub name: String,
    pub status: RunStatus,
    pub conclusion: Option<Conclusion>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct JobDetail {
    pub id: u64,
    pub run_id: u64,
    pub name: String,
    pub workflow_name: Option<String>,
    pub status: RunStatus,
    pub conclusion: Option<Conclusion>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    #[serde(default)]
    pub steps: Vec<JobStep>,
}

#[derive(Deserialize)]
struct CheckRunsPage {
    total_count: u32,
    check_runs: Vec<RawCheckRun>,
}

#[derive(Deserialize)]
struct RawCheckRun {
    id: u64,
    name: String,
    status: RunStatus,
    conclusion: Option<Conclusion>,
    started_at: Option<String>,
    completed_at: Option<String>,
    html_url: Option<String>,
    details_url: Option<String>,
    app: Option<RawApp>,
    output: Option<RawOutput>,
}

#[derive(Deserialize)]
struct RawApp {
    slug: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawOutput {
    title: Option<String>,
    summary: Option<String>,
}

#[derive(Deserialize)]
struct CombinedStatus {
    statuses: Vec<CommitStatus>,
}

impl RawCheckRun {
    fn into_run(self) -> CheckRun {
        let is_actions = self
            .app
            .as_ref()
            .and_then(|app| app.slug.as_deref())
            .is_some_and(|slug| slug == GITHUB_ACTIONS_SLUG);
        let ids = is_actions
            .then(|| self.details_url.as_deref().and_then(actions_ids))
            .flatten();
        CheckRun {
            id: self.id,
            name: self.name,
            status: self.status,
            conclusion: self.conclusion,
            started_at: self.started_at,
            completed_at: self.completed_at,
            html_url: self.html_url,
            details_url: self.details_url,
            app: self.app.and_then(|app| app.name.or(app.slug)),
            title: self.output.as_ref().and_then(|output| output.title.clone()),
            summary: self.output.and_then(|output| output.summary),
            run_id: ids.map(|(run, _)| run),
            job_id: ids.map(|(_, job)| job),
        }
    }
}

/// `https://github.com/o/r/actions/runs/1289/job/5002` has run 1289 and job 5002.
fn actions_ids(url: &str) -> Option<(u64, u64)> {
    let rest = url.split("/actions/runs/").nth(1)?;
    let (run, rest) = rest.split_once("/job/")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((run.parse().ok()?, digits.parse().ok()?))
}

const MAX_PAGES: u32 = 10;

fn repo_path(slug: &RepoSlug) -> String {
    format!("/repos/{}/{}", slug.owner, slug.name)
}

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, GithubError> {
    serde_json::from_str(body).map_err(|error| GithubError::Response(error.to_string()))
}

impl Github {
    pub fn checks(&self, slug: &RepoSlug, sha: &str) -> Result<ChecksSnapshot, GithubError> {
        let mut runs = Vec::new();
        for page in 1..=MAX_PAGES {
            let path = format!("{}/commits/{sha}/check-runs?per_page=100&page={page}", repo_path(slug));
            let body: CheckRunsPage = parse(&self.rest_get(&path)?)?;
            let received = body.check_runs.len();
            runs.extend(body.check_runs.into_iter().map(RawCheckRun::into_run));
            if received == 0 || runs.len() >= body.total_count as usize {
                break;
            }
        }
        let path = format!("{}/commits/{sha}/status?per_page=100", repo_path(slug));
        let combined: CombinedStatus = parse(&self.rest_get(&path)?)?;
        Ok(ChecksSnapshot {
            sha: sha.to_string(),
            runs,
            statuses: combined.statuses,
        })
    }

    pub fn workflow_run(&self, slug: &RepoSlug, run_id: u64) -> Result<WorkflowRun, GithubError> {
        parse(&self.rest_get(&format!("{}/actions/runs/{run_id}", repo_path(slug)))?)
    }

    pub fn job(&self, slug: &RepoSlug, job_id: u64) -> Result<JobDetail, GithubError> {
        parse(&self.rest_get(&format!("{}/actions/jobs/{job_id}", repo_path(slug)))?)
    }

    pub fn job_log(&self, slug: &RepoSlug, job_id: u64) -> Result<String, GithubError> {
        self.rest_get(&format!("{}/actions/jobs/{job_id}/logs", repo_path(slug)))
    }

    pub fn parsed_job_log(&self, slug: &RepoSlug, job_id: u64) -> Result<ParsedLog, GithubError> {
        Ok(parse_log(&self.job_log(slug, job_id)?))
    }

    /// Returns false in dry-run mode, where nothing is sent.
    pub fn rerun_failed_jobs(&self, slug: &RepoSlug, run_id: u64) -> Result<bool, GithubError> {
        self.rest_post(
            "rerunFailedJobs",
            &format!("{}/actions/runs/{run_id}/rerun-failed-jobs", repo_path(slug)),
            None,
        )
    }

    pub fn rerun_job(&self, slug: &RepoSlug, job_id: u64) -> Result<bool, GithubError> {
        self.rest_post(
            "rerunJob",
            &format!("{}/actions/jobs/{job_id}/rerun", repo_path(slug)),
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::DryRunLog;
    use crate::github::http::fake::FakeHttp;
    use std::sync::Arc;

    const RUNS: &str = include_str!("../../tests/fixtures/github/check_runs.json");
    const STATUS: &str = include_str!("../../tests/fixtures/github/commit_status.json");
    const JOB: &str = include_str!("../../tests/fixtures/github/job.json");
    const RUN: &str = include_str!("../../tests/fixtures/github/workflow_run.json");
    const LOG: &str = include_str!("../../tests/fixtures/github/job_log.txt");
    const SHA: &str = "3f2a91c0d4e5b6a7988776655443322110ffeedd";

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

    fn snapshot_http() -> Arc<FakeHttp> {
        Arc::new(
            FakeHttp::new()
                .reply(
                    "/commits/3f2a91c0d4e5b6a7988776655443322110ffeedd/check-runs?per_page=100&page=1",
                    200,
                    RUNS,
                )
                .reply("/commits/3f2a91c0d4e5b6a7988776655443322110ffeedd/status", 200, STATUS),
        )
    }

    #[test]
    fn reads_the_check_runs_and_statuses_of_a_commit() {
        let http = snapshot_http();
        let snapshot = client(&http).checks(&slug(), SHA).unwrap();
        http.assert_done();
        let names: Vec<(&str, RunStatus, Option<Conclusion>)> = snapshot
            .runs
            .iter()
            .map(|run| (run.name.as_str(), run.status, run.conclusion))
            .collect();
        assert_eq!(
            names,
            [
                ("lint", RunStatus::Completed, Some(Conclusion::Success)),
                ("unit tests", RunStatus::Completed, Some(Conclusion::Failure)),
                ("build", RunStatus::InProgress, None),
                ("e2e", RunStatus::Completed, Some(Conclusion::Skipped)),
                ("Vercel", RunStatus::Completed, Some(Conclusion::Success)),
                ("deploy", RunStatus::Queued, None),
            ]
        );
        assert_eq!(snapshot.statuses.len(), 2);
    }

    #[test]
    fn links_actions_jobs_to_their_run() {
        let snapshot = client(&snapshot_http()).checks(&slug(), SHA).unwrap();
        let unit = &snapshot.runs[1];
        assert_eq!((unit.run_id, unit.job_id), (Some(1289), Some(5002)));
        assert_eq!(unit.title.as_deref(), Some("1 test failed"));
        let vercel = &snapshot.runs[4];
        assert_eq!((vercel.run_id, vercel.job_id), (None, None));
        assert_eq!(vercel.app.as_deref(), Some("Vercel"));
    }

    #[test]
    fn follows_pages_until_all_runs_are_read() {
        let page = |ids: &[u64], total: u32| {
            let runs: Vec<String> = ids
                .iter()
                .map(|id| format!(r#"{{"id":{id},"name":"job {id}","status":"completed","conclusion":"success"}}"#))
                .collect();
            format!(r#"{{"total_count":{total},"check_runs":[{}]}}"#, runs.join(","))
        };
        let http = Arc::new(
            FakeHttp::new()
                .reply("page=1", 200, &page(&[1, 2], 3))
                .reply("page=2", 200, &page(&[3], 3))
                .reply("/status", 200, r#"{"state":"success","statuses":[]}"#),
        );
        let snapshot = client(&http).checks(&slug(), "abc").unwrap();
        assert_eq!(snapshot.runs.len(), 3);
        http.assert_done();
    }

    #[test]
    fn reads_a_workflow_run_and_a_job_with_its_steps() {
        let http = Arc::new(FakeHttp::new().reply("/actions/runs/1289", 200, RUN).reply(
            "/actions/jobs/5002",
            200,
            JOB,
        ));
        let github = client(&http);
        let run = github.workflow_run(&slug(), 1289).unwrap();
        assert_eq!((run.name.as_deref(), run.run_number), (Some("CI"), 1289));
        let job = github.job(&slug(), 5002).unwrap();
        assert_eq!((job.run_id, job.name.as_str()), (1289, "unit tests"));
        assert_eq!(job.steps.len(), 4);
        assert_eq!(job.steps[3].conclusion, Some(Conclusion::Failure));
    }

    #[test]
    fn downloads_and_parses_the_job_log() {
        let http = Arc::new(FakeHttp::new().reply("/actions/jobs/5002/logs", 200, LOG));
        let log = client(&http).parsed_job_log(&slug(), 5002).unwrap();
        assert_eq!(log.first_error(), Some("Process completed with exit code 1."));
        assert_eq!(
            http.requests()[0].url,
            "https://api.github.com/repos/o/r/actions/jobs/5002/logs"
        );
    }

    #[test]
    fn expired_logs_are_not_found() {
        let http = Arc::new(FakeHttp::new().reply("/logs", 404, r#"{"message":"Not Found"}"#));
        assert!(matches!(
            client(&http).job_log(&slug(), 5002),
            Err(GithubError::NotFound(_))
        ));
    }

    #[test]
    fn reruns_failed_jobs_and_single_jobs() {
        let http = Arc::new(
            FakeHttp::new()
                .reply("/actions/runs/1289/rerun-failed-jobs", 201, "")
                .reply("/actions/jobs/5002/rerun", 201, ""),
        );
        let github = client(&http);
        assert!(github.rerun_failed_jobs(&slug(), 1289).unwrap());
        assert!(github.rerun_job(&slug(), 5002).unwrap());
        let sent = http.requests();
        assert_eq!(sent[0].method, crate::github::http::Method::Post);
        assert_eq!(
            sent[0].url,
            "https://api.github.com/repos/o/r/actions/runs/1289/rerun-failed-jobs"
        );
    }

    #[test]
    fn a_rerun_without_permission_is_forbidden() {
        let http =
            Arc::new(FakeHttp::new().reply("rerun", 403, r#"{"message":"Resource not accessible by integration"}"#));
        assert_eq!(
            client(&http).rerun_failed_jobs(&slug(), 1).unwrap_err(),
            GithubError::Forbidden("Resource not accessible by integration".into())
        );
    }

    #[test]
    fn dry_run_logs_a_rerun_instead_of_sending_it() {
        let http = Arc::new(FakeHttp::new());
        let log = Arc::new(DryRunLog::default());
        let github = client(&http).with_dry_run(log.clone());
        assert!(!github.rerun_failed_jobs(&slug(), 1289).unwrap());
        assert!(http.requests().is_empty());
        assert_eq!(log.mutations()[0].name, "rerunFailedJobs");
    }

    #[test]
    fn reads_run_and_job_ids_from_details_urls() {
        assert_eq!(actions_ids("https://github.com/o/r/actions/runs/7/job/8"), Some((7, 8)));
        assert_eq!(
            actions_ids("https://github.com/o/r/actions/runs/7/job/8?pr=1"),
            Some((7, 8))
        );
        assert_eq!(actions_ids("https://vercel.com/o/r/abc"), None);
        assert_eq!(actions_ids("https://github.com/o/r/actions/runs/7"), None);
    }
}
