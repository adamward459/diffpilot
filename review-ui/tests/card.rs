mod common;

use common::*;
use diffpilot_core::review::Severity;
use diffpilot_core::threads::{CLAUDE_NAME, ThreadAnchor, ThreadId};
use diffpilot_review_ui::{ReviewSession, ThreadCard, metrics};
use gpui::{Entity, TestAppContext, VisualTestContext, px};

fn card(
    cx: &mut TestAppContext,
    pick: impl FnOnce(&Fixture, &mut TestAppContext) -> ThreadId,
) -> (Fixture, Entity<ThreadCard>, &mut VisualTestContext) {
    let f = fixture(cx, Ok(two_findings()));
    let id = pick(&f, cx);
    let session: Entity<ReviewSession> = f.session.clone();
    let (card, vcx) = root_window(cx, |_, cx| ThreadCard::new(session, id, cx));
    (f, card, vcx)
}

fn finished_review(f: &Fixture, cx: &mut TestAppContext) {
    f.session.update(cx, |session, cx| session.start_review(cx));
    cx.run_until_parked();
}

fn thread_with(f: &Fixture, cx: &mut TestAppContext, severity: Option<Severity>) -> ThreadId {
    finished_review(f, cx);
    f.threads(cx)
        .into_iter()
        .find(|t| t.finding.as_ref().map(|m| m.severity) == severity)
        .map(|t| t.id)
        .unwrap()
}

#[gpui::test]
async fn a_nit_starts_collapsed_to_its_header(cx: &mut TestAppContext) {
    let (_, card, cx) = card(cx, |f, cx| thread_with(f, cx, Some(Severity::Nit)));
    let measured = cx.update(|window, cx| card.read(cx).height(px(600.), window, cx));
    assert_eq!(f32::from(measured), 2.0 * metrics::BORDER + metrics::FINDING_HEADER);
}

#[gpui::test]
async fn an_open_finding_is_taller_than_its_header_and_a_longer_body_is_taller_still(cx: &mut TestAppContext) {
    let (f, card, cx) = card(cx, |f, cx| thread_with(f, cx, Some(Severity::MustFix)));
    let header = 2.0 * metrics::BORDER + metrics::FINDING_HEADER;
    let short = f32::from(cx.update(|window, cx| card.read(cx).height(px(600.), window, cx)));
    assert!(short >= header + metrics::LINE + metrics::FINDING_FOOTER, "{short}");
    let id = card.read_with(cx, |card, _| card.thread_id());
    f.post(cx, id, &"word ".repeat(400));
    let long = f32::from(cx.update(|window, cx| card.read(cx).height(px(300.), window, cx)));
    assert!(long > short + 5.0 * metrics::LINE, "{long} vs {short}");
}

#[gpui::test]
async fn the_height_does_not_change_between_two_measures(cx: &mut TestAppContext) {
    let (f, card, cx) = card(cx, |f, cx| {
        let id = f.user_thread(cx);
        f.post(cx, id, "a short note with `code`");
        id
    });
    let id = card.read_with(cx, |card, _| card.thread_id());
    f.session.update(cx, |session, cx| session.ask_claude(id, cx));
    f.replier.send("some reply words ");
    cx.run_until_parked();
    let first = cx.update(|window, cx| card.read(cx).height(px(500.), window, cx));
    let again = cx.update(|window, cx| card.read(cx).height(px(500.), window, cx));
    assert_eq!(first, again);
}

#[gpui::test]
async fn a_streaming_reply_makes_the_card_taller_as_text_arrives(cx: &mut TestAppContext) {
    let (f, card, cx) = card(cx, |f, cx| {
        let id = f.user_thread(cx);
        f.post(cx, id, "@claude explain");
        id
    });
    let before = cx.update(|window, cx| card.read(cx).height(px(300.), window, cx));
    f.replier.send(&"long reply ".repeat(200));
    cx.run_until_parked();
    let after = cx.update(|window, cx| card.read(cx).height(px(300.), window, cx));
    assert!(after > before);
}

#[gpui::test]
async fn a_removed_thread_has_no_height(cx: &mut TestAppContext) {
    let (f, card, cx) = card(cx, |f, cx| f.user_thread(cx));
    let id = card.read_with(cx, |card, _| card.thread_id());
    f.session.update(cx, |session, cx| session.remove_thread(id, cx));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|window, cx| card.read(cx).height(px(500.), window, cx)),
        px(0.)
    );
}

#[gpui::test]
async fn typing_in_a_draft_and_pressing_command_return_posts_and_asks_claude(cx: &mut TestAppContext) {
    let (f, card, cx) = card(cx, |f, cx| {
        f.session.update(cx, |session, cx| {
            session.create_thread(
                ThreadAnchor::line("src/a.ts", diffpilot_core::threads::Side::New, 2),
                cx,
            )
        })
    });
    let id = card.read_with(cx, |card, _| card.thread_id());
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
    cx.simulate_input("@claude is this needed?");
    cx.simulate_keystrokes("cmd-enter");
    cx.run_until_parked();

    let thread = f.thread(cx, id).unwrap();
    assert_eq!(thread.comments.len(), 2, "{thread:?}");
    assert_eq!(thread.comments[0].body, "@claude is this needed?");
    assert_eq!(thread.comments[1].author, CLAUDE_NAME);
    assert_eq!(f.replier.asked(), 1);
}
