mod common;

use common::*;
use diffpilot::review::session::{NO_REPLY_MESSAGE, NO_REPO_MESSAGE};
use diffpilot::review::{ReviewEvent, ThreadChange};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::{Effort, ModelName, ReviewModel, ReviewScope, Severity};
use diffpilot_core::run::{RunKind, RunState};
use diffpilot_core::store::Store;
use diffpilot_core::threads::ThreadStatus;
use diffpilot_core::threads::{CLAUDE_NAME, Side, ThreadAnchor, ThreadId, USER_NAME};
use gpui::TestAppContext;
use std::path::PathBuf;
use std::sync::Arc;

#[gpui::test]
async fn a_run_ends_done_with_threads_planned_from_the_findings(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    f.start(cx);
    assert!(f.session.read_with(cx, |s, _| s.reviewing()));
    f.claude.finish();
    f.wait_for_end(cx);

    assert_eq!(f.state(cx), Some(RunState::Done));
    let counts = f.session.read_with(cx, |s, _| s.review_counts());
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
    let f = fixture(cx, two_findings());
    f.review(cx);
    let events = f.events.borrow().clone();
    assert_eq!(events.first(), Some(&ReviewEvent::RunChanged));
    assert_eq!(events.last(), Some(&ReviewEvent::RunChanged));
    assert!(events.contains(&ReviewEvent::RunFinished(RunState::Done)));
    let added = events
        .iter()
        .filter(|event| matches!(event, ReviewEvent::ThreadsChanged(ThreadChange::Added(_))))
        .count();
    assert_eq!(added, 3);
}

#[gpui::test]
async fn the_request_carries_the_setup_the_popover_collected(cx: &mut TestAppContext) {
    let f = fixture(cx, result("", vec![]));
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
    f.review(cx);
    let prompt = f.claude.prompt();
    assert!(prompt.contains("git diff --merge-base main -- 'src/a.ts'"), "{prompt}");
    assert!(prompt.contains("Security"));
    assert!(prompt.contains("> look at auth"));
    assert_eq!(f.claude.model(), "haiku");
    let run = f.current(cx).unwrap();
    assert_eq!(run.model, ReviewModel::new(ModelName::Haiku, Effort::Low));
    assert_eq!(
        f.settings.reviews_model(),
        ReviewModel::new(ModelName::Haiku, Effort::Low)
    );
    let repo = f.session.read_with(cx, |s, _| s.host().repo.clone().unwrap());
    assert_eq!(f.settings.review_focuses(&repo), ["Security"]);
}

#[gpui::test]
async fn an_empty_review_still_ends_done_with_no_threads(cx: &mut TestAppContext) {
    let f = fixture(cx, result("", vec![]));
    f.review(cx);
    assert_eq!(f.state(cx), Some(RunState::Done));
    assert!(f.session.read_with(cx, |s, _| s.review_counts().is_empty()));
    assert!(f.threads(cx).is_empty());
}

#[gpui::test]
async fn stop_ends_the_run_as_stopped_and_adds_no_threads(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    f.start(cx);
    f.wait_for_launches(cx, 1);
    f.session.update(cx, |session, cx| session.stop_claude(cx));
    f.wait_for_end(cx);
    assert_eq!(f.state(cx), Some(RunState::Stopped));
    assert_eq!(f.problem(cx), None);
    assert!(f.threads(cx).is_empty());
}

#[gpui::test]
async fn stopping_when_nothing_runs_changes_nothing(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.session.update(cx, |session, cx| session.stop_claude(cx));
    assert!(f.events.borrow().is_empty());
    f.review(cx);
    let before = f.events.borrow().len();
    f.session.update(cx, |session, cx| session.stop_claude(cx));
    assert_eq!(f.events.borrow().len(), before);
    assert_eq!(f.state(cx), Some(RunState::Done));
}

#[gpui::test]
async fn starting_while_a_run_is_going_queues_the_next_one_behind_it(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    f.start(cx);
    f.wait_for_launches(cx, 1);
    f.start(cx);
    assert_eq!(f.session.read_with(cx, |s, _| s.queued().len()), 1);
    cx.run_until_parked();
    assert_eq!(f.claude.launches(), 1);

    f.claude.finish();
    f.wait_for_launches(cx, 2);
    f.wait_for_end(cx);
    assert_eq!(f.claude.launches(), 2);
    assert!(f.session.read_with(cx, |s, _| s.queued().is_empty()));
}

#[gpui::test]
async fn a_failure_shows_its_message_and_the_next_run_can_succeed(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.fail_with("boom");
    f.review(cx);
    assert_eq!(f.state(cx), Some(RunState::Failed));
    assert_eq!(f.problem(cx).as_deref(), Some("boom"));
    assert!(f.threads(cx).is_empty());
    assert!(f.events.borrow().contains(&ReviewEvent::RunFinished(RunState::Failed)));

    f.claude.review_with(&two_findings());
    f.review(cx);
    assert_eq!(f.state(cx), Some(RunState::Done));
    assert_eq!(f.problem(cx), None);
    assert_eq!(f.threads(cx).len(), 3);
}

#[gpui::test]
async fn a_claude_command_that_cannot_start_is_a_failure_with_its_message(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.engine().set_claude_binary(Some("/nonexistent/claude".into()));
    f.review(cx);
    assert_eq!(f.state(cx), Some(RunState::Failed));
    assert!(f.problem(cx).is_some());
}

#[gpui::test]
async fn without_a_repo_the_run_fails_before_claude_starts(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.session.update(cx, |session, cx| session.set_repo(None, cx));
    f.start(cx);
    cx.run_until_parked();
    assert_eq!(f.problem(cx).as_deref(), Some(NO_REPO_MESSAGE));
    assert_eq!(f.current(cx), None);
    assert_eq!(f.claude.launches(), 0);
}

#[gpui::test]
async fn running_again_replaces_the_earlier_review_threads_when_it_is_done_and_keeps_the_users(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx, two_findings());
    f.review(cx);
    let first: Vec<ThreadId> = f.threads(cx).iter().map(|t| t.id).collect();
    let mine = f.user_thread(cx);
    f.post(cx, mine, "my note");

    f.claude.review_with(&result(
        "Second.",
        vec![finding("src/a.ts", Some(3), Severity::Question, "Why")],
    ));
    f.claude.hold();
    f.start(cx);
    f.wait_for_launches(cx, 2);
    assert!(
        first.iter().all(|id| f.thread(cx, *id).is_some()),
        "the old threads stay while the new review runs"
    );
    f.claude.finish();
    f.wait_for_end(cx);

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
    let counts = f.session.read_with(cx, |s, _| s.review_counts());
    assert_eq!(counts.get(&Severity::Question), Some(&1));
    assert_eq!(counts.len(), 1);

    let removed = f
        .events
        .borrow()
        .iter()
        .filter(|event| matches!(event, ReviewEvent::ThreadsChanged(ThreadChange::Removed(id)) if first.contains(id)))
        .count();
    assert_eq!(removed, first.len());
}

#[gpui::test]
async fn a_failed_second_review_keeps_the_first_ones_threads(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.review(cx);
    let first: Vec<ThreadId> = f.threads(cx).iter().map(|t| t.id).collect();
    f.claude.fail_with("boom");
    f.review(cx);
    assert_eq!(f.state(cx), Some(RunState::Failed));
    assert!(first.iter().all(|id| f.thread(cx, *id).is_some()));
}

#[gpui::test]
async fn a_reply_to_a_thread_the_next_review_replaces_leaves_nothing_behind(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.review(cx);
    let summary = f.threads(cx)[0].id;
    f.claude.hold();
    f.session.update(cx, |session, cx| session.ask_claude(summary, cx));
    assert!(f.replying(cx, summary));
    f.start(cx);
    f.claude.finish();
    f.wait_for_end(cx);
    assert!(f.thread(cx, summary).is_none());
    assert!(!f.replying(cx, summary));
}

#[gpui::test]
async fn run_again_starts_a_review_with_the_current_setup_once_the_run_has_ended(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    assert!(!f.session.read_with(cx, |s, _| s.can_run_again()));
    f.review(cx);
    assert!(f.session.read_with(cx, |s, _| s.can_run_again()));
    f.session.update(cx, |session, cx| session.run_again(cx));
    f.wait_for_end(cx);
    assert_eq!(f.claude.launches(), 2);
}

#[gpui::test]
async fn a_queued_review_replaces_the_findings_of_the_review_before_it(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    f.start(cx);
    f.wait_for_launches(cx, 1);
    f.start(cx);
    f.claude.finish();
    f.wait_for_launches(cx, 2);
    f.wait_for_end(cx);
    assert_eq!(f.threads(cx).len(), 3);
}

#[gpui::test]
async fn a_review_that_returns_nothing_keeps_the_old_findings(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.review(cx);
    let first: Vec<ThreadId> = f.threads(cx).iter().map(|t| t.id).collect();
    assert_eq!(first.len(), 3);

    f.claude.end_without_review();
    f.review(cx);
    assert_eq!(f.state(cx), Some(RunState::Done));
    let kept: Vec<ThreadId> = f.threads(cx).iter().map(|t| t.id).collect();
    assert_eq!(kept, first);
}

#[gpui::test]
async fn a_post_without_a_mention_adds_one_comment_and_asks_no_one(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    let id = f.user_thread(cx);
    f.post(cx, id, "  just a note  ");
    cx.run_until_parked();
    let thread = f.thread(cx, id).unwrap();
    assert_eq!(thread.comments.len(), 1);
    assert_eq!(
        (thread.comments[0].author.as_str(), thread.comments[0].body.as_str()),
        (USER_NAME, "just a note")
    );
    assert!(f.current(cx).is_none(), "no run was asked for");
}

#[gpui::test]
async fn blank_text_is_not_posted(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    let id = f.user_thread(cx);
    let before = f.events.borrow().len();
    f.post(cx, id, " \n ");
    assert!(f.thread(cx, id).unwrap().comments.is_empty());
    assert_eq!(f.events.borrow().len(), before);
}

#[gpui::test]
async fn a_post_that_mentions_claude_queues_a_reply_run_for_its_thread(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude is this needed?");
    let thread = f.thread(cx, id).unwrap();
    assert_eq!(thread.comments.len(), 1, "no empty Claude comment waits in the thread");
    assert!(f.replying(cx, id));
    let run = f.current(cx).unwrap();
    assert_eq!((run.kind, run.threads.as_slice()), (RunKind::Reply, [id].as_slice()));
    f.wait_for_launches(cx, 1);
    let prompt = f.claude.prompt();
    assert!(prompt.contains(&format!("- Thread `{}`", id.0)), "{prompt}");
    assert!(prompt.contains("Make the smallest change"), "{prompt}");
    f.claude.finish();
}

#[gpui::test]
async fn a_reply_that_cannot_edit_is_told_so(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    f.settings.set_edit_permission(EditPermission::AskEach);
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude rename this");
    f.wait_for_launches(cx, 1);
    assert!(f.claude.prompt().contains("You cannot edit files in this run."));
    f.claude.finish();
}

#[gpui::test]
async fn an_address_that_only_looks_like_a_mention_does_not_start_a_reply(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    let id = f.user_thread(cx);
    f.post(cx, id, "mail me@claude.com or ask @claudette");
    assert!(f.current(cx).is_none());
}

#[gpui::test]
async fn ask_claude_replies_without_a_new_user_comment(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    let id = f.user_thread(cx);
    f.post(cx, id, "what about this?");
    f.session.update(cx, |session, cx| session.ask_claude(id, cx));
    assert_eq!(f.thread(cx, id).unwrap().comments.len(), 1);
    assert!(f.replying(cx, id));
    f.claude.finish();
}

#[gpui::test]
async fn a_second_ask_while_a_reply_runs_is_ignored(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude one");
    f.session.update(cx, |session, cx| session.ask_claude(id, cx));
    f.post(cx, id, "@claude two");
    let runs = f.session.read_with(cx, |session, _| {
        session.current().into_iter().chain(session.queued()).count()
    });
    assert_eq!(runs, 1);
    f.claude.finish();
}

#[gpui::test]
async fn claudes_reply_shows_in_the_thread_and_the_end_of_the_run_allows_the_next_ask(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude hi");
    f.claude_replies(cx, id, "Hello. The line is needed.");
    assert!(f.replying(cx, id), "the run goes on until Claude ends it");
    f.claude.finish();
    f.wait_for_reply_end(cx, id);
    let bodies: Vec<String> = f
        .thread(cx, id)
        .unwrap()
        .comments
        .iter()
        .map(|c| c.body.clone())
        .collect();
    assert_eq!(
        bodies,
        ["@claude hi", "Hello. The line is needed."],
        "no note after a reply"
    );
    assert!(f.thread(cx, id).unwrap().addressed.is_none(), "an answer is not a fix");

    f.session.update(cx, |session, cx| session.ask_claude(id, cx));
    assert!(f.replying(cx, id), "a finished reply allows the next ask");
    f.wait_for_reply_end(cx, id);
}

#[gpui::test]
async fn a_reply_run_that_made_the_change_marks_the_thread_addressed(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude rename it");
    f.claude_calls(
        cx,
        "resolve",
        serde_json::json!({ "threadId": id.0.to_string(), "summary": "Fixed: renamed it." }),
    );
    f.wait_for_comment(cx, id, "Fixed: renamed it.");
    f.claude.finish();
    f.wait_for_reply_end(cx, id);
    let thread = f.thread(cx, id).unwrap();
    assert_eq!(
        thread.addressed.map(|mark| mark.summary).as_deref(),
        Some("Fixed: renamed it.")
    );
    assert_eq!(
        thread.status,
        ThreadStatus::Open,
        "the reviewer resolves it after a check"
    );
}

#[gpui::test]
async fn a_reply_run_that_ends_without_a_reply_says_so(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude hi");
    f.wait_for_reply_end(cx, id);
    let comment = f.thread(cx, id).unwrap().comments[1].clone();
    assert_eq!(
        (comment.author.as_str(), comment.body.as_str()),
        (CLAUDE_NAME, NO_REPLY_MESSAGE)
    );
}

#[gpui::test]
async fn a_stopped_reply_run_leaves_no_note(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude hi");
    f.wait_for_launches(cx, 1);
    f.session.update(cx, |session, cx| session.stop_claude(cx));
    f.wait_for_reply_end(cx, id);
    assert_eq!(f.state(cx), Some(RunState::Stopped));
    assert_eq!(f.thread(cx, id).unwrap().comments.len(), 1);
}

#[gpui::test]
async fn removing_a_thread_stops_its_reply_run(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    let id = f.user_thread(cx);
    let other = f.session.update(cx, |session, cx| {
        session.create_thread(ThreadAnchor::file("src/a.ts"), cx)
    });
    f.post(cx, id, "@claude hi");
    f.wait_for_launches(cx, 1);
    f.session.update(cx, |session, cx| session.remove_thread(id, cx));
    assert!(f.thread(cx, id).is_none());
    assert!(
        f.events
            .borrow()
            .contains(&ReviewEvent::ThreadsChanged(ThreadChange::Removed(id)))
    );
    f.wait_for_end(cx);
    assert_eq!(f.state(cx), Some(RunState::Stopped));
    assert!(f.thread(cx, other).is_some());
}

#[gpui::test]
async fn removing_an_unknown_thread_announces_nothing(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.session
        .update(cx, |session, cx| session.remove_thread(ThreadId(42), cx));
    assert!(f.events.borrow().is_empty());
}

#[gpui::test]
async fn a_second_thread_on_the_same_anchor_is_the_first_one(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
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
    let f = fixture(cx, two_findings());
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
    let f = fixture(cx, two_findings());
    let repo = f.session.read_with(cx, |s, _| s.host().repo.clone().unwrap());
    f.session.update(cx, |session, cx| {
        session.update_setup(|s| _ = s.focuses.insert("Tests".into()), cx)
    });
    f.review(cx);
    f.session
        .update(cx, |session, cx| session.set_repo(Some(PathBuf::from("/other")), cx));
    assert!(f.session.read_with(cx, |s, _| s.setup().focuses.is_empty()));
    f.session.update(cx, |session, cx| session.set_repo(Some(repo), cx));
    assert!(f.session.read_with(cx, |s, _| s.setup().focuses.contains("Tests")));
}

#[gpui::test]
async fn dropping_the_session_stops_its_reply_run(cx: &mut TestAppContext) {
    let f = fixture(cx, two_findings());
    f.claude.hold();
    let id = f.user_thread(cx);
    f.post(cx, id, "@claude hi");
    f.wait_for_launches(cx, 1);
    let run = f.current(cx).unwrap().id;
    let engine = f.claude.engine();
    let Fixture { session, .. } = f;
    drop(session);
    cx.update(|_| {});
    wait_until(cx, "the reply run to stop", |_| {
        engine.run(run).is_some_and(|run| run.state == RunState::Stopped)
    });
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
    let f = fixture(cx, two_findings());
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
