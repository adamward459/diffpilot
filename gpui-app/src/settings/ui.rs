//! The visual pieces of the Settings window, drawn from the v2 design tokens. They are local on purpose: the
//! ui-kit crate will hold shared versions of the theme, buttons and switches, and this file is what that swap
//! replaces.

use super::icons;
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Div, ElementId, Hsla, MouseButton, SharedString, Stateful, Svg,
    Transformation, Window, deferred, div, percentage, px, rgb, svg,
};
use gpui_component::ActiveTheme;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone)]
pub struct Tokens {
    pub body: Hsla,
    pub toolbar: Hsla,
    pub card: Hsla,
    pub line: Hsla,
    pub line2: Hsla,
    pub text: Hsla,
    pub text2: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub claude: Hsla,
    pub ok: Hsla,
    pub err: Hsla,
    pub warn: Hsla,
    pub cap: Hsla,
    pub cap_border: Hsla,
    pub press: Hsla,
    pub pop: Hsla,
    pub selected_segment: Hsla,
    pub mono: SharedString,
    pub font: SharedString,
}

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn mix(over: u32, with: u32, percent: f32) -> Hsla {
    let channel = |shift: u32| {
        let base = ((over >> shift) & 0xff) as f32;
        let top = ((with >> shift) & 0xff) as f32;
        (base + (top - base) * percent / 100.0).round() as u32
    };
    color((channel(16) << 16) | (channel(8) << 8) | channel(0))
}

impl Tokens {
    pub fn of(cx: &App) -> Self {
        let theme = cx.theme();
        let (mono, font) = (theme.mono_font_family.clone(), theme.font_family.clone());
        if theme.is_dark() {
            let editor = 0x1f1f1f;
            Self {
                body: color(0x181818),
                toolbar: color(0x181818),
                card: color(0x252526),
                line: color(0x2b2b2b),
                line2: color(0x3c3c3c),
                text: color(0xcccccc),
                text2: color(0xb4b4b4),
                muted: color(0x9d9d9d),
                accent: color(0x0078d4),
                accent_text: color(0x4daafc),
                claude: color(0xe8916c),
                ok: color(0x2ea043),
                err: color(0xf14c4c),
                warn: color(0xcca700),
                cap: mix(editor, 0xffffff, 7.0),
                cap_border: color(0x3c3c3c),
                press: mix(editor, 0xffffff, 10.0),
                pop: color(0x202020),
                selected_segment: color(0x3c3c3c),
                mono,
                font,
            }
        } else {
            let editor = 0xffffff;
            Self {
                body: color(0xf8f8f8),
                toolbar: color(0xf8f8f8),
                card: color(0xffffff),
                line: color(0xe5e5e5),
                line2: color(0xcecece),
                text: color(0x3b3b3b),
                text2: color(0x555555),
                muted: color(0x6e7681),
                accent: color(0x005fb8),
                accent_text: color(0x005fb8),
                claude: color(0xc15f3c),
                ok: color(0x2ea043),
                err: color(0xe51400),
                warn: color(0xbf8803),
                cap: mix(editor, 0x000000, 3.0),
                cap_border: color(0xcecece),
                press: mix(editor, 0x000000, 8.0),
                pop: color(0xf8f8f8),
                selected_segment: color(0xffffff),
                mono,
                font,
            }
        }
    }
}

pub fn icon(path: &'static str, size: f32, color: Hsla) -> Svg {
    svg().path(path).size(px(size)).flex_none().text_color(color)
}

pub fn spinner(id: impl Into<ElementId>, size: f32, color: Hsla) -> impl IntoElement {
    icon(icons::SPINNER, size, color).with_animation(
        id,
        Animation::new(Duration::from_millis(900)).repeat(),
        |icon, delta| icon.with_transformation(Transformation::rotate(percentage(delta))),
    )
}

pub fn tag<E: InteractiveElement>(element: E, name: impl Into<String>) -> E {
    let name = name.into();
    element.debug_selector(move || name)
}

pub fn section(t: &Tokens, title: &'static str, content: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(t.text)
                .child(title),
        )
        .child(content)
}

pub fn card(t: &Tokens, rows: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(t.line)
        .bg(t.card)
        .children(rows.into_iter().enumerate().map(|(ix, row)| {
            div()
                .when(ix > 0, |this| this.border_t_1().border_color(t.line))
                .child(row)
        }))
}

pub fn note(t: &Tokens, color: Hsla, text: impl Into<SharedString>) -> Div {
    div()
        .px(px(13.))
        .py(px(8.))
        .text_size(px(12.))
        .line_height(px(17.))
        .text_color(color)
        .child(text.into())
        .bg(t.card)
}

pub fn sub_text(t: &Tokens, text: impl Into<SharedString>) -> Div {
    div().text_size(px(11.)).text_color(t.muted).child(text.into())
}

pub fn mono(t: &Tokens, text: impl Into<SharedString>) -> Div {
    div()
        .font_family(t.mono.clone())
        .text_size(px(11.))
        .text_color(t.text2)
        .child(text.into())
}

#[derive(IntoElement)]
pub struct Row {
    tokens: Tokens,
    title: SharedString,
    sub: Option<AnyElement>,
    leading: Option<&'static str>,
    control: Option<AnyElement>,
    dim: bool,
}

impl Row {
    pub fn new(t: &Tokens, title: impl Into<SharedString>) -> Self {
        Self {
            tokens: t.clone(),
            title: title.into(),
            sub: None,
            leading: None,
            control: None,
            dim: false,
        }
    }

    pub fn sub(mut self, sub: impl IntoElement) -> Self {
        self.sub = Some(sub.into_any_element());
        self
    }

    pub fn leading(mut self, icon: &'static str) -> Self {
        self.leading = Some(icon);
        self
    }

    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    pub fn dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }
}

impl RenderOnce for Row {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let t = self.tokens;
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(13.))
            .py(px(10.))
            .min_h(px(52.))
            .when(self.dim, |this| this.opacity(0.55))
            .when_some(self.leading, |this, path| this.child(icon(path, 14., t.muted)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(t.text)
                            .child(self.title),
                    )
                    .children(self.sub),
            )
            .children(
                self.control
                    .map(|control| div().flex().flex_none().items_center().gap(px(8.)).child(control)),
            )
    }
}

pub fn button(t: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>, enabled: bool) -> Stateful<Div> {
    let press = t.press;
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(22.))
        .px(px(8.))
        .rounded(px(5.))
        .border_1()
        .border_color(t.cap_border)
        .bg(t.cap)
        .text_size(px(12.))
        .text_color(t.text)
        .whitespace_nowrap()
        .child(label.into())
        .when(enabled, |this| {
            this.cursor_pointer().hover(move |style| style.bg(press))
        })
        .when(!enabled, |this| this.opacity(0.5))
}

pub fn switch(t: &Tokens, id: impl Into<ElementId>, on: bool, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .flex_none()
        .w(px(32.))
        .h(px(19.))
        .p(px(2.))
        .rounded(px(10.))
        .bg(if on { t.accent } else { t.line2 })
        .when(on, |this| this.justify_end())
        .when(enabled, |this| this.cursor_pointer())
        .when(!enabled, |this| this.opacity(0.5))
        .child(div().size(px(15.)).rounded_full().bg(color(0xffffff)))
}

pub fn picker(t: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>, enabled: bool) -> Stateful<Div> {
    let press = t.press;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(22.))
        .pl(px(10.))
        .pr(px(6.))
        .rounded(px(6.))
        .border_1()
        .border_color(t.cap_border)
        .bg(t.cap)
        .text_size(px(12.))
        .text_color(t.text)
        .whitespace_nowrap()
        .child(label.into())
        .child(icon(icons::UP_DOWN, 10., t.muted))
        .when(enabled, |this| {
            this.cursor_pointer().hover(move |style| style.bg(press))
        })
        .when(!enabled, |this| this.opacity(0.5))
}

pub fn segmented(
    t: &Tokens,
    id: &'static str,
    labels: &[&'static str],
    selected: usize,
    enabled: bool,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let press = t.press;
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(6.))
        .bg(t.cap)
        .border_1()
        .border_color(t.cap_border)
        .when(!enabled, |this| this.opacity(0.5))
        .children(labels.iter().enumerate().map(|(ix, label)| {
            let on_select = on_select.clone();
            let chosen = ix == selected;
            tag(
                div()
                    .id(ElementId::NamedInteger(id.into(), ix as u64))
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(20.))
                    .px(px(10.))
                    .rounded(px(4.))
                    .text_size(px(12.))
                    .text_color(t.text)
                    .when(chosen, |this| this.bg(t.selected_segment))
                    .when(enabled && !chosen, |this| {
                        this.cursor_pointer().hover(move |style| style.bg(press))
                    })
                    .when(enabled, |this| {
                        this.on_click(move |_, window, cx| on_select(ix, window, cx))
                    })
                    .child(*label),
                format!("{id}:{label}"),
            )
        }))
}

pub struct MenuItem {
    pub label: SharedString,
    pub checked: bool,
    pub on_select: Rc<dyn Fn(&mut Window, &mut App)>,
}

pub enum MenuEntry {
    Heading(&'static str),
    Item(MenuItem),
    Separator,
}

/// Covers the window under an open menu, so a click anywhere else closes it and reaches nothing below.
pub fn backdrop(on_dismiss: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    deferred(
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| on_dismiss(window, cx)),
    )
    .with_priority(1)
}

/// Opens under its parent, which must be `relative`.
pub fn menu(t: &Tokens, id: &'static str, entries: Vec<MenuEntry>) -> impl IntoElement {
    let hover = t.press;
    let items = entries.into_iter().enumerate().map(|(ix, entry)| -> AnyElement {
        match entry {
            MenuEntry::Heading(text) => div()
                .px(px(10.))
                .pt(px(4.))
                .text_size(px(11.))
                .text_color(t.muted)
                .child(text)
                .into_any_element(),
            MenuEntry::Separator => div().h(px(1.)).my(px(4.)).bg(t.line).into_any_element(),
            MenuEntry::Item(item) => {
                let on_select = item.on_select;
                let name = format!("{id}:{}", item.label);
                tag(
                    div()
                        .id(ElementId::NamedInteger(id.into(), ix as u64))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(24.))
                        .px(px(8.))
                        .rounded(px(4.))
                        .text_size(px(12.))
                        .text_color(t.text)
                        .cursor_pointer()
                        .hover(move |style| style.bg(hover))
                        .on_click(move |_, window, cx| on_select(window, cx))
                        .child(div().w(px(12.)).when(item.checked, |this| this.child("✓")))
                        .child(item.label),
                    name,
                )
                .into_any_element()
            }
        }
    });
    deferred(
        div()
            .absolute()
            .top(px(26.))
            .right_0()
            .occlude()
            .min_w(px(150.))
            .p(px(4.))
            .rounded(px(10.))
            .border_1()
            .border_color(t.line2)
            .bg(t.pop)
            .shadow_lg()
            .children(items),
    )
    .with_priority(2)
}
