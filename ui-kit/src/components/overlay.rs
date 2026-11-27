use crate::components::{Presence, glass_panel, tag};
use crate::theme::Theme;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ElementId, FocusHandle, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, SharedString,
    Window, black, div, px,
};
use std::rc::Rc;

const DIALOG_DIM: f32 = 0.4;

type OnBackdrop = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;
type OnKey = Box<dyn Fn(&KeyDownEvent, &mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
}

/// A glass panel pinned to one window edge over a transparent backdrop. It slides and fades in and out with
/// `presence`. Tags: `{tag}` on the root while open, `{tag}:closing` while closing, and `{tag}:panel` on the panel.
pub fn side_sheet(id: impl Into<SharedString>, presence: &Presence) -> SideSheet {
    SideSheet {
        id: id.into(),
        presence: *presence,
        edge: Edge::Right,
        width: px(400.),
        inset: px(8.),
        tag: None,
        focus: None,
        on_key_down: None,
        on_backdrop: None,
        children: Vec::new(),
    }
}

#[derive(IntoElement)]
pub struct SideSheet {
    id: SharedString,
    presence: Presence,
    edge: Edge,
    width: Pixels,
    inset: Pixels,
    tag: Option<(&'static str, &'static str)>,
    focus: Option<FocusHandle>,
    on_key_down: Option<OnKey>,
    on_backdrop: Option<OnBackdrop>,
    children: Vec<AnyElement>,
}

impl SideSheet {
    pub fn edge(mut self, edge: Edge) -> Self {
        self.edge = edge;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = px(width);
        self
    }

    pub fn inset(mut self, inset: f32) -> Self {
        self.inset = px(inset);
        self
    }

    /// Names the root and the panel for tests.
    pub fn tags(mut self, root: &'static str, panel: &'static str) -> Self {
        self.tag = Some((root, panel));
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

    pub fn on_backdrop_click(mut self, listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_backdrop = Some(Rc::new(listener));
        self
    }
}

impl ParentElement for SideSheet {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SideSheet {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let open = self.presence.is_open();
        let (root_tag, panel_tag) = self.tag.unwrap_or(("sheet", "sheet:panel"));
        let (edge, width, inset) = (self.edge, self.width, self.inset);
        let panel = tag(
            glass_panel(&theme).id(ElementId::Name(format!("{}-panel", self.id).into())),
            || panel_tag,
        )
        .when(open, |this| this.occlude())
        .absolute()
        .top(inset)
        .bottom(inset)
        .w(width)
        .flex()
        .flex_col()
        .children(self.children);
        let panel = self
            .presence
            .animate(format!("{}-sheet", self.id), panel, move |panel, t| {
                let offset = inset - (width + inset) * (1. - t);
                match edge {
                    Edge::Right => panel.right(offset),
                    Edge::Left => panel.left(offset),
                }
                .opacity(t)
            });
        let backdrop = div()
            .id(ElementId::Name(format!("{}-backdrop", self.id).into()))
            .absolute()
            .inset_0()
            .when(open, |this| this.occlude())
            .when_some(self.on_backdrop.filter(|_| open), |this, listener| {
                this.on_mouse_down(MouseButton::Left, move |event, window, cx| listener(event, window, cx))
            });
        let root = div()
            .id(ElementId::Name(format!("{}-overlay", self.id).into()))
            .absolute()
            .inset_0()
            .when_some(self.focus, |this, focus| this.track_focus(&focus))
            .when_some(self.on_key_down, |this, listener| this.on_key_down(listener))
            .child(backdrop)
            .child(panel);
        tag(root, || {
            if open {
                root_tag.to_string()
            } else {
                format!("{root_tag}:closing")
            }
        })
    }
}

/// A dimmed backdrop with a centered glass panel. It fades in and out with `presence`. Tags:
/// `{backdrop}` on the backdrop while open, `{backdrop}:closing` while closing, and `{panel}` on the panel.
pub fn dialog(id: impl Into<SharedString>, presence: &Presence) -> Dialog {
    Dialog {
        id: id.into(),
        presence: *presence,
        width: px(480.),
        height: px(360.),
        margin: px(32.),
        tag: None,
        on_backdrop: None,
        children: Vec::new(),
    }
}

#[derive(IntoElement)]
pub struct Dialog {
    id: SharedString,
    presence: Presence,
    width: Pixels,
    height: Pixels,
    margin: Pixels,
    tag: Option<(&'static str, &'static str)>,
    on_backdrop: Option<OnBackdrop>,
    children: Vec<AnyElement>,
}

impl Dialog {
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = px(width);
        self.height = px(height);
        self
    }

    pub fn margin(mut self, margin: f32) -> Self {
        self.margin = px(margin);
        self
    }

    /// Names the backdrop and the panel for tests.
    pub fn tags(mut self, backdrop: &'static str, panel: &'static str) -> Self {
        self.tag = Some((backdrop, panel));
        self
    }

    pub fn on_backdrop_click(mut self, listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_backdrop = Some(Rc::new(listener));
        self
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Dialog {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let open = self.presence.is_open();
        let (backdrop_tag, panel_tag) = self.tag.unwrap_or(("dialog", "dialog:panel"));
        let panel = tag(
            glass_panel(&theme).id(ElementId::Name(format!("{}-panel", self.id).into())),
            || panel_tag,
        )
        .w(self.width)
        .h(self.height)
        .max_w_full()
        .max_h_full()
        .overflow_hidden()
        .when(open, |this| {
            this.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        })
        .children(self.children);
        let dim: Hsla = black().opacity(DIALOG_DIM);
        let backdrop = div()
            .id(ElementId::Name(format!("{}-backdrop", self.id).into()))
            .absolute()
            .inset_0()
            .when(open, |this| this.occlude())
            .flex()
            .items_center()
            .justify_center()
            .p(self.margin)
            .bg(dim)
            .when_some(self.on_backdrop.filter(|_| open), |this, listener| {
                this.on_mouse_down(MouseButton::Left, move |event, window, cx| listener(event, window, cx))
            })
            .child(panel);
        let backdrop = self
            .presence
            .animate(format!("{}-fade", self.id), backdrop, |backdrop, t| backdrop.opacity(t));
        tag(div().absolute().inset_0().child(backdrop), || {
            if open {
                backdrop_tag.to_string()
            } else {
                format!("{backdrop_tag}:closing")
            }
        })
    }
}
