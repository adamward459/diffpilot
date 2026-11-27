use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrState {
    Open,
    Closed,
    Merged,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
    #[serde(other)]
    Unknown,
}

/// The `statusCheckRollup` state of a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChecksState {
    Expected,
    Error,
    Failure,
    Pending,
    Success,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub node_id: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: PrState,
    pub is_draft: bool,
    pub author: String,
    pub base_ref: String,
    pub head_ref: String,
    pub head_sha: String,
    pub review_decision: Option<ReviewDecision>,
    pub requested_from_you: bool,
    pub checks: Option<ChecksState>,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub review_thread_count: u32,
    pub head_repo: Option<String>,
    pub is_cross_repository: bool,
}

impl PullRequest {
    pub fn is_open(&self) -> bool {
        self.state == PrState::Open
    }
}

#[cfg(test)]
pub(crate) fn sample_pr(number: u64, head_ref: &str) -> PullRequest {
    PullRequest {
        node_id: format!("PR_{number}"),
        number,
        title: format!("PR {number}"),
        url: format!("https://github.com/o/r/pull/{number}"),
        state: PrState::Open,
        is_draft: false,
        author: "mira".into(),
        base_ref: "main".into(),
        head_ref: head_ref.into(),
        head_sha: "0".repeat(40),
        review_decision: None,
        requested_from_you: false,
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
