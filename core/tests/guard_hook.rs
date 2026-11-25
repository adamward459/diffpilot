use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

struct Outcome {
    code: i32,
    stderr: String,
}

fn hook(cwd: &Path, tool: &str, command: &str) -> Outcome {
    let input = json!({
        "session_id": "s",
        "hook_event_name": "PreToolUse",
        "cwd": cwd,
        "tool_name": tool,
        "tool_input": { "command": command },
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffpilot-guard"))
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    Outcome {
        code: output.status.code().unwrap(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "alias.ship", "push origin HEAD"]);
    git(&["config", "alias.sh", "!git push"]);
    git(&["config", "alias.st", "status -sb"]);
    dir
}

#[test]
fn the_hook_command_blocks_every_way_to_push_with_exit_code_2() {
    let dir = repo();
    let pushes = [
        "git push",
        "git push origin main",
        "git -C . push origin HEAD",
        "git -c user.name=x push --force",
        "sh -c 'git push'",
        "bash -lc \"git -C . push\"",
        "env GIT_TRACE=1 git push",
        "echo origin | xargs git push",
        "cd sub && git push",
        "git ship",
        "git sh",
        "python3 -c \"import os; os.system('git push')\"",
    ];
    for command in pushes {
        let outcome = hook(dir.path(), "Bash", command);
        assert_eq!(outcome.code, 2, "should block {command:?}");
        assert!(outcome.stderr.starts_with("[diffpilot guard]"), "{}", outcome.stderr);
    }
    assert!(
        hook(dir.path(), "Bash", "git push")
            .stderr
            .contains("git push is not allowed")
    );
}

#[test]
fn the_hook_command_lets_safe_commands_through_with_exit_code_0() {
    let dir = repo();
    for command in [
        "git status",
        "git st",
        "git log --oneline",
        "git diff --merge-base main",
        "ls",
        "cargo test",
    ] {
        let outcome = hook(dir.path(), "Bash", command);
        assert_eq!(outcome.code, 0, "should allow {command:?}: {}", outcome.stderr);
        assert_eq!(outcome.stderr, "");
    }
    assert_eq!(hook(dir.path(), "Edit", "git push").code, 0);
}

#[test]
fn garbage_on_stdin_is_blocked() {
    let dir = repo();
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffpilot-guard"))
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not json").unwrap();
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(2));
}
