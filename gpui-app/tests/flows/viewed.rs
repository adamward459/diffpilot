use diffpilot::testing::{click, compare_to, fixture, open_repo_in_workspace, shows};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::viewed::ViewedStore;
use gpui::TestAppContext;
use std::sync::Arc;

#[gpui::test]
fn a_viewed_file_stays_viewed_when_the_repo_is_reopened(app: &mut TestAppContext) {
    let dir = fixture();
    let settings = Arc::new(Settings::in_memory());
    let store = Arc::new(ViewedStore::in_memory());

    let review_store = Arc::new(Store::open_in_memory().expect("an in-memory store opens"));
    let (_, cx) = open_repo_in_workspace(app, dir.path(), settings.clone(), store.clone(), review_store);
    assert!(shows(cx, "sidebar:summary:0/3 viewed"));

    compare_to(cx, "Staged Changes Only");
    assert!(
        shows(cx, "sidebar:summary:0/1 viewed"),
        "the sidebar lists the staged file only"
    );
    assert!(shows(cx, "sidebar:row:src/staged.rs"));

    click(cx, "sidebar:row:src/staged.rs");
    assert!(
        shows(cx, "jump-bar:path:src/staged.rs"),
        "the toolbar shows the open file"
    );

    click(cx, "pane:viewed:src/staged.rs");
    assert!(
        shows(cx, "sidebar:summary:1/1 viewed"),
        "the sidebar counts the file as viewed"
    );
    assert!(shows(cx, "sidebar:is-viewed:src/staged.rs"));

    let review_store = Arc::new(Store::open_in_memory().expect("an in-memory store opens"));
    let (_, cx) = open_repo_in_workspace(app, dir.path(), settings, store, review_store);
    assert!(
        shows(cx, "sidebar:summary:1/1 viewed"),
        "the reopened repo remembers the staged comparison and the viewed file"
    );
    assert!(shows(cx, "sidebar:is-viewed:src/staged.rs"));
}
