use gpui::prelude::*;
use gpui::{Animation, AnimationElement, AnimationExt, AnyElement, Context, ElementId, Styled, ease_out_quint, px};
use std::time::Duration;

pub const OPEN_DURATION: Duration = Duration::from_millis(160);
pub const DRAWER_DURATION: Duration = Duration::from_millis(220);
const SLIDE: f32 = 6.;

/// Fades an overlay in while it slides `SLIDE` px into place. `from_above` is true when the overlay opens
/// below its trigger, so it drops down; otherwise it rises.
pub fn open_motion<E: Styled + IntoElement + 'static>(
    element: E,
    id: impl Into<ElementId>,
    from_above: bool,
) -> AnimationElement<E> {
    let sign = if from_above { -1. } else { 1. };
    element.relative().with_animation(
        id,
        Animation::new(OPEN_DURATION).with_easing(ease_out_quint()),
        move |element, t| element.opacity(t).top(px(sign * SLIDE * (1. - t))),
    )
}

/// A drawer that stays mounted while it closes, so the close can animate. Each open or close starts a new
/// generation, which keys the animation so it replays.
#[derive(Clone, Copy, Debug)]
pub struct Drawer {
    open: bool,
    mounted: bool,
    generation: u32,
}

impl Drawer {
    pub fn new(open: bool) -> Self {
        Self {
            open,
            mounted: open,
            generation: 0,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// True while the drawer is on screen, including its close animation.
    pub fn is_mounted(&self) -> bool {
        self.mounted
    }

    pub fn is_moving(&self) -> bool {
        self.generation > 0
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Returns the generation to pass to `settle_after` when the drawer started closing.
    pub fn set_open(&mut self, open: bool) -> Option<u32> {
        if self.open == open {
            return None;
        }
        self.open = open;
        self.mounted = true;
        self.generation += 1;
        (!open).then_some(self.generation)
    }

    /// Unmounts a closed drawer once its close animation is over. A later open or close makes this a no-op.
    pub fn settle(&mut self, generation: u32) -> bool {
        let settled = !self.open && self.generation == generation && self.mounted;
        if settled {
            self.mounted = false;
        }
        settled
    }

    /// Wraps `element` in the drawer animation. `apply` gets the element and how open the drawer is, from
    /// 0 (closed) to 1 (open). A drawer that has not moved yet is drawn fully open.
    pub fn animate<E: IntoElement + 'static>(
        &self,
        name: &'static str,
        element: E,
        apply: impl Fn(E, f32) -> E + 'static,
    ) -> AnyElement {
        if !self.is_moving() {
            return apply(element, 1.).into_any_element();
        }
        let opening = self.open;
        element
            .with_animation(
                ElementId::NamedInteger(name.into(), self.generation as u64),
                Animation::new(DRAWER_DURATION).with_easing(ease_out_quint()),
                move |element, t| apply(element, if opening { t } else { 1. - t }),
            )
            .into_any_element()
    }
}

/// Runs `settle` on the view once the drawer close animation has finished.
pub fn settle_after<T: 'static>(cx: &mut Context<T>, settle: impl FnOnce(&mut T, &mut Context<T>) + 'static) {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(DRAWER_DURATION).await;
        this.update(cx, |this, cx| settle(this, cx)).ok();
    })
    .detach();
}
