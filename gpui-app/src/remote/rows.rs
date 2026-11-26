use diffpilot_core::github::checks::{CheckRun, ChecksSnapshot, Counts, Outcome, Overall};
use diffpilot_core::github::types::{ChecksState, PullRequest, ReviewDecision};
use diffpilot_core::sync::{BranchStatus, PullMode};
use diffpilot_ui_kit::Theme;
use gpui::Hsla;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Running,
    Failed,
    Passed,
    Muted,
}

impl Tone {
    pub fn color(self, theme: &Theme) -> Hsla {
        match self {
            Tone::Running => theme.amber,
            Tone::Failed => theme.danger,
            Tone::Passed => theme.ui.green,
            Tone::Muted => theme.ui.faint,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tone::Running => "running",
            Tone::Failed => "failed",
            Tone::Passed => "passed",
            Tone::Muted => "muted",
        }
    }
}

fn commits(count: u32) -> String {
    format!("{count} commit{}", if count == 1 { "" } else { "s" })
}

pub fn sync_header(status: &BranchStatus) -> String {
    if status.upstream.is_none() {
        return "No upstream branch".into();
    }
    match (status.ahead, status.behind) {
        (0, 0) => "Up to date".into(),
        (ahead, 0) => format!("{} to push", commits(ahead)),
        (0, behind) => format!("{} to pull", commits(behind)),
        (ahead, behind) => format!("{} to push · {} to pull", commits(ahead), commits(behind)),
    }
}

pub fn fetch_detail(last_fetch: Option<SystemTime>, now: SystemTime) -> String {
    let Some(then) = last_fetch else {
        return "Not fetched yet".into();
    };
    let seconds = now.duration_since(then).map_or(0, |elapsed| elapsed.as_secs());
    let (value, unit) = match seconds {
        0..=59 => return "Last fetched just now".into(),
        60..=3599 => (seconds / 60, "min"),
        3600..=86_399 => (seconds / 3600, "hr"),
        _ => (seconds / 86_400, "day"),
    };
    let plural = if unit == "day" && value != 1 { "s" } else { "" };
    format!("Last fetched {value} {unit}{plural} ago")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRow {
    pub title: String,
    pub detail: String,
    pub enabled: bool,
}

pub fn pull_row(status: &BranchStatus, mode: PullMode) -> SyncRow {
    let Some(upstream) = &status.upstream else {
        return SyncRow {
            title: "Pull".into(),
            detail: "No upstream branch".into(),
            enabled: false,
        };
    };
    if status.behind == 0 {
        return SyncRow {
            title: "Pull".into(),
            detail: "Nothing to pull".into(),
            enabled: false,
        };
    }
    let detail = match mode {
        PullMode::FastForward => format!("Fast-forward to {upstream}"),
        PullMode::Rebase | PullMode::RebaseMerges => format!("Rebase onto {upstream}"),
    };
    SyncRow {
        title: format!("Pull {}", commits(status.behind)),
        detail,
        enabled: true,
    }
}

pub fn push_row(status: &BranchStatus) -> SyncRow {
    let Some(upstream) = &status.upstream else {
        return SyncRow {
            title: "Publish branch".into(),
            detail: "Push and set the upstream".into(),
            enabled: true,
        };
    };
    if status.behind > 0 {
        return SyncRow {
            title: format!("Push {}", commits(status.ahead)),
            detail: "After you pull".into(),
            enabled: false,
        };
    }
    if status.ahead == 0 {
        return SyncRow {
            title: "Push".into(),
            detail: "Nothing to push".into(),
            enabled: false,
        };
    }
    SyncRow {
        title: format!("Push {}", commits(status.ahead)),
        detail: format!("To {upstream}"),
        enabled: true,
    }
}

pub fn pull_first_warning(status: &BranchStatus) -> Option<String> {
    (status.upstream.is_some() && status.behind > 0).then(|| {
        format!(
            "Pull first. The remote has {} you do not have. Push works after you pull.",
            commits(status.behind)
        )
    })
}

pub fn review_text(decision: Option<ReviewDecision>) -> Option<&'static str> {
    match decision? {
        ReviewDecision::Approved => Some("approved"),
        ReviewDecision::ChangesRequested => Some("changes requested"),
        ReviewDecision::ReviewRequired => Some("review requested"),
        ReviewDecision::Unknown => None,
    }
}

pub fn review_badge(decision: Option<ReviewDecision>) -> Option<(&'static str, Tone)> {
    match decision? {
        ReviewDecision::Approved => Some(("Approved", Tone::Passed)),
        ReviewDecision::ChangesRequested => Some(("Changes requested", Tone::Failed)),
        ReviewDecision::ReviewRequired | ReviewDecision::Unknown => None,
    }
}

pub fn pr_title(pr: &PullRequest) -> String {
    format!("#{} {}", pr.number, pr.title)
}

pub fn pr_detail(pr: &PullRequest) -> String {
    let mut parts = vec![pr.author.as_str()];
    parts.extend(review_text(pr.review_decision));
    parts.push(pr.head_ref.as_str());
    parts.join(" · ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrRow {
    pub number: u64,
    pub title: String,
    pub detail: String,
    pub checked: bool,
}

pub fn pr_rows(prs: &[PullRequest], current: Option<u64>) -> Vec<PrRow> {
    prs.iter()
        .map(|pr| PrRow {
            number: pr.number,
            title: pr_title(pr),
            detail: pr_detail(pr),
            checked: current == Some(pr.number),
        })
        .collect()
}

pub fn rollup_tone(state: Option<ChecksState>) -> Option<Tone> {
    match state? {
        ChecksState::Success => Some(Tone::Passed),
        ChecksState::Failure | ChecksState::Error => Some(Tone::Failed),
        ChecksState::Pending | ChecksState::Expected => Some(Tone::Running),
        ChecksState::Unknown => None,
    }
}

pub fn overall_tone(overall: Overall) -> Option<Tone> {
    match overall {
        Overall::None => None,
        Overall::Passing => Some(Tone::Passed),
        Overall::Pending => Some(Tone::Running),
        Overall::Failing => Some(Tone::Failed),
    }
}

pub fn outcome_tone(outcome: Outcome) -> Tone {
    match outcome {
        Outcome::Passed => Tone::Passed,
        Outcome::Failed => Tone::Failed,
        Outcome::Running | Outcome::ActionRequired => Tone::Running,
        Outcome::Queued | Outcome::Skipped | Outcome::Cancelled => Tone::Muted,
    }
}

pub fn checks_detail(counts: Counts, overall: Overall) -> Option<String> {
    match overall {
        Overall::None => None,
        Overall::Failing => Some(format!("{} failed", counts.failed)),
        Overall::Pending => Some(format!(
            "{}/{}",
            counts.total() - counts.running - counts.queued,
            counts.total()
        )),
        Overall::Passing => Some("All passed".into()),
    }
}

pub fn duration_text(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    pub name: String,
    pub tone: Tone,
    pub duration: Option<String>,
    pub rerun: Option<u64>,
    pub log_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobGroup {
    pub title: String,
    pub jobs: Vec<JobRow>,
}

const OTHER_GROUP: &str = "Other";
const STATUS_GROUP: &str = "Commit statuses";

fn job_row(run: &CheckRun, now: i64) -> JobRow {
    JobRow {
        name: run.name.clone(),
        tone: outcome_tone(run.outcome()),
        duration: run.duration_seconds(now).map(duration_text),
        rerun: run.job_id.filter(|_| run.is_actions_job()),
        log_url: run.html_url.clone().or_else(|| run.details_url.clone()),
    }
}

/// The check data names the app that ran a job, not its workflow stage, so jobs group by app. Commit statuses
/// from the older status API form the last group.
pub fn job_groups(snapshot: &ChecksSnapshot, now: i64) -> Vec<JobGroup> {
    let mut groups: Vec<JobGroup> = Vec::new();
    for run in &snapshot.runs {
        let title = run.app.as_deref().unwrap_or(OTHER_GROUP);
        let row = job_row(run, now);
        match groups.iter_mut().find(|group| group.title == title) {
            Some(group) => group.jobs.push(row),
            None => groups.push(JobGroup {
                title: title.to_string(),
                jobs: vec![row],
            }),
        }
    }
    if !snapshot.statuses.is_empty() {
        groups.push(JobGroup {
            title: STATUS_GROUP.into(),
            jobs: snapshot
                .statuses
                .iter()
                .map(|status| JobRow {
                    name: status.context.clone(),
                    tone: outcome_tone(status.outcome()),
                    duration: None,
                    rerun: None,
                    log_url: status.target_url.clone(),
                })
                .collect(),
        });
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffpilot_core::github::checks::{CommitStatus, Conclusion, RunStatus, StatusState};
    use diffpilot_core::github::types::PrState;
    use std::time::Duration;

    fn status(ahead: u32, behind: u32) -> BranchStatus {
        BranchStatus {
            branch: Some("feature".into()),
            upstream: Some("origin/feature".into()),
            ahead,
            behind,
            dirty: false,
            last_fetch: None,
        }
    }

    fn pr(number: u64, review: Option<ReviewDecision>) -> PullRequest {
        PullRequest {
            node_id: format!("PR_{number}"),
            number,
            title: "Bulk select in the inbox".into(),
            url: format!("https://github.com/o/r/pull/{number}"),
            state: PrState::Open,
            is_draft: false,
            author: "jordan-lee".into(),
            base_ref: "main".into(),
            head_ref: "feature/bulk-select".into(),
            head_sha: "a".repeat(40),
            review_decision: review,
            checks: None,
            body: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            review_thread_count: 0,
            head_repo: Some("o/r".into()),
            is_cross_repository: false,
        }
    }

    fn run(name: &str, app: Option<&str>, conclusion: Conclusion, job: Option<u64>) -> CheckRun {
        CheckRun {
            id: 1,
            name: name.into(),
            status: RunStatus::Completed,
            conclusion: Some(conclusion),
            started_at: Some("2026-11-25T10:00:00Z".into()),
            completed_at: Some("2026-11-25T10:02:05Z".into()),
            html_url: Some(format!("https://example.com/{name}")),
            details_url: None,
            app: app.map(Into::into),
            title: None,
            summary: None,
            run_id: job.map(|_| 7),
            job_id: job,
        }
    }

    #[test]
    fn the_header_counts_what_is_waiting_in_each_direction() {
        assert_eq!(sync_header(&status(4, 1)), "4 commits to push · 1 commit to pull");
        assert_eq!(sync_header(&status(1, 0)), "1 commit to push");
        assert_eq!(sync_header(&status(0, 2)), "2 commits to pull");
        assert_eq!(sync_header(&status(0, 0)), "Up to date");
        let mut none = status(0, 0);
        none.upstream = None;
        assert_eq!(sync_header(&none), "No upstream branch");
    }

    #[test]
    fn the_fetch_row_says_how_long_ago() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let ago = |seconds| fetch_detail(Some(now - Duration::from_secs(seconds)), now);
        assert_eq!(ago(10), "Last fetched just now");
        assert_eq!(ago(180), "Last fetched 3 min ago");
        assert_eq!(ago(7300), "Last fetched 2 hr ago");
        assert_eq!(ago(86_400), "Last fetched 1 day ago");
        assert_eq!(ago(3 * 86_400), "Last fetched 3 days ago");
        assert_eq!(fetch_detail(None, now), "Not fetched yet");
    }

    #[test]
    fn the_pull_row_names_what_pulling_does() {
        let row = pull_row(&status(0, 3), PullMode::Rebase);
        assert_eq!(
            (row.title.as_str(), row.detail.as_str(), row.enabled),
            ("Pull 3 commits", "Rebase onto origin/feature", true)
        );
        let row = pull_row(&status(0, 1), PullMode::FastForward);
        assert_eq!(
            (row.title.as_str(), row.detail.as_str()),
            ("Pull 1 commit", "Fast-forward to origin/feature")
        );
        assert!(!pull_row(&status(2, 0), PullMode::Rebase).enabled);
    }

    #[test]
    fn push_is_dimmed_until_the_pull_is_done() {
        let row = push_row(&status(4, 1));
        assert_eq!(
            (row.title.as_str(), row.detail.as_str(), row.enabled),
            ("Push 4 commits", "After you pull", false)
        );
        assert_eq!(
            pull_first_warning(&status(4, 1)).as_deref(),
            Some("Pull first. The remote has 1 commit you do not have. Push works after you pull.")
        );
        let ready = push_row(&status(4, 0));
        assert_eq!((ready.detail.as_str(), ready.enabled), ("To origin/feature", true));
        assert_eq!(pull_first_warning(&status(4, 0)), None);
        assert!(!push_row(&status(0, 0)).enabled);
    }

    #[test]
    fn a_branch_without_an_upstream_can_be_published() {
        let mut none = status(0, 0);
        none.upstream = None;
        let row = push_row(&none);
        assert_eq!((row.title.as_str(), row.enabled), ("Publish branch", true));
        assert!(!pull_row(&none, PullMode::Rebase).enabled);
        assert_eq!(pull_first_warning(&none), None);
    }

    #[test]
    fn a_pull_request_row_shows_author_review_and_branch() {
        let row = &pr_rows(&[pr(479, Some(ReviewDecision::ReviewRequired))], None)[0];
        assert_eq!(row.title, "#479 Bulk select in the inbox");
        assert_eq!(row.detail, "jordan-lee · review requested · feature/bulk-select");
        assert_eq!(
            pr_detail(&pr(1, Some(ReviewDecision::ChangesRequested))),
            "jordan-lee · changes requested · feature/bulk-select"
        );
        assert_eq!(
            pr_detail(&pr(1, Some(ReviewDecision::Approved))),
            "jordan-lee · approved · feature/bulk-select"
        );
        assert_eq!(pr_detail(&pr(1, None)), "jordan-lee · feature/bulk-select");
    }

    #[test]
    fn the_row_of_the_checked_out_branch_is_marked() {
        let rows = pr_rows(&[pr(1, None), pr(2, None)], Some(2));
        assert_eq!(rows.iter().map(|row| row.checked).collect::<Vec<_>>(), [false, true]);
    }

    #[test]
    fn dots_follow_the_state_of_the_checks() {
        assert_eq!(rollup_tone(Some(ChecksState::Pending)), Some(Tone::Running));
        assert_eq!(rollup_tone(Some(ChecksState::Failure)), Some(Tone::Failed));
        assert_eq!(rollup_tone(Some(ChecksState::Success)), Some(Tone::Passed));
        assert_eq!(rollup_tone(None), None);
        assert_eq!(overall_tone(Overall::None), None);
        assert_eq!(outcome_tone(Outcome::Queued), Tone::Muted);
    }

    #[test]
    fn the_capsule_counts_finished_checks_while_some_run() {
        let counts = Counts {
            passed: 3,
            failed: 0,
            running: 2,
            queued: 1,
            other: 0,
        };
        assert_eq!(checks_detail(counts, Overall::Pending).as_deref(), Some("3/6"));
        assert_eq!(
            checks_detail(
                Counts {
                    failed: 2,
                    ..Counts::default()
                },
                Overall::Failing
            )
            .as_deref(),
            Some("2 failed")
        );
        assert_eq!(checks_detail(Counts::default(), Overall::None), None);
    }

    #[test]
    fn durations_read_like_a_stopwatch() {
        assert_eq!(duration_text(34), "34s");
        assert_eq!(duration_text(125), "2m 05s");
        assert_eq!(duration_text(3725), "1h 02m");
    }

    #[test]
    fn jobs_group_by_app_and_only_actions_jobs_can_rerun() {
        let snapshot = ChecksSnapshot {
            sha: "s".into(),
            runs: vec![
                run("lint", Some("GitHub Actions"), Conclusion::Success, Some(11)),
                run("Vercel", Some("Vercel"), Conclusion::Success, None),
                run("tests", Some("GitHub Actions"), Conclusion::Failure, Some(12)),
                run("mystery", None, Conclusion::Success, None),
            ],
            statuses: vec![CommitStatus {
                context: "ci/legacy".into(),
                state: StatusState::Pending,
                description: None,
                target_url: Some("https://ci.example/1".into()),
            }],
        };
        let groups = job_groups(&snapshot, 0);
        let titles: Vec<&str> = groups.iter().map(|group| group.title.as_str()).collect();
        assert_eq!(titles, ["GitHub Actions", "Vercel", "Other", "Commit statuses"]);
        let actions: Vec<(&str, Tone, Option<u64>)> = groups[0]
            .jobs
            .iter()
            .map(|job| (job.name.as_str(), job.tone, job.rerun))
            .collect();
        assert_eq!(
            actions,
            [("lint", Tone::Passed, Some(11)), ("tests", Tone::Failed, Some(12))]
        );
        assert_eq!(groups[0].jobs[1].duration.as_deref(), Some("2m 05s"));
        assert_eq!(groups[1].jobs[0].rerun, None);
        assert_eq!(groups[3].jobs[0].tone, Tone::Running);
        assert_eq!(groups[3].jobs[0].log_url.as_deref(), Some("https://ci.example/1"));
    }
}
