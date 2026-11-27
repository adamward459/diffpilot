use crate::components::{OVERLAY_RADIUS, glass_panel, open_motion};
use crate::icons::Glyph;
use crate::theme::Theme;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, Corner, Div, ElementId, IntoElement, Pixels, RenderOnce, SharedString,
    TransformationMatrix, Window, canvas, div, point, px, radians, size,
};
use gpui_component::popover::{Popover, PopoverState};
use gpui_component::{Selectable, v_flex};
use std::cell::Cell;
use std::f32::consts::PI;
use std::rc::Rc;

const ARROW_WIDTH: f32 = 22.;
const ARROW_HEIGHT: f32 = 9.;

type Content = Rc<dyn Fn(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> AnyElement>;

/// A popover with a macOS-style arrow that points at the center of its trigger.
pub fn popover(id: impl Into<SharedString>) -> ArrowPopover {
    let id = id.into();
    ArrowPopover {
        id: id.clone(),
        inner: Popover::new(ElementId::Name(id)).appearance(false),
        anchor: Corner::TopLeft,
        content: None,
    }
}

#[derive(IntoElement)]
pub struct ArrowPopover {
    id: SharedString,
    inner: Popover,
    anchor: Corner,
    content: Option<Content>,
}

impl ArrowPopover {
    pub fn anchor(mut self, anchor: Corner) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.inner = self.inner.open(open);
        self
    }

    pub fn on_open_change(mut self, callback: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.inner = self.inner.on_open_change(callback);
        self
    }

    pub fn trigger<T: Selectable + IntoElement + 'static>(mut self, trigger: T) -> Self {
        self.inner = self.inner.trigger(trigger);
        self
    }

    pub fn content<E: IntoElement>(
        mut self,
        content: impl Fn(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> E + 'static,
    ) -> Self {
        self.content = Some(Rc::new(move |state, window, cx| {
            content(state, window, cx).into_any_element()
        }));
        self
    }
}

impl RenderOnce for ArrowPopover {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let trigger = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let below = matches!(self.anchor, Corner::TopLeft | Corner::TopRight);
        let mut inner = self.inner.anchor(self.anchor);
        if let Some(content) = self.content {
            let trigger = trigger.clone();
            let id = self.id;
            inner = inner.content(move |state, window, cx| {
                let body = content(state, window, cx);
                frame(&id, body, below, trigger.clone(), cx)
            });
        }
        div().relative().child(inner).child(
            canvas(move |bounds, _, _| trigger.set(Some(bounds)), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
    }
}

fn frame(
    id: &SharedString,
    body: AnyElement,
    below: bool,
    trigger: Rc<Cell<Option<Bounds<Pixels>>>>,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let panel = glass_panel(&theme).child(v_flex().p_3().child(body));
    open_motion(
        arrow_frame(panel, below, trigger),
        ElementId::Name(format!("{id}:open").into()),
        below,
    )
    .into_any_element()
}

/// Pads `panel` on the arrow side and paints the arrow at the trigger's center.
pub(crate) fn arrow_frame(panel: impl IntoElement, below: bool, trigger: Rc<Cell<Option<Bounds<Pixels>>>>) -> Div {
    let arrow = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            if let Some(trigger) = trigger.get() {
                paint_arrow(window, bounds, trigger, below, cx);
            }
        },
    )
    .absolute()
    .left_0()
    .right_0()
    .h(px(ARROW_HEIGHT + 1.))
    .map(|this| if below { this.top_0() } else { this.bottom_0() });
    div()
        .relative()
        .map(|this| {
            if below {
                this.pt(px(ARROW_HEIGHT))
            } else {
                this.pb(px(ARROW_HEIGHT))
            }
        })
        .child(panel)
        .child(arrow)
}

fn paint_arrow(window: &mut Window, bounds: Bounds<Pixels>, trigger: Bounds<Pixels>, below: bool, cx: &App) {
    let theme = Theme::of(cx);
    let (top, bottom) = theme.glass_edges(theme.chrome.glass);
    let (fill, border, radius) = (
        if below { top } else { bottom },
        theme.chrome.glass_border,
        OVERLAY_RADIUS,
    );
    let half = ARROW_WIDTH / 2.;
    let inset = half + radius;
    let (left, right) = (f32::from(bounds.left()) + inset, f32::from(bounds.right()) - inset);
    if left > right {
        return;
    }
    let x = f32::from(trigger.center().x).clamp(left, right);
    let arrow = Bounds::new(
        point(px(x - half), bounds.top()),
        size(px(ARROW_WIDTH), px(ARROW_HEIGHT + 1.)),
    );
    let turn = if below {
        TransformationMatrix::unit()
    } else {
        let center = arrow.center().scale(window.scale_factor());
        TransformationMatrix::unit()
            .translate(point(center.x * 2., center.y * 2.))
            .compose(TransformationMatrix::unit().rotate(radians(PI)))
    };
    for (glyph, color) in [(Glyph::PopoverArrowFill, fill), (Glyph::PopoverArrowBorder, border)] {
        let _ = window.paint_svg(arrow, glyph.path().into(), turn, color, cx);
    }
}
