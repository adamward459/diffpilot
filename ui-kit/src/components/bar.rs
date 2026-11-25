use super::form::glyph_icon;
use super::glass::{OnSelect, glass_surface};
use super::interaction::DISABLED_OPACITY;
use crate::icons::Glyph;
use crate::theme::Theme;
use gpui::prelude::*;
use gpui::{ClickEvent, Div, FontWeight, Hsla, SharedString, Stateful, Window, div, px};
use gpui_component::tooltip::Tooltip;
use gpui_component::{Icon, h_flex};

pub const BAR_HEIGHT: f32 = 30.0;

pub struct SwitchItem {
    pub glyph: Glyph,
    pub glyph_color: Option<Hsla>,
    pub label: &'static str,
    pub hint: &'static str,
    pub enabled: bool,
}

pub fn switch_bar(theme: &Theme, id: &'static str, items: &[SwitchItem], selected: usize, on_select: OnSelect) -> Div {
    let thumb = theme.chrome.segment;
    glass_surface(h_flex().h(px(30.)).p(px(3.)).gap(px(2.)).rounded_full(), theme).children(
        items.iter().enumerate().map(|(index, item)| {
            let on_select = on_select.clone();
            let chosen = index == selected;
            let enabled = item.enabled;
            let hover = theme.hover;
            h_flex()
                .id((id, index))
                .flex_1()
                .h_full()
                .justify_center()
                .gap(px(5.))
                .rounded_full()
                .text_size(px(12.))
                .text_color(theme.chrome.label)
                .when(chosen, |this| this.bg(thumb))
                .when(!enabled, |this| this.opacity(DISABLED_OPACITY))
                .when(enabled && !chosen, |this| {
                    this.cursor_pointer().hover(move |style| style.bg(hover))
                })
                .when(enabled, |this| {
                    this.on_click(move |_, window, cx| on_select(index, window, cx))
                })
                .child(glyph_icon(item.glyph, 12., item.glyph_color.unwrap_or(theme.ui.text2)))
                .child(item.label)
                .child(div().text_size(px(10.5)).text_color(theme.ui.faint).child(item.hint))
        }),
    )
}

pub fn jump_bar(theme: &Theme) -> Div {
    h_flex()
        .h(px(BAR_HEIGHT))
        .flex_shrink_0()
        .px(px(10.))
        .gap(px(6.))
        .border_b_1()
        .border_color(theme.ui.line)
}

pub fn status_bar(theme: &Theme) -> Div {
    h_flex()
        .h(px(BAR_HEIGHT))
        .flex_shrink_0()
        .px(px(12.))
        .gap(px(10.))
        .border_t_1()
        .border_color(theme.ui.line)
        .text_size(px(12.))
        .text_color(theme.ui.muted)
}

pub struct Crumb {
    pub glyph: Glyph,
    pub color: Option<Hsla>,
    pub label: SharedString,
    pub last: bool,
}

pub fn crumbs(theme: &Theme, items: Vec<Crumb>) -> Div {
    let count = items.len();
    h_flex()
        .min_w_0()
        .gap(px(5.))
        .overflow_hidden()
        .text_size(px(12.))
        .text_color(theme.ui.text2)
        .whitespace_nowrap()
        .children(
            items
                .into_iter()
                .enumerate()
                .flat_map(|(index, crumb)| {
                    let separator = (index + 1 < count).then(|| glyph_icon(Glyph::ChevronRight, 10., theme.ui.faint));
                    let item = h_flex()
                        .gap(px(4.))
                        .child(glyph_icon(crumb.glyph, 12., crumb.color.unwrap_or(theme.ui.muted)))
                        .child(
                            div()
                                .text_color(if crumb.last { theme.ui.text } else { theme.ui.text2 })
                                .child(crumb.label),
                        );
                    [
                        Some(item.into_any_element()),
                        separator.map(IntoElement::into_any_element),
                    ]
                })
                .flatten(),
        )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChipTone {
    Amber,
    Blue,
    Claude,
}

pub fn chip_label(theme: &Theme, text: impl Into<SharedString>, tone: ChipTone) -> Div {
    let (fill, text_color) = match tone {
        ChipTone::Amber => (theme.chrome.badge_uncommitted_fill, theme.chrome.uncommitted),
        ChipTone::Blue => (theme.accent.opacity(0.2), theme.accent_text),
        ChipTone::Claude => (theme.ui.claude_bg, theme.ui.claude),
    };
    h_flex()
        .flex_none()
        .h(px(18.))
        .px(px(6.))
        .gap(px(4.))
        .rounded_full()
        .bg(fill)
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(text_color)
        .whitespace_nowrap()
        .child(text.into())
}

pub fn bar_button(
    theme: &Theme,
    id: &'static str,
    glyph: Glyph,
    tooltip: &'static str,
    enabled: bool,
    on: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
    let (hover, press) = (theme.hover, theme.press);
    h_flex()
        .id(id)
        .size(px(22.))
        .flex_none()
        .justify_center()
        .rounded_full()
        .when(on, |this| this.bg(press))
        .when(!enabled, |this| this.opacity(DISABLED_OPACITY))
        .when(enabled, |this| {
            this.cursor_pointer()
                .hover(move |style| style.bg(hover))
                .active(move |style| style.bg(press))
                .on_click(click)
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .child(Icon::new(glyph).size(px(14.)).text_color(theme.ui.text2))
}

pub struct IconSegment {
    pub glyph: Glyph,
    pub tooltip: SharedString,
}

pub fn icon_switch(
    theme: &Theme,
    id: &'static str,
    items: Vec<IconSegment>,
    selected: usize,
    on_select: OnSelect,
) -> Div {
    let thumb = theme.chrome.segment;
    glass_surface(h_flex().h(px(28.)).p(px(2.)).gap(px(2.)).rounded_full(), theme).children(
        items.into_iter().enumerate().map(|(index, item)| {
            let on_select = on_select.clone();
            let tooltip = item.tooltip;
            let hover = theme.hover;
            h_flex()
                .id((id, index))
                .w(px(28.))
                .h_full()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .when(index == selected, |this| this.bg(thumb))
                .when(index != selected, |this| this.hover(move |style| style.bg(hover)))
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .on_click(move |_, window, cx| on_select(index, window, cx))
                .child(Icon::new(item.glyph).size(px(13.)).text_color(theme.ui.text2))
        }),
    )
}

pub fn activity_capsule(theme: &Theme, ahead: u32, behind: u32) -> Div {
    let count = |glyph: Glyph, value: u32| {
        h_flex()
            .gap(px(2.))
            .child(glyph_icon(glyph, 11., theme.ui.muted))
            .child(value.to_string())
    };
    super::glass::capsule(theme, 28.)
        .px(px(10.))
        .gap(px(8.))
        .text_size(px(12.))
        .text_color(theme.ui.text2)
        .child(count(Glyph::ArrowUp, ahead))
        .child(count(Glyph::ArrowDown, behind))
}
