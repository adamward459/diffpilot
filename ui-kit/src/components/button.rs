use super::glass::rim;
use super::interaction::{DISABLED_OPACITY, State};
use crate::icons::Glyph;
use crate::theme::{Rise, Theme};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, StyleRefinement, Styled, Window, div, px,
};
use gpui_component::menu::DropdownMenu;
use gpui_component::tooltip::Tooltip;
use gpui_component::{Icon, Selectable, StyledExt};
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Plain,
    Claude,
    Primary,
    Ghost,
    Tint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Compact,
    Regular,
}

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A glass capsule button: 26 pt compact, 30 pt regular. gpui-component's `Button` turns its label red on hover.
#[derive(IntoElement)]
pub struct Button {
    base: gpui::Stateful<gpui::Div>,
    style: StyleRefinement,
    label: Option<SharedString>,
    hint: Option<SharedString>,
    icon: Option<(Glyph, f32, Option<Hsla>)>,
    tooltip: Option<SharedString>,
    children: Vec<AnyElement>,
    look: Look,
    scale: Scale,
    caret: bool,
    state: State,
    disabled: bool,
    selected: bool,
    on_click: Option<Handler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            style: StyleRefinement::default(),
            label: None,
            hint: None,
            icon: None,
            tooltip: None,
            children: Vec::new(),
            look: Look::Plain,
            scale: Scale::Compact,
            caret: false,
            state: State::Normal,
            disabled: false,
            selected: false,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn icon(mut self, glyph: Glyph, size: f32) -> Self {
        self.icon = Some((glyph, size, None));
        self
    }

    pub fn icon_color(mut self, glyph: Glyph, size: f32, color: Hsla) -> Self {
        self.icon = Some((glyph, size, Some(color)));
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }

    pub fn regular(mut self) -> Self {
        self.scale = Scale::Regular;
        self
    }

    pub fn caret(mut self, caret: bool) -> Self {
        self.caret = caret;
        self
    }

    pub fn state(mut self, state: State) -> Self {
        self.state = state;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.base.interactivity()
    }
}

impl Selectable for Button {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl DropdownMenu for Button {}

struct Colors {
    fill: Hsla,
    border: Hsla,
    text: Hsla,
    hover: Hsla,
    press: Hsla,
}

fn colors(look: Look, theme: &Theme) -> Colors {
    let ui = &theme.ui;
    let clear = gpui::transparent_black();
    match look {
        Look::Plain => Colors {
            fill: ui.cap,
            border: ui.cap_border,
            text: ui.text,
            hover: theme.hovered(ui.cap),
            press: theme.pressed(ui.cap),
        },
        Look::Claude => Colors {
            fill: ui.claude_bg,
            border: ui.claude_border,
            text: ui.claude,
            hover: ui.claude.opacity(0.2),
            press: ui.claude.opacity(0.28),
        },
        Look::Primary => Colors {
            fill: theme.accent,
            border: clear,
            text: gpui::white(),
            hover: theme.lifted(theme.accent, false),
            press: theme.lifted(theme.accent, true),
        },
        Look::Tint => Colors {
            fill: theme.accent.opacity(0.14),
            border: theme.accent.opacity(0.6),
            text: theme.accent,
            hover: theme.accent.opacity(0.22),
            press: theme.accent.opacity(0.3),
        },
        Look::Ghost => Colors {
            fill: clear,
            border: clear,
            text: ui.text,
            hover: theme.hover,
            press: theme.press,
        },
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = colors(self.look, &theme);
        let (size, weight, pad, height) = match self.scale {
            Scale::Compact => (13., FontWeight::MEDIUM, 11., 26.),
            Scale::Regular => (14., FontWeight::NORMAL, 13., 30.),
        };
        let glass = self.look != Look::Ghost;
        let rise = match self.state {
            State::Hover => Rise::Hover,
            State::Pressed => Rise::Pressed,
            _ => Rise::Rest,
        };
        let disabled = self.disabled || self.state == State::Disabled;
        let selected = self.selected && self.look == Look::Ghost;
        let on_click = self.on_click.filter(|_| !disabled);
        let (fill, live) = match self.state {
            State::Hover => (c.hover, false),
            State::Pressed => (c.press, false),
            _ => (if selected { c.hover } else { c.fill }, !disabled),
        };
        self.base
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(5.))
            .h(px(height))
            .px(px(pad))
            .rounded_full()
            .border_1()
            .border_color(c.border)
            .when(glass, |b| {
                b.relative()
                    .bg(theme.glass_body(fill))
                    .shadow(theme.slab_shadow(rise))
                    .child(rim(&theme, None))
            })
            .when(!glass, |b| b.bg(fill))
            .text_color(c.text)
            .text_size(px(size))
            .font_weight(weight)
            .whitespace_nowrap()
            .when(disabled, |b| b.opacity(DISABLED_OPACITY))
            .when(!disabled, |b| b.cursor_pointer())
            .when(live && glass, |b| {
                let (hover, press) = (theme.glass_body(c.hover), theme.glass_body(c.press));
                let (lower, set_down) = (theme.slab_shadow(Rise::Hover), theme.slab_shadow(Rise::Pressed));
                b.hover(move |s| s.bg(hover).shadow(lower))
                    .active(move |s| s.bg(press).shadow(set_down))
            })
            .when(live && !glass, |b| b.hover(|s| s.bg(c.hover)).active(|s| s.bg(c.press)))
            .refine_style(&self.style)
            .when_some(on_click, |b, handler| {
                b.on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
            .when_some(self.icon, |b, (glyph, size, color)| {
                let icon = Icon::new(glyph).size(px(size));
                b.child(match color {
                    Some(color) => icon.text_color(color),
                    None => icon,
                })
            })
            .when_some(self.tooltip, |b, text| {
                b.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
            })
            .children(self.label)
            .children(self.children)
            .when(self.caret, |b| b.child(Icon::new(Glyph::ChevronDown).size(px(13.))))
            .when_some(self.hint, |b, hint| {
                b.child(div().text_size(px(11.)).opacity(0.75).child(hint))
            })
    }
}

pub fn icon_button(id: impl Into<ElementId>, glyph: Glyph, color: Hsla) -> Button {
    Button::new(id)
        .look(Look::Ghost)
        .icon_color(glyph, 14., color)
        .w(px(30.))
        .px(px(0.))
        .rounded_full()
}

pub fn chip(name: &'static str, on: bool) -> Button {
    let chip = Button::new(name)
        .label(name)
        .regular()
        .px(px(12.))
        .rounded_full()
        .text_size(px(13.))
        .look(if on { Look::Tint } else { Look::Plain });
    if on { chip.icon(Glyph::CheckThin, 11.) } else { chip }
}
