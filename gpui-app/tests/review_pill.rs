mod common;

use common::*;
use diffpilot::review::pill::RESULT_SECONDS;
use diffpilot::review::{PillEvent, ReviewPill, StopReview};
use diffpilot_core::claude::ReviewError;
use diffpilot_core::review::ReviewRun;
use diffpilot_core::threads::ThreadAnchor;
use gpui::{Entity, TestAppContext, VisualTestContext};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

struct Pill {
    f: Fixture,
    pill: Entity<ReviewPill>,
    events: Rc<RefCell<Vec<PillEvent>>>,
}

fn pill(
    cx: &mut TestAppContext,
    outcome: Result<diffpilot_core::review::ReviewResult, ReviewError>,
) -> (Pill, &mut VisualTestContext) {
    let f = fixture(cx, outcome);
    let session = f.session.clone();
    let (pill, vcx) = root_window(cx, |window, cx| ReviewPill::new(session, window, cx));
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    vcx.update(|_, cx| {
        cx.subscribe(&pill, move |_, event: &PillEvent, _| sink.borrow_mut().push(*event))
            .detach();
    });
    (Pill { f, pill, events }, vcx)
}

impl Pill {
    fn needs_room(&self, cx: &mut VisualTestContext) -> bool {
        self.pill.read_with(cx, |pill, _| pill.needs_room())
    }
}

#[gpui::test]
async fn the_pill_needs_room_while_running_and_while_its_result_shows(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, Ok(two_findings()));
    assert!(!p.needs_room(cx));
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    assert!(p.needs_room(cx));
    cx.run_until_parked();
    assert!(p.needs_room(cx), "the result shows");
    assert!(p.pill.read_with(cx, |pill, _| pill.shows_result()));

    cx.executor().advance_clock(Duration::from_secs(RESULT_SECONDS - 1));
    assert!(p.needs_room(cx));
    cx.executor().advance_clock(Duration::from_secs(2));
    assert!(!p.needs_room(cx));
    assert!(!p.pill.read_with(cx, |pill, _| pill.shows_result()));
    assert_eq!(
        *p.events.borrow(),
        [PillEvent::NeedsRoomChanged(true), PillEvent::NeedsRoomChanged(false)]
    );
}

#[gpui::test]
async fn stopping_gives_the_room_back_at_once(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, Ok(two_findings()));
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    assert!(p.needs_room(cx));
    p.pill.update(cx, |pill, cx| pill.stop(cx));
    assert!(!p.needs_room(cx));
    assert_eq!(p.f.session.read_with(cx, |s, _| s.run().clone()), ReviewRun::Idle);
}

#[gpui::test]
async fn command_period_stops_a_running_review_wherever_the_focus_is(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, Ok(two_findings()));
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    cx.simulate_keystrokes("cmd-.");
    assert_eq!(p.f.session.read_with(cx, |s, _| s.run().clone()), ReviewRun::Idle);
    cx.run_until_parked();
    assert_eq!(p.f.session.read_with(cx, |s, _| s.run().clone()), ReviewRun::Idle);
    cx.dispatch_action(StopReview);
}

#[gpui::test]
async fn a_failed_run_opens_the_popover_and_a_new_run_can_close_it(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, Err(ReviewError::Failed("boom".into())));
    assert!(!p.pill.read_with(cx, |pill, _| pill.is_open()));
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    cx.run_until_parked();
    assert!(p.pill.read_with(cx, |pill, _| pill.is_open()));
    assert!(!p.needs_room(cx), "a failure keeps the toolbar compact");
    p.pill.update(cx, |pill, cx| pill.set_open(false, cx));
    assert!(!p.pill.read_with(cx, |pill, _| pill.is_open()));
}

#[gpui::test]
async fn the_comment_button_opens_a_file_thread_and_tells_the_host(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, Ok(two_findings()));
    p.pill.update(cx, |pill, cx| pill.comment_on_file(cx));
    let threads =
        p.f.session
            .read_with(cx, |s, _| s.threads().cloned().collect::<Vec<_>>());
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].anchor, ThreadAnchor::file("src/a.ts"));
    assert_eq!(*p.events.borrow(), [PillEvent::CommentOnFile(threads[0].id)]);

    p.f.session.update(cx, |session, cx| session.set_visible_path(None, cx));
    p.pill.update(cx, |pill, cx| pill.comment_on_file(cx));
    assert_eq!(p.f.session.read_with(cx, |s, _| s.thread_count()), 1);
}

#[gpui::test]
async fn the_pill_draws_in_every_run_state(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, Ok(two_findings()));
    cx.run_until_parked();
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    cx.run_until_parked();
    p.pill.update(cx, |pill, cx| pill.set_open(true, cx));
    cx.run_until_parked();
    p.f.reviewer.will_return(Err(ReviewError::Failed("boom".into())));
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    cx.run_until_parked();
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    cx.run_until_parked();
}
