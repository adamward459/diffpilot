mod common;

use common::*;
use diffpilot::review::pill::RESULT_SECONDS;
use diffpilot::review::{PillEvent, ReviewPill, StopReview};
use diffpilot_core::review::ReviewResult;
use diffpilot_core::run::RunState;
use gpui::{Entity, TestAppContext, VisualTestContext};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

struct Pill {
    f: Fixture,
    pill: Entity<ReviewPill>,
    events: Rc<RefCell<Vec<PillEvent>>>,
}

fn pill(cx: &mut TestAppContext, result: ReviewResult) -> (Pill, &mut VisualTestContext) {
    let f = fixture(cx, result);
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

    fn wait_for_end(&self, cx: &mut VisualTestContext) {
        cx.executor().allow_parking();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self
            .f
            .session
            .read_with(cx, |s, _| s.current().is_some_and(|run| run.state.is_finished()))
        {
            assert!(Instant::now() < deadline, "the run did not end");
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(20));
        }
        cx.run_until_parked();
    }
}

#[gpui::test]
async fn the_pill_needs_room_while_running_and_while_its_result_shows(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, two_findings());
    p.f.claude.hold();
    assert!(!p.needs_room(cx));
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    assert!(p.needs_room(cx));
    p.f.claude.finish();
    p.wait_for_end(cx);
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
async fn stopping_gives_the_room_back_once_the_run_has_stopped(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, two_findings());
    p.f.claude.hold();
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    assert!(p.needs_room(cx));
    p.pill.update(cx, |pill, cx| pill.stop(cx));
    p.wait_for_end(cx);
    assert!(!p.needs_room(cx));
    assert_eq!(p.f.state(cx), Some(RunState::Stopped));
}

#[gpui::test]
async fn command_period_stops_a_running_review_wherever_the_focus_is(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, two_findings());
    p.f.claude.hold();
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    p.f.wait_for_launches(cx, 1);
    cx.simulate_keystrokes("cmd-.");
    p.wait_for_end(cx);
    assert_eq!(p.f.state(cx), Some(RunState::Stopped));
    cx.dispatch_action(StopReview);
}

#[gpui::test]
async fn a_failed_run_opens_the_popover_and_a_new_run_can_close_it(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, two_findings());
    p.f.claude.fail_with("boom");
    assert!(!p.pill.read_with(cx, |pill, _| pill.is_open()));
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    p.wait_for_end(cx);
    assert!(p.pill.read_with(cx, |pill, _| pill.is_open()));
    assert!(!p.needs_room(cx), "a failure keeps the toolbar compact");
    p.pill.update(cx, |pill, cx| pill.set_open(false, cx));
    assert!(!p.pill.read_with(cx, |pill, _| pill.is_open()));
}

#[gpui::test]
async fn the_pill_draws_in_every_run_state(cx: &mut TestAppContext) {
    let (p, cx) = pill(cx, two_findings());
    cx.run_until_parked();
    p.f.claude.hold();
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    cx.run_until_parked();
    p.pill.update(cx, |pill, cx| pill.set_open(true, cx));
    cx.run_until_parked();
    p.f.claude.fail_with("boom");
    p.f.claude.finish();
    p.wait_for_end(cx);
    p.f.session.update(cx, |session, cx| session.start_review(cx));
    p.wait_for_end(cx);
}
