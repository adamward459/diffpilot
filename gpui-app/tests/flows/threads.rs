use diffpilot::testing::{canned_claude, click, fake_claude, fixture, open_repo_in, press, shows, type_text};
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::USER_NAME;
use diffpilot_core::viewed::ViewedStore;
use gpui::{TestAppContext, VisualTestContext};
use std::path::Path;
use std::sync::Arc;

fn open(
    app: &mut TestAppContext,
    repo: &Path,
    store: Arc<diffpilot_core::store::Store>,
) -> &'static mut VisualTestContext {
    let (_, cx) = open_repo_in(
        app,
        repo,
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
        store,
        canned_claude(),
    );
    cx
}

fn comment_on_file(cx: &mut VisualTestContext, path: &str, text: &str) {
    click(cx, &format!("sidebar:row:{path}"));
    click(cx, &format!("pane:comment-on-file:{path}"));
    type_text(cx, text);
    press(cx, "cmd-enter");
}

#[gpui::test]
fn delete_removes_a_thread_for_good(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    let dir = fixture();
    let cx = open(app, dir.path(), fake.store());
    comment_on_file(cx, "docs/committed.md", "Delete me.");
    comment_on_file(cx, "base.txt", "Keep me.");
    assert!(shows(cx, "jump-bar:threads:2"));

    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "thread:delete:docs/committed.md");

    let again = open(app, dir.path(), fake.store());
    click(again, "sidebar:row:docs/committed.md");
    assert!(shows(again, "jump-bar:threads:1"), "only the kept thread is left");
    assert!(!shows(again, &format!("comment:{USER_NAME}:Delete me.")));
    click(again, "sidebar:row:base.txt");
    assert!(shows(again, &format!("comment:{USER_NAME}:Keep me.")));
}
