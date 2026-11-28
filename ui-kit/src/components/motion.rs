use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, ElementId, Entity, EntityId, SharedString, Styled, Window,
    ease_out_quint, px,
};
use std::cell::Cell;
use std::time::{Duration, Instant};

pub const OVERLAY_DURATION: Duration = Duration::from_millis(160);
pub const PANEL_DURATION: Duration = Duration::from_millis(220);
pub const STATE_DURATION: Duration = Duration::from_millis(140);
pub const RISE_DURATION: Duration = Duration::from_millis(300);
pub const CARD_DELAY: Duration = Duration::from_millis(140);
pub const POP_DURATION: Duration = Duration::from_millis(360);
pub const PROGRESS_DURATION: Duration = Duration::from_millis(450);
pub const BAND_STEP: Duration = Duration::from_millis(40);
pub const BAND_FADE: Duration = Duration::from_millis(280);
pub const PULSE_PERIOD: Duration = Duration::from_millis(1600);
const SLIDE: f32 = 6.;
const RISE: f32 = 8.;

thread_local! {
    static CLOSING: Cell<usize> = const { Cell::new(0) };
}

/// How many close animations on this thread are still waiting to unmount their component. Tests use it to move a fake clock
/// forward only when a close is pending.
pub fn closing_motions() -> usize {
    CLOSING.with(Cell::get)
}

/// True when the user asked the system to reduce motion. New animations skip their move when it is on.
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs, reason = "objc 0.2 macros test a cargo-clippy cfg")]
pub fn reduced() -> bool {
    use objc::runtime::{BOOL, Object, YES};
    use objc::{class, msg_send, sel, sel_impl};
    // SAFETY: `NSWorkspace.sharedWorkspace` is a class singleton and the property returns a plain BOOL.
    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let on: BOOL = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
        on == YES
    }
}

#[cfg(not(target_os = "macos"))]
pub fn reduced() -> bool {
    false
}

/// Progress from 0 to 1 of a move that starts after `delay` and then takes `duration`, eased out.
pub fn staged(elapsed: Duration, delay: Duration, duration: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() - delay.as_secs_f32()) / duration.as_secs_f32();
    ease_out_quint()(t.clamp(0., 1.))
}

/// How far a staggered line group has come: line `index` starts `index * step` after the first.
pub fn stagger(elapsed: Duration, index: usize, step: Duration, duration: Duration) -> f32 {
    staged(elapsed, step * index as u32, duration)
}

/// When the last of `count` staggered lines is done.
pub fn stagger_end(count: usize, step: Duration, duration: Duration) -> Duration {
    step * count.saturating_sub(1) as u32 + duration
}

/// Where a looping motion is in its cycle, from 0 up to but not including 1. All loops share one clock, so
/// they stay in step across redraws.
pub fn loop_phase(period: Duration) -> f32 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    cycle(START.get_or_init(Instant::now).elapsed(), period)
}

fn cycle(elapsed: Duration, period: Duration) -> f32 {
    (elapsed.as_secs_f32() % period.as_secs_f32()) / period.as_secs_f32()
}

/// Scale of a check that pops in: 0.3, up to 1.15 at 60% of the move, then back to 1. `t` goes from 0 to 1.
pub fn pop_scale(t: f32) -> f32 {
    const PEAK: f32 = 0.6;
    if t < PEAK {
        0.3 + (1.15 - 0.3) * ease_out_quint()(t / PEAK)
    } else {
        1.15 - 0.15 * ((t - PEAK) / (1. - PEAK)).min(1.)
    }
}

/// Fades `element` in while it rises a few px, after `delay`. A new `key` plays it again.
pub fn rise<E: Styled + IntoElement + 'static>(key: impl Into<ElementId>, delay: Duration, element: E) -> AnyElement {
    if reduced() {
        return element.into_any_element();
    }
    let total = delay + RISE_DURATION;
    element
        .relative()
        .with_animation(key, Animation::new(total), move |element, t| {
            let moved = staged(total.mul_f32(t), delay, RISE_DURATION);
            element.opacity(moved).top(px(RISE * (1. - moved)))
        })
        .into_any_element()
}

struct Closing;

impl Closing {
    fn begin() -> Self {
        CLOSING.with(|count| count.set(count.get() + 1));
        Self
    }
}

impl Drop for Closing {
    fn drop(&mut self) {
        CLOSING.with(|count| count.set(count.get() - 1));
    }
}

/// A component that stays mounted while it closes, so the close can animate. Each open or close starts a new
/// generation, which keys the animation so it replays. An interrupted move starts from where the last one was.
#[derive(Clone, Copy, Debug)]
pub struct Presence {
    open: bool,
    mounted: bool,
    generation: u32,
    duration: Duration,
    from: f32,
    since: Option<Instant>,
}

impl Presence {
    pub fn new(open: bool) -> Self {
        Self::with_duration(open, PANEL_DURATION)
    }

    pub fn overlay(open: bool) -> Self {
        Self::with_duration(open, OVERLAY_DURATION)
    }

    /// An overlay that opens as soon as it is drawn, for one that only exists while it is open.
    pub fn entering() -> Self {
        let mut presence = Self::overlay(false);
        presence.begin(true);
        presence
    }

    fn with_duration(open: bool, duration: Duration) -> Self {
        Self {
            open,
            mounted: open,
            generation: 0,
            duration,
            from: 0.,
            since: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// True while the component is on screen, including its close animation.
    pub fn is_mounted(&self) -> bool {
        self.mounted
    }

    pub fn is_closing(&self) -> bool {
        self.mounted && !self.open
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// How open the component is right now, from 0 (closed) to 1 (open).
    pub fn openness(&self) -> f32 {
        let target = if self.open { 1. } else { 0. };
        match self.since {
            None => target,
            Some(since) => {
                let t = (since.elapsed().as_secs_f32() / self.duration.as_secs_f32()).min(1.);
                self.from + (target - self.from) * ease_out_quint()(t)
            }
        }
    }

    /// Opens or closes a field of the view `T`. A close unmounts the field's component after its animation,
    /// unless it was opened again in the meantime.
    pub fn set_open<T: 'static>(&mut self, open: bool, cx: &mut Context<T>, field: fn(&mut T) -> &mut Presence) {
        let Some(generation) = self.begin(open) else { return };
        cx.notify();
        if !open {
            let (duration, pending) = (self.duration, Closing::begin());
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(duration).await;
                drop(pending);
                this.update(cx, |this, cx| {
                    if field(this).settle(generation) {
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// Like `set_open`, for a presence kept in keyed state. `view` is re-rendered when the close is over.
    pub fn drive(state: &Entity<Presence>, open: bool, view: EntityId, cx: &mut App) {
        let started = state.update(cx, |presence, cx| {
            let generation = presence.begin(open);
            if generation.is_some() {
                cx.notify();
            }
            generation.map(|generation| (generation, presence.duration))
        });
        let Some((generation, duration)) = started.filter(|_| !open) else {
            return;
        };
        let (state, pending) = (state.clone(), Closing::begin());
        cx.spawn(async move |cx| {
            cx.background_executor().timer(duration).await;
            drop(pending);
            state
                .update(cx, |presence, cx| {
                    if presence.settle(generation) {
                        cx.notify();
                    }
                })
                .ok();
            cx.update(|cx| cx.notify(view)).ok();
        })
        .detach();
    }

    fn begin(&mut self, open: bool) -> Option<u32> {
        if self.open == open {
            return None;
        }
        self.from = self.openness();
        self.open = open;
        self.mounted = true;
        self.generation += 1;
        self.since = Some(Instant::now());
        Some(self.generation)
    }

    fn settle(&mut self, generation: u32) -> bool {
        let settled = !self.open && self.generation == generation && self.mounted;
        if settled {
            self.mounted = false;
        }
        settled
    }

    /// Fades an overlay in and out while it slides `SLIDE` px. `from_above` is true when the overlay opens below
    /// its trigger, so it drops down; otherwise it rises.
    pub fn slide<E: Styled + IntoElement + 'static>(
        &self,
        name: impl Into<SharedString>,
        element: E,
        from_above: bool,
    ) -> AnyElement {
        let sign = if from_above { -1. } else { 1. };
        self.animate(name, element.relative(), move |element, t| {
            element.opacity(t).top(px(sign * SLIDE * (1. - t)))
        })
    }

    /// Wraps `element` in the presence animation. `apply` gets the element and how open the component is, from
    /// 0 (closed) to 1 (open). A component that has not moved yet is drawn fully open.
    pub fn animate<E: IntoElement + 'static>(
        &self,
        name: impl Into<SharedString>,
        element: E,
        apply: impl Fn(E, f32) -> E + 'static,
    ) -> AnyElement {
        if self.generation == 0 {
            return apply(element, if self.open { 1. } else { 0. }).into_any_element();
        }
        let (from, to) = (self.from, if self.open { 1. } else { 0. });
        element
            .with_animation(
                ElementId::NamedInteger(name.into(), self.generation as u64),
                Animation::new(self.duration).with_easing(ease_out_quint()),
                move |element, t| apply(element, from + (to - from) * t),
            )
            .into_any_element()
    }
}

/// A control value that moves to a new target over `STATE_DURATION`, such as a switch thumb or the chosen segment.
/// A stateless control keeps its `Transition` in keyed state, so it knows the value it came from.
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    from: f32,
    to: f32,
    generation: u32,
    since: Option<Instant>,
    duration: Duration,
    ease: Ease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ease {
    OutQuint,
    Linear,
}

impl Ease {
    fn at(self, t: f32) -> f32 {
        match self {
            Self::OutQuint => ease_out_quint()(t),
            Self::Linear => t,
        }
    }
}

impl Transition {
    pub fn new(to: f32) -> Self {
        Self {
            from: to,
            to,
            generation: 0,
            since: None,
            duration: STATE_DURATION,
            ease: Ease::OutQuint,
        }
    }

    /// The transition of the control keyed by `id` toward `to`. The first call for an `id` has nothing to move
    /// from, so the control is drawn at rest.
    pub fn follow(id: impl Into<ElementId>, to: f32, window: &mut Window, cx: &mut App) -> Self {
        Self::follow_over(id, to, STATE_DURATION, Ease::OutQuint, window, cx)
    }

    /// Like `follow`, with its own duration and easing. With reduced motion the value jumps to `to`.
    pub fn follow_over(
        id: impl Into<ElementId>,
        to: f32,
        duration: Duration,
        ease: Ease,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let state = window.use_keyed_state(id, cx, |_, _| Transition::new(to));
        state.update(cx, |transition, _| {
            transition.duration = duration;
            transition.ease = ease;
            transition.retarget(to);
        });
        if reduced() {
            return Self::new(to);
        }
        *state.read(cx)
    }

    pub fn from(&self) -> f32 {
        self.from
    }

    pub fn to(&self) -> f32 {
        self.to
    }

    pub fn is_moving(&self) -> bool {
        self.generation > 0
    }

    pub fn value(&self) -> f32 {
        let Some(since) = self.since else { return self.to };
        let t = (since.elapsed().as_secs_f32() / self.duration.as_secs_f32()).min(1.);
        self.from + (self.to - self.from) * self.ease.at(t)
    }

    fn retarget(&mut self, to: f32) {
        if self.to != to {
            self.from = self.value();
            self.to = to;
            self.generation += 1;
            self.since = Some(Instant::now());
        }
    }

    /// Wraps `element` in the move. `apply` gets the element and the value between `from` and `to`.
    pub fn animate<E: IntoElement + 'static>(
        &self,
        name: impl Into<SharedString>,
        element: E,
        apply: impl Fn(E, f32) -> E + 'static,
    ) -> AnyElement {
        if !self.is_moving() {
            return apply(element, self.to).into_any_element();
        }
        let (from, to, ease) = (self.from, self.to, self.ease);
        element
            .with_animation(
                ElementId::NamedInteger(name.into(), self.generation as u64),
                Animation::new(self.duration).with_easing(move |t| ease.at(t)),
                move |element, t| apply(element, from + (to - from) * t),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closing_component_stays_mounted_until_it_settles() {
        let mut presence = Presence::new(true);
        let generation = presence.begin(false).expect("a close starts");
        assert!(presence.is_closing() && presence.is_mounted() && !presence.is_open());
        assert!(presence.settle(generation));
        assert!(!presence.is_mounted());
    }

    #[test]
    fn opening_again_during_a_close_keeps_the_component() {
        let mut presence = Presence::new(true);
        let closing = presence.begin(false).expect("a close starts");
        presence.begin(true).expect("an open starts");
        assert!(!presence.settle(closing), "the old close timer does nothing");
        assert!(presence.is_open() && presence.is_mounted());
    }

    #[test]
    fn closing_twice_in_a_row_settles_on_the_latest_close() {
        let mut presence = Presence::new(true);
        let first = presence.begin(false).expect("a close starts");
        presence.begin(true);
        let second = presence.begin(false).expect("a second close starts");
        assert!(!presence.settle(first));
        assert!(presence.settle(second));
    }

    #[test]
    fn a_repeated_request_changes_nothing() {
        let mut presence = Presence::new(false);
        assert_eq!(presence.begin(false), None);
        assert_eq!(presence.generation(), 0);
        assert!(!presence.is_mounted());
    }

    #[test]
    fn an_interrupted_move_starts_from_where_it_was() {
        let mut presence = Presence::new(false);
        presence.begin(true);
        let opening = presence.openness();
        assert!((0. ..=1.).contains(&opening));
        presence.begin(false);
        assert!(
            presence.from <= 1. && presence.from >= opening - 0.01,
            "the close starts at the open progress"
        );
        assert_eq!(Presence::new(true).openness(), 1.);
        assert_eq!(Presence::new(false).openness(), 0.);
    }

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn a_staged_move_waits_for_its_delay_then_eases_out() {
        assert_eq!(staged(MS(100), MS(140), MS(300)), 0.);
        assert_eq!(staged(MS(440), MS(140), MS(300)), 1.);
        assert_eq!(staged(MS(900), MS(140), MS(300)), 1.);
        let early = staged(MS(200), MS(140), MS(300));
        assert!(early > 0.2 && early < 1., "ease out is quick at the start: {early}");
    }

    #[test]
    fn staggered_lines_start_one_step_after_each_other() {
        assert!(stagger(MS(40), 0, MS(40), MS(280)) > 0.);
        assert_eq!(stagger(MS(40), 1, MS(40), MS(280)), 0.);
        assert_eq!(stagger(MS(1000), 5, MS(40), MS(280)), 1.);
        assert_eq!(stagger_end(3, MS(40), MS(280)), MS(360));
        assert_eq!(stagger_end(0, MS(40), MS(280)), MS(280));
    }

    #[test]
    fn a_loop_wraps_at_its_period() {
        assert_eq!(cycle(MS(0), MS(1600)), 0.);
        assert!((cycle(MS(800), MS(1600)) - 0.5).abs() < 1e-6);
        assert!(cycle(MS(1700), MS(1600)) < 0.1);
    }

    #[test]
    fn a_pop_overshoots_then_settles_at_full_size() {
        assert!((pop_scale(0.) - 0.3).abs() < 1e-6);
        assert!((pop_scale(0.6) - 1.15).abs() < 1e-6);
        assert!((pop_scale(1.) - 1.).abs() < 1e-6);
        assert!(pop_scale(0.8) < 1.15 && pop_scale(0.8) > 1.);
    }

    #[test]
    fn a_transition_moves_from_where_it_was() {
        let mut transition = Transition::new(0.);
        assert!(!transition.is_moving());
        assert_eq!(transition.value(), 0.);
        transition.retarget(1.);
        assert!(transition.is_moving());
        assert_eq!((transition.from(), transition.to()), (0., 1.));
        let generation = transition.generation;
        transition.retarget(1.);
        assert_eq!(transition.generation, generation, "the same target is not a new move");
        transition.retarget(0.);
        assert_eq!(transition.to(), 0.);
        assert!(transition.from() >= 0. && transition.from() <= 1.);
    }
}
