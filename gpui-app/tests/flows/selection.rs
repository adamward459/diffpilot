use diffpilot::testing::{click, clipboard, drag_below, fixture, git, open_repo, wait_until_shows, write};
use diffpilot_core::editor::Editor;
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::TestAppContext;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[gpui::test]
fn dragging_over_lines_copies_them_and_opens_the_first_one(app: &mut TestAppContext) {
    let dir = fixture();
    write(dir.path(), "docs/notes.txt", "one\ntwo\n");
    git(dir.path(), &["add", "docs/notes.txt"]);
    git(dir.path(), &["commit", "-q", "-m", "notes"]);
    let tools = tempfile::tempdir().expect("tempdir");
    let record = tools.path().join("opened");
    let editor = tools.path().join("editor");
    std::fs::write(
        &editor,
        format!("#!/bin/sh\nprintf '%s' \"$1:$2\" > '{}'\n", record.display()),
    )
    .expect("script");
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let settings = Arc::new(Settings::in_memory());
    settings.set_editor(Some(Editor::Custom(format!("{} {{file}} {{line}}", editor.display()))));
    let (_, cx) = open_repo(app, dir.path(), settings, Arc::new(ViewedStore::in_memory()));
    wait_until_shows(cx, "pane:hunk:docs/notes.txt:0");

    drag_below(cx, "pane:hunk:docs/notes.txt:0", 10.0, 30.0);
    wait_until_shows(cx, "pane:selection-hud:docs/notes.txt:2");
    click(cx, "pane:selection-copy");
    assert_eq!(clipboard(cx).as_deref(), Some("one\ntwo"));

    click(cx, "pane:selection-open");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !record.exists() {
        assert!(Instant::now() < deadline, "the editor never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let opened = std::fs::read_to_string(&record).expect("record");
    assert!(
        opened.ends_with("docs/notes.txt:1"),
        "the editor opens the first selected line: {opened}"
    );
}
