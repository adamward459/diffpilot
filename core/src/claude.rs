use crate::cancel::CancelToken;
use crate::paths::find_executable;
use crate::process_groups;
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

pub const SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"},"findings":{"type":"array","items":{"type":"object","properties":{"path":{"type":"string"},"line":{"type":["integer","null"]},"severity":{"type":"string","enum":["must-fix","suggestion","nit","question"]},"title":{"type":"string"},"body":{"type":"string"}},"required":["path","severity","title","body"]}}},"required":["summary","findings"]}"#;

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

#[cfg(test)]
mod tests {
    use super::*;
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
}
