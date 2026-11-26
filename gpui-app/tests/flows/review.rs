use crate::common::{StubReplier, StubReviewer, finding, result};
use diffpilot::repo_view::Claude;
use diffpilot::testing::{click, fixture, open_repo, press, shows, type_text};
use diffpilot_core::review::Severity;
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::{CLAUDE_NAME, USER_NAME};
use diffpilot_core::viewed::ViewedStore;
use gpui::TestAppContext;
use std::sync::Arc;

#[gpui::test]
fn a_comment_a_review_and_a_reply_end_up_as_threads_in_the_pane(app: &mut TestAppContext) {
    let dir = fixture();
    let reviewer = StubReviewer::new(Ok(result(
        "Fine overall.",
        vec![
            finding("base.txt", Some(2), Severity::MustFix, "Edited"),
            finding("src/staged.rs", None, Severity::Nit, "Name"),
        ],
    )));
    let replier = Arc::new(StubReplier::default());
    let claude = Claude {
        reviewer: reviewer.clone(),
        replier: replier.clone(),
    };
    let (_, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
        claude,
    );

    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "pill:comment-on-file");
    type_text(cx, "Why this file?");
    press(cx, "cmd-enter");
    assert!(
        shows(cx, &format!("comment:{USER_NAME}:Why this file?")),
        "the pane shows the comment"
    );
    assert!(shows(cx, "jump-bar:threads:1"));
    assert!(shows(cx, "sidebar:comments:docs/committed.md:1"));

    click(cx, "review:open");
    click(cx, "review:start");
    assert!(shows(cx, "review:done"), "the pill reports the finished review");
    assert_eq!(reviewer.calls(), 1);
    assert!(
        shows(cx, "jump-bar:threads:4"),
        "the review added a summary and two findings"
    );
    assert!(shows(cx, "sidebar:comments:base.txt:1"));
    assert!(shows(cx, "sidebar:comments:src/staged.rs:1"));
    assert!(
        shows(cx, "sidebar:comments:docs/committed.md:1"),
        "the review keeps the comment"
    );

    click(cx, "sidebar:row:src/staged.rs");
    assert!(shows(cx, "finding:Name"), "the pane shows the finding on the file");
    click(cx, "sidebar:row:base.txt");
    assert!(shows(cx, "finding:Edited"), "the pane shows the finding on the line");

    click(cx, "thread:reply:Edited");
    type_text(cx, "@claude is this really needed?");
    press(cx, "cmd-enter");
    assert!(shows(
        cx,
        &format!("comment:{USER_NAME}:@claude is this really needed?")
    ));
    assert_eq!(replier.asked(), 1);

    replier.send("Yes, ");
    assert!(
        shows(cx, &format!("comment:{CLAUDE_NAME}:Yes, ")),
        "the reply streams in"
    );
    replier.send("the line changed.");
    replier.finish();
    assert!(shows(cx, &format!("comment:{CLAUDE_NAME}:Yes, the line changed.")));
}
