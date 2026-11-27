use diffpilot::settings::Section;
use diffpilot::testing::{
    bounds, click, click_corner, fixture, fixture_named, open_settings, open_workspace, press, shows, type_text,
    wait_for_motion,
};
use diffpilot_core::settings::Settings;
use gpui::{TestAppContext, VisualTestContext};
use std::sync::Arc;

#[gpui::test]
fn settings_open_over_the_repo_and_escape_brings_the_repo_back(app: &mut TestAppContext) {
    let dir = fixture();
    let settings = Arc::new(Settings::in_memory());
    let cx = open_settings(app, Some(dir.path()), settings.clone(), Section::General);
    assert!(shows(cx, "settings:search"), "the page opens inside the window");

    type_text(cx, "wrap");
    assert!(shows(cx, "settings:shows:Diff"), "only the matching section is left");
    click(cx, "wrap-lines");
    assert!(settings.wrap_lines(), "a row found by search still works");

    press(cx, "escape");
    click(cx, "comparison-label");
    assert!(
        shows(cx, "open-folder:dialog"),
        "escape closes the page and the repo works as before"
    );
}

#[gpui::test]
fn settings_open_as_a_dialog_over_the_app_and_a_click_outside_closes_it(app: &mut TestAppContext) {
    let dir = fixture();
    let settings = Arc::new(Settings::in_memory());
    let cx = open_settings(app, Some(dir.path()), settings, Section::General);

    let window = bounds(cx, "settings:backdrop");
    let dialog = bounds(cx, "settings:dialog");
    let repo = bounds(cx, "comparison-label");
    assert!(
        dialog.size.width < window.size.width && dialog.size.height < window.size.height,
        "the dialog is smaller than the window"
    );
    assert!(
        window.contains(&repo.center()) && !dialog.contains(&repo.center()),
        "the app stays visible behind"
    );

    click(cx, "settings:search");
    assert!(shows(cx, "settings:search"), "a click inside the dialog keeps it open");

    click_corner(cx, "settings:backdrop");
    click(cx, "comparison-label");
    assert!(
        shows(cx, "open-folder:dialog"),
        "a click on the backdrop closes the dialog and the app works again"
    );
}

#[gpui::test]
fn the_dialog_is_still_on_screen_while_it_closes_and_gone_after(app: &mut TestAppContext) {
    let dir = fixture();
    let cx = open_settings(app, Some(dir.path()), Arc::new(Settings::in_memory()), Section::General);

    press(cx, "escape");
    assert!(
        shows(cx, "settings:backdrop:closing"),
        "the dialog is still drawn right after Escape"
    );

    wait_for_motion(cx);
    click(cx, "comparison-label");
    assert!(
        shows(cx, "open-folder:dialog"),
        "after the close time the app answers clicks"
    );
}

#[gpui::test]
fn opening_settings_again_during_the_close_keeps_the_dialog_open(app: &mut TestAppContext) {
    let dir = fixture();
    let cx = open_settings(app, Some(dir.path()), Arc::new(Settings::in_memory()), Section::General);

    press(cx, "escape");
    press(cx, "cmd-,");
    wait_for_motion(cx);
    click(cx, "comparison-label");
    assert!(
        !shows(cx, "open-folder:dialog"),
        "the dialog stayed open, so the click landed on its backdrop"
    );
}

#[gpui::test]
fn settings_show_the_app_version_even_when_search_matches_nothing(app: &mut TestAppContext) {
    let settings = Arc::new(Settings::in_memory());
    let cx = open_settings(app, None, settings, Section::General);
    let version = format!("settings:version:{}", env!("CARGO_PKG_VERSION"));
    assert!(
        shows(cx, &version),
        "the version shows at the bottom of the section list"
    );

    type_text(cx, "zzzznomatch");
    assert!(shows(cx, "settings:shows:"), "no section matches the search");
    assert!(shows(cx, &version), "the version stays when the list is empty");
}

fn opens_after(app: &mut TestAppContext, steps: impl FnOnce(&mut VisualTestContext)) {
    let (_dir, repo) = fixture_named("alpha");
    let settings = Arc::new(Settings::in_memory());
    open_settings(app, None, settings.clone(), Section::General);
    let cx = open_workspace(app, &repo, settings);
    steps(cx);
    press(cx, "cmd-,");
    assert!(shows(cx, "settings:search"), "cmd-, opens Settings");
}

#[gpui::test]
fn settings_open_after_the_sidebar_that_had_focus_is_hidden(app: &mut TestAppContext) {
    opens_after(app, |cx| {
        click(cx, "sidebar:row:docs/committed.md");
        press(cx, "ctrl-cmd-s");
    });
}

#[gpui::test]
fn settings_open_from_the_guide(app: &mut TestAppContext) {
    opens_after(app, |cx| {
        click(cx, "sidebar:row:docs/committed.md");
        press(cx, "cmd-2");
    });
}

#[gpui::test]
fn settings_open_after_a_menu_closes(app: &mut TestAppContext) {
    opens_after(app, |cx| {
        click(cx, "comparison-label");
        press(cx, "escape");
    });
}

#[gpui::test]
fn settings_open_after_a_click_in_the_diff(app: &mut TestAppContext) {
    opens_after(app, |cx| {
        click(cx, "sidebar:row:docs/committed.md");
        click(cx, "pane:hunk:docs/committed.md:0");
    });
}
