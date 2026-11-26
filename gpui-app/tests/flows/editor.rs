use diffpilot::testing::{canned_claude, click, fixture, open_repo, wait_until_shows};
use diffpilot_core::editor::Editor;
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::TestAppContext;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[gpui::test]
fn the_open_in_editor_button_starts_the_editor_on_the_file(app: &mut TestAppContext) {
    let dir = fixture();
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

    let (_, cx) = open_repo(
        app,
        dir.path(),
        settings,
        Arc::new(ViewedStore::in_memory()),
        canned_claude(),
    );
    wait_until_shows(cx, "pane:open-in-editor:docs/committed.md");
    click(cx, "pane:open-in-editor:docs/committed.md");

    let deadline = Instant::now() + Duration::from_secs(10);
    while !record.exists() {
        assert!(Instant::now() < deadline, "the editor never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let opened = std::fs::read_to_string(&record).expect("record");
    assert!(
        opened.ends_with("docs/committed.md:1"),
        "the editor opens the first changed line: {opened}"
    );
}
