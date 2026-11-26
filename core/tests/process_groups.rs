mod common;

use common::*;
use diffpilot_core::process_groups;
use diffpilot_core::review::{Effort, ModelName, ReviewModel};
use diffpilot_core::run::{RunKind, RunState};
use diffpilot_core::runner::RunRequest;
use diffpilot_core::store::Store;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[test]
fn quitting_stops_a_running_cli_and_clears_the_record() {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let records = work.path().join("records");
    process_groups::record_in(&records);
    let marker = work.path().join("pid");
    let claude = fake_claude(
        work.path(),
        &format!("echo $$ > '{}'\nexec sleep 300", marker.display()),
    );
    let engine = engine(Arc::new(Store::open_in_memory().unwrap()), claude, work.path());
    let rx = engine.subscribe();
    let model = ReviewModel::new(ModelName::Sonnet, Effort::Low);
    let request = RunRequest::new(
        repo.path(),
        key(repo.path()),
        RunKind::Review,
        "Review",
        "Review it.",
        model,
    );
    let id = engine.submit(request).unwrap();
    wait_for("the CLI to start", Duration::from_secs(5), || {
        std::fs::read_to_string(&marker).is_ok_and(|pid| pid.ends_with('\n'))
    });
    let pid = std::fs::read_to_string(&marker).unwrap().trim().to_string();
    let record = records.join(format!("{}.json", std::process::id()));
    assert!(std::fs::read_to_string(&record).unwrap().contains(&pid));

    let started = std::time::Instant::now();
    process_groups::stop_all();
    assert!(started.elapsed() < Duration::from_millis(100));
    wait_for("the CLI to exit", Duration::from_secs(5), || !alive(&pid));
    let (state, _) = wait_finished(&rx, id, Duration::from_secs(5));
    assert_ne!(state, RunState::Done);
    assert!(!record.exists());
}
