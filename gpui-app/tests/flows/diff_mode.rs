use diffpilot::settings::Tab;
use diffpilot::testing::{canned_claude, click, fixture, open_repo, open_settings, shows};
use diffpilot_core::diff::DiffMode;
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::TestAppContext;
use std::sync::Arc;

#[gpui::test]
fn the_diff_mode_chosen_in_settings_applies_to_every_session_of_a_repo_opened_after(app: &mut TestAppContext) {
    let settings = Arc::new(Settings::in_memory());
    let settings_cx = open_settings(app, settings.clone(), Tab::Diff);

    assert_eq!(settings.diff_mode(), DiffMode::Unified);
    click(settings_cx, "layout:Split");
    assert_eq!(settings.diff_mode(), DiffMode::Split, "the choice is saved");

    let dir = fixture();
    let store = Arc::new(ViewedStore::in_memory());
    let (_, cx) = open_repo(app, dir.path(), settings.clone(), store, canned_claude());
    assert!(
        shows(cx, "diff-mode:Split:selected"),
        "a repo opened now starts in Split"
    );

    click(cx, "comparison-base");
    click(cx, "comparison-base:Staged Changes Only");
    assert!(shows(cx, "sidebar:summary:0/1 viewed"));
    assert!(shows(cx, "diff-mode:Split:selected"), "so does the next session");

    click(cx, "comparison-base");
    click(cx, "comparison-base:Working Tree Changes Only");
    assert!(shows(cx, "sidebar:summary:0/2 viewed"));
    assert!(shows(cx, "diff-mode:Split:selected"));
}
