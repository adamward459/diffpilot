use diffpilot::testing::{
    canned_claude, fixture, open_repo, pick_comparison, select_file, sidebar_paths, toggle_viewed, viewed_count,
    visible_file,
};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::TestAppContext;
use std::sync::Arc;

#[gpui::test]
fn a_viewed_file_stays_viewed_when_the_repo_is_reopened(app: &mut TestAppContext) {
    let dir = fixture();
    let settings = Arc::new(Settings::in_memory());
    let store = Arc::new(ViewedStore::in_memory());

    let (view, cx) = open_repo(app, dir.path(), settings.clone(), store.clone(), canned_claude());
    assert_eq!(viewed_count(&view, cx), 0);

    pick_comparison(&view, Comparison::Staged, cx);
    assert_eq!(sidebar_paths(&view, cx), ["src", "src/staged.rs"]);

    select_file(&view, "src/staged.rs", cx);
    assert_eq!(visible_file(&view, cx).as_deref(), Some("src/staged.rs"));

    toggle_viewed(&view, "src/staged.rs", cx);
    assert_eq!(viewed_count(&view, cx), 1, "the sidebar counts the file as viewed");

    let (reopened, cx) = open_repo(app, dir.path(), settings, store, canned_claude());
    assert_eq!(
        sidebar_paths(&reopened, cx),
        ["src", "src/staged.rs"],
        "the reopened repo remembers the staged comparison"
    );
    assert_eq!(viewed_count(&reopened, cx), 1, "the file is still viewed");
}
