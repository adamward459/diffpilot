use diffpilot::testing::{
    FakeGithub, cancel_next_pick, click, double_click, fixture_named, install_github, open_empty_workspace,
    open_workspace, pick_next, press, remote_fixture, shows, type_text, wait_for_motion, wait_until_shows,
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

fn known(settings: &Settings, folders: &[&PathBuf]) {
    for folder in folders {
        settings.remember_folder(&folder.canonicalize().expect("folder"));
    }
}

#[gpui::test]
fn the_sheet_shows_at_startup_and_enter_opens_the_first_recent_folder(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_beta_dir, beta) = fixture_named("beta");
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha, &beta]);
    let cx = open_empty_workspace(app, settings);

    assert!(shows(cx, "open-folder:dialog"), "no folder is open, so the sheet shows");
    assert!(shows(cx, "open:folder:alpha"));
    assert!(shows(cx, "open:folder:beta"));
    wait_until_shows(cx, "open:badge:alpha:no remote");
    press(cx, "enter");
    assert!(shows(cx, "folder-active:alpha"));
}

#[gpui::test]
fn picking_a_row_then_enter_opens_that_folder(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_beta_dir, beta) = fixture_named("beta");
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha, &beta]);
    let cx = open_workspace(app, &alpha, settings);

    press(cx, "cmd-o");
    assert!(
        shows(cx, "open-folder:dialog"),
        "the shortcut opens the sheet, not a picker"
    );
    click(cx, "open:folder:beta");
    assert!(shows(cx, "open:selected:beta"));
    press(cx, "enter");
    assert!(shows(cx, "folder-active:beta"));
}

#[gpui::test]
fn the_folder_menu_opens_the_sheet(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha]);
    let cx = open_workspace(app, &alpha, settings);

    click(cx, "folder-title");
    click(cx, "folder:Open Folder…");
    assert!(shows(cx, "open-folder:dialog"));
}

#[gpui::test]
fn picking_a_different_base_opens_the_folder_with_that_comparison(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_beta_dir, beta) = fixture_named("beta");
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha, &beta]);
    let cx = open_empty_workspace(app, settings);

    click(cx, "open:folder:beta");
    wait_until_shows(cx, "open-base");
    click(cx, "open-base");
    click(cx, "open-base:Staged Changes Only");
    assert!(shows(cx, "open:open-base-text:Staged Changes"));
    click(cx, "open-folder-confirm");

    assert!(shows(cx, "folder-active:beta"));
    wait_until_shows(cx, "sidebar:row:src/staged.rs");
    assert!(
        !shows(cx, "sidebar:row:docs/committed.md"),
        "only the staged file is compared"
    );
}

#[gpui::test]
fn browse_picks_a_folder_and_makes_it_the_selection(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_gamma_dir, gamma) = fixture_named("gamma");
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha]);
    let cx = open_empty_workspace(app, settings);

    pick_next(cx, &[&gamma]);
    click(cx, "open:browse");
    assert!(shows(cx, "open:selected:gamma"));
    assert!(shows(cx, "open:folder:gamma"), "the picked folder joins the list");

    cancel_next_pick(cx);
    click(cx, "open:browse");
    press(cx, "enter");
    assert!(shows(cx, "folder-active:gamma"));
}

#[gpui::test]
fn a_typed_path_that_is_not_a_folder_says_so(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha]);
    let cx = open_empty_workspace(app, settings);

    type_text(cx, "/no/such/folder");
    press(cx, "enter");
    assert!(shows(cx, "open:error"));
    assert!(!shows(cx, "folder-active:alpha"));
}

#[gpui::test]
fn a_typed_path_opens_when_it_is_a_folder(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_beta_dir, beta) = fixture_named("beta");
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha]);
    let cx = open_empty_workspace(app, settings);

    type_text(cx, &beta.canonicalize().expect("beta").display().to_string());
    press(cx, "enter");
    assert!(shows(cx, "folder-active:beta"), "Enter opens a path that is a folder");
}

#[gpui::test]
fn cancel_opens_nothing_and_saves_nothing(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_shared_dir, shared) = shared_folder();
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha]);
    let cx = open_empty_workspace(app, settings.clone());

    click(cx, "open:linked-edit");
    pick_next(cx, &[&shared]);
    click(cx, "linked-choose");
    assert!(shows(cx, &format!("linked-folder:{}", shared.display())));
    click(cx, "open-folder-cancel");
    wait_for_motion(cx);

    let alpha = alpha.canonicalize().expect("alpha");
    assert!(!shows(cx, "folder-active:alpha"));
    assert_eq!(
        settings.folders(),
        std::slice::from_ref(&alpha),
        "the recent folders are as they were"
    );
    assert!(settings.linked_folders(&alpha).is_empty(), "no link is saved");

    press(cx, "cmd-o");
    press(cx, "escape");
    wait_for_motion(cx);
    assert!(!shows(cx, "folder-active:alpha"), "Escape cancels too");
}

#[gpui::test]
fn linked_folders_are_edited_on_the_sheet_and_kept_for_the_repo(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_typed_dir, typed) = shared_folder();
    let (_picked_dir, picked) = shared_folder();
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha]);
    let cx = open_empty_workspace(app, settings.clone());

    click(cx, "open:linked-edit");
    click(cx, "linked-input");
    type_text(cx, &typed.display().to_string());
    press(cx, "enter");
    pick_next(cx, &[&picked]);
    click(cx, "linked-choose");
    let name = |path: &PathBuf| path.file_name().expect("name").to_string_lossy().into_owned();
    assert!(shows(cx, &format!("open:linked-chip:{}", name(&typed))));
    assert!(shows(cx, &format!("open:linked-chip:{}", name(&picked))));
    assert!(
        settings
            .linked_folders(&alpha.canonicalize().expect("alpha"))
            .is_empty(),
        "nothing is saved before Open"
    );

    click(cx, "open-folder-confirm");
    assert!(shows(cx, "folder-active:alpha"));
    let alpha = alpha.canonicalize().expect("alpha");
    assert_eq!(settings.linked_folders(&alpha), [typed.clone(), picked.clone()]);

    let cx = open_workspace(app, &alpha, settings);
    press(cx, "cmd-2");
    assert!(
        shows(cx, &format!("guide-linked-chip:{}", name(&typed)))
            && shows(cx, &format!("guide-linked-chip:{}", name(&picked))),
        "the Guide lists the folders linked on the sheet"
    );
}

#[gpui::test]
fn a_linked_folder_that_cannot_be_linked_says_why(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_shared_dir, shared) = shared_folder();
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha]);
    let cx = open_empty_workspace(app, settings);

    click(cx, "open:linked-edit");
    click(cx, "linked-input");
    type_text(cx, &shared.join("missing").display().to_string());
    press(cx, "enter");
    assert!(shows(cx, "linked-error"), "a path with no folder is refused");
}

#[gpui::test]
fn picking_a_known_folder_from_the_menu_skips_the_sheet(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_beta_dir, beta) = fixture_named("beta");
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha, &beta]);
    let cx = open_workspace(app, &alpha, settings);
    click(cx, "folder-title");
    click(cx, "folder:beta");
    assert!(shows(cx, "folder-active:beta"));
    assert!(!shows(cx, "open-folder:dialog"), "a known folder opens at once");
}

fn with_pull_requests(
    app: &mut TestAppContext,
) -> (
    diffpilot::testing::RemoteFixture,
    &'static mut gpui::VisualTestContext,
    Arc<Settings>,
) {
    install_github(app, FakeGithub::new());
    let fixture = remote_fixture();
    fixture.add_pull_request(479, "bulk.txt");
    let settings = Arc::new(Settings::in_memory());
    settings.remember_folder(&fixture.repo.canonicalize().expect("repo"));
    let cx = open_workspace(app, &fixture.repo, settings.clone());
    press(cx, "cmd-o");
    wait_until_shows(cx, "open:pr:479");
    (fixture, cx, settings)
}

#[gpui::test]
fn the_sheet_lists_pull_requests_and_a_badge_counts_them(app: &mut TestAppContext) {
    let (_fixture, cx, _) = with_pull_requests(app);
    assert!(shows(cx, "open:badge:repo:2 PRs"));
    assert!(shows(cx, "open:prs-count:2"));
    assert!(shows(cx, "open:pr:482"));
}

#[gpui::test]
fn clicking_a_pull_request_fills_the_branches_and_the_chip_returns(app: &mut TestAppContext) {
    let (_fixture, cx, _) = with_pull_requests(app);
    wait_until_shows(cx, "open-base");

    click(cx, "open:pr:479");
    assert!(shows(cx, "open:chip:479"));
    assert!(shows(cx, "open:open-branch-text:feature/bulk-select"));
    assert!(shows(cx, "open:open-base-text:main"));

    click(cx, "open:chip-clear");
    assert!(
        shows(cx, "open:open-branch-text:feature"),
        "the folder's own comparison is back"
    );
}

#[gpui::test]
fn changing_a_branch_drops_the_pull_request(app: &mut TestAppContext) {
    let (_fixture, cx, _) = with_pull_requests(app);
    click(cx, "open:pr:479");
    assert!(shows(cx, "open:chip:479"));

    click(cx, "open-base");
    click(cx, "open-base:Staged Changes Only");
    assert!(shows(cx, "open:open-base-text:Staged Changes"));
    assert!(shows(cx, "open:open-branch-text:feature"));
}

#[gpui::test]
fn opening_a_pull_request_shows_its_diff_without_a_checkout(app: &mut TestAppContext) {
    let (fixture, cx, _) = with_pull_requests(app);
    double_click(cx, "open:pr:479");

    wait_until_shows(cx, "sidebar:row:bulk.txt");
    let head = std::fs::read_to_string(fixture.repo.join(".git/HEAD")).expect("HEAD");
    assert_eq!(head.trim(), "ref: refs/heads/feature", "the checkout did not move");
}

#[gpui::test]
fn the_open_button_opens_the_picked_pull_request(app: &mut TestAppContext) {
    let (_fixture, cx, _) = with_pull_requests(app);
    click(cx, "open:pr:479");
    click(cx, "open-folder-confirm");
    wait_until_shows(cx, "sidebar:row:bulk.txt");
}

#[gpui::test]
fn the_search_filters_pull_requests_by_title(app: &mut TestAppContext) {
    let (_fixture, cx, _) = with_pull_requests(app);
    click(cx, "open:search");
    type_text(cx, "Undo");
    assert!(shows(cx, "open:prs-count:1"));
}

#[gpui::test]
fn the_search_keeps_the_folders_that_match_a_pull_request_title(app: &mut TestAppContext) {
    install_github(app, FakeGithub::new());
    let fixture = remote_fixture();
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let settings = Arc::new(Settings::in_memory());
    known(&settings, &[&alpha, &fixture.repo]);
    let cx = open_workspace(app, &alpha, settings);
    press(cx, "cmd-o");
    wait_until_shows(cx, "open:badge:repo:2 PRs");
    assert!(shows(cx, "open:selected:alpha"));

    type_text(cx, "timer");
    assert!(
        shows(cx, "open:selected:repo"),
        "alpha has no such pull request, so the selection moves to the folder that has"
    );
}
