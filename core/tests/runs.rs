mod common;

use common::*;
use diffpilot_core::changes::FileStatus;
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::{Effort, ModelName, ReviewModel};
use diffpilot_core::run::{LogKind, RunKind, RunOutcome, RunState, StepState};
use diffpilot_core::runner::{EngineError, RunEvent, RunRequest};
use diffpilot_core::snapshot::{self, Until};
use diffpilot_core::store::Store;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const WITHIN: Duration = Duration::from_secs(30);

fn model() -> ReviewModel {
    ReviewModel::new(ModelName::Sonnet, Effort::Low)
}

fn request(repo: &Path, kind: RunKind) -> RunRequest {
    RunRequest::new(repo, key(repo), kind, "Fixing 1 comment", "Rename x to count.", model())
}

/// Replays `FIX_STREAM` and records what it was started with. When `edit` is set it also edits a.ts.
fn recording_claude(work: &Path, repo: &Path, edit: bool) -> std::path::PathBuf {
    let stream = work.join("stream.jsonl");
    let real = std::fs::canonicalize(repo).unwrap();
    std::fs::write(&stream, FIX_STREAM.replace("REPO", &real.to_string_lossy())).unwrap();
    let out = work.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let edit = if edit {
        "printf 'const count = 1;\\nexport { count };\\n' > a.ts; echo new > created.txt"
    } else {
        ""
    };
    fake_claude(
        work,
        &format!(
            r#"
for arg in "$@"; do printf '%s\n' "$arg" >> '{out}/args.txt'; done
while [ $# -gt 0 ]; do
  if [ "$1" = "--settings" ]; then cp "$2" '{out}/settings.json'; fi
  shift
done
pwd > '{out}/cwd.txt'
{edit}
cat '{stream}'
"#,
            out = out.display(),
            stream = stream.display(),
        ),
    )
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn a_run_streams_its_log_plan_and_result_and_is_saved() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine(store, recording_claude(work.path(), repo.path(), false), work.path());
    let rx = engine.subscribe();

    let id = engine.submit(request(repo.path(), RunKind::MakeChanges)).unwrap();
    let (state, events) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done);

    assert_eq!(events.first(), Some(&RunEvent::Queued { run: id }));
    assert!(events.contains(&RunEvent::Started { run: id }));
    let kinds: Vec<LogKind> = events
        .iter()
        .filter_map(|event| match event {
            RunEvent::Log { entry, .. } => Some(entry.kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [
            LogKind::Thinking,
            LogKind::Plan,
            LogKind::Message,
            LogKind::Read,
            LogKind::Edit
        ]
    );

    let run = engine.run(id).unwrap();
    assert_eq!(run.state, RunState::Done);
    assert_eq!(run.outcome, Some(RunOutcome::Text("Renamed x to count.".into())));
    assert_eq!(run.num_turns, Some(4));
    assert_eq!(
        run.claude_session.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    let steps: Vec<_> = run.steps.iter().map(|step| (step.title.as_str(), step.state)).collect();
    assert_eq!(
        steps,
        [("Renaming x", StepState::Active), ("Check callers", StepState::Pending)]
    );
    assert!(run.started_at_ms.is_some() && run.finished_at_ms.is_some());
    assert_eq!(
        run.log.iter().find(|entry| entry.kind == LogKind::Read).unwrap().text,
        "Read a.ts"
    );

    let past = engine.past_runs(&key(repo.path()), 10).unwrap();
    assert_eq!(past.len(), 1);
    assert_eq!(
        (past[0].id, past[0].state, past[0].kind),
        (id, RunState::Done, RunKind::MakeChanges)
    );
    assert_eq!(engine.store().load_run(id).unwrap().unwrap().log.len(), 5);
}

#[test]
fn every_run_is_started_with_the_deny_list_the_hook_and_the_chosen_model() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine(store, recording_claude(work.path(), repo.path(), false), work.path());
    let rx = engine.subscribe();

    let id = engine.submit(request(repo.path(), RunKind::FixComments)).unwrap();
    wait_finished(&rx, id, WITHIN);

    let args = lines(&work.path().join("out/args.txt"));
    let after = |flag: &str| args[args.iter().position(|arg| arg == flag).unwrap() + 1].clone();
    assert_eq!(args[0], "-p");
    assert_eq!(args[1], "Rename x to count.");
    assert_eq!(after("--output-format"), "stream-json");
    assert!(args.contains(&"--verbose".to_string()));
    assert!(args.contains(&"--strict-mcp-config".to_string()));
    assert_eq!(
        (after("--model").as_str(), after("--effort").as_str()),
        ("sonnet", "low")
    );
    assert_eq!(after("--permission-mode"), "acceptEdits");
    let deny = args.iter().position(|arg| arg == "--disallowedTools").unwrap();
    assert_eq!(
        args[deny + 1..deny + 4],
        ["Bash(git push:*)", "Bash(git commit:*)", "Bash(gh:*)"]
    );

    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(work.path().join("out/settings.json")).unwrap()).unwrap();
    let command = settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.contains("diffpilot-guard"), "{command}");
    assert_eq!(settings["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    assert!(
        !work.path().join("runs/run-1").exists(),
        "the run folder is removed at the end"
    );
    let cwd = std::fs::read_to_string(work.path().join("out/cwd.txt")).unwrap();
    assert_eq!(
        std::fs::canonicalize(cwd.trim()).unwrap(),
        std::fs::canonicalize(repo.path()).unwrap()
    );
}

#[test]
fn a_review_run_returns_structured_findings() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let claude = fake_claude(
        work.path(),
        r#"cat <<'EOF'
{"type":"system","subtype":"init","session_id":"s","model":"m","tools":[]}
{"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"","structured_output":{"summary":"One bug.","findings":[{"path":"a.ts","line":1,"severity":"must-fix","title":"Null","body":"Check it."}]}}
EOF"#,
    );
    let engine = engine(Arc::new(Store::open_in_memory().unwrap()), claude, work.path());
    let rx = engine.subscribe();
    let review =
        diffpilot_core::review::ReviewRequest::new(repo.path(), vec!["--merge-base".into(), "main".into()], "Tests");
    let id = engine
        .submit(RunRequest::review(&review, key(repo.path()), "Reviewing"))
        .unwrap();
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done);
    let Some(RunOutcome::Review(result)) = engine.run(id).unwrap().outcome else {
        panic!("no review");
    };
    assert_eq!(result.summary, "One bug.");
    assert_eq!(result.findings[0].line, Some(1));
}

#[test]
fn a_run_that_reports_an_error_fails_with_its_message() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let claude = fake_claude(
        work.path(),
        r#"echo '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Not logged in. Run /login."}'; exit 1"#,
    );
    let engine = engine(Arc::new(Store::open_in_memory().unwrap()), claude, work.path());
    let rx = engine.subscribe();
    let id = engine.submit(request(repo.path(), RunKind::MakeChanges)).unwrap();
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Failed);
    assert_eq!(
        engine.run(id).unwrap().error.as_deref(),
        Some("Not logged in. Run /login.")
    );
}

#[test]
fn a_run_that_dies_without_output_fails_with_the_stderr_text() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let claude = fake_claude(work.path(), "echo 'boom: bad flag' >&2; exit 3");
    let engine = engine(Arc::new(Store::open_in_memory().unwrap()), claude, work.path());
    let rx = engine.subscribe();
    let id = engine.submit(request(repo.path(), RunKind::MakeChanges)).unwrap();
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Failed);
    assert_eq!(engine.run(id).unwrap().error.as_deref(), Some("boom: bad flag"));
}

#[test]
fn stop_ends_the_process_group_and_marks_the_run_stopped() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let pid_file = work.path().join("child.pid");
    let claude = fake_claude(
        work.path(),
        &format!(
            "sleep 60 & echo $! > '{}'\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s\"}}'\nwait",
            pid_file.display()
        ),
    );
    let engine = engine(Arc::new(Store::open_in_memory().unwrap()), claude, work.path());
    let rx = engine.subscribe();
    let id = engine.submit(request(repo.path(), RunKind::MakeChanges)).unwrap();
    wait_for("the child to start", WITHIN, || {
        pid_file.exists() && !std::fs::read_to_string(&pid_file).unwrap().is_empty()
    });
    let child: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
    assert_eq!(engine.active(repo.path()).unwrap().state, RunState::Running);

    assert!(engine.stop(id));
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Stopped);
    wait_for("the child to die", WITHIN, || unsafe { libc_kill(child) } != 0);
    assert!(!engine.stop(id), "a finished run cannot be stopped again");
    assert_eq!(
        engine.past_runs(&key(repo.path()), 5).unwrap()[0].state,
        RunState::Stopped
    );
}

unsafe fn libc_kill(pid: i32) -> i32 {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, 0) }
}

#[test]
fn a_second_run_in_the_same_repo_waits_and_a_queued_run_can_be_stopped() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let gate = work.path().join("gate");
    let claude = fake_claude(
        work.path(),
        &format!(
            "while [ ! -e '{}' ]; do sleep 0.02; done\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\"}}'",
            gate.display()
        ),
    );
    let engine = engine(Arc::new(Store::open_in_memory().unwrap()), claude, work.path());
    let rx = engine.subscribe();
    let first = engine.submit(request(repo.path(), RunKind::MakeChanges)).unwrap();
    let second = engine.submit(request(repo.path(), RunKind::MakeChanges)).unwrap();
    let third = engine.submit(request(repo.path(), RunKind::MakeChanges)).unwrap();

    assert_eq!(engine.run(second).unwrap().state, RunState::Queued);
    assert_eq!(
        engine.queued(repo.path()).iter().map(|run| run.id).collect::<Vec<_>>(),
        [second, third]
    );
    assert!(engine.stop(third));
    assert_eq!(engine.run(third).unwrap().state, RunState::Stopped);
    assert_eq!(engine.queued(repo.path()).len(), 1);

    std::fs::write(&gate, "").unwrap();
    let (state, events) = wait_finished(&rx, second, WITHIN);
    assert_eq!(state, RunState::Done);
    let order: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            RunEvent::Finished { run, .. } => Some(*run),
            _ => None,
        })
        .collect();
    assert_eq!(order, [third, first, second]);
    assert!(engine.active(repo.path()).is_none());
}

#[test]
fn runs_in_different_repos_go_in_parallel() {
    let (one, two) = (temp_repo(), temp_repo());
    let work = tempfile::tempdir().unwrap();
    let gate = work.path().join("gate");
    let claude = fake_claude(
        work.path(),
        &format!(
            "while [ ! -e '{}' ]; do sleep 0.02; done\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\"}}'",
            gate.display()
        ),
    );
    let engine = engine(Arc::new(Store::open_in_memory().unwrap()), claude, work.path());
    let a = engine.submit(request(one.path(), RunKind::MakeChanges)).unwrap();
    let b = engine.submit(request(two.path(), RunKind::MakeChanges)).unwrap();
    wait_for("both to run", WITHIN, || {
        engine.run(a).unwrap().state == RunState::Running && engine.run(b).unwrap().state == RunState::Running
    });
    std::fs::write(&gate, "").unwrap();
}

#[test]
fn an_editing_run_is_snapshotted_and_its_changes_can_be_shown_and_undone() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("mine.txt"), "my own file\n").unwrap();
    let engine = engine(
        Arc::new(Store::open_in_memory().unwrap()),
        recording_claude(work.path(), repo.path(), true),
        work.path(),
    );
    let rx = engine.subscribe();
    let id = engine.submit(request(repo.path(), RunKind::FixComments)).unwrap();
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done);

    let run = engine.run(id).unwrap();
    let changed: Vec<_> = run
        .changed_files
        .iter()
        .map(|file| (file.path.as_str(), file.status))
        .collect();
    assert_eq!(
        changed,
        [("a.ts", FileStatus::Modified), ("created.txt", FileStatus::Added)]
    );
    let taken = run.snapshot.clone().unwrap();
    let diff = snapshot::diff(repo.path(), &taken, Until::RunEnd, &["a.ts".to_string()]).unwrap();
    assert_eq!(diff.len(), 1);
    assert!(git(repo.path(), &["for-each-ref", "refs/diffpilot"]).contains("refs/diffpilot/runs/1/before"));

    let stored = engine.store().load_run(id).unwrap().unwrap();
    assert_eq!(stored.snapshot, Some(taken.clone()));
    let shown = engine.show_change(id, &[]).unwrap();
    assert_eq!(
        shown.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(),
        ["a.ts", "created.txt"]
    );
    let report = engine.undo_run(id, false).unwrap();
    assert!(report.is_clean());
    assert!(engine.run(id).unwrap().undone_at_ms.is_some());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.ts")).unwrap(),
        "const x = 1;\nexport { x };\n"
    );
    assert!(!repo.path().join("created.txt").exists());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("mine.txt")).unwrap(),
        "my own file\n"
    );
}

#[test]
fn a_review_does_not_snapshot() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let claude = fake_claude(
        work.path(),
        r#"echo '{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":{"summary":"ok","findings":[]}}'"#,
    );
    let engine = engine(Arc::new(Store::open_in_memory().unwrap()), claude, work.path());
    let rx = engine.subscribe();
    let review = diffpilot_core::review::ReviewRequest::new(repo.path(), vec!["HEAD".into()], "x");
    let id = engine
        .submit(RunRequest::review(&review, key(repo.path()), "Reviewing"))
        .unwrap();
    wait_finished(&rx, id, WITHIN);
    assert!(engine.run(id).unwrap().snapshot.is_none());
    assert_eq!(git(repo.path(), &["for-each-ref", "refs/diffpilot"]), "");
}

#[test]
fn a_missing_or_broken_hook_stops_every_run_before_it_starts() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let claude = fake_claude(work.path(), "echo hi");
    let make = |guard: &Path| {
        let mut config = diffpilot_core::runner::EngineConfig::new(Arc::new(Store::open_in_memory().unwrap()));
        config.claude_binary = Some(claude.clone());
        config.guard_binary = guard.to_path_buf();
        config.run_dir = work.path().join("runs");
        diffpilot_core::runner::RunEngine::new(config)
    };
    let missing = make(Path::new("/nonexistent/diffpilot-guard"));
    assert!(matches!(
        missing.submit(request(repo.path(), RunKind::MakeChanges)),
        Err(EngineError::GuardMissing(_))
    ));
    let lenient = fake_claude(work.path(), "exit 0");
    let always_allows = make(&lenient);
    assert!(matches!(
        always_allows.submit(request(repo.path(), RunKind::MakeChanges)),
        Err(EngineError::GuardMissing(_))
    ));
    assert!(missing.past_runs(&key(repo.path()), 5).unwrap().is_empty());
}

#[test]
fn asking_before_edits_needs_the_mcp_server_which_this_engine_does_not_have_yet() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let engine = engine(
        Arc::new(Store::open_in_memory().unwrap()),
        fake_claude(work.path(), "echo"),
        work.path(),
    );
    let mut ask = request(repo.path(), RunKind::FixComments);
    ask.edit_permission = EditPermission::AskOnce;
    assert!(matches!(engine.submit(ask), Err(EngineError::Policy(_))));
}

#[test]
fn a_missing_claude_is_reported_before_anything_is_queued() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let engine = engine(
        Arc::new(Store::open_in_memory().unwrap()),
        work.path().join("no-such-claude"),
        work.path(),
    );
    let outcome = engine.submit(request(repo.path(), RunKind::MakeChanges));
    assert!(matches!(outcome, Err(EngineError::ClaudeNotFound)) || outcome.is_ok());
}

#[test]
fn runs_left_going_by_a_quit_app_are_failed_on_the_next_start() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = store.session(&key(repo.path())).unwrap();
    let id = store.insert_run(session, RunKind::MakeChanges, "Old").unwrap();
    let mut run = diffpilot_core::run::Run::new(id, session, repo.path(), RunKind::MakeChanges, "Old", model(), 0);
    run.state = RunState::Running;
    store.save_run(&run).unwrap();
    let engine = engine(store, fake_claude(work.path(), "echo"), work.path());
    let loaded = engine.run(id).unwrap();
    assert_eq!(loaded.state, RunState::Failed);
    assert_eq!(loaded.error.as_deref(), Some("diffpilot quit during this run."));
}

#[test]
fn only_the_newest_editing_runs_keep_their_snapshot_refs() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let mut config = config(
        Arc::new(Store::open_in_memory().unwrap()),
        recording_claude(work.path(), repo.path(), false),
        work.path(),
    );
    config.keep_snapshots = 2;
    let engine = diffpilot_core::runner::RunEngine::new(config);
    let rx = engine.subscribe();
    let refs = || git(repo.path(), &["for-each-ref", "--format=%(refname)", "refs/diffpilot"]);
    let first = engine.submit(request(repo.path(), RunKind::FixComments)).unwrap();
    wait_finished(&rx, first, WITHIN);
    let second = engine.submit(request(repo.path(), RunKind::FixComments)).unwrap();
    wait_finished(&rx, second, WITHIN);
    assert!(refs().contains("runs/1/before") && refs().contains("runs/2/before"));
    let third = engine.submit(request(repo.path(), RunKind::FixComments)).unwrap();
    wait_finished(&rx, third, WITHIN);
    assert!(!refs().contains("runs/1/"), "{}", refs());
    assert!(refs().contains("runs/2/before") && refs().contains("runs/3/before"));
}
