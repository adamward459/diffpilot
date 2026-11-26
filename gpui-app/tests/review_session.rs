mod common;

use common::*;
use diffpilot::review::session::{NO_REPLY_MESSAGE, NO_REPO_MESSAGE};
use diffpilot::review::{ReviewEvent, ThreadChange};
use diffpilot_core::cancel::CancelToken;
use diffpilot_core::claude::ReviewError;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::review::{Effort, ModelName, ReviewModel, ReviewRun, ReviewScope, Severity};
use diffpilot_core::store::Store;
use diffpilot_core::threads::ThreadStatus;
use diffpilot_core::threads::{CLAUDE_NAME, Side, ThreadAnchor, ThreadId, USER_NAME};
use gpui::TestAppContext;
use std::path::PathBuf;
use std::sync::Arc;

#[gpui::test]
async fn a_run_ends_done_with_threads_planned_from_the_findings(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.start(cx);
    assert!(matches!(f.run(cx), ReviewRun::Running { .. }));
    cx.run_until_parked();

    let ReviewRun::Done { counts } = f.run(cx) else {
        panic!("run is {:?}", f.run(cx))
    };
    assert_eq!(counts.get(&Severity::MustFix), Some(&1));
    assert_eq!(counts.get(&Severity::Nit), Some(&1));

    let threads = f.threads(cx);
    let anchors: Vec<_> = threads.iter().map(|t| t.anchor.clone()).collect();
    assert_eq!(
        anchors,
        [
            ThreadAnchor::Change,
            ThreadAnchor::line("src/a.ts", Side::New, 2),
            ThreadAnchor::Change
        ]
    );
    assert!(threads[0].finding.is_none());
    assert_eq!(threads[0].comments[0].body, "Looks fine.");
    assert_eq!(threads[1].finding.as_ref().unwrap().title, "Bad");
    assert_eq!(
        threads[2].finding.as_ref().unwrap().location.as_deref(),
        Some("elsewhere.py")
    );
    assert!(threads.iter().all(|t| t.comments[0].author == CLAUDE_NAME));
}

#[gpui::test]
async fn a_run_announces_running_threads_then_done(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.start(cx);
    cx.run_until_parked();
    let events = f.events.borrow().clone();
    assert_eq!(events.first(), Some(&ReviewEvent::RunChanged));
    assert_eq!(events.last(), Some(&ReviewEvent::RunChanged));
    let added = events
        .iter()
        .filter(|event| matches!(event, ReviewEvent::ThreadsChanged(ThreadChange::Added(_))))
        .count();
    assert_eq!(added, 3);
}

#[gpui::test]
async fn the_request_carries_the_setup_the_popover_collected(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(result("", vec![])));
    f.session.update(cx, |session, cx| {
        session.update_setup(
            |setup| {
                setup.focuses.insert("Security".into());
                setup.instructions = " look at auth ".into();
                setup.scope = ReviewScope::OpenFile;
                setup.model = Some(ReviewModel::new(ModelName::Haiku, Effort::Low));
            },
            cx,
        )
    });
    f.start(cx);
    cx.run_until_parked();
    let request = f.reviewer.requests.lock().unwrap()[0].clone();
    assert_eq!(request.repo, PathBuf::from("/repo"));
    assert_eq!(request.revisions, ["--merge-base", "main"]);
    assert_eq!(request.focus, "Security");
    assert_eq!(request.instructions, "look at auth");
    assert_eq!(request.paths, ["src/a.ts"]);
    assert_eq!(request.model, ReviewModel::new(ModelName::Haiku, Effort::Low));
    assert_eq!(
        f.settings.reviews_model(),
        ReviewModel::new(ModelName::Haiku, Effort::Low)
    );
    assert_eq!(f.settings.review_focuses(&PathBuf::from("/repo")), ["Security"]);
}

#[gpui::test]
async fn an_empty_review_still_ends_done_with_no_threads(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(result("", vec![])));
    f.start(cx);
    cx.run_until_parked();
    assert_eq!(f.run(cx).label(), "done:0/0/0/0");
    assert!(f.threads(cx).is_empty());
}

#[gpui::test]
async fn stop_cancels_the_run_and_returns_to_idle(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.start(cx);
    f.session.update(cx, |session, cx| session.stop_review(cx));
    assert_eq!(f.run(cx), ReviewRun::Idle);
    cx.run_until_parked();
    assert_eq!(f.run(cx), ReviewRun::Idle);
    assert!(f.threads(cx).is_empty());
    assert!(f.reviewer.tokens.lock().unwrap().iter().all(CancelToken::is_cancelled));
}

#[gpui::test]
async fn stopping_when_nothing_runs_changes_nothing(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.session.update(cx, |session, cx| session.stop_review(cx));
    assert!(f.events.borrow().is_empty());
    f.start(cx);
    cx.run_until_parked();
    let before = f.events.borrow().len();
    f.session.update(cx, |session, cx| session.stop_review(cx));
    assert_eq!(f.events.borrow().len(), before);
    assert!(matches!(f.run(cx), ReviewRun::Done { .. }));
}

#[gpui::test]
async fn starting_while_a_run_is_going_does_not_start_another(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.start(cx);
    f.start(cx);
    cx.run_until_parked();
    assert_eq!(f.reviewer.calls(), 1);
}

#[gpui::test]
async fn a_failure_shows_its_message_and_the_next_run_can_succeed(cx: &mut TestAppContext) {
    let f = fixture(cx, Err(ReviewError::Failed("boom".into())));
    f.start(cx);
    cx.run_until_parked();
    assert_eq!(f.run(cx), ReviewRun::Failed("boom".into()));
    assert!(f.threads(cx).is_empty());

    f.reviewer.will_return(Ok(two_findings()));
    f.start(cx);
    assert!(matches!(f.run(cx), ReviewRun::Running { .. }));
    cx.run_until_parked();
    assert!(matches!(f.run(cx), ReviewRun::Done { .. }));
}

#[gpui::test]
async fn a_missing_claude_command_is_a_failure_with_its_own_message(cx: &mut TestAppContext) {
    let f = fixture(cx, Err(ReviewError::NotFound));
    f.start(cx);
    cx.run_until_parked();
    assert_eq!(f.run(cx), ReviewRun::Failed(ReviewError::NotFound.to_string()));
}

#[gpui::test]
async fn a_cancelled_result_that_was_not_stopped_goes_back_to_idle(cx: &mut TestAppContext) {
    let f = fixture(cx, Err(ReviewError::Cancelled));
    f.start(cx);
    cx.run_until_parked();
    assert_eq!(f.run(cx), ReviewRun::Idle);
}

#[gpui::test]
async fn without_a_repo_the_run_fails_before_the_reviewer_is_called(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.session.update(cx, |session, cx| session.set_repo(None, cx));
    f.start(cx);
    cx.run_until_parked();
    assert_eq!(f.run(cx), ReviewRun::Failed(NO_REPO_MESSAGE.into()));
    assert_eq!(f.reviewer.calls(), 0);
}

#[gpui::test]
async fn running_again_replaces_the_earlier_review_threads_and_keeps_the_users(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.start(cx);
    cx.run_until_parked();
    let first: Vec<ThreadId> = f.threads(cx).iter().map(|t| t.id).collect();
    let mine = f.user_thread(cx);
    f.post(cx, mine, "my note");

    f.reviewer.will_return(Ok(result(
        "Second.",
        vec![finding("src/a.ts", Some(3), Severity::Question, "Why")],
    )));
    f.start(cx);
    cx.run_until_parked();

    let threads = f.threads(cx);
    let ids: Vec<ThreadId> = threads.iter().map(|t| t.id).collect();
    assert!(first.iter().all(|id| !ids.contains(id)), "old review threads are gone");
    assert!(ids.contains(&mine));
    assert_eq!(f.thread(cx, mine).unwrap().comments[0].body, "my note");
    let titles: Vec<_> = threads
        .iter()
        .filter_map(|t| t.finding.as_ref().map(|m| m.title.clone()))
        .collect();
    assert_eq!(titles, ["Why"]);
    assert_eq!(threads.len(), 3);
    assert_eq!(f.run(cx).label(), "done:0/0/0/1");

    let removed = f
        .events
        .borrow()
        .iter()
        .filter(|event| matches!(event, ReviewEvent::ThreadsChanged(ThreadChange::Removed(id)) if first.contains(id)))
        .count();
    assert_eq!(removed, first.len());
}

#[gpui::test]
async fn rerunning_ends_a_reply_streaming_in_a_replaced_thread(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.start(cx);
    cx.run_until_parked();
    let summary = f.threads(cx)[0].id;
    f.session.update(cx, |session, cx| session.ask_claude(summary, cx));
    assert!(f.session.read_with(cx, |session, _| session.is_replying(summary)));
    f.start(cx);
    cx.run_until_parked();
    assert!(f.thread(cx, summary).is_none());
    assert!(f.replier.tokens.lock().unwrap()[0].is_cancelled());
}

#[gpui::test]
async fn a_post_without_a_mention_adds_one_comment_and_asks_no_one(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    f.post(cx, id, "  just a note  ");
    cx.run_until_parked();
    let thread = f.thread(cx, id).unwrap();
    assert_eq!(thread.comments.len(), 1);
    assert_eq!(
        (thread.comments[0].author.as_str(), thread.comments[0].body.as_str()),
        (USER_NAME, "just a note")
    );
    assert_eq!(f.replier.asked(), 0);
}

#[gpui::test]
async fn blank_text_is_not_posted(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    let before = f.events.borrow().len();
    f.post(cx, id, " \n ");
    assert!(f.thread(cx, id).unwrap().comments.is_empty());
    assert_eq!(f.events.borrow().len(), before);
}

#[gpui::test]
async fn a_post_that_mentions_claude_starts_a_reply(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude is this needed?");
    let thread = f.thread(cx, id).unwrap();
    assert_eq!(thread.comments.len(), 2);
    assert_eq!(thread.comments[1].author, CLAUDE_NAME);
    assert!(thread.comments[1].is_streaming);
    assert!(thread.comments[1].body.is_empty());
    assert_eq!(f.replier.asked(), 1);
    assert_eq!(
        f.replier.asked.lock().unwrap()[0].comments[0].body,
        "@claude is this needed?"
    );
    assert!(f.session.read_with(cx, |session, _| session.is_replying(id)));
}

#[gpui::test]
async fn an_address_that_only_looks_like_a_mention_does_not_start_a_reply(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    f.post(cx, id, "mail me@claude.com or ask @claudette");
    assert_eq!(f.replier.asked(), 0);
}

#[gpui::test]
async fn ask_claude_replies_without_a_new_user_comment(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    f.post(cx, id, "what about this?");
    f.session.update(cx, |session, cx| session.ask_claude(id, cx));
    let thread = f.thread(cx, id).unwrap();
    assert_eq!(thread.comments.len(), 2);
    assert!(thread.comments[1].is_streaming);
    assert_eq!(f.replier.asked(), 1);
}

#[gpui::test]
async fn a_second_ask_while_a_reply_streams_is_ignored(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude one");
    f.session.update(cx, |session, cx| session.ask_claude(id, cx));
    f.post(cx, id, "@claude two");
    assert_eq!(f.replier.asked(), 1);
    let claude = f
        .thread(cx, id)
        .unwrap()
        .comments
        .iter()
        .filter(|c| c.author == CLAUDE_NAME)
        .count();
    assert_eq!(claude, 1);
}

#[gpui::test]
async fn reply_chunks_append_to_one_comment_and_the_end_of_the_stream_finishes_it(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude hi");
    f.events.borrow_mut().clear();

    f.replier.send("one ");
    cx.run_until_parked();
    f.replier.send("two");
    cx.run_until_parked();
    let streaming = f.thread(cx, id).unwrap();
    assert_eq!(streaming.comments[1].body, "one two");
    assert!(streaming.comments[1].is_streaming);
    assert_eq!(f.events.borrow().len(), 2);

    f.replier.finish();
    cx.run_until_parked();
    let done = f.thread(cx, id).unwrap();
    assert_eq!(done.comments[1].body, "one two");
    assert!(!done.comments[1].is_streaming);
    assert!(!f.session.read_with(cx, |session, _| session.is_replying(id)));

    f.session.update(cx, |session, cx| session.ask_claude(id, cx));
    assert_eq!(f.replier.asked(), 2, "a finished reply allows the next ask");
}

#[gpui::test]
async fn a_reply_that_ends_without_text_says_so_instead_of_staying_empty(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude hi");
    f.replier.finish();
    cx.run_until_parked();
    let comment = f.thread(cx, id).unwrap().comments[1].clone();
    assert_eq!(comment.body, NO_REPLY_MESSAGE);
    assert!(!comment.is_streaming);
}

#[gpui::test]
async fn removing_a_thread_cancels_its_reply_and_drops_late_chunks(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    let other = f.session.update(cx, |session, cx| {
        session.create_thread(ThreadAnchor::file("src/a.ts"), cx)
    });
    f.post(cx, id, "@claude hi");
    f.session.update(cx, |session, cx| session.remove_thread(id, cx));
    assert!(f.thread(cx, id).is_none());
    assert!(f.replier.tokens.lock().unwrap()[0].is_cancelled());
    assert!(
        f.events
            .borrow()
            .contains(&ReviewEvent::ThreadsChanged(ThreadChange::Removed(id)))
    );

    f.replier.send("late");
    cx.run_until_parked();
    assert!(f.thread(cx, id).is_none());
    assert!(f.thread(cx, other).is_some());
}

#[gpui::test]
async fn removing_an_unknown_thread_announces_nothing(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.session
        .update(cx, |session, cx| session.remove_thread(ThreadId(42), cx));
    assert!(f.events.borrow().is_empty());
}

#[gpui::test]
async fn a_second_thread_on_the_same_anchor_is_the_first_one(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let first = f.user_thread(cx);
    let again = f.user_thread(cx);
    assert_eq!(first, again);
    let added = f
        .events
        .borrow()
        .iter()
        .filter(|event| matches!(event, ReviewEvent::ThreadsChanged(ThreadChange::Added(_))))
        .count();
    assert_eq!(added, 1);
}

#[gpui::test]
async fn the_title_follows_the_comparison_and_file_count(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.session
        .update(cx, |session, cx| session.set_branch_name(Some("feature".into()), cx));
    assert_eq!(
        f.session.read_with(cx, |s, _| s.title()),
        "feature compared to main · 1 file"
    );
    f.session
        .update(cx, |session, cx| session.set_comparison(Comparison::Staged, cx));
    assert_eq!(f.session.read_with(cx, |s, _| s.title()), "Staged changes · 1 file");
}

#[gpui::test]
async fn the_setup_is_remembered_per_repo_and_reloaded_when_the_repo_changes(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    f.session.update(cx, |session, cx| {
        session.update_setup(|s| _ = s.focuses.insert("Tests".into()), cx)
    });
    f.start(cx);
    cx.run_until_parked();
    f.session
        .update(cx, |session, cx| session.set_repo(Some(PathBuf::from("/other")), cx));
    assert!(f.session.read_with(cx, |s, _| s.setup().focuses.is_empty()));
    f.session
        .update(cx, |session, cx| session.set_repo(Some(PathBuf::from("/repo")), cx));
    assert!(f.session.read_with(cx, |s, _| s.setup().focuses.contains("Tests")));
}

#[gpui::test]
async fn dropping_the_session_cancels_a_streaming_reply(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude hi");
    let token = f.replier.tokens.lock().unwrap()[0].clone();
    let Fixture { session, .. } = f;
    drop(session);
    cx.update(|_| {});
    cx.run_until_parked();
    assert!(token.is_cancelled());
}

fn texts(cx: &mut TestAppContext, session: &gpui::Entity<diffpilot::review::ReviewSession>) -> Vec<String> {
    session.read_with(cx, |session, _| {
        session
            .threads()
            .flat_map(|thread| thread.comments.iter().map(|comment| comment.body.clone()))
            .collect()
    })
}

#[gpui::test]
async fn threads_survive_opening_the_same_review_again(cx: &mut TestAppContext) {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let first = session_on(cx, &store, Comparison::checked_out("main", true));
    let id = first.update(cx, |session, cx| {
        let id = session.create_thread(ThreadAnchor::line("src/a.ts", Side::New, 1), cx);
        session.post(id, "Rename this.", cx);
        id
    });
    drop(first);
    cx.run_until_parked();

    let second = session_on(cx, &store, Comparison::checked_out("main", true));
    assert_eq!(texts(cx, &second), ["Rename this."]);
    assert_eq!(
        second.read_with(cx, |session, _| session.thread(id).map(|t| t.id)),
        Some(id)
    );
}

#[gpui::test]
async fn two_comparisons_keep_separate_threads(cx: &mut TestAppContext) {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let against_main = session_on(cx, &store, Comparison::checked_out("main", true));
    let against_develop = session_on(cx, &store, Comparison::checked_out("develop", true));
    against_main.update(cx, |session, cx| {
        let id = session.create_thread(ThreadAnchor::file("src/a.ts"), cx);
        session.post(id, "On main.", cx);
    });
    assert_eq!(texts(cx, &against_main), ["On main."]);
    assert!(texts(cx, &against_develop).is_empty());
}

#[gpui::test]
async fn resolving_keeps_the_thread_in_the_store_and_out_of_the_counts(cx: &mut TestAppContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = f.user_thread(cx);
    f.post(cx, id, "Fix this.");
    assert_eq!(f.session.read_with(cx, |s, _| s.thread_count()), 1);

    f.session.update(cx, |session, cx| session.resolve_thread(id, cx));

    assert!(f.threads(cx).is_empty());
    assert_eq!(f.session.read_with(cx, |s, _| s.thread_count()), 0);
    assert!(f.session.read_with(cx, |s, _| s.comment_counts().is_empty()));
    assert_eq!(f.thread(cx, id).unwrap().status, ThreadStatus::Resolved);
    assert_eq!(
        f.events.borrow().last(),
        Some(&ReviewEvent::ThreadsChanged(ThreadChange::Removed(id)))
    );
}
