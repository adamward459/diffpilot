use super::glass::{CONTROL_HEIGHT, rim, thumb};
use crate::icons::Glyph;
use crate::theme::{Rise, Theme};
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Div, ElementId, FontWeight, Hsla, SharedString, Stateful, Transformation,
    Window, div, percentage, px,
};
use gpui_component::{ActiveTheme, Icon};
use std::rc::Rc;
use std::time::Duration;

pub fn glyph_icon(glyph: Glyph, size: f32, color: Hsla) -> Icon {
    Icon::new(glyph).size(px(size)).flex_none().text_color(color)
}

// 12 steps, one per spoke in spinner.svg.
pub fn spinner(id: impl Into<ElementId>, size: f32, color: Hsla) -> impl IntoElement {
    glyph_icon(Glyph::Spinner, size, color).with_animation(
        id,
        Animation::new(Duration::from_millis(1000)).repeat(),
        |icon, delta| icon.transform(Transformation::rotate(percentage((delta * 12.).floor() / 12.))),
    )
}

/// Names an element so tests can find it.
pub fn tag<E: InteractiveElement, S: Into<String>>(element: E, name: impl FnOnce() -> S) -> E {
    element.debug_selector(|| name().into())
}

pub fn section(theme: &Theme, title: &'static str, content: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .text_size(crate::theme::text_px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ui.text)
                .child(title),
        )
        .child(content)
}

/// Rows grouped on the raised surface, with a hairline between them.
pub fn grouped(theme: &Theme, rows: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.ui.line)
        .bg(theme.ui.raised)
        .children(rows.into_iter().enumerate().map(|(ix, row)| {
            div()
                .when(ix > 0, |this| this.border_t_1().border_color(theme.ui.line))
                .child(row)
        }))
}

pub fn note(theme: &Theme, color: Hsla, text: impl Into<SharedString>) -> Div {
    div()
        .px(px(13.))
        .py(px(8.))
        .text_size(crate::theme::text_px(14.))
        .line_height(px(20.))
        .text_color(color)
        .bg(theme.ui.raised)
        .child(text.into())
}

pub fn caption(theme: &Theme, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(crate::theme::text_px(13.))
        .line_height(px(18.))
        .text_color(theme.ui.muted)
        .child(text.into())
}

pub fn mono_text(theme: &Theme, cx: &App, text: impl Into<SharedString>) -> Div {
    div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(crate::theme::text_px(13.))
        .text_color(theme.ui.text2)
        .child(text.into())
}

#[derive(IntoElement)]
pub struct Row {
    theme: Theme,
    title: SharedString,
    sub: Option<AnyElement>,
    leading: Option<Glyph>,
    control: Option<AnyElement>,
    dim: bool,
}

impl Row {
    pub fn new(theme: &Theme, title: impl Into<SharedString>) -> Self {
        Self {
            theme: *theme,
            title: title.into(),
            sub: None,
            leading: None,
            control: None,
            dim: false,
        }
    }

    pub fn subtitle(mut self, sub: impl IntoElement) -> Self {
        self.sub = Some(sub.into_any_element());
        self
    }

    pub fn leading(mut self, glyph: Glyph) -> Self {
        self.leading = Some(glyph);
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
        let t = self.theme;
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(9.))
            .min_h(px(if self.sub.is_some() { 56. } else { 44. }))
            .when(self.dim, |this| this.opacity(0.55))
            .when_some(self.leading, |this, glyph| {
                this.child(glyph_icon(glyph, 14., t.ui.muted))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(crate::theme::text_px(15.))
                            .line_height(px(20.))
                            .text_color(t.ui.text)
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

pub fn switch(theme: &Theme, id: impl Into<ElementId>, on: bool, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .flex_none()
        .w(px(38.))
        .h(px(22.))
        .p(px(2.))
        .rounded(px(11.))
        .bg(if on { theme.accent } else { theme.ui.line2 })
        .when(on, |this| this.justify_end())
        .when(enabled, |this| this.cursor_pointer())
        .when(!enabled, |this| this.opacity(0.5))
        .child(
            div()
                .size(px(18.))
                .rounded_full()
                .bg(theme.on_accent)
                .shadow(theme.slab_shadow(Rise::Pressed)),
        )
}

/// A closed dropdown: label and up-down chevrons. The caller opens a `menu` under it.
pub fn picker(theme: &Theme, id: impl Into<ElementId>, label: impl Into<SharedString>, enabled: bool) -> Stateful<Div> {
    let press = theme.pressed(theme.ui.cap);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(CONTROL_HEIGHT))
        .pl(px(12.))
        .pr(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.ui.cap_border)
        .relative()
        .bg(theme.glass_body(theme.ui.cap))
        .shadow(theme.slab_shadow(Rise::Rest))
        .child(rim(theme, Some(7.)))
        .text_size(crate::theme::text_px(14.))
        .text_color(theme.ui.text)
        .whitespace_nowrap()
        .child(label.into())
        .child(glyph_icon(Glyph::ChevronsUpDown, 11., theme.ui.muted))
        .when(enabled, |this| {
            this.cursor_pointer().hover(move |style| style.bg(press))
        })
        .when(!enabled, |this| this.opacity(0.5))
}

/// The design's rectangular segmented control: a raised thumb on a capsule fill.
pub fn segmented_control(
    theme: &Theme,
    id: &'static str,
    labels: &[&'static str],
    selected: usize,
    enabled: bool,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let (hover, press) = (theme.hover, theme.press);
    let text = theme.ui.text;
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(8.))
        .relative()
        .bg(theme.glass_body(theme.chrome.glass))
        .border_1()
        .border_color(theme.chrome.glass_border)
        .shadow(theme.slab_shadow(Rise::Rest))
        .child(rim(theme, Some(7.)))
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
                    .h(px(24.))
                    .px(px(12.))
                    .rounded(px(6.))
                    .text_size(crate::theme::text_px(13.))
                    .text_color(text)
                    .when(chosen, |this| thumb(this, theme, Some(6.)))
                    .when(enabled && !chosen, |this| {
                        this.cursor_pointer()
                            .hover(move |style| style.bg(hover))
                            .active(move |style| style.bg(press))
                    })
                    .when(enabled, |this| {
                        this.on_click(move |_, window, cx| on_select(ix, window, cx))
                    })
                    .child(*label),
                || format!("{id}:{label}"),
            )
        }))
}
