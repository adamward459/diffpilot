use diffpilot::testing::{click, fixture_named, open_workspace, press, shows};
use diffpilot_core::settings::Settings;
use gpui::TestAppContext;
use std::sync::Arc;

#[gpui::test]
fn the_folder_menu_and_the_shortcuts_switch_between_repos(app: &mut TestAppContext) {
    let (_alpha_dir, alpha) = fixture_named("alpha");
    let (_beta_dir, beta) = fixture_named("beta");
    let (_gamma_dir, gamma) = fixture_named("gamma");
    let settings = Arc::new(Settings::in_memory());
    for folder in [&alpha, &beta, &gamma] {
        settings.remember_folder(&folder.canonicalize().expect("folder"));
    }
    let cx = open_workspace(app, &alpha, settings);
    assert!(shows(cx, "folder-active:alpha"));

    click(cx, "folder-title");
    assert!(shows(cx, "folder:gamma"), "the menu lists every known folder");
    click(cx, "folder:beta");
    assert!(
        shows(cx, "folder-active:beta"),
        "the menu switches to the repo it picked"
    );

    click(cx, "sidebar:row:docs/committed.md");
    press(cx, "ctrl-cmd-3");
    assert!(shows(cx, "folder-active:gamma"), "ctrl-cmd-3 opens the third folder");
}

#[gpui::test]
fn the_sidebar_shortcut_hides_the_sidebar(app: &mut TestAppContext) {
    let (_dir, repo) = fixture_named("alpha");
    let cx = open_workspace(app, &repo, Arc::new(Settings::in_memory()));
    assert!(!shows(cx, "toolbar:sidebar-toggle"));
    click(cx, "sidebar:row:docs/committed.md");
    press(cx, "ctrl-cmd-s");
    assert!(
        shows(cx, "toolbar:sidebar-toggle"),
        "the toolbar offers the toggle once the sidebar is hidden"
    );
}

#[gpui::test]
fn the_sidebar_button_hides_the_sidebar(app: &mut TestAppContext) {
    let (_dir, repo) = fixture_named("alpha");
    let cx = open_workspace(app, &repo, Arc::new(Settings::in_memory()));
    click(cx, "sidebar:toggle");
    assert!(shows(cx, "toolbar:sidebar-toggle"));
}
