use diffpilot::settings::Section;
use diffpilot::testing::{
    bounds, click, click_corner, fixture, fixture_named, open_settings, open_workspace, press, shows, type_text,
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
    click(cx, "comparison-base");
    assert!(
        shows(cx, "comparison-base:Staged Changes Only"),
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
    let repo = bounds(cx, "comparison-base");
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
    click(cx, "comparison-base");
    assert!(
        shows(cx, "comparison-base:Staged Changes Only"),
        "a click on the backdrop closes the dialog and the app works again"
    );
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
        click(cx, "comparison-base");
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
