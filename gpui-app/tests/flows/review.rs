use crate::common::{StubReplier, StubReviewer, finding, result};
use diffpilot::repo_view::Claude;
use diffpilot::testing::{fixture, open_repo};
use diffpilot_core::review::{ReviewRun, Severity};
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
    let (view, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
        claude,
    );
    let (sidebar, pane, pill) = view.read_with(cx, |view, _| (view.sidebar(), view.pane(), view.pill()));
    let review = pane.read_with(cx, |pane, _| pane.review().cloned().expect("a review session"));

    pill.update(cx, |pill, cx| pill.comment_on_file(cx));
    cx.run_until_parked();
    let mine = review.read_with(cx, |review, _| review.threads().next().map(|thread| thread.id));
    let mine = mine.expect("the comment opens a thread");
    assert!(
        pane.read_with(cx, |pane, _| pane.card(mine).is_some()),
        "the pane shows the thread"
    );
    review.update(cx, |review, cx| review.post(mine, "Why this file?", cx));
    let note = review.read_with(cx, |review, _| review.thread(mine).cloned().expect("thread"));
    assert_eq!(note.comments.len(), 1);
    assert_eq!(note.comments[0].author, USER_NAME);
    assert_eq!(note.comments[0].body, "Why this file?");

    review.update(cx, |review, cx| review.start_review(cx));
    cx.run_until_parked();
    assert_eq!(reviewer.calls(), 1);
    assert!(review.read_with(cx, |review, _| matches!(review.run(), ReviewRun::Done { .. })));
    let counts = sidebar.read_with(cx, |sidebar, _| sidebar.comments().clone());
    assert_eq!(counts.get("base.txt"), Some(&1));
    assert_eq!(counts.get("src/staged.rs"), Some(&1));
    assert!(
        review.read_with(cx, |review, _| review.thread(mine).is_some()),
        "the review keeps the user's thread"
    );

    let finding_thread = review.read_with(cx, |review, _| {
        review
            .threads()
            .find(|thread| thread.finding.as_ref().is_some_and(|f| f.title == "Edited"))
            .map(|thread| thread.id)
    });
    let finding_thread = finding_thread.expect("a thread for the finding");
    review.update(cx, |review, cx| {
        review.post(finding_thread, "@claude is this really needed?", cx)
    });
    assert_eq!(replier.asked(), 1);
    replier.send("Yes, ");
    cx.run_until_parked();
    replier.send("the line changed.");
    replier.finish();
    cx.run_until_parked();

    let thread = review.read_with(cx, |review, _| review.thread(finding_thread).cloned().expect("thread"));
    let last = thread.comments.last().expect("a reply");
    assert_eq!(last.author, CLAUDE_NAME);
    assert_eq!(last.body, "Yes, the line changed.");
    assert!(!last.is_streaming);
}
