use diffpilot_core::cancel::CancelToken;
use diffpilot_core::claude::{ClaudeCliReviewer, ReviewError, Reviewer};
use diffpilot_core::process_groups;
use diffpilot_core::review::ReviewRequest;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn quitting_stops_a_running_cli_and_clears_the_record() {
    let dir = tempfile::tempdir().unwrap();
    let records = dir.path().join("records");
    process_groups::record_in(&records);
    let marker = dir.path().join("pid");
    let binary = dir.path().join("claude");
    std::fs::write(
        &binary,
        format!("#!/bin/sh\necho $$ > '{}'\nexec sleep 300\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut request = ReviewRequest::new(dir.path(), vec![], "Tests");
    request.repo = dir.path().to_path_buf();
    let review = std::thread::spawn(move || {
        let reviewer = ClaudeCliReviewer {
            binary: Some(binary),
            extra_arguments: Vec::new(),
        };
        reviewer.review(&request, &CancelToken::new())
    });
    wait_for("the CLI to start", || {
        std::fs::read_to_string(&marker).is_ok_and(|pid| pid.ends_with('\n'))
    });
    let pid = std::fs::read_to_string(&marker).unwrap().trim().to_string();
    let record = records.join(format!("{}.json", std::process::id()));
    assert!(std::fs::read_to_string(&record).unwrap().contains(&pid));

    let started = Instant::now();
    process_groups::stop_all();
    assert!(started.elapsed() < Duration::from_millis(100));
    wait_for("the CLI to exit", || !alive(&pid));
    assert!(matches!(review.join().unwrap(), Err(ReviewError::Failed(_))));
    assert!(!record.exists());
}
