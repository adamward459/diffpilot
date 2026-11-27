use diffpilot::testing::{
    cancel_next_pick, click, fixture_named, open_empty_workspace, open_workspace, pick_next, press, shows, type_text,
    wait_for_motion,
};
use diffpilot_core::settings::Settings;
use gpui::TestAppContext;
use std::path::PathBuf;
use std::sync::Arc;

fn shared_folder() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("path");
    (dir, path)
}

#[gpui::test]
fn linked_folders_are_added_in_the_open_dialog_and_kept_for_the_repo(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_typed_dir, typed) = shared_folder();
    let (_picked_dir, picked) = shared_folder();
    let settings = Arc::new(Settings::in_memory());
    let cx = open_empty_workspace(app, settings.clone());

    pick_next(cx, &[&alpha]);
    press(cx, "cmd-o");
    assert!(shows(cx, "open-folder:dialog"), "the picker leads to the dialog");
    assert!(!shows(cx, "folder-active:alpha"), "the repo is not open yet");

    click(cx, "guide-link-input");
    type_text(cx, &typed.display().to_string());
    click(cx, "guide-link-submit");
    assert!(shows(cx, &format!("guide-linked-folder:{}", typed.display())));

    pick_next(cx, &[&picked]);
    click(cx, "guide-add-folders");
    assert!(shows(cx, &format!("guide-linked-folder:{}", picked.display())));

    click(cx, "open-folder-confirm");
    assert!(shows(cx, "folder-active:alpha"), "Open shows the repo");
    let alpha = alpha.canonicalize().expect("alpha");
    assert_eq!(settings.linked_folders(&alpha), [typed.clone(), picked.clone()]);

    let cx = open_workspace(app, &alpha, settings);
    press(cx, "cmd-2");
    assert!(
        shows(cx, &format!("guide-linked-folder:{}", typed.display()))
            && shows(cx, &format!("guide-linked-folder:{}", picked.display())),
        "the Guide lists the folders linked in the dialog"
    );
}

#[gpui::test]
fn enter_opens_the_folder_from_the_dialog(app: &mut TestAppContext) {
    let (_dir, alpha) = fixture_named("alpha");
    let cx = open_empty_workspace(app, Arc::new(Settings::in_memory()));
    pick_next(cx, &[&alpha]);
    press(cx, "cmd-o");
    press(cx, "enter");
    assert!(shows(cx, "folder-active:alpha"));
}

#[gpui::test]
fn cancel_opens_nothing_and_saves_nothing(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_shared_dir, shared) = shared_folder();
    let settings = Arc::new(Settings::in_memory());
    let cx = open_empty_workspace(app, settings.clone());

    pick_next(cx, &[&alpha]);
    press(cx, "cmd-o");
    pick_next(cx, &[&shared]);
    click(cx, "guide-add-folders");
    assert!(shows(cx, &format!("guide-linked-folder:{}", shared.display())));
    click(cx, "open-folder-cancel");
    wait_for_motion(cx);

    let alpha = alpha.canonicalize().expect("alpha");
    assert!(!shows(cx, "folder-active:alpha"));
    assert!(settings.folders().is_empty(), "no folder is remembered");
    assert!(settings.linked_folders(&alpha).is_empty(), "no link is saved");

    pick_next(cx, &[&alpha]);
    press(cx, "cmd-o");
    press(cx, "escape");
    wait_for_motion(cx);
    assert!(!shows(cx, "folder-active:alpha"), "Escape cancels too");
}

#[gpui::test]
fn a_linked_folder_that_cannot_be_linked_says_why(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_shared_dir, shared) = shared_folder();
    let cx = open_empty_workspace(app, Arc::new(Settings::in_memory()));
    pick_next(cx, &[&alpha]);
    press(cx, "cmd-o");

    click(cx, "guide-link-input");
    type_text(cx, &shared.join("missing").display().to_string());
    click(cx, "guide-link-submit");
    assert!(shows(cx, "guide-link-error"), "a path with no folder is refused");

    let inside = alpha.join("docs");
    pick_next(cx, &[&shared, &inside]);
    click(cx, "guide-add-folders");
    assert!(shows(cx, &format!("guide-linked-folder:{}", shared.display())));
    assert!(
        shows(cx, &format!("guide-link-refused:{}", inside.display())),
        "a picked folder inside the repo shows its error"
    );
}

#[gpui::test]
fn picking_a_known_folder_from_the_menu_skips_the_dialog(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_beta_dir, beta) = fixture_named("beta");
    let settings = Arc::new(Settings::in_memory());
    for folder in [&alpha, &beta] {
        settings.remember_folder(&folder.canonicalize().expect("folder"));
    }
    let cx = open_workspace(app, &alpha, settings);
    click(cx, "folder-title");
    click(cx, "folder:beta");
    assert!(shows(cx, "folder-active:beta"));
    assert!(!shows(cx, "open-folder:dialog"), "a known folder opens at once");
}

#[gpui::test]
fn a_cancelled_picker_shows_no_dialog(app: &mut TestAppContext) {
    let cx = open_empty_workspace(app, Arc::new(Settings::in_memory()));
    cancel_next_pick(cx);
    press(cx, "cmd-o");
    assert!(!shows(cx, "open-folder:dialog"));
}
