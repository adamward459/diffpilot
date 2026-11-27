use super::{ChangeRequest, Checks, Forge, ForgeError, ForgeKind, Job, JobId, Outcome, Review, Stage};
use crate::github::Github;
use crate::github::checks::{CheckRun, ChecksSnapshot, CommitStatus, Conclusion, RunStatus, StatusState};
use crate::github::pr_git::pull_head;
use crate::github::remote::RepoSlug;
use crate::github::time::parse_timestamp;
use crate::github::types::{ChecksState, PullRequest, ReviewDecision};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

const OTHER_STAGE: &str = "Other";
const STATUS_STAGE: &str = "Commit statuses";

pub struct GithubForge {
    github: Github,
    slug: RepoSlug,
}

impl GithubForge {
    pub fn new(github: Github, slug: RepoSlug) -> Self {
        Self { github, slug }
    }
}

impl Forge for GithubForge {
    fn kind(&self) -> ForgeKind {
        ForgeKind::GitHub
    }

    fn web_url(&self) -> String {
        self.slug.web_url()
    }

    fn change_requests(&self) -> Result<Vec<ChangeRequest>, ForgeError> {
        Ok(self
            .github
            .list_pull_requests(&self.slug)?
            .into_iter()
            .filter(PullRequest::is_open)
            .map(|pr| change_request(pr, &self.slug))
            .collect())
    }

    fn checks(&self, sha: &str) -> Result<Checks, ForgeError> {
        let snapshot = self.github.checks(&self.slug, sha)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
        Ok(checks(&snapshot, now))
    }

    fn rerun_failed(&self, checks: &Checks) -> Result<(), ForgeError> {
        for run in &checks.failed_runs {
            self.github.rerun_failed_jobs(&self.slug, *run)?;
        }
        Ok(())
    }

    fn rerun_job(&self, job: JobId) -> Result<(), ForgeError> {
        self.github.rerun_job(&self.slug, job.0)?;
        Ok(())
    }

    fn head_refspec(&self, number: u64) -> String {
        pull_head(number)
    }
}

fn change_request(pr: PullRequest, slug: &RepoSlug) -> ChangeRequest {
    let repo = slug.full_name();
    ChangeRequest {
        number: pr.number,
        title: pr.title,
        url: pr.url,
        author: pr.author,
        head_ref: pr.head_ref,
        base_ref: pr.base_ref,
        head_sha: pr.head_sha,
        from_this_repo: pr
            .head_repo
            .as_deref()
            .is_some_and(|head| head.eq_ignore_ascii_case(&repo)),
        review: match pr.review_decision {
            Some(ReviewDecision::Approved) => Review::Approved,
            Some(ReviewDecision::ChangesRequested) => Review::ChangesRequested,
            Some(ReviewDecision::ReviewRequired) => Review::Requested,
            Some(ReviewDecision::Unknown) | None => Review::None,
        },
        requested_from_you: pr.requested_from_you,
        checks: pr.checks.and_then(rollup_outcome),
    }
}

fn rollup_outcome(state: ChecksState) -> Option<Outcome> {
    match state {
        ChecksState::Success => Some(Outcome::Passed),
        ChecksState::Failure | ChecksState::Error => Some(Outcome::Failed),
        ChecksState::Pending | ChecksState::Expected => Some(Outcome::Running),
        ChecksState::Unknown => None,
    }
}

fn run_outcome(run: &CheckRun) -> Outcome {
    match (run.status, run.conclusion) {
        (RunStatus::InProgress, _) => Outcome::Running,
        (RunStatus::Completed, Some(conclusion)) => match conclusion {
            Conclusion::Success => Outcome::Passed,
            Conclusion::Failure | Conclusion::TimedOut | Conclusion::StartupFailure => Outcome::Failed,
            Conclusion::Cancelled => Outcome::Cancelled,
            Conclusion::ActionRequired => Outcome::ActionRequired,
            Conclusion::Neutral | Conclusion::Skipped | Conclusion::Stale | Conclusion::Unknown => Outcome::Skipped,
        },
        (RunStatus::Completed, None) => Outcome::Skipped,
        _ => Outcome::Queued,
    }
}

fn status_outcome(status: &CommitStatus) -> Outcome {
    match status.state {
        StatusState::Success => Outcome::Passed,
        StatusState::Failure | StatusState::Error | StatusState::Unknown => Outcome::Failed,
        StatusState::Pending => Outcome::Running,
    }
}

/// Seconds from start to end, or to `now` (epoch seconds) while the check still runs.
fn duration_seconds(run: &CheckRun, outcome: Outcome, now: i64) -> Option<u64> {
    let started = parse_timestamp(run.started_at.as_deref()?)?;
    let ended = run
        .completed_at
        .as_deref()
        .and_then(parse_timestamp)
        .or_else(|| (!outcome.is_finished()).then_some(now))?;
    u64::try_from(ended - started).ok()
}

fn job(run: &CheckRun, now: i64) -> Job {
    let outcome = run_outcome(run);
    Job {
        name: run.name.clone(),
        outcome,
        duration_seconds: duration_seconds(run, outcome, now),
        log_url: run.html_url.clone().or_else(|| run.details_url.clone()),
        rerun: run.job_id.map(JobId),
    }
}

/// The check data names the app that ran a job, not its workflow stage, so jobs group by app. Commit statuses
/// from the older status API form the last group.
fn checks(snapshot: &ChecksSnapshot, now: i64) -> Checks {
    let mut stages: Vec<Stage> = Vec::new();
    for run in &snapshot.runs {
        let name = run.app.as_deref().unwrap_or(OTHER_STAGE);
        let job = job(run, now);
        match stages.iter_mut().find(|stage| stage.name == name) {
            Some(stage) => stage.jobs.push(job),
            None => stages.push(Stage {
                name: name.to_string(),
                jobs: vec![job],
            }),
        }
    }
    if !snapshot.statuses.is_empty() {
        stages.push(Stage {
            name: STATUS_STAGE.into(),
            jobs: snapshot
                .statuses
                .iter()
                .map(|status| Job {
                    name: status.context.clone(),
                    outcome: status_outcome(status),
                    duration_seconds: None,
                    log_url: status.target_url.clone(),
                    rerun: None,
                })
                .collect(),
        });
    }
    let failed_runs: BTreeSet<u64> = snapshot
        .runs
        .iter()
        .filter(|run| run_outcome(run) == Outcome::Failed)
        .filter_map(|run| run.run_id)
        .collect();
    Checks {
        sha: snapshot.sha.clone(),
        stages,
        failed_runs: failed_runs.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::Overall;
    use crate::github::http::fake::FakeHttp;
    use std::sync::Arc;

    const LIST: &str = include_str!("../../tests/fixtures/github/list_prs.json");
    const RUNS: &str = include_str!("../../tests/fixtures/github/check_runs.json");
    const STATUS: &str = include_str!("../../tests/fixtures/github/commit_status.json");
    const SHA: &str = "3f2a91c0d4e5b6a7988776655443322110ffeedd";
    const NOW: i64 = 1_795_601_000;

    fn forge(http: &Arc<FakeHttp>) -> GithubForge {
        let slug = RepoSlug {
            host: "github.com".into(),
            owner: "o".into(),
            name: "r".into(),
        };
        GithubForge::new(Github::new(http.clone(), "github.com", "tok"), slug)
    }

    fn fixture_checks() -> Checks {
        let http = Arc::new(
            FakeHttp::new()
                .reply("/check-runs?per_page=100&page=1", 200, RUNS)
                .reply("/status", 200, STATUS),
        );
        let snapshot = forge(&http).github.checks(&forge(&http).slug, SHA).unwrap();
        checks(&snapshot, NOW)
    }

    fn job_of<'a>(checks: &'a Checks, name: &str) -> &'a Job {
        checks
            .stages
            .iter()
            .flat_map(|stage| &stage.jobs)
            .find(|job| job.name == name)
            .unwrap()
    }

    #[test]
    fn lists_the_open_pull_requests_as_change_requests() {
        let http = Arc::new(FakeHttp::new().reply("graphql", 200, LIST));
        let found = forge(&http).change_requests().unwrap();
        let summary: Vec<(u64, &str, bool, Review, Option<Outcome>)> = found
            .iter()
            .map(|cr| (cr.number, cr.author.as_str(), cr.from_this_repo, cr.review, cr.checks))
            .collect();
        assert_eq!(
            summary,
            [
                (12, "alice", true, Review::ChangesRequested, Some(Outcome::Passed)),
                (13, "ghost", false, Review::None, None),
            ]
        );
        assert_eq!(
            found.iter().map(|cr| cr.requested_from_you).collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(
            (
                found[0].head_ref.as_str(),
                found[0].base_ref.as_str(),
                found[0].head_sha.as_str()
            ),
            ("feat", "main", "abc123")
        );
    }

    #[test]
    fn review_and_rollup_states_map_to_forge_types() {
        let slug = RepoSlug {
            host: "github.com".into(),
            owner: "O".into(),
            name: "R".into(),
        };
        let http = Arc::new(FakeHttp::new().reply("graphql", 200, LIST));
        let mut pr = forge(&http).github.list_pull_requests(&slug).unwrap().remove(0);
        for (decision, review) in [
            (Some(ReviewDecision::Approved), Review::Approved),
            (Some(ReviewDecision::ReviewRequired), Review::Requested),
            (Some(ReviewDecision::Unknown), Review::None),
            (None, Review::None),
        ] {
            pr.review_decision = decision;
            assert_eq!(change_request(pr.clone(), &slug).review, review);
        }
        assert!(change_request(pr.clone(), &slug).from_this_repo);
        for (state, outcome) in [
            (ChecksState::Failure, Some(Outcome::Failed)),
            (ChecksState::Error, Some(Outcome::Failed)),
            (ChecksState::Pending, Some(Outcome::Running)),
            (ChecksState::Expected, Some(Outcome::Running)),
            (ChecksState::Unknown, None),
        ] {
            pr.checks = Some(state);
            assert_eq!(change_request(pr.clone(), &slug).checks, outcome);
        }
    }

    #[test]
    fn jobs_group_by_app_and_commit_statuses_come_last() {
        let checks = fixture_checks();
        let stages: Vec<(&str, usize)> = checks
            .stages
            .iter()
            .map(|stage| (stage.name.as_str(), stage.jobs.len()))
            .collect();
        assert_eq!(stages, [("GitHub Actions", 5), ("Vercel", 1), ("Commit statuses", 2)]);
        let actions: Vec<&str> = checks.stages[0].jobs.iter().map(|job| job.name.as_str()).collect();
        assert_eq!(actions, ["lint", "unit tests", "build", "e2e", "deploy"]);
    }

    #[test]
    fn a_run_without_an_app_lands_in_other() {
        let mut snapshot = ChecksSnapshot {
            sha: "s".into(),
            runs: Vec::new(),
            statuses: Vec::new(),
        };
        snapshot.runs.push(CheckRun {
            id: 1,
            name: "mystery".into(),
            status: RunStatus::Completed,
            conclusion: Some(Conclusion::Success),
            started_at: None,
            completed_at: None,
            html_url: None,
            details_url: Some("https://example.com/d".into()),
            app: None,
            title: None,
            summary: None,
            run_id: None,
            job_id: None,
        });
        let checks = checks(&snapshot, NOW);
        assert_eq!(checks.stages[0].name, "Other");
        assert_eq!(
            checks.stages[0].jobs[0].log_url.as_deref(),
            Some("https://example.com/d")
        );
    }

    #[test]
    fn only_actions_jobs_can_rerun_and_failed_runs_are_collected() {
        let checks = fixture_checks();
        assert_eq!(job_of(&checks, "unit tests").rerun, Some(JobId(5002)));
        assert_eq!(job_of(&checks, "Vercel").rerun, None);
        assert_eq!(job_of(&checks, "ci/legacy").rerun, None);
        assert_eq!(checks.failed_runs, [1289]);
        assert!(checks.can_rerun_failed());
    }

    #[test]
    fn outcomes_follow_status_and_conclusion() {
        let checks = fixture_checks();
        let outcomes: Vec<(&str, Outcome)> = checks
            .stages
            .iter()
            .flat_map(|stage| &stage.jobs)
            .map(|job| (job.name.as_str(), job.outcome))
            .collect();
        assert_eq!(
            outcomes,
            [
                ("lint", Outcome::Passed),
                ("unit tests", Outcome::Failed),
                ("build", Outcome::Running),
                ("e2e", Outcome::Skipped),
                ("deploy", Outcome::Queued),
                ("Vercel", Outcome::Passed),
                ("ci/legacy", Outcome::Passed),
                ("ci/lint-old", Outcome::Running),
            ]
        );
        assert_eq!(checks.overall(), Overall::Failing);
        assert_eq!(
            checks.counts(),
            crate::forge::Counts {
                passed: 3,
                failed: 1,
                running: 2,
                queued: 1,
                other: 1
            }
        );
        assert!(!checks.is_complete());
    }

    #[test]
    fn measures_how_long_each_job_took() {
        let checks = fixture_checks();
        assert_eq!(job_of(&checks, "lint").duration_seconds, Some(34));
        assert_eq!(job_of(&checks, "build").duration_seconds, Some(120));
        assert_eq!(job_of(&checks, "deploy").duration_seconds, None);
        assert_eq!(job_of(&checks, "ci/legacy").duration_seconds, None);
    }

    #[test]
    fn a_commit_without_checks_has_no_overall_state() {
        let http = Arc::new(
            FakeHttp::new()
                .reply("check-runs", 200, r#"{"total_count":0,"check_runs":[]}"#)
                .reply("/status", 200, r#"{"state":"pending","statuses":[]}"#),
        );
        let checks = forge(&http).checks("abc").unwrap();
        assert!(checks.stages.is_empty());
        assert_eq!(checks.overall(), Overall::None);
        assert!(checks.is_complete());
    }

    #[test]
    fn reruns_go_through_the_workflow_run_and_job_endpoints() {
        let http = Arc::new(
            FakeHttp::new()
                .reply("/actions/runs/1289/rerun-failed-jobs", 201, "")
                .reply("/actions/runs/1290/rerun-failed-jobs", 201, "")
                .reply("/actions/jobs/5002/rerun", 201, ""),
        );
        let forge = forge(&http);
        let mut checks = fixture_checks();
        checks.failed_runs = vec![1289, 1290];
        forge.rerun_failed(&checks).unwrap();
        forge.rerun_job(JobId(5002)).unwrap();
        http.assert_done();
    }

    #[test]
    fn the_head_of_a_pull_request_is_its_pull_ref() {
        let forge = forge(&Arc::new(FakeHttp::new()));
        assert_eq!(forge.head_refspec(7), "refs/pull/7/head");
        assert_eq!(
            (forge.kind(), forge.web_url().as_str()),
            (ForgeKind::GitHub, "https://github.com/o/r")
        );
    }
}
