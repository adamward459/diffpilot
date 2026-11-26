use crate::cancel::CancelToken;
use crate::paths::find_executable;
use crate::process_groups;
use crate::threads::{ReviewThread, ThreadAnchor};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewError {
    #[error("{0}")]
    Failed(String),
    #[error("The run was cancelled.")]
    Cancelled,
}

/// Chunks arrive on the returned channel. It closes when the reply ends, fails or is cancelled.
pub trait Replier: Send + Sync {
    fn reply(&self, thread: &ReviewThread, cancel: &CancelToken) -> async_channel::Receiver<String>;
}

pub const SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"},"findings":{"type":"array","items":{"type":"object","properties":{"path":{"type":"string"},"line":{"type":["integer","null"]},"severity":{"type":"string","enum":["must-fix","suggestion","nit","question"]},"title":{"type":"string"},"body":{"type":"string"}},"required":["path","severity","title","body"]}}},"required":["summary","findings"]}"#;

const READ_ONLY_GIT: [&str; 3] = ["Bash(git diff:*)", "Bash(git log:*)", "Bash(git show:*)"];

pub fn claude_binary() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/claude"));
    }
    candidates.push("/opt/homebrew/bin/claude".into());
    candidates.push("/usr/local/bin/claude".into());
    find_executable(&candidates, "claude")
}

pub(crate) fn claude_command(binary: &Path, repo: &Path) -> Command {
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

pub(crate) struct Finished {
    pub status: ExitStatus,
    pub stderr: Vec<u8>,
}

/// `on_stdout` returns false to stop the process early, which reports `Cancelled`.
/// On cancel the child's process group is stopped and its pipe threads are left to end on their own.
pub(crate) fn run_process(
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
    let _tracked = process_groups::Tracked::start(child.id());
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
    use crate::review::Severity;
    use crate::threads::{Side, ThreadStore};
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    fn fake_claude(dir: &Path, script: &str) -> PathBuf {
        let path = dir.join("claude");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn run(binary: PathBuf, dir: &Path, cancel: &CancelToken) -> Option<ReviewError> {
        run_process(claude_command(&binary, dir), cancel, |_| true).err()
    }

    #[test]
    fn the_schema_is_valid_json() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(schema["required"][0], "summary");
    }

    #[test]
    fn cancelling_kills_the_cli_process() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("pid");
        let binary = fake_claude(dir.path(), &format!("echo $$ > '{}'\nexec sleep 30", marker.display()));
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
        let result = run(binary, dir.path(), &cancel);
        waiter.join().unwrap();
        assert_eq!(result, Some(ReviewError::Cancelled));
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
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        let ready = marker.clone();
        let waiter = std::thread::spawn(move || {
            while std::fs::read_to_string(&ready).map_or(true, |pid| !pid.ends_with('\n')) {
                std::thread::sleep(Duration::from_millis(20));
            }
            trigger.cancel();
        });
        assert_eq!(run(binary, dir.path(), &cancel), Some(ReviewError::Cancelled));
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
        assert_eq!(run(binary, dir.path(), &cancel), Some(ReviewError::Cancelled));
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
    fn a_missing_binary_is_a_spawn_failure() {
        let missing = run(
            PathBuf::from("/nonexistent/claude"),
            Path::new("/"),
            &CancelToken::new(),
        );
        assert!(matches!(missing, Some(ReviewError::Failed(_))));
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
