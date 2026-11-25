use crate::assets;
use crate::palette::Palette;
use diffpilot_core::review::Severity;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton,
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

/// A 22 pt button in the Swift app's look. gpui-component's `Button` turns its label red on hover.
#[derive(IntoElement)]
pub struct Btn {
    base: gpui::Stateful<gpui::Div>,
    style: StyleRefinement,
    label: Option<SharedString>,
    hint: Option<SharedString>,
    icon: Option<(&'static str, f32, Option<Hsla>)>,
    tooltip: Option<SharedString>,
    children: Vec<AnyElement>,
    look: Look,
    scale: Scale,
    disabled: bool,
    selected: bool,
    on_click: Option<Handler>,
}

impl Btn {
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

    pub fn icon(mut self, path: &'static str, size: f32) -> Self {
        self.icon = Some((path, size, None));
        self
    }

    pub fn icon_color(mut self, path: &'static str, size: f32, color: Hsla) -> Self {
        self.icon = Some((path, size, Some(color)));
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

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for Btn {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Btn {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl InteractiveElement for Btn {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.base.interactivity()
    }
}

impl Selectable for Btn {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl DropdownMenu for Btn {}

struct Colors {
    fill: Hsla,
    border: Hsla,
    text: Hsla,
    hover: Hsla,
    press: Hsla,
}

fn colors(look: Look, palette: Palette) -> Colors {
    match look {
        Look::Plain => Colors {
            fill: palette.cap,
            border: palette.cap_border,
            text: palette.text,
            hover: palette.field.blend(palette.text.opacity(0.11)),
            press: palette.field.blend(palette.text.opacity(0.17)),
        },
        Look::Claude => Colors {
            fill: palette.claude_bg,
            border: palette.claude_border,
            text: palette.claude,
            hover: palette.claude.opacity(0.2),
            press: palette.claude.opacity(0.28),
        },
        Look::Primary => Colors {
            fill: palette.accent,
            border: gpui::transparent_black(),
            text: gpui::white(),
            hover: palette.accent_hover,
            press: palette.accent_press,
        },
        Look::Tint => Colors {
            fill: palette.accent.opacity(0.14),
            border: palette.accent.opacity(0.6),
            text: palette.accent,
            hover: palette.accent.opacity(0.22),
            press: palette.accent.opacity(0.3),
        },
        Look::Ghost => Colors {
            fill: gpui::transparent_black(),
            border: gpui::transparent_black(),
            text: palette.text,
            hover: palette.text.opacity(0.07),
            press: palette.text.opacity(0.12),
        },
    }
}

impl RenderOnce for Btn {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = Palette::of(cx);
        let c = colors(self.look, palette);
        let (size, weight, pad, radius) = match self.scale {
            Scale::Compact => (12., FontWeight::MEDIUM, 8., 5.),
            Scale::Regular => (13., FontWeight::NORMAL, 10., 6.),
        };
        let disabled = self.disabled;
        let selected = self.selected && self.look == Look::Ghost;
        let on_click = self.on_click.filter(|_| !disabled);
        self.base
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .h(px(22.))
            .px(px(pad))
            .rounded(px(radius))
            .border_1()
            .border_color(c.border)
            .bg(if selected { c.hover } else { c.fill })
            .text_color(c.text)
            .text_size(px(size))
            .font_weight(weight)
            .whitespace_nowrap()
            .when(disabled, |b| b.opacity(0.45))
            .when(!disabled, |b| {
                b.cursor_pointer().hover(|s| s.bg(c.hover)).active(|s| s.bg(c.press))
            })
            .refine_style(&self.style)
            .when_some(on_click, |b, handler| {
                b.on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
            .when_some(self.icon, |b, (path, size, color)| {
                let icon = Icon::empty().path(path).size(px(size));
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
            .when_some(self.hint, |b, hint| {
                b.child(div().text_size(px(10.)).opacity(0.75).child(hint))
            })
    }
}

pub fn severity_badge(severity: Severity, palette: Palette, size: f32) -> AnyElement {
    let color = palette.severity(severity);
    let glyph = |text: &'static str, size: f32| {
        div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(gpui::white())
            .text_size(px(size))
            .font_weight(FontWeight::BOLD)
            .child(text)
    };
    let shape = match severity {
        Severity::Suggestion => div()
            .relative()
            .size(px(size))
            .child(Icon::empty().path(assets::TRIANGLE).size(px(size)).text_color(color))
            .child(
                div()
                    .absolute()
                    .top(px(size * 0.2))
                    .size_full()
                    .child(glyph("!", size * 0.6)),
            ),
        _ => {
            let text = match severity {
                Severity::MustFix => "×",
                Severity::Question => "?",
                _ => "",
            };
            div()
                .relative()
                .size(px(size))
                .rounded_full()
                .bg(color)
                .child(glyph(text, size * 0.75))
        }
    };
    shape.flex_none().into_any_element()
}

pub fn filled_check(color: Hsla, size: f32) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .bg(color)
        .child(
            Icon::empty()
                .path(assets::CHECK)
                .size(px(size * 0.7))
                .text_color(gpui::white()),
        )
        .into_any_element()
}

pub fn avatar(author: &str, claude: bool, palette: Palette, size: f32) -> AnyElement {
    let base = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .text_color(gpui::white())
        .text_size(px(10.))
        .font_weight(FontWeight::BOLD);
    if claude {
        base.bg(palette.claude)
            .child(Icon::empty().path(assets::SPARKLES).size(px(13.)))
            .into_any_element()
    } else {
        let initials: String = author.split(' ').filter_map(|word| word.chars().next()).collect();
        base.bg(palette.user_avatar).child(initials).into_any_element()
    }
}

pub fn card_shell(palette: Palette, border: Hsla, dark: bool) -> gpui::Div {
    gpui_component::v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(px(12.))
        .border_1()
        .border_color(border)
        .bg(palette.card)
        .text_color(palette.text)
        .shadow(vec![BoxShadow {
            color: gpui::black().opacity(if dark { 0.45 } else { 0.12 }),
            offset: gpui::point(px(0.), px(4.)),
            blur_radius: px(18.),
            spread_radius: px(0.),
        }])
}

pub fn icon_button(id: impl Into<ElementId>, path: &'static str, color: Hsla) -> Btn {
    Btn::new(id)
        .look(Look::Ghost)
        .icon_color(path, 11., color)
        .w(px(22.))
        .px(px(0.))
}

pub fn chip(name: &'static str, on: bool) -> Btn {
    let chip = Btn::new(name)
        .label(name)
        .regular()
        .px(px(11.))
        .rounded_full()
        .text_size(px(12.))
        .look(if on { Look::Tint } else { Look::Plain });
    if on { chip.icon(assets::CHECK, 10.) } else { chip }
}
