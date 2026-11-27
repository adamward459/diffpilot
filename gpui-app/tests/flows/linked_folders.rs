use diffpilot::testing::{
    bounds, click, fixture_named, open_empty_workspace, open_workspace, pick_next, press, shows, type_text,
    wait_for_motion, wait_until_shows,
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
fn the_linked_pill_sits_on_its_own_line_under_the_folder_name(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let cx = open_workspace(app, &alpha, Arc::new(Settings::in_memory()));
    let title = bounds(cx, "folder-title");
    let pill = bounds(cx, "linked-pill");
    assert!(
        pill.top() >= title.bottom(),
        "the pill is below the name, not beside it: title {title:?}, pill {pill:?}"
    );
}

#[gpui::test]
fn the_title_bar_pill_counts_links_and_its_popover_adds_and_removes_one(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_shared_dir, shared) = shared_folder();
    let alpha = alpha.canonicalize().expect("alpha");
    let settings = Arc::new(Settings::in_memory());
    let cx = open_workspace(app, &alpha, settings.clone());
    assert!(shows(cx, "linked-count:Link folders"), "no links yet");

    click(cx, "linked-pill");
    assert!(shows(cx, "linked-title"), "the pill opens the popover");
    click(cx, "linked-input");
    type_text(cx, &shared.display().to_string());
    press(cx, "enter");
    assert_eq!(
        settings.linked_folders(&alpha),
        std::slice::from_ref(&shared),
        "the link is saved at once"
    );
    assert!(shows(cx, "linked-count:1 linked"));

    click(cx, &format!("linked-remove:{}", shared.display()));
    assert!(settings.linked_folders(&alpha).is_empty(), "removing saves too");
}

#[gpui::test]
fn the_shortcut_opens_the_popover_with_the_field_ready_to_type(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_shared_dir, shared) = shared_folder();
    let alpha = alpha.canonicalize().expect("alpha");
    let settings = Arc::new(Settings::in_memory());
    let cx = open_workspace(app, &alpha, settings.clone());

    press(cx, "alt-cmd-l");
    assert!(shows(cx, "linked-title"));
    type_text(cx, &shared.display().to_string());
    press(cx, "enter");
    assert_eq!(settings.linked_folders(&alpha), [shared]);
}

#[gpui::test]
fn the_folder_menu_opens_the_linked_folders_of_the_open_folder(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let alpha = alpha.canonicalize().expect("alpha");
    let settings = Arc::new(Settings::in_memory());
    settings.remember_folder(&alpha);
    let cx = open_workspace(app, &alpha, settings);

    click(cx, "folder-title");
    assert!(shows(cx, "folder:Open Folder…"));
    click(cx, "folder:Linked Folders for alpha…");
    assert!(shows(cx, "linked-title"));
}

#[gpui::test]
fn choosing_folders_links_them_and_a_refused_one_can_be_dismissed(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_shared_dir, shared) = shared_folder();
    let alpha = alpha.canonicalize().expect("alpha");
    let settings = Arc::new(Settings::in_memory());
    let cx = open_workspace(app, &alpha, settings.clone());

    click(cx, "linked-pill");
    let inside = alpha.join("docs");
    pick_next(cx, &[&shared, &inside]);
    click(cx, "linked-choose");
    assert_eq!(settings.linked_folders(&alpha), [shared]);
    assert!(shows(cx, &format!("linked-refused:{}", inside.display())));
    click(cx, &format!("linked-dismiss:{}", inside.display()));
}

#[gpui::test]
fn a_missing_folder_can_be_located_or_removed(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_moved_dir, moved) = shared_folder();
    let alpha = alpha.canonicalize().expect("alpha");
    let gone = alpha.parent().expect("parent").join("gone-folder");
    let settings = Arc::new(Settings::in_memory());
    settings.set_linked_folders(&alpha, vec![gone.clone()]);
    let cx = open_workspace(app, &alpha, settings.clone());

    click(cx, "linked-pill");
    assert!(shows(cx, &format!("linked-missing:{}", gone.display())));
    pick_next(cx, &[&moved]);
    click(cx, &format!("linked-locate:{}", gone.display()));
    assert_eq!(
        settings.linked_folders(&alpha),
        std::slice::from_ref(&moved),
        "Locate… swaps the path"
    );

    settings.set_linked_folders(&alpha, vec![gone.clone()]);
    press(cx, "escape");
    wait_for_motion(cx);
    press(cx, "alt-cmd-l");
    click(cx, &format!("linked-remove:{}", gone.display()));
    assert!(settings.linked_folders(&alpha).is_empty(), "Remove unlinks it");
}

#[gpui::test]
fn the_guide_card_shows_chips_and_edit_opens_the_popover(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_shared_dir, shared) = shared_folder();
    let alpha = alpha.canonicalize().expect("alpha");
    let gone = alpha.parent().expect("parent").join("gone-folder");
    let settings = Arc::new(Settings::in_memory());
    let cx = open_workspace(app, &alpha, settings.clone());

    press(cx, "cmd-2");
    assert!(shows(cx, "guide-linked-none"), "no links says so");
    click(cx, "guide-linked-edit");
    assert!(shows(cx, "linked-title"), "Link… opens the popover");

    settings.set_linked_folders(&alpha, vec![shared.clone(), gone.clone()]);
    press(cx, "escape");
    wait_for_motion(cx);
    let name = |path: &PathBuf| path.file_name().expect("name").to_string_lossy().into_owned();
    assert!(shows(cx, &format!("guide-linked-chip:{}", name(&shared))));
    assert!(shows(cx, &format!("guide-linked-missing:{}", name(&gone))));
    click(cx, "guide-linked-edit");
    assert!(
        shows(cx, &format!("linked-folder:{}", shared.display())),
        "Edit… lists the links"
    );
}

#[gpui::test]
fn no_folder_open_means_no_pill(app: &mut TestAppContext) {
    let cx = open_empty_workspace(app, Arc::new(Settings::in_memory()));
    press(cx, "alt-cmd-l");
    assert!(!shows(cx, "linked-title"));
}

#[gpui::test]
fn the_popover_says_why_a_completed_folder_cannot_be_linked(app: &mut TestAppContext) {
    let (alpha_dir, alpha) = fixture_named("alpha");
    let alpha = alpha.canonicalize().expect("alpha");
    let parent = alpha.parent().expect("parent").to_path_buf();
    let settings = Arc::new(Settings::in_memory());
    let cx = open_workspace(app, &alpha, settings.clone());

    click(cx, "linked-pill");
    click(cx, "linked-input");
    type_text(cx, &format!("{}/al", parent.display()));
    wait_until_shows(cx, "hint-reason:alpha:This folder");
    press(cx, "down");
    press(cx, "enter");
    assert!(shows(cx, "linked-error"), "Enter on the repo itself is refused");
    assert!(settings.linked_folders(&alpha).is_empty());
    drop(alpha_dir);
}
