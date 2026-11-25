//! What a Claude run is: its kind, plan, progress and log. UI-free; the engine in `runner` fills it in.
//!
//! The event merge follows diffity's `run_state` (MIT, see THIRD_PARTY_NOTICES), driven by stream-json
//! events instead of ACP updates.

use crate::review::{ReviewModel, ReviewResult};
pub use crate::snapshot::ChangedFile;
use crate::store::SessionId;
use crate::stream::{ResultEvent, StreamEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub use crate::store::RunId;

pub const MAX_LOG_ENTRIES: usize = 2000;
const MAX_ENTRY_CHARS: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunKind {
    Review,
    FixComments,
    MakeChanges,
    Reply,
}

impl RunKind {
    /// Runs that may change files. They get a snapshot first.
    pub fn edits(self) -> bool {
        !matches!(self, Self::Review)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Review => "Review",
            Self::FixComments => "Fix Comments",
            Self::MakeChanges => "Make Changes",
            Self::Reply => "Reply",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunState {
    Queued,
    Running,
    Done,
    Failed,
    Stopped,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }

    pub fn is_finished(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Stopped)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepState {
    Pending,
    Active,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStep {
    pub title: String,
    pub state: StepState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogKind {
    Thinking,
    Message,
    Read,
    Search,
    Edit,
    Command,
    Comment,
    Plan,
    Tool,
    Blocked,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub at_ms: u64,
    pub kind: LogKind,
    pub text: String,
    pub tool_use_id: Option<String>,
    pub failed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RunOutcome {
    Review(ReviewResult),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub session: SessionId,
    pub repo: PathBuf,
    pub kind: RunKind,
    pub title: String,
    pub state: RunState,
    pub model: ReviewModel,
    pub queued_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub steps: Vec<RunStep>,
    pub units: Option<Progress>,
    #[serde(default)]
    pub threads: Vec<crate::threads::ThreadId>,
    pub activity: Option<String>,
    pub log: Vec<LogEntry>,
    pub outcome: Option<RunOutcome>,
    pub error: Option<String>,
    pub claude_session: Option<String>,
    pub changed_files: Vec<ChangedFile>,
    #[serde(default)]
    pub snapshot: Option<crate::snapshot::Snapshot>,
    #[serde(default)]
    pub undone_at_ms: Option<i64>,
    pub num_turns: Option<u32>,
    pub cost_usd: Option<f64>,
}

/// One row of the Past Runs menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub id: RunId,
    pub kind: RunKind,
    pub title: String,
    pub state: RunState,
    pub model: ReviewModel,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
}

impl Run {
    pub fn new(
        id: RunId,
        session: SessionId,
        repo: &Path,
        kind: RunKind,
        title: &str,
        model: ReviewModel,
        queued_at_ms: i64,
    ) -> Self {
        Self {
            id,
            session,
            repo: repo.to_path_buf(),
            kind,
            title: title.to_string(),
            state: RunState::Queued,
            model,
            queued_at_ms,
            started_at_ms: None,
            finished_at_ms: None,
            steps: Vec::new(),
            units: None,
            threads: Vec::new(),
            activity: None,
            log: Vec::new(),
            outcome: None,
            error: None,
            claude_session: None,
            changed_files: Vec::new(),
            snapshot: None,
            undone_at_ms: None,
            num_turns: None,
            cost_usd: None,
        }
    }

    pub fn summary(&self) -> RunSummary {
        RunSummary {
            id: self.id,
            kind: self.kind,
            title: self.title.clone(),
            state: self.state,
            model: self.model,
            started_at_ms: self.started_at_ms,
            finished_at_ms: self.finished_at_ms,
        }
    }

    /// "3 of 4": units of work the request named (threads to fix) win over plan steps.
    pub fn progress(&self) -> Option<Progress> {
        if let Some(units) = self.units {
            return Some(units);
        }
        if self.steps.is_empty() {
            return None;
        }
        let done = self.steps.iter().filter(|step| step.state == StepState::Done).count();
        Some(Progress {
            done: done as u32,
            total: self.steps.len() as u32,
        })
    }

    pub fn elapsed_ms(&self, now_ms: i64) -> Option<u64> {
        let started = self.started_at_ms?;
        let end = self.finished_at_ms.unwrap_or(now_ms);
        u64::try_from(end - started).ok()
    }

    /// The log entry a tool result belongs to, so a failed call can mark it.
    fn entry_of(&mut self, tool_use_id: &str) -> Option<&mut LogEntry> {
        self.log
            .iter_mut()
            .rev()
            .find(|entry| entry.tool_use_id.as_deref() == Some(tool_use_id))
    }

    fn push(&mut self, entry: LogEntry) -> LogEntry {
        if self.log.len() >= MAX_LOG_ENTRIES {
            self.log.remove(0);
        }
        self.log.push(entry.clone());
        entry
    }

    fn entry(&self, now_ms: i64, kind: LogKind, text: &str, tool_use_id: Option<&str>) -> LogEntry {
        LogEntry {
            at_ms: self
                .started_at_ms
                .map_or(0, |started| u64::try_from(now_ms - started).unwrap_or(0)),
            kind,
            text: clipped(text),
            tool_use_id: tool_use_id.map(String::from),
            failed: false,
        }
    }

    /// Merges one stream event. Returns the log entries it added.
    pub fn apply(&mut self, event: &StreamEvent, repo: &Path, now_ms: i64) -> Vec<LogEntry> {
        match event {
            StreamEvent::Init { session_id, .. } => {
                self.claude_session = Some(session_id.clone()).filter(|id| !id.is_empty());
                Vec::new()
            }
            StreamEvent::Text { text, .. } => {
                if text.trim().is_empty() {
                    return Vec::new();
                }
                self.activity = Some("writing".into());
                let entry = self.entry(now_ms, LogKind::Message, text.trim(), None);
                vec![self.push(entry)]
            }
            StreamEvent::Thinking { text } => {
                self.activity = Some("thinking".into());
                let line = text.trim().lines().next().unwrap_or("Thinking");
                let entry = self.entry(now_ms, LogKind::Thinking, line, None);
                vec![self.push(entry)]
            }
            StreamEvent::ToolUse { id, name, input } => self.tool_use(id, name, input, repo, now_ms),
            StreamEvent::ToolResult {
                tool_use_id,
                text,
                is_error,
            } => self.tool_result(tool_use_id, text, *is_error, now_ms),
            StreamEvent::Result(result) => {
                self.finish_result(result);
                Vec::new()
            }
        }
    }

    fn tool_use(&mut self, id: &str, name: &str, input: &Value, repo: &Path, now_ms: i64) -> Vec<LogEntry> {
        if name == "TodoWrite" {
            self.steps = plan_steps(input);
            self.activity = self
                .steps
                .iter()
                .find(|step| step.state == StepState::Active)
                .map(|step| step.title.clone());
            let entry = self.entry(
                now_ms,
                LogKind::Plan,
                &format!("Plan: {} steps", self.steps.len()),
                Some(id),
            );
            return vec![self.push(entry)];
        }
        let described = describe_tool(name, input, repo);
        self.activity = Some(described.activity);
        let entry = self.entry(now_ms, described.kind, &described.text, Some(id));
        vec![self.push(entry)]
    }

    fn tool_result(&mut self, id: &str, text: &str, is_error: bool, now_ms: i64) -> Vec<LogEntry> {
        let blocked = is_error && text.contains(crate::guard::BLOCK_MARKER);
        if let Some(entry) = self.entry_of(id) {
            entry.failed = is_error;
        }
        if !is_error {
            return Vec::new();
        }
        let (kind, text) = if blocked {
            (LogKind::Blocked, text.to_string())
        } else {
            (LogKind::Error, text.lines().next().unwrap_or_default().to_string())
        };
        let entry = self.entry(now_ms, kind, &text, Some(id));
        vec![self.push(entry)]
    }

    fn finish_result(&mut self, result: &ResultEvent) {
        self.num_turns = result.num_turns;
        self.cost_usd = result.cost_usd;
        if result.is_error {
            self.error = Some(
                result
                    .result
                    .clone()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| format!("Claude stopped: {}.", result.subtype.replace('_', " "))),
            );
            return;
        }
        if let Some(output) = &result.structured_output {
            match serde_json::from_value::<ReviewResult>(output.clone()) {
                Ok(review) => self.outcome = Some(RunOutcome::Review(review)),
                Err(_) => self.error = Some("Claude returned a review in a shape diffpilot cannot read.".into()),
            }
        } else if let Some(review) = self
            .kind
            .eq(&RunKind::Review)
            .then(|| result.result.as_deref().and_then(review_in_text))
            .flatten()
        {
            self.outcome = Some(RunOutcome::Review(review));
        } else if let Some(text) = result.result.as_deref().filter(|text| !text.trim().is_empty()) {
            self.outcome = Some(RunOutcome::Text(text.trim().to_string()));
        }
    }
}

/// A review Claude wrote as JSON text, when the structured output field is missing.
fn review_in_text(text: &str) -> Option<ReviewResult> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(text.get(start..=end)?).ok()
}

fn clipped(text: &str) -> String {
    if text.chars().count() <= MAX_ENTRY_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_ENTRY_CHARS).collect();
    format!("{cut}…")
}

fn plan_steps(input: &Value) -> Vec<RunStep> {
    input
        .get("todos")
        .and_then(Value::as_array)
        .map(|todos| {
            todos
                .iter()
                .map(|todo| {
                    let state = match todo.get("status").and_then(Value::as_str) {
                        Some("completed") => StepState::Done,
                        Some("in_progress") => StepState::Active,
                        _ => StepState::Pending,
                    };
                    let title = match state {
                        StepState::Active => todo.get("activeForm").or_else(|| todo.get("content")),
                        _ => todo.get("content"),
                    };
                    RunStep {
                        title: title.and_then(Value::as_str).unwrap_or_default().to_string(),
                        state,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

struct Described {
    kind: LogKind,
    text: String,
    activity: String,
}

fn relative(path: &str, repo: &Path) -> String {
    Path::new(path)
        .strip_prefix(repo)
        .map_or_else(|_| path.to_string(), |rest| rest.to_string_lossy().into_owned())
}

fn input_text<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn describe_tool(name: &str, input: &Value, repo: &Path) -> Described {
    let path = || relative(input_text(input, "file_path"), repo);
    let (kind, text, activity) = match name {
        "Read" => (LogKind::Read, format!("Read {}", path()), format!("reading {}", path())),
        "Grep" | "Glob" => {
            let pattern = input_text(input, "pattern");
            (
                LogKind::Search,
                format!("Searched {pattern}"),
                format!("searching for {pattern}"),
            )
        }
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let target = if name == "NotebookEdit" {
                relative(input_text(input, "notebook_path"), repo)
            } else {
                path()
            };
            (LogKind::Edit, format!("Edited {target}"), format!("editing {target}"))
        }
        "Bash" => {
            let command = input_text(input, "command");
            let short: String = command.lines().next().unwrap_or_default().chars().take(120).collect();
            (
                LogKind::Command,
                format!("Ran {short}"),
                "running a command".to_string(),
            )
        }
        _ => match name.strip_prefix("mcp__diffpilot__") {
            Some(tool) => {
                let (text, activity) = describe_mcp(tool, input);
                (LogKind::Comment, text, activity)
            }
            None => (LogKind::Tool, format!("Used {name}"), format!("using {name}")),
        },
    };
    Described { kind, text, activity }
}

fn describe_mcp(tool: &str, input: &Value) -> (String, String) {
    let place = || {
        let file = input_text(input, "file");
        match input.get("startLine").and_then(Value::as_u64) {
            Some(line) if line > 0 => format!("{file}:{line}"),
            _ => file.to_string(),
        }
    };
    match tool {
        "add_comment" => (format!("Commented on {}", place()), "commenting".into()),
        "add_general_comment" => ("Wrote the summary".into(), "commenting".into()),
        "reply" => ("Replied to a thread".into(), "replying".into()),
        "resolve" => ("Marked a thread as addressed".into(), "addressing a thread".into()),
        "dismiss" => ("Dismissed a thread".into(), "addressing a thread".into()),
        "list_threads" => ("Read the threads".into(), "reading the threads".into()),
        "get_diff" => ("Read the diff".into(), "reading the diff".into()),
        other => (format!("Used {other}"), format!("using {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{Effort, ModelName};
    use serde_json::json;

    fn run() -> Run {
        let mut run = Run::new(
            RunId(1),
            SessionId(1),
            Path::new("/r"),
            RunKind::FixComments,
            "Fixing 2 comments",
            ReviewModel::new(ModelName::Sonnet, Effort::Medium),
            0,
        );
        run.state = RunState::Running;
        run.started_at_ms = Some(1_000);
        run
    }

    fn use_tool(run: &mut Run, id: &str, name: &str, input: Value) -> Vec<LogEntry> {
        run.apply(
            &StreamEvent::ToolUse {
                id: id.into(),
                name: name.into(),
                input,
            },
            Path::new("/r"),
            3_500,
        )
    }

    #[test]
    fn tool_calls_become_readable_log_lines_with_a_time_since_start() {
        let mut run = run();
        let read = use_tool(&mut run, "1", "Read", json!({"file_path": "/r/src/a.ts"}));
        assert_eq!(
            (read[0].kind, read[0].text.as_str(), read[0].at_ms),
            (LogKind::Read, "Read src/a.ts", 2_500)
        );
        assert_eq!(run.activity.as_deref(), Some("reading src/a.ts"));
        let edit = use_tool(&mut run, "2", "Edit", json!({"file_path": "/r/src/a.ts"}));
        assert_eq!(
            (edit[0].kind, edit[0].text.as_str()),
            (LogKind::Edit, "Edited src/a.ts")
        );
        assert_eq!(run.activity.as_deref(), Some("editing src/a.ts"));
        let search = use_tool(&mut run, "3", "Grep", json!({"pattern": "undo"}));
        assert_eq!(search[0].kind, LogKind::Search);
        let command = use_tool(&mut run, "4", "Bash", json!({"command": "npm test\nsecond line"}));
        assert_eq!(
            (command[0].kind, command[0].text.as_str()),
            (LogKind::Command, "Ran npm test")
        );
        let comment = use_tool(
            &mut run,
            "5",
            "mcp__diffpilot__add_comment",
            json!({"file": "a.ts", "startLine": 7, "body": "x"}),
        );
        assert_eq!(
            (comment[0].kind, comment[0].text.as_str()),
            (LogKind::Comment, "Commented on a.ts:7")
        );
        assert_eq!(run.log.len(), 5);
    }

    #[test]
    fn a_todo_list_replaces_the_plan_and_drives_progress() {
        let mut run = run();
        assert_eq!(run.progress(), None);
        use_tool(
            &mut run,
            "1",
            "TodoWrite",
            json!({"todos": [
                {"content": "Read comments", "status": "completed", "activeForm": "Reading comments"},
                {"content": "Edit a.ts", "status": "in_progress", "activeForm": "Editing a.ts"},
                {"content": "Run tests", "status": "pending", "activeForm": "Running tests"},
            ]}),
        );
        let titles: Vec<_> = run.steps.iter().map(|step| (step.title.as_str(), step.state)).collect();
        assert_eq!(
            titles,
            [
                ("Read comments", StepState::Done),
                ("Editing a.ts", StepState::Active),
                ("Run tests", StepState::Pending)
            ]
        );
        assert_eq!(run.progress(), Some(Progress { done: 1, total: 3 }));
        assert_eq!(run.activity.as_deref(), Some("Editing a.ts"));
        run.units = Some(Progress { done: 2, total: 4 });
        assert_eq!(run.progress(), Some(Progress { done: 2, total: 4 }));
    }

    #[test]
    fn a_failed_tool_result_marks_its_line_and_adds_an_error() {
        let mut run = run();
        use_tool(&mut run, "9", "Bash", json!({"command": "false"}));
        let added = run.apply(
            &StreamEvent::ToolResult {
                tool_use_id: "9".into(),
                text: "exit 1\nmore".into(),
                is_error: true,
            },
            Path::new("/r"),
            4_000,
        );
        assert!(run.log[0].failed);
        assert_eq!((added[0].kind, added[0].text.as_str()), (LogKind::Error, "exit 1"));
    }

    #[test]
    fn a_blocked_command_is_logged_as_blocked() {
        let mut run = run();
        use_tool(&mut run, "9", "Bash", json!({"command": "git -C . push"}));
        let message = format!("{} git push is not allowed in a run.", crate::guard::BLOCK_MARKER);
        let added = run.apply(
            &StreamEvent::ToolResult {
                tool_use_id: "9".into(),
                text: message,
                is_error: true,
            },
            Path::new("/r"),
            4_000,
        );
        assert_eq!(added[0].kind, LogKind::Blocked);
    }

    #[test]
    fn the_result_line_sets_the_outcome_or_the_error() {
        let result = |is_error, text: &str, structured: Option<Value>| {
            StreamEvent::Result(ResultEvent {
                is_error,
                subtype: if is_error {
                    "error_during_execution".into()
                } else {
                    "success".into()
                },
                result: Some(text.into()),
                structured_output: structured,
                num_turns: Some(2),
                duration_ms: None,
                cost_usd: Some(0.1),
            })
        };
        let mut ok = run();
        ok.apply(&result(false, " All done. ", None), Path::new("/r"), 5_000);
        assert_eq!(ok.outcome, Some(RunOutcome::Text("All done.".into())));
        assert_eq!((ok.num_turns, ok.cost_usd), (Some(2), Some(0.1)));

        let mut review = run();
        let structured = json!({"summary": "Fine.", "findings": []});
        review.apply(&result(false, "", Some(structured)), Path::new("/r"), 5_000);
        assert!(matches!(review.outcome, Some(RunOutcome::Review(ref found)) if found.summary == "Fine."));

        let mut failed = run();
        failed.apply(&result(true, "Not logged in", None), Path::new("/r"), 5_000);
        assert_eq!(failed.error.as_deref(), Some("Not logged in"));
        assert_eq!(failed.outcome, None);
    }

    #[test]
    fn a_review_written_as_json_text_is_still_read() {
        let mut review = run();
        review.kind = RunKind::Review;
        let text = "Here is the review:\n```json\n{\"summary\":\"Fine.\",\"findings\":[{\"path\":\"a\",\"severity\":\"nit\",\"title\":\"t\",\"body\":\"b\"}]}\n```";
        review.apply(
            &StreamEvent::Result(ResultEvent {
                is_error: false,
                subtype: "success".into(),
                result: Some(text.into()),
                structured_output: None,
                num_turns: None,
                duration_ms: None,
                cost_usd: None,
            }),
            Path::new("/r"),
            1,
        );
        assert!(matches!(review.outcome, Some(RunOutcome::Review(ref found)) if found.findings.len() == 1));
    }

    #[test]
    fn the_log_is_capped_and_long_lines_are_clipped() {
        let mut run = run();
        for number in 0..MAX_LOG_ENTRIES + 5 {
            use_tool(&mut run, &number.to_string(), "Read", json!({"file_path": "/r/a"}));
        }
        assert_eq!(run.log.len(), MAX_LOG_ENTRIES);
        assert_eq!(run.log[0].tool_use_id.as_deref(), Some("5"));
        let long = "x".repeat(MAX_ENTRY_CHARS + 50);
        let added = run.apply(
            &StreamEvent::Text {
                message_id: "m".into(),
                text: long,
            },
            Path::new("/r"),
            2_000,
        );
        assert_eq!(added[0].text.chars().count(), MAX_ENTRY_CHARS + 1);
    }

    #[test]
    fn a_run_round_trips_through_json() {
        let mut run = run();
        use_tool(&mut run, "1", "Read", json!({"file_path": "/r/a"}));
        let json = serde_json::to_string(&run).unwrap();
        assert_eq!(serde_json::from_str::<Run>(&json).unwrap(), run);
    }

    #[test]
    fn only_review_runs_are_read_only() {
        assert!(!RunKind::Review.edits());
        assert!(RunKind::FixComments.edits() && RunKind::MakeChanges.edits() && RunKind::Reply.edits());
    }
}
