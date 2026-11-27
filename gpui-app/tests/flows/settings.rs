use diffpilot::settings::Section;
use diffpilot::testing::{click, fixture, fixture_named, open_settings, open_workspace, press, shows, type_text};
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

    click(cx, "comparison-base");
    assert!(
        !shows(cx, "comparison-base:Staged Changes Only"),
        "the page covers the repo, so its controls cannot be clicked"
    );

    press(cx, "escape");
    click(cx, "comparison-base");
    assert!(
        shows(cx, "comparison-base:Staged Changes Only"),
        "escape closes the page and the repo works as before"
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
