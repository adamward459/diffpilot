#![allow(dead_code)]

use diffpilot_core::run::{RunId, RunState};
use diffpilot_core::runner::{EngineConfig, RunEngine, RunEvent};
use diffpilot_core::store::{SessionKey, Store};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn temp_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("a.ts"), "const x = 1;\nexport { x };\n").unwrap();
    std::fs::write(dir.path().join("b.ts"), "export const b = 2;\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "base"]);
    dir
}

pub fn key(repo: &Path) -> SessionKey {
    SessionKey::new(&repo.to_string_lossy(), "main", "main")
}

/// A `claude` that runs `script` in the repo folder.
pub fn fake_claude(dir: &Path, script: &str) -> PathBuf {
    let path = dir.join("claude");
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

pub fn config(store: Arc<Store>, claude: PathBuf, work: &Path) -> EngineConfig {
    let mut config = EngineConfig::new(store);
    config.claude_binary = Some(claude);
    config.guard_binary = PathBuf::from(env!("CARGO_BIN_EXE_diffpilot-guard"));
    config.run_dir = work.join("runs");
    config
}

pub fn engine(store: Arc<Store>, claude: PathBuf, work: &Path) -> RunEngine {
    RunEngine::new(config(store, claude, work))
}

pub fn wait_finished(rx: &async_channel::Receiver<RunEvent>, id: RunId, within: Duration) -> (RunState, Vec<RunEvent>) {
    let deadline = Instant::now() + within;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        while let Ok(event) = rx.try_recv() {
            let finished = match &event {
                RunEvent::Finished { run, state } if *run == id => Some(*state),
                _ => None,
            };
            seen.push(event);
            if let Some(state) = finished {
                return (state, seen);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("run {id:?} did not finish in {within:?}; events: {seen:?}");
}

pub fn wait_for(what: &str, within: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// Recorded in the shape of `claude -p --output-format stream-json --verbose`.
pub const FIX_STREAM: &str = r#"{"type":"system","subtype":"init","cwd":"/r","session_id":"11111111-2222-3333-4444-555555555555","tools":["Read","Edit","Bash","TodoWrite"],"mcp_servers":[],"model":"claude-sonnet-5-5","permissionMode":"acceptEdits"}
{"type":"assistant","message":{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"thinking","thinking":"I need to rename x.","signature":"abc"}]},"session_id":"11111111-2222-3333-4444-555555555555"}
{"type":"assistant","message":{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"TodoWrite","input":{"todos":[{"content":"Rename x","status":"in_progress","activeForm":"Renaming x"},{"content":"Check callers","status":"pending","activeForm":"Checking callers"}]}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"Todos have been modified successfully"}]}}
{"type":"assistant","message":{"id":"msg_2","type":"message","role":"assistant","content":[{"type":"text","text":"I will rename it."},{"type":"tool_use","id":"toolu_2","name":"Read","input":{"file_path":"REPO/a.ts"}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_2","content":"const x = 1;"}]}}
{"type":"assistant","message":{"id":"msg_3","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_3","name":"Edit","input":{"file_path":"REPO/a.ts","old_string":"x","new_string":"count"}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_3","content":"The file REPO/a.ts has been updated."}]}}
{"type":"result","subtype":"success","is_error":false,"duration_ms":2100,"duration_api_ms":1800,"num_turns":4,"result":"Renamed x to count.","session_id":"11111111-2222-3333-4444-555555555555","total_cost_usd":0.012}
"#;
