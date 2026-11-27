use crate::components::{Button, Look, OVERLAY_RADIUS, glass_panel, tag};
use crate::theme::{Theme, text_px, text_size};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ClickEvent, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton,
    MouseDownEvent, Pixels, Point, RenderOnce, SharedString, Size, Window, black, canvas, div, ease_out_quint, point,
    px, size,
};
use std::rc::Rc;
use std::time::{Duration, Instant};

const DIM: f32 = 0.5;
const PADDING: f32 = 6.;
const GAP: f32 = 12.;
const MARGIN: f32 = 12.;
const CARD_WIDTH: f32 = 300.;
const MOVE: Duration = Duration::from_millis(220);

type Target = Rc<dyn Fn(&Window, &App) -> Option<Bounds<Pixels>>>;
type OnDismiss = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;
type OnKey = Box<dyn Fn(&KeyDownEvent, &mut Window, &mut App)>;
type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A dim layer over the whole window with a rounded cut-out around `target`, and a card next to it. The layer
/// blocks the mouse. A change of `step` makes the cut-out glide to the new target.
pub fn tour(id: impl Into<SharedString>, step: usize) -> Tour {
    Tour {
        id: id.into(),
        step,
        target: Rc::new(|_, _| None),
        radius: OVERLAY_RADIUS,
        focus: None,
        on_key_down: None,
        on_dismiss: None,
        card: None,
    }
}

#[derive(IntoElement)]
pub struct Tour {
    id: SharedString,
    step: usize,
    target: Target,
    radius: f32,
    focus: Option<FocusHandle>,
    on_key_down: Option<OnKey>,
    on_dismiss: Option<OnDismiss>,
    card: Option<AnyElement>,
}

impl Tour {
    /// Read at paint time, so the cut-out follows the target after a resize.
    pub fn target(mut self, target: impl Fn(&Window, &App) -> Option<Bounds<Pixels>> + 'static) -> Self {
        self.target = Rc::new(target);
        self
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }

    pub fn on_key_down(mut self, listener: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_key_down = Some(Box::new(listener));
        self
    }

    pub fn on_dismiss(mut self, listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(listener));
        self
    }

    pub fn card(mut self, card: impl IntoElement) -> Self {
        self.card = Some(card.into_any_element());
        self
    }
}

struct Glide {
    step: usize,
    from: Bounds<Pixels>,
    since: Instant,
}

impl Glide {
    fn at(&mut self, step: usize, target: Bounds<Pixels>) -> (Bounds<Pixels>, bool) {
        if step != self.step {
            let (shown, _) = self.at(self.step, target);
            *self = Glide {
                step,
                from: shown,
                since: Instant::now(),
            };
        }
        let t = (self.since.elapsed().as_secs_f32() / MOVE.as_secs_f32()).min(1.);
        let eased = ease_out_quint()(t);
        let lerp = |from: Pixels, to: Pixels| from + (to - from) * eased;
        let shown = Bounds {
            origin: point(
                lerp(self.from.origin.x, target.origin.x),
                lerp(self.from.origin.y, target.origin.y),
            ),
            size: size(
                lerp(self.from.size.width, target.size.width),
                lerp(self.from.size.height, target.size.height),
            ),
        };
        (shown, t < 1.)
    }
}

struct Stage {
    scrim: AnyElement,
    ring: Option<AnyElement>,
    card: AnyElement,
}

impl RenderOnce for Tour {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let glide: Entity<Option<Glide>> = window.use_keyed_state(
            gpui::ElementId::Name(format!("{}-glide", self.id).into()),
            cx,
            |_, _| None,
        );
        let Tour {
            id,
            step,
            target,
            radius,
            focus,
            on_key_down,
            on_dismiss,
            card,
        } = self;
        let ring_radius = radius + PADDING;
        let stage = canvas(
            move |window_bounds, window, cx| {
                let aim = target(window, cx).map(|bounds| bounds.dilate(px(PADDING)));
                let shown = aim.map(|aim| {
                    let (shown, moving) = glide.update(cx, |glide, _| {
                        glide
                            .get_or_insert_with(|| Glide {
                                step,
                                from: aim,
                                since: Instant::now() - MOVE,
                            })
                            .at(step, aim)
                    });
                    if moving {
                        window.request_animation_frame();
                    }
                    shown
                });
                let mut scrim = scrim(shown, window_bounds, ring_radius);
                scrim.prepaint_as_root(
                    shown.map_or(window_bounds.origin, |shown| {
                        shown.origin - point(hole_reach(window_bounds), hole_reach(window_bounds))
                    }),
                    definite(scrim_size(shown, window_bounds)),
                    window,
                    cx,
                );
                let ring = shown.map(|shown| {
                    let mut ring =
                        tag(div().w(shown.size.width).h(shown.size.height), || "tour:spotlight").into_any_element();
                    ring.prepaint_as_root(shown.origin, definite(shown.size), window, cx);
                    ring
                });
                let mut card = card.unwrap_or_else(|| div().into_any_element());
                let wanted = card.layout_as_root(
                    size(AvailableSpace::Definite(px(CARD_WIDTH)), AvailableSpace::MinContent),
                    window,
                    cx,
                );
                let origin = place(shown, wanted, window_bounds);
                card.prepaint_at(origin, window, cx);
                Stage { scrim, ring, card }
            },
            move |_, mut stage, window, cx| {
                stage.scrim.paint(window, cx);
                if let Some(ring) = &mut stage.ring {
                    ring.paint(window, cx);
                }
                stage.card.paint(window, cx);
            },
        )
        .absolute()
        .inset_0();

        div()
            .id(gpui::ElementId::Name(format!("{id}-overlay").into()))
            .absolute()
            .inset_0()
            .occlude()
            .when_some(focus, |this, focus| this.track_focus(&focus))
            .when_some(on_key_down, |this, listener| this.on_key_down(listener))
            .when_some(on_dismiss, |this, listener| {
                this.on_mouse_down(MouseButton::Left, move |event, window, cx| listener(event, window, cx))
            })
            .child(stage)
    }
}

fn definite(size: Size<Pixels>) -> Size<AvailableSpace> {
    gpui::size(
        AvailableSpace::Definite(size.width),
        AvailableSpace::Definite(size.height),
    )
}

/// The side of the target with the most free room wins. A target that leaves no room, such as a whole pane,
/// gets the card over its middle.
fn place(target: Option<Bounds<Pixels>>, card: Size<Pixels>, window: Bounds<Pixels>) -> Point<Pixels> {
    let inner = window.dilate(-px(MARGIN));
    let clamp = |origin: Point<Pixels>| {
        let max = point(
            (inner.right() - card.width).max(inner.left()),
            (inner.bottom() - card.height).max(inner.top()),
        );
        point(origin.x.clamp(inner.left(), max.x), origin.y.clamp(inner.top(), max.y))
    };
    let Some(target) = target else {
        return clamp(window.center() - point(card.width / 2., card.height / 2.));
    };
    let gap = px(GAP);
    let across = |origin: Pixels, length: Pixels, card_length: Pixels| origin + (length - card_length) / 2.;
    let sides = [
        (
            inner.bottom() - target.bottom() - gap - card.height,
            point(
                across(target.left(), target.size.width, card.width),
                target.bottom() + gap,
            ),
        ),
        (
            target.top() - inner.top() - gap - card.height,
            point(
                across(target.left(), target.size.width, card.width),
                target.top() - gap - card.height,
            ),
        ),
        (
            inner.right() - target.right() - gap - card.width,
            point(
                target.right() + gap,
                across(target.top(), target.size.height, card.height),
            ),
        ),
        (
            target.left() - inner.left() - gap - card.width,
            point(
                target.left() - gap - card.width,
                across(target.top(), target.size.height, card.height),
            ),
        ),
    ];
    let (room, origin) = sides
        .into_iter()
        .reduce(|best, side| if side.0 > best.0 { side } else { best })
        .expect("four sides");
    if room < px(0.) {
        return clamp(target.center() - point(card.width / 2., card.height / 2.));
    }
    clamp(origin)
}

fn hole_reach(window: Bounds<Pixels>) -> Pixels {
    window.size.width.max(window.size.height)
}

fn scrim_size(shown: Option<Bounds<Pixels>>, window: Bounds<Pixels>) -> Size<Pixels> {
    match shown {
        Some(shown) => shown.size + size(hole_reach(window) * 2., hole_reach(window) * 2.),
        None => window.size,
    }
}

fn scrim(shown: Option<Bounds<Pixels>>, window: Bounds<Pixels>, radius: f32) -> AnyElement {
    let dim = black().opacity(DIM);
    let size = scrim_size(shown, window);
    let base = div().w(size.width).h(size.height);
    if shown.is_none() {
        return base.bg(dim).into_any_element();
    }
    let reach = hole_reach(window);
    let mut base = base.border_color(dim).rounded(reach + px(radius));
    let widths = &mut base.style().border_widths;
    widths.top = Some(reach.into());
    widths.right = Some(reach.into());
    widths.bottom = Some(reach.into());
    widths.left = Some(reach.into());
    base.into_any_element()
}

/// The content of a tour step: title, text, a counter, and Back, Next and Skip buttons. Tags: `tour:card`,
/// `tour:title:{title}`, `tour:counter:{counter}`, and `tour:back`, `tour:next` and `tour:skip` on the buttons.
#[derive(IntoElement)]
pub struct TourCard {
    title: SharedString,
    body: SharedString,
    counter: SharedString,
    can_go_back: bool,
    next_label: SharedString,
    on_back: OnClick,
    on_next: OnClick,
    on_skip: OnClick,
}

pub fn tour_card(
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    counter: impl Into<SharedString>,
    on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_next: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_skip: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> TourCard {
    TourCard {
        title: title.into(),
        body: body.into(),
        counter: counter.into(),
        can_go_back: true,
        next_label: "Next".into(),
        on_back: Rc::new(on_back),
        on_next: Rc::new(on_next),
        on_skip: Rc::new(on_skip),
    }
}

impl TourCard {
    pub fn can_go_back(mut self, can: bool) -> Self {
        self.can_go_back = can;
        self
    }

    pub fn next_label(mut self, label: impl Into<SharedString>) -> Self {
        self.next_label = label.into();
        self
    }
}

impl RenderOnce for TourCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let title = self.title.to_string();
        let counter = self.counter.to_string();
        let (on_back, on_next, on_skip) = (self.on_back, self.on_next, self.on_skip);
        let buttons = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                tag(
                    Button::new("tour-skip")
                        .look(Look::Ghost)
                        .label("Skip")
                        .on_click(move |event, window, cx| on_skip(event, window, cx)),
                    || "tour:skip",
                )
                .into_any_element(),
            )
            .child(div().flex_1())
            .children(self.can_go_back.then(|| {
                tag(
                    Button::new("tour-back")
                        .label("Back")
                        .on_click(move |event, window, cx| on_back(event, window, cx)),
                    || "tour:back",
                )
            }))
            .child(tag(
                Button::new("tour-next")
                    .look(Look::Primary)
                    .label(self.next_label)
                    .on_click(move |event, window, cx| on_next(event, window, cx)),
                || "tour:next",
            ));
        tag(
            glass_panel(&theme)
                .id("tour-card")
                .w(px(CARD_WIDTH))
                .p(px(14.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(tag(
                    div()
                        .text_size(text_px(text_size::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.ui.text)
                        .child(self.title),
                    move || format!("tour:title:{title}"),
                ))
                .child(
                    div()
                        .text_size(text_px(text_size::BODY))
                        .text_color(theme.ui.text2)
                        .child(self.body),
                )
                .child(tag(
                    div()
                        .text_size(text_px(text_size::SECONDARY))
                        .text_color(theme.ui.muted)
                        .child(self.counter),
                    move || format!("tour:counter:{counter}"),
                ))
                .child(buttons),
            || "tour:card",
        )
    }
}
