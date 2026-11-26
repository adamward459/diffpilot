use diffpilot::settings::Section;
use diffpilot::testing::{click, fixture, open_settings, press, shows, type_text};
use diffpilot_core::settings::Settings;
use gpui::TestAppContext;
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
