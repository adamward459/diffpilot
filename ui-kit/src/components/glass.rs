use crate::icons::Glyph;
use crate::theme::Theme;
use gpui::prelude::*;
use gpui::{App, ClickEvent, Div, IntoElement, SharedString, Stateful, Styled, Window, div, px};
use gpui_component::tooltip::Tooltip;
use gpui_component::{Icon, h_flex};
use std::rc::Rc;

pub type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub fn glass_surface(row: Div, theme: &Theme) -> Div {
    row.items_center()
        .bg(theme.chrome.glass)
        .border_1()
        .border_color(theme.chrome.glass_border)
        .shadow(theme.glass_shadow())
}

pub fn capsule(theme: &Theme, height: f32) -> Div {
    glass_surface(h_flex().h(px(height)).flex_shrink_0().rounded_full(), theme)
}

pub fn filter_field(theme: &Theme, input: impl IntoElement) -> Div {
    glass_surface(
        h_flex()
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .px(px(10.))
            .gap(px(6.))
            .rounded_full(),
        theme,
    )
    .child(
        Icon::new(Glyph::ListFilter)
            .size(px(12.))
            .text_color(theme.chrome.secondary),
    )
    .child(div().flex_1().min_w_0().child(input))
}

pub fn glass_button(
    id: &'static str,
    glyph: Glyph,
    tooltip: &'static str,
    on: bool,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    glass_surface(
        h_flex().size(px(28.)).flex_shrink_0().justify_center().rounded_full(),
        theme,
    )
    .id(id)
    .cursor_pointer()
    .when(on, |this| this.bg(theme.accent).border_color(theme.accent))
    .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
    .on_click(click)
    .child(
        Icon::new(glyph)
            .size(px(14.))
            .text_color(if on { theme.on_accent } else { theme.chrome.label }),
    )
}

pub fn segmented(
    id: &'static str,
    titles: &[&'static str],
    selected: usize,
    height: f32,
    theme: &Theme,
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
            .text_color(theme.chrome.label)
            .cursor_pointer()
            .when(index == selected, |this| this.bg(theme.chrome.segment))
            .on_click(move |_, window, cx| on_select(index, window, cx))
            .child(SharedString::from(*title))
    });
    capsule(theme, height).id(id).px(px(4.)).children(segments)
}

pub fn toggle_capsule(
    id: &'static str,
    glyph: Glyph,
    tooltip: &'static str,
    on: bool,
    height: f32,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    capsule(theme, height)
        .id(id)
        .w(px(height))
        .justify_center()
        .cursor_pointer()
        .when(on, |this| this.bg(theme.chrome.segment))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .on_click(click)
        .child(Icon::new(glyph).size_4().text_color(theme.chrome.label))
}
