use crate::glyph::Glyph;
use crate::theme::{Palette, glass_shadow};
use gpui::prelude::*;
use gpui::{App, ClickEvent, Div, ElementId, Hsla, IntoElement, SharedString, Stateful, Styled, Window, div, px};
use gpui_component::tooltip::Tooltip;
use gpui_component::{Icon, h_flex};
use std::rc::Rc;

pub type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub fn glass_surface(row: Div, palette: &Palette) -> Div {
    row.items_center()
        .bg(palette.glass)
        .border_1()
        .border_color(palette.glass_border)
        .shadow(glass_shadow(palette))
}

pub fn capsule(palette: &Palette, height: f32) -> Div {
    glass_surface(h_flex().h(px(height)).flex_shrink_0().rounded_full(), palette)
}

pub fn filter_field(palette: &Palette, input: impl IntoElement) -> Div {
    glass_surface(
        h_flex()
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .px(px(10.))
            .gap(px(6.))
            .rounded_full(),
        palette,
    )
    .child(Icon::new(Glyph::ListFilter).size(px(12.)).text_color(palette.secondary))
    .child(div().flex_1().min_w_0().child(input))
}

pub fn glass_button(
    id: &'static str,
    glyph: Glyph,
    tooltip: &'static str,
    on: bool,
    palette: &Palette,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    glass_surface(
        h_flex().size(px(28.)).flex_shrink_0().justify_center().rounded_full(),
        palette,
    )
    .id(id)
    .cursor_pointer()
    .when(on, |this| this.bg(palette.accent).border_color(palette.accent))
    .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
    .on_click(click)
    .child(
        Icon::new(glyph)
            .size(px(14.))
            .text_color(if on { palette.on_accent } else { palette.label }),
    )
}

pub fn segmented(
    id: &'static str,
    titles: &[&'static str],
    selected: usize,
    height: f32,
    palette: &Palette,
    on_select: OnSelect,
) -> Stateful<Div> {
    let segments = titles.iter().enumerate().map(|(index, title)| {
        let on_select = on_select.clone();
        div()
            .id(*title)
            .h(px(height - 8.))
            .px(px(11.))
            .flex()
            .items_center()
            .rounded_full()
            .text_size(px(13.))
            .text_color(palette.label)
            .cursor_pointer()
            .when(index == selected, |this| this.bg(palette.segment))
            .on_click(move |_, window, cx| on_select(index, window, cx))
            .child(SharedString::from(*title))
    });
    capsule(palette, height).id(id).px(px(4.)).children(segments)
}

pub fn toggle_capsule(
    id: &'static str,
    glyph: Glyph,
    tooltip: &'static str,
    on: bool,
    height: f32,
    palette: &Palette,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    capsule(palette, height)
        .id(id)
        .w(px(height))
        .justify_center()
        .cursor_pointer()
        .when(on, |this| this.bg(palette.segment))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .on_click(click)
        .child(Icon::new(glyph).size_4().text_color(palette.label))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Plain,
    Uncommitted,
    OnAccent,
}

pub fn status_badge(code: &'static str, tone: BadgeTone, mono_font: SharedString, palette: &Palette) -> Div {
    let (fill, text): (Hsla, Hsla) = match tone {
        BadgeTone::OnAccent => (palette.badge_selected_fill, palette.on_accent_dim),
        BadgeTone::Uncommitted => (palette.badge_uncommitted_fill, palette.orange),
        BadgeTone::Plain => (palette.badge_fill, palette.secondary),
    };
    div()
        .min_w(px(16.))
        .h(px(16.))
        .px(px(3.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(fill)
        .text_color(text)
        .font_family(mono_font)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_size(px(10.5))
        .child(code)
}

pub fn viewed_check(
    id: impl Into<ElementId>,
    viewed: bool,
    color: Hsla,
    hover_border: Hsla,
    group: &'static str,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(16.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .when(!viewed, |this| {
            this.border_1()
                .border_color(hover_border)
                .opacity(0.)
                .group_hover(group, |style| style.opacity(1.))
        })
        .on_click(click)
        .when(viewed, |this| {
            this.child(Icon::new(Glyph::Check).size(px(12.)).text_color(color))
        })
}
