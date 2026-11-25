use crate::cancel::CancelToken;
use crate::paths::find_executable;
use crate::prompt;
use crate::review::{ReviewRequest, ReviewResult};
use crate::threads::{ReviewThread, ThreadAnchor};
use serde::Deserialize;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewError {
    #[error("The claude command was not found. Install Claude Code, then try again.")]
    NotFound,
    #[error("{0}")]
    Failed(String),
    #[error("The run was cancelled.")]
    Cancelled,
}

/// Runs on the caller's thread and blocks. Call it from a background executor.
pub trait Reviewer: Send + Sync {
    fn review(&self, request: &ReviewRequest, cancel: &CancelToken) -> Result<ReviewResult, ReviewError>;
}

/// Chunks arrive on the returned channel. It closes when the reply ends, fails or is cancelled.
pub trait Replier: Send + Sync {
    fn reply(&self, thread: &ReviewThread, cancel: &CancelToken) -> async_channel::Receiver<String>;
}

pub const DEFAULT_REVIEW_DELAY: Duration = Duration::from_millis(1500);

pub struct ScriptedReviewer {
    pub result: ReviewResult,
    pub delay: Duration,
    pub failure: Option<String>,
}

impl Default for ScriptedReviewer {
    fn default() -> Self {
        Self {
            result: default_scripted_result(),
            delay: DEFAULT_REVIEW_DELAY,
            failure: None,
        }
    }
}

impl Reviewer for ScriptedReviewer {
    fn review(&self, _request: &ReviewRequest, cancel: &CancelToken) -> Result<ReviewResult, ReviewError> {
        if cancel.wait(self.delay) {
            return Err(ReviewError::Cancelled);
        }
        match &self.failure {
            Some(message) => Err(ReviewError::Failed(message.clone())),
            None => Ok(self.result.clone()),
        }
    }
}

pub fn default_scripted_result() -> ReviewResult {
    use crate::review::{Finding, Severity};
    let finding = |path: &str, line: Option<u32>, severity, title: &str, body: &str| Finding {
        path: path.into(),
        line,
        severity,
        title: title.into(),
        body: body.into(),
    };
    ReviewResult {
        summary: "The archive flow works, but one path skips the undo step.".into(),
        findings: vec![
            finding(
                "docs/archive.md",
                Some(2),
                Severity::MustFix,
                "Undo window is not documented",
                "The doc names no undo window. Add the time limit in `docs/archive.md:2`.",
            ),
            finding(
                "src/api/archive.ts",
                Some(9999),
                Severity::Suggestion,
                "Retry is missing",
                "A failed archive call is not retried. Wrap it in the shared retry helper.",
            ),
            finding(
                "src/api/client.ts",
                None,
                Severity::Nit,
                "Name is vague",
                "Rename `req` to `request`.",
            ),
            finding(
                "scripts/missing.py",
                Some(3),
                Severity::Question,
                "Is this file still used?",
                "Nothing in the diff calls this script. Should it stay?",
            ),
        ],
    }
}

/// `DIFFPILOT_REVIEWER=scripted` picks the scripted reviewer, and `DIFFPILOT_REVIEW_SECONDS` sets its delay.
pub fn reviewer_from_env() -> Box<dyn Reviewer> {
    reviewer_from(
        std::env::var("DIFFPILOT_REVIEWER").ok().as_deref(),
        std::env::var("DIFFPILOT_REVIEW_SECONDS").ok().as_deref(),
    )
}

pub fn reviewer_from(kind: Option<&str>, seconds: Option<&str>) -> Box<dyn Reviewer> {
    if kind != Some("scripted") {
        return Box::new(ClaudeCliReviewer::default());
    }
    Box::new(ScriptedReviewer {
        delay: scripted_delay(seconds),
        ..ScriptedReviewer::default()
    })
}

pub fn scripted_delay(seconds: Option<&str>) -> Duration {
    seconds
        .and_then(|text| text.trim().parse::<f64>().ok())
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .unwrap_or(DEFAULT_REVIEW_DELAY)
}

pub const SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"},"findings":{"type":"array","items":{"type":"object","properties":{"path":{"type":"string"},"line":{"type":["integer","null"]},"severity":{"type":"string","enum":["must-fix","suggestion","nit","question"]},"title":{"type":"string"},"body":{"type":"string"}},"required":["path","severity","title","body"]}}},"required":["summary","findings"]}"#;

const READ_ONLY_GIT: [&str; 3] = ["Bash(git diff:*)", "Bash(git log:*)", "Bash(git show:*)"];

#[derive(Debug, Clone, Default)]
pub struct ClaudeCliReviewer {
    pub binary: Option<PathBuf>,
    pub extra_arguments: Vec<String>,
}

impl ClaudeCliReviewer {
    pub fn arguments(request: &ReviewRequest) -> Vec<String> {
        let mut args: Vec<String> = [
            "-p",
            &prompt::render(request),
            "--output-format",
            "json",
            "--json-schema",
            SCHEMA,
            "--tools",
            "Read,Grep,Glob,Bash",
            "--model",
            request.model.name.as_str(),
            "--effort",
            request.model.effort.as_str(),
            "--allowedTools",
        ]
        .map(String::from)
        .into();
        args.extend(READ_ONLY_GIT.map(String::from));
        args
    }

    pub fn parse(output: &[u8]) -> Result<ReviewResult, ReviewError> {
        #[derive(Deserialize)]
        struct Envelope {
            is_error: Option<bool>,
            result: Option<String>,
            structured_output: Option<ReviewResult>,
        }
        let envelope: Envelope = serde_json::from_slice(output)
            .map_err(|_| ReviewError::Failed("Claude returned output that is not valid JSON.".into()))?;
        if envelope.is_error == Some(true) {
            return Err(ReviewError::Failed(
                envelope.result.unwrap_or_else(|| "Claude reported an error.".into()),
            ));
        }
        envelope
            .structured_output
            .ok_or_else(|| ReviewError::Failed("Claude returned no structured review.".into()))
    }
}

pub fn claude_binary() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/claude"));
    }
    candidates.push("/opt/homebrew/bin/claude".into());
    candidates.push("/usr/local/bin/claude".into());
    find_executable(&candidates, "claude")
}

fn claude_command(binary: &Path, repo: &Path) -> Command {
    let mut command = Command::new(binary);
    command.current_dir(repo);
    let home = binary.parent().map(|dir| dir.to_string_lossy().into_owned());
    let path: Vec<String> = home
        .into_iter()
        .chain(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(String::from))
        .collect();
    command.env("PATH", path.join(":"));
    command
}

impl Reviewer for ClaudeCliReviewer {
    fn review(&self, request: &ReviewRequest, cancel: &CancelToken) -> Result<ReviewResult, ReviewError> {
        let binary = self
            .binary
            .clone()
            .or_else(claude_binary)
            .ok_or(ReviewError::NotFound)?;
        let mut command = claude_command(&binary, &request.repo);
        command.args(Self::arguments(request)).args(&self.extra_arguments);

        let stdout = Arc::new(Mutex::new(Vec::new()));
        let sink = stdout.clone();
        let finished = run_process(command, cancel, move |chunk| {
            sink.lock().unwrap().extend_from_slice(chunk);
            true
        })?;
        let output = stdout.lock().unwrap().clone();
        if let Ok(result) = Self::parse(&output) {
            return Ok(result);
        }
        if !finished.status.success() {
            let message = String::from_utf8_lossy(&finished.stderr).trim().to_string();
            return Err(ReviewError::Failed(if message.is_empty() {
                match finished.status.code() {
                    Some(code) => format!("claude exited with status {code}."),
                    None => "claude was stopped by a signal.".into(),
                }
            } else {
                message
            }));
        }
        Self::parse(&output)
    }
}

struct Finished {
    status: ExitStatus,
    stderr: Vec<u8>,
}

/// `on_stdout` returns false to stop the process early, which reports `Cancelled`.
/// On cancel the child's process group is stopped and its pipe threads are left to end on their own.
fn run_process(
    mut command: Command,
    cancel: &CancelToken,
    mut on_stdout: impl FnMut(&[u8]) -> bool + Send + 'static,
) -> Result<Finished, ReviewError> {
    const POLL: Duration = Duration::from_millis(20);
    if cancel.is_cancelled() {
        return Err(ReviewError::Cancelled);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| ReviewError::Failed(error.to_string()))?;
    let (mut stdout, mut stderr) = (
        child.stdout.take().expect("stdout is piped"),
        child.stderr.take().expect("stderr is piped"),
    );
    let stopped = Arc::new(AtomicBool::new(false));
    let reader_stopped = stopped.clone();
    let out_thread = std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) if !on_stdout(&buffer[..count]) => {
                    reader_stopped.store(true, Ordering::SeqCst);
                    break;
                }
                Ok(_) => {}
            }
        }
    });
    let err_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });
    loop {
        if cancel.wait(POLL) || stopped.load(Ordering::SeqCst) {
            stop_group(&mut child);
            return Err(ReviewError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let _ = out_thread.join();
                let stderr = err_thread.join().unwrap_or_default();
                return Ok(Finished { status, stderr });
            }
            Ok(None) => {}
            Err(error) => return Err(ReviewError::Failed(error.to_string())),
        }
    }
}

/// SIGTERM first, so the CLI can stop the tools it started, then SIGKILL for whatever is left.
fn stop_group(child: &mut Child) {
    const GRACE: Duration = Duration::from_secs(2);
    const POLL: Duration = Duration::from_millis(20);
    let Ok(group) = libc::pid_t::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    // SAFETY: killpg only sends a signal; the group is the child's own, made by `process_group(0)`.
    unsafe { libc::killpg(group, libc::SIGTERM) };
    let started = Instant::now();
    while started.elapsed() < GRACE && matches!(child.try_wait(), Ok(None)) {
        std::thread::sleep(POLL);
    }
    unsafe { libc::killpg(group, libc::SIGKILL) };
    let _ = child.wait();
}

pub const DEFAULT_REPLY_INTERVAL: Duration = Duration::from_millis(33);

pub const DEFAULT_REPLY_TEXT: &str = "I read the change around this line. The `onDismiss` callback now runs after the sheet animates out, so a caller that reads layout inside it sees the final frame. I would keep it, and add a test that dismisses twice in a row. I did not change any code yet.";

pub struct ScriptedReplier {
    pub text: String,
    pub interval: Duration,
}

impl Default for ScriptedReplier {
    fn default() -> Self {
        Self {
            text: DEFAULT_REPLY_TEXT.into(),
            interval: DEFAULT_REPLY_INTERVAL,
        }
    }
}

impl Replier for ScriptedReplier {
    fn reply(&self, _thread: &ReviewThread, cancel: &CancelToken) -> async_channel::Receiver<String> {
        let words: Vec<String> = self.text.split(' ').map(|word| format!("{word} ")).collect();
        let interval = self.interval;
        let cancel = cancel.clone();
        let (sender, receiver) = async_channel::unbounded();
        std::thread::spawn(move || {
            for word in words {
                if cancel.is_cancelled() || sender.send_blocking(word).is_err() {
                    break;
                }
                if cancel.wait(interval) {
                    break;
                }
            }
        });
        receiver
    }
}

#[derive(Debug, Clone)]
pub struct ClaudeCliReplier {
    pub repo: PathBuf,
    pub binary: Option<PathBuf>,
    pub model: &'static str,
}

impl ClaudeCliReplier {
    pub fn new(repo: &Path) -> Self {
        Self {
            repo: repo.to_path_buf(),
            binary: None,
            model: "sonnet",
        }
    }
}

pub fn reply_prompt(thread: &ReviewThread) -> String {
    let place = match &thread.anchor {
        ThreadAnchor::File { path } => format!("the file `{path}`"),
        ThreadAnchor::Line { path, side, line } => {
            format!(
                "`{path}` line {line} on the {} side of the diff",
                format!("{side:?}").to_lowercase()
            )
        }
        ThreadAnchor::Change => "the change as a whole".into(),
    };
    let mut text = format!(
        "You are Claude, replying in a comment thread inside diffpilot, a code-review app. The thread is on {place}. Answer the last comment in a few short sentences. Do not edit files.\n\n"
    );
    if let Some(finding) = &thread.finding {
        text.push_str(&format!("Review finding: {}\n\n", finding.title));
    }
    for comment in thread.comments.iter().filter(|comment| !comment.is_streaming) {
        text.push_str(&format!("{}: {}\n\n", comment.author, comment.body));
    }
    text
}

impl Replier for ClaudeCliReplier {
    fn reply(&self, thread: &ReviewThread, cancel: &CancelToken) -> async_channel::Receiver<String> {
        let (sender, receiver) = async_channel::unbounded();
        let Some(binary) = self.binary.clone().or_else(claude_binary) else {
            return receiver;
        };
        let mut command = claude_command(&binary, &self.repo);
        command
            .args([
                "-p",
                &reply_prompt(thread),
                "--model",
                self.model,
                "--tools",
                "Read,Grep,Glob,Bash",
            ])
            .arg("--allowedTools")
            .args(READ_ONLY_GIT);
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            let mut decoder = Utf8Chunks::default();
            let tx = sender.clone();
            let _ = run_process(command, &cancel, move |chunk| {
                let text = decoder.push(chunk);
                text.is_empty() || tx.send_blocking(text).is_ok()
            });
        });
        receiver
    }
}

/// Holds back bytes of a character that a read cut in half.
#[derive(Default)]
struct Utf8Chunks {
    pending: Vec<u8>,
}

impl Utf8Chunks {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        match std::str::from_utf8(&self.pending) {
            Ok(text) => {
                let text = text.to_string();
                self.pending.clear();
                text
            }
            Err(error) if error.error_len().is_none() => {
                let valid = error.valid_up_to();
                let text = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
                self.pending.drain(..valid);
                text
            }
            Err(_) => {
                let text = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                text
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{Effort, ModelName, ReviewModel, Severity};
    use crate::threads::{Side, ThreadStore};
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    fn request() -> ReviewRequest {
        ReviewRequest::new(Path::new("/r"), vec!["--merge-base".into(), "main".into()], "Tests")
    }

    fn fake_claude(dir: &Path, script: &str) -> PathBuf {
        let path = dir.join("claude");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn cli(binary: PathBuf) -> ClaudeCliReviewer {
        ClaudeCliReviewer {
            binary: Some(binary),
            extra_arguments: Vec::new(),
        }
    }

    #[test]
    fn parses_the_structured_output_of_the_cli() {
        let json = r#"{"type":"result","subtype":"success","is_error":false,"result":"","session_id":"x",
             "structured_output":{"summary":"One bug.","findings":[
               {"path":"a.ts","line":4,"severity":"must-fix","title":"Null","body":"Check it."},
               {"path":"b.ts","line":null,"severity":"question","title":"Why","body":"Is it used?"},
               {"path":"c.ts","severity":"nit","title":"Name","body":"Rename."}]}}"#;
        let result = ClaudeCliReviewer::parse(json.as_bytes()).unwrap();
        assert_eq!(result.summary, "One bug.");
        let severities: Vec<_> = result.findings.iter().map(|f| f.severity).collect();
        assert_eq!(severities, [Severity::MustFix, Severity::Question, Severity::Nit]);
        let lines: Vec<_> = result.findings.iter().map(|f| f.line).collect();
        assert_eq!(lines, [Some(4), None, None]);
        assert_eq!(result.counts().len(), 3);
    }

    #[test]
    fn cli_errors_become_messages() {
        let failed = |message: &str| Err(ReviewError::Failed(message.into()));
        assert_eq!(
            ClaudeCliReviewer::parse(br#"{"is_error":true,"result":"Not logged in"}"#),
            failed("Not logged in")
        );
        assert_eq!(
            ClaudeCliReviewer::parse(br#"{"is_error":false,"result":"hi"}"#),
            failed("Claude returned no structured review.")
        );
        assert_eq!(
            ClaudeCliReviewer::parse(b"oops"),
            failed("Claude returned output that is not valid JSON.")
        );
    }

    #[test]
    fn cli_arguments_allow_only_reading_git() {
        let args = ClaudeCliReviewer::arguments(&request());
        assert_eq!(args[0], "-p");
        assert!(args[1].contains("git diff --merge-base main"));
        assert!(args.contains(&"--json-schema".to_string()));
        let at = args.iter().position(|arg| arg == "--allowedTools").unwrap();
        assert_eq!(
            args[at + 1..],
            ["Bash(git diff:*)", "Bash(git log:*)", "Bash(git show:*)"]
        );
    }

    #[test]
    fn cli_arguments_carry_the_chosen_model_and_effort() {
        let mut chosen = request();
        chosen.model = ReviewModel::new(ModelName::Sonnet, Effort::Medium);
        let args = ClaudeCliReviewer::arguments(&chosen);
        let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("--model"), "sonnet");
        assert_eq!(after("--effort"), "medium");
    }

    #[test]
    fn the_schema_is_valid_json() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(schema["required"][0], "summary");
    }

    #[test]
    fn a_fake_cli_run_returns_the_parsed_review() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_claude(
            dir.path(),
            r#"echo '{"is_error":false,"structured_output":{"summary":"ok","findings":[]}}'"#,
        );
        let mut request = request();
        request.repo = dir.path().to_path_buf();
        let result = cli(binary).review(&request, &CancelToken::new()).unwrap();
        assert_eq!(result.summary, "ok");
    }

    #[test]
    fn a_failing_cli_reports_stderr_then_the_status() {
        let dir = tempfile::tempdir().unwrap();
        let mut request = request();
        request.repo = dir.path().to_path_buf();
        let with_message = fake_claude(dir.path(), "echo 'Not logged in' >&2; exit 1");
        assert_eq!(
            cli(with_message).review(&request, &CancelToken::new()),
            Err(ReviewError::Failed("Not logged in".into()))
        );
        let silent = fake_claude(dir.path(), "exit 3");
        assert_eq!(
            cli(silent).review(&request, &CancelToken::new()),
            Err(ReviewError::Failed("claude exited with status 3.".into()))
        );
    }

    #[test]
    fn cancelling_kills_the_cli_process() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("pid");
        let binary = fake_claude(dir.path(), &format!("echo $$ > '{}'\nexec sleep 30", marker.display()));
        let mut request = request();
        request.repo = dir.path().to_path_buf();
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        let ready = marker.clone();
        let waiter = std::thread::spawn(move || {
            while std::fs::read_to_string(&ready).map_or(true, |pid| !pid.ends_with('\n')) {
                std::thread::sleep(Duration::from_millis(20));
            }
            trigger.cancel();
        });
        let started = Instant::now();
        let result = cli(binary).review(&request, &cancel);
        waiter.join().unwrap();
        assert_eq!(result, Err(ReviewError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid = std::fs::read_to_string(&marker).unwrap().trim().to_string();
        std::thread::sleep(Duration::from_millis(200));
        let alive = Command::new("kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!alive.success(), "process {pid} still runs");
    }

    #[test]
    fn cancelling_lets_the_cli_stop_its_own_children() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("child");
        let binary = fake_claude(
            dir.path(),
            &format!(
                "sleep 300 &\nchild=$!\necho $child > '{}'\ntrap 'kill $child; exit 143' TERM\nwait $child",
                marker.display()
            ),
        );
        let mut request = request();
        request.repo = dir.path().to_path_buf();
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        let ready = marker.clone();
        let waiter = std::thread::spawn(move || {
            while std::fs::read_to_string(&ready).map_or(true, |pid| !pid.ends_with('\n')) {
                std::thread::sleep(Duration::from_millis(20));
            }
            trigger.cancel();
        });
        assert_eq!(cli(binary).review(&request, &cancel), Err(ReviewError::Cancelled));
        waiter.join().unwrap();
        let pid = std::fs::read_to_string(&marker).unwrap().trim().to_string();
        std::thread::sleep(Duration::from_millis(200));
        let alive = Command::new("kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        if alive.success() {
            let _ = Command::new("kill").args(["-9", &pid]).status();
        }
        assert!(!alive.success(), "the CLI's child {pid} still runs");
    }

    #[test]
    fn cancelling_kills_children_that_ignore_sigterm() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("child");
        let binary = fake_claude(
            dir.path(),
            &format!(
                "trap '' TERM\nsh -c \"trap '' TERM; sleep 300\" &\necho $! > '{}'\nwait",
                marker.display()
            ),
        );
        let mut request = request();
        request.repo = dir.path().to_path_buf();
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        let ready = marker.clone();
        let waiter = std::thread::spawn(move || {
            while std::fs::read_to_string(&ready).map_or(true, |pid| !pid.ends_with('\n')) {
                std::thread::sleep(Duration::from_millis(20));
            }
            trigger.cancel();
        });
        let started = Instant::now();
        assert_eq!(cli(binary).review(&request, &cancel), Err(ReviewError::Cancelled));
        waiter.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid = std::fs::read_to_string(&marker).unwrap().trim().to_string();
        std::thread::sleep(Duration::from_millis(200));
        let alive = Command::new("kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        if alive.success() {
            let _ = Command::new("kill").args(["-9", &pid]).status();
        }
        assert!(!alive.success(), "the child {pid} that ignores SIGTERM still runs");
    }

    #[test]
    fn a_missing_binary_is_not_found_or_a_spawn_failure() {
        let reviewer = cli(PathBuf::from("/nonexistent/claude"));
        assert!(matches!(
            reviewer.review(&request(), &CancelToken::new()),
            Err(ReviewError::Failed(_))
        ));
    }

    #[test]
    fn the_scripted_reviewer_returns_its_result_or_its_failure() {
        let quick = ScriptedReviewer {
            delay: Duration::ZERO,
            ..ScriptedReviewer::default()
        };
        assert_eq!(quick.review(&request(), &CancelToken::new()).unwrap().findings.len(), 4);
        let failing = ScriptedReviewer {
            delay: Duration::ZERO,
            failure: Some("No network".into()),
            ..ScriptedReviewer::default()
        };
        assert_eq!(
            failing.review(&request(), &CancelToken::new()),
            Err(ReviewError::Failed("No network".into()))
        );
    }

    #[test]
    fn cancelling_stops_the_scripted_reviewer_and_drops_its_result() {
        let slow = ScriptedReviewer {
            delay: Duration::from_secs(30),
            ..ScriptedReviewer::default()
        };
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            trigger.cancel();
        });
        let started = Instant::now();
        assert_eq!(slow.review(&request(), &cancel), Err(ReviewError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_review_delay_comes_from_the_environment_value() {
        assert_eq!(scripted_delay(Some("0.25")), Duration::from_millis(250));
        assert_eq!(scripted_delay(Some("0")), Duration::ZERO);
        assert_eq!(scripted_delay(Some("soon")), DEFAULT_REVIEW_DELAY);
        assert_eq!(scripted_delay(Some("-1")), DEFAULT_REVIEW_DELAY);
        assert_eq!(scripted_delay(None), DEFAULT_REVIEW_DELAY);
    }

    #[test]
    fn the_scripted_reviewer_is_picked_only_when_asked_for() {
        let scripted = reviewer_from(Some("scripted"), Some("0"));
        assert!(scripted.review(&request(), &CancelToken::new()).is_ok());
        let cli = reviewer_from(None, None);
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert_eq!(cli.review(&request(), &cancelled), Err(ReviewError::Cancelled));
    }

    fn thread() -> ReviewThread {
        ThreadStore::new().create(ThreadAnchor::file("a.ts"))
    }

    #[test]
    fn the_scripted_replier_streams_the_text_word_by_word() {
        let replier = ScriptedReplier {
            text: "one two three".into(),
            interval: Duration::ZERO,
        };
        let receiver = replier.reply(&thread(), &CancelToken::new());
        let mut chunks = Vec::new();
        while let Ok(chunk) = receiver.recv_blocking() {
            chunks.push(chunk);
        }
        assert_eq!(chunks, ["one ", "two ", "three "]);
    }

    #[test]
    fn cancelling_closes_the_reply_stream() {
        let replier = ScriptedReplier {
            text: "a b c d e f g".into(),
            interval: Duration::from_secs(30),
        };
        let cancel = CancelToken::new();
        let receiver = replier.reply(&thread(), &cancel);
        assert_eq!(receiver.recv_blocking().unwrap(), "a ");
        cancel.cancel();
        assert!(receiver.recv_blocking().is_err());
    }

    #[test]
    fn the_cli_replier_streams_what_the_process_prints() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_claude(dir.path(), "printf 'héllo '; printf 'world'");
        let replier = ClaudeCliReplier {
            binary: Some(binary),
            ..ClaudeCliReplier::new(dir.path())
        };
        let receiver = replier.reply(&thread(), &CancelToken::new());
        let mut text = String::new();
        while let Ok(chunk) = receiver.recv_blocking() {
            text.push_str(&chunk);
        }
        assert_eq!(text, "héllo world");
    }

    #[test]
    fn a_cancelled_cli_reply_kills_the_process_and_closes_the_stream() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_claude(dir.path(), "exec sleep 30");
        let replier = ClaudeCliReplier {
            binary: Some(binary),
            ..ClaudeCliReplier::new(dir.path())
        };
        let cancel = CancelToken::new();
        let receiver = replier.reply(&thread(), &cancel);
        std::thread::sleep(Duration::from_millis(100));
        cancel.cancel();
        let started = Instant::now();
        assert!(receiver.recv_blocking().is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn the_reply_prompt_names_the_place_and_the_conversation() {
        let mut store = ThreadStore::new();
        let line = store.create(ThreadAnchor::line("src/a.ts", Side::New, 7));
        store.post(line.id, "Why this?");
        let prompt = reply_prompt(store.get(line.id).unwrap());
        assert!(prompt.contains("`src/a.ts` line 7 on the new side of the diff"));
        assert!(prompt.contains("You: Why this?"));

        let found = store.add_finding(
            ThreadAnchor::Change,
            Some(crate::review::FindingMeta {
                severity: Severity::Nit,
                title: "Name".into(),
                location: None,
            }),
            "Rename it.",
        );
        let prompt = reply_prompt(&found);
        assert!(prompt.contains("the change as a whole"));
        assert!(prompt.contains("Review finding: Name"));
        assert!(prompt.contains("Claude: Rename it."));
    }

    #[test]
    fn the_reply_prompt_skips_the_comment_still_streaming() {
        let mut store = ThreadStore::new();
        let id = store.create(ThreadAnchor::Change).id;
        store.begin_claude_reply(id);
        store.append_claude(id, "partial", false);
        let thread = store.get(id).unwrap().clone();
        assert!(!reply_prompt(&thread).contains("partial"));
    }

    #[test]
    fn utf8_split_across_reads_is_rejoined() {
        let bytes = "é".as_bytes();
        let mut decoder = Utf8Chunks::default();
        assert_eq!(decoder.push(&bytes[..1]), "");
        assert_eq!(decoder.push(&bytes[1..]), "é");
        assert_eq!(decoder.push(&[0xff]), "\u{fffd}");
    }
}
