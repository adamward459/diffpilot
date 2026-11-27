use diffpilot::settings::Section;
use diffpilot::testing::{
    bounds, click, fake_claude, fixture_named, install_settings, open_settings, open_workspace, press, show_settings,
    type_text, wait_until_shows,
};
use diffpilot_core::settings::Settings;
use gpui::TestAppContext;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

#[gpui::test]
fn a_long_repo_path_wraps_in_its_data_row_and_keeps_the_buttons_inside_the_dialog(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    let store = fake.store();
    let name = "a-repository-with-a-very-long-name-".repeat(5);
    let (_dir, repo) = fixture_named(&name);
    install_settings(app, Arc::new(Settings::in_memory()));
    let cx = open_workspace(app, &repo, Arc::new(Settings::in_memory()));
    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "pane:comment-on-file:docs/committed.md");
    type_text(cx, "Note.");
    press(cx, "cmd-enter");
    let key = store.projects().unwrap()[0].repo.clone();

    show_settings(cx, Section::Data);
    wait_until_shows(cx, &format!("data:expand:{key}"));
    let dialog = bounds(cx, "settings:dialog");
    let delete = bounds(cx, &format!("data:delete:repo:{key}"));
    let expand = bounds(cx, &format!("data:expand:{key}"));
    assert!(delete.right() <= dialog.right(), "{delete:?} vs {dialog:?}");
    assert!(expand.right() <= dialog.right(), "{expand:?} vs {dialog:?}");
    let row = bounds(cx, &format!("data:repo:{key}:1 thread · 1 comment"));
    assert!(
        row.size.height > gpui::px(60.),
        "the path wrapped onto more lines: {row:?}"
    );
}

#[gpui::test]
fn a_long_claude_path_wraps_beside_the_buttons_instead_of_running_under_them(app: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let long = dir.path().join("a-directory-with-a-long-name".repeat(4));
    std::fs::create_dir_all(&long).unwrap();
    let script = long.join("claude");
    std::fs::write(&script, "#!/bin/sh\necho 2.0.0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let settings = Arc::new(Settings::in_memory());
    settings.set_claude_path(Some(&script));
    let cx = open_settings(app, None, settings, Section::Claude);
    wait_until_shows(cx, "claude:path");
    let dialog = bounds(cx, "settings:dialog");
    let path = bounds(cx, "claude:path");
    let set_path = bounds(cx, "claude:set-path");
    assert!(path.right() <= dialog.right(), "{path:?} vs {dialog:?}");
    assert!(path.right() <= set_path.left(), "{path:?} vs {set_path:?}");
    assert!(
        path.size.height > gpui::px(40.),
        "the path wrapped onto more lines: {path:?}"
    );
}
