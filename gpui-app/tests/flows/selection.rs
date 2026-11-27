use diffpilot::diff_pane::layout::GUTTER;
use diffpilot::testing::{
    click, click_below, clipboard, drag_below, fixture, git, git_output, open_repo, press, wait_until,
    wait_until_shows, write,
};
use diffpilot_core::editor::Editor;
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::TestAppContext;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[gpui::test]
fn dragging_over_the_numbers_selects_lines_to_copy_and_open_the_first_one(app: &mut TestAppContext) {
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

    drag_below(cx, "pane:hunk:docs/notes.txt:0", (10.0, 10.0), (10.0, 30.0));
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

const CELL: f32 = 7.2;
const SECOND_ROW: f32 = 30.0;

fn open_edit(app: &mut TestAppContext) -> (tempfile::TempDir, &'static mut gpui::VisualTestContext) {
    let dir = fixture();
    write(dir.path(), "base.txt", "base\nlet alpha = beta;\nmore\n");
    let (_, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
    );
    wait_until_shows(cx, "sidebar:row:base.txt");
    click(cx, "sidebar:row:base.txt");
    wait_until_shows(cx, "pane:hunk:base.txt:0");
    (dir, cx)
}

fn code(column: f32) -> f32 {
    GUTTER + column * CELL
}

#[gpui::test]
fn clicking_a_line_selects_it_and_shows_the_action_bar(app: &mut TestAppContext) {
    let (_dir, cx) = open_edit(app);
    click_below(cx, "pane:hunk:base.txt:0", (code(3.0), SECOND_ROW), 1);
    wait_until_shows(cx, "pane:selection-hud:base.txt:1");
}

#[gpui::test]
fn clicking_a_hunk_header_selects_the_hunk_and_copies_its_patch(app: &mut TestAppContext) {
    let (_dir, cx) = open_edit(app);
    click(cx, "pane:hunk:base.txt:0");
    wait_until_shows(cx, "pane:selection-hud:base.txt:3");
    click(cx, "pane:selection-copy");
    assert_eq!(
        clipboard(cx).as_deref(),
        Some("@@ -1 +1,3 @@\n base\n+let alpha = beta;\n+more")
    );
}

#[gpui::test]
fn dragging_over_code_selects_characters_and_command_c_copies_them(app: &mut TestAppContext) {
    let (_dir, cx) = open_edit(app);
    drag_below(
        cx,
        "pane:hunk:base.txt:0",
        (code(4.0), SECOND_ROW),
        (code(9.0), SECOND_ROW),
    );
    press(cx, "cmd-c");
    assert_eq!(clipboard(cx).as_deref(), Some("alpha"));
}

#[gpui::test]
fn dragging_over_code_can_span_lines(app: &mut TestAppContext) {
    let (_dir, cx) = open_edit(app);
    drag_below(
        cx,
        "pane:hunk:base.txt:0",
        (code(12.0), SECOND_ROW),
        (code(2.0), SECOND_ROW + 20.0),
    );
    press(cx, "cmd-c");
    assert_eq!(clipboard(cx).as_deref(), Some("beta;\nmo"));
}

#[gpui::test]
fn double_clicking_a_word_selects_it(app: &mut TestAppContext) {
    let (_dir, cx) = open_edit(app);
    click_below(cx, "pane:hunk:base.txt:0", (code(6.5), SECOND_ROW), 2);
    press(cx, "cmd-c");
    assert_eq!(clipboard(cx).as_deref(), Some("alpha"));
}

#[gpui::test]
fn staging_and_unstaging_act_on_the_selected_line_only(app: &mut TestAppContext) {
    let (dir, cx) = open_edit(app);
    let staged = || git_output(dir.path(), &["diff", "--cached", "--", "base.txt"]);

    click_below(cx, "pane:hunk:base.txt:0", (code(3.0), SECOND_ROW), 1);
    click(cx, "pane:selection-stage");
    wait_until(cx, "staging the line", || staged().contains("+let alpha = beta;"));
    assert!(!staged().contains("+more"), "only the selected line is staged");

    click_below(cx, "pane:hunk:base.txt:0", (code(3.0), SECOND_ROW), 1);
    click(cx, "pane:selection-unstage");
    wait_until(cx, "unstaging the line", || staged().is_empty());
}

#[gpui::test]
fn discarding_asks_first_and_then_removes_the_selected_line(app: &mut TestAppContext) {
    let (dir, cx) = open_edit(app);
    let file = dir.path().join("base.txt");

    click_below(cx, "pane:hunk:base.txt:0", (code(3.0), SECOND_ROW), 1);
    click(cx, "pane:selection-discard");
    click(cx, "pane:selection-discard-cancel");
    assert_eq!(
        std::fs::read_to_string(&file).expect("read"),
        "base\nlet alpha = beta;\nmore\n",
        "cancel keeps the line"
    );

    click(cx, "pane:selection-discard");
    click(cx, "pane:selection-discard-confirm");
    wait_until(cx, "discarding the line", || {
        std::fs::read_to_string(&file).is_ok_and(|text| text == "base\nmore\n")
    });
}
