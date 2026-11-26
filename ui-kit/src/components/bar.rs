use super::form::{glyph_icon, tag};
use super::glass::{CAPSULE_HEIGHT, CONTROL_HEIGHT, OnSelect, glass_surface, thumb};
use super::interaction::DISABLED_OPACITY;
use crate::icons::Glyph;
use crate::theme::Theme;
use gpui::prelude::*;
use gpui::{ClickEvent, Div, FontWeight, Hsla, SharedString, Stateful, Window, div, px};
use gpui_component::tooltip::Tooltip;
use gpui_component::{Icon, h_flex};

pub const BAR_HEIGHT: f32 = 40.0;

pub struct SwitchItem {
    pub glyph: Glyph,
    pub glyph_color: Option<Hsla>,
    pub label: &'static str,
    pub hint: &'static str,
    pub enabled: bool,
}

pub fn switch_bar(theme: &Theme, id: &'static str, items: &[SwitchItem], selected: usize, on_select: OnSelect) -> Div {
    glass_surface(
        h_flex().h(px(CAPSULE_HEIGHT)).p(px(3.)).gap(px(2.)).rounded_full(),
        theme,
    )
    .children(items.iter().enumerate().map(|(index, item)| {
        let on_select = on_select.clone();
        let chosen = index == selected;
        let enabled = item.enabled;
        let hover = theme.hover;
        h_flex()
            .id((id, index))
            .flex_1()
            .h_full()
            .justify_center()
            .gap(px(6.))
            .rounded_full()
            .text_size(px(14.))
            .text_color(theme.chrome.label)
            .when(chosen, |this| thumb(this, theme, None))
            .when(!enabled, |this| this.opacity(DISABLED_OPACITY))
            .when(enabled && !chosen, |this| {
                this.cursor_pointer().hover(move |style| style.bg(hover))
            })
            .when(enabled, |this| {
                this.on_click(move |_, window, cx| on_select(index, window, cx))
            })
            .child(glyph_icon(item.glyph, 16., item.glyph_color.unwrap_or(theme.ui.text2)))
            .child(item.label)
            .child(div().text_size(px(12.5)).text_color(theme.ui.faint).child(item.hint))
    }))
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
        .px(px(14.))
        .gap(px(12.))
        .border_t_1()
        .border_color(theme.ui.line)
        .text_size(px(14.))
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
        .gap(px(6.))
        .overflow_hidden()
        .text_size(px(14.))
        .text_color(theme.ui.text2)
        .whitespace_nowrap()
        .children(
            items
                .into_iter()
                .enumerate()
                .flat_map(|(index, crumb)| {
                    let separator = (index + 1 < count).then(|| glyph_icon(Glyph::ChevronRight, 12., theme.ui.faint));
                    let item = h_flex()
                        .gap(px(4.))
                        .child(glyph_icon(crumb.glyph, 16., crumb.color.unwrap_or(theme.ui.muted)))
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
        .h(px(22.))
        .px(px(8.))
        .gap(px(4.))
        .rounded_full()
        .bg(fill)
        .text_size(px(13.))
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
        .size(px(CONTROL_HEIGHT))
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
        .child(Icon::new(glyph).size(px(18.)).text_color(theme.ui.text2))
}

pub struct IconSegment {
    pub name: &'static str,
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
    glass_surface(
        h_flex().h(px(CAPSULE_HEIGHT)).p(px(3.)).gap(px(2.)).rounded_full(),
        theme,
    )
    .children(items.into_iter().enumerate().map(|(index, item)| {
        let on_select = on_select.clone();
        let tooltip = item.tooltip;
        let hover = theme.hover;
        let marked = index == selected;
        tag(h_flex().id((id, index)), || format!("{id}:{}", item.name))
            .w(px(40.))
            .h_full()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .when(index == selected, |this| thumb(this, theme, None))
            .when(index != selected, |this| this.hover(move |style| style.bg(hover)))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_click(move |_, window, cx| on_select(index, window, cx))
            .child(
                div()
                    .when(marked, |this| tag(this, || format!("{id}:{}:selected", item.name)))
                    .child(Icon::new(item.glyph).size(px(18.)).text_color(theme.ui.text2)),
            )
    }))
}

pub fn activity_capsule(theme: &Theme) -> Div {
    super::glass::capsule(theme, CAPSULE_HEIGHT)
        .px(px(4.))
        .gap(px(2.))
        .text_size(px(14.))
        .text_color(theme.ui.text2)
}

pub fn status_dot(color: Hsla) -> Div {
    div().size(px(8.)).flex_none().rounded_full().bg(color)
}

pub fn label_segment(theme: &Theme, id: &'static str, items: &[(&'static str, bool)], selected: usize) -> Div {
    glass_surface(
        h_flex().h(px(CONTROL_HEIGHT)).p(px(3.)).gap(px(2.)).rounded_full(),
        theme,
    )
    .children(items.iter().enumerate().map(|(index, (label, enabled))| {
        h_flex()
            .id((id, index))
            .h_full()
            .px(px(12.))
            .rounded_full()
            .text_size(px(14.))
            .text_color(theme.chrome.label)
            .when(index == selected, |this| thumb(this, theme, None))
            .when(!enabled, |this| this.opacity(DISABLED_OPACITY))
            .child(*label)
    }))
}
