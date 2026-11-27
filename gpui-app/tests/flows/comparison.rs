use diffpilot::testing::{click, compare_to, fixture_named, open_workspace, press, shows, wait_until_shows};
use diffpilot_core::settings::Settings;
use gpui::TestAppContext;
use std::sync::Arc;

#[gpui::test]
fn the_title_bar_names_the_comparison_and_opens_the_sheet_on_it(app: &mut TestAppContext) {
    let (_dir, alpha) = fixture_named("alpha");
    let cx = open_workspace(app, &alpha, Arc::new(Settings::in_memory()));
    wait_until_shows(cx, "comparison-text:feature \u{2192} main");

    click(cx, "comparison-label");
    assert!(shows(cx, "open-folder:dialog"));
    wait_until_shows(cx, "open:selected:alpha");
    wait_until_shows(cx, "open:open-branch-text:feature");
    assert!(shows(cx, "open:open-base-text:main"));
    press(cx, "escape");
}

#[gpui::test]
fn opening_with_another_base_changes_the_files_and_the_label(app: &mut TestAppContext) {
    let (_dir, alpha) = fixture_named("alpha");
    let cx = open_workspace(app, &alpha, Arc::new(Settings::in_memory()));
    wait_until_shows(cx, "sidebar:summary:0/3 viewed");

    compare_to(cx, "Working Tree Changes Only");
    wait_until_shows(cx, "comparison-text:feature \u{2192} Working Tree Changes");
    wait_until_shows(cx, "sidebar:summary:0/2 viewed");

    compare_to(cx, "Staged Changes Only");
    wait_until_shows(cx, "comparison-text:feature \u{2192} Staged Changes");
    wait_until_shows(cx, "sidebar:summary:0/1 viewed");
}
