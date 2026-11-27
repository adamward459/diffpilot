use super::interaction::{DISABLED_OPACITY, Fills, State};
use crate::icons::Glyph;
use crate::theme::{Rise, Theme};
use gpui::prelude::*;
use gpui::{App, ClickEvent, Div, Hsla, IntoElement, SharedString, Stateful, Styled, Window, div, px};
use gpui_component::tooltip::Tooltip;
use gpui_component::{Icon, h_flex};
use std::rc::Rc;

pub type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub const CONTROL_HEIGHT: f32 = 32.;
pub const CAPSULE_HEIGHT: f32 = 38.;

pub const OVERLAY_RADIUS: f32 = 12.;
pub const OVERLAY_PADDING: f32 = 5.;
pub const OVERLAY_ROW_HEIGHT: f32 = 24.;
pub const OVERLAY_ROW_RADIUS: f32 = 6.;
pub const OVERLAY_ROW_PADDING: f32 = 6.;
pub const OVERLAY_TEXT: f32 = crate::theme::text_size::BODY;
pub const OVERLAY_DETAIL_TEXT: f32 = crate::theme::text_size::SECONDARY;

pub fn glass_fills(theme: &Theme, rest: Hsla) -> Fills {
    Fills {
        rest,
        hover: theme.hovered(rest),
        press: theme.pressed(rest),
    }
}

/// Paints `fills` as a glass slab in `state`. A live slab lowers under the pointer and sets down on a press; the
/// glass stays put and only its light and shadow change.
pub fn slab<E: Styled + StatefulInteractiveElement>(element: E, theme: &Theme, fills: Fills, state: State) -> E {
    let rest = theme.glass_body(fills.rest);
    let hover = theme.glass_body(fills.hover);
    let press = theme.glass_body(fills.press);
    let (shadow_hover, shadow_press) = (theme.slab_shadow(Rise::Hover), theme.slab_shadow(Rise::Pressed));
    match state {
        State::Normal => element
            .bg(rest)
            .shadow(theme.slab_shadow(Rise::Rest))
            .hover(move |style| style.bg(hover).shadow(shadow_hover))
            .active(move |style| style.bg(press).shadow(shadow_press)),
        State::Hover => element.bg(hover).shadow(shadow_hover),
        State::Pressed => element.bg(press).shadow(shadow_press),
        State::Disabled => element
            .bg(rest)
            .shadow(theme.slab_shadow(Rise::Rest))
            .opacity(DISABLED_OPACITY),
    }
}

pub fn thumb<E: Styled + ParentElement>(element: E, theme: &Theme, radius: Option<f32>) -> E {
    element
        .relative()
        .bg(theme.glass_body(theme.chrome.segment))
        .shadow(theme.slab_shadow(Rise::Hover))
        .child(rim(theme, radius))
}

/// The light caught along the glass edge: bright across the top, dimmer along the bottom. A border on one side of
/// a rounded box tapers around its corners, which draws the curved highlight. `radius` is `None` for a capsule.
pub fn rim(theme: &Theme, radius: Option<f32>) -> Div {
    let layer = |div: Div| {
        let div = div.absolute().inset_0();
        match radius {
            Some(radius) => div.rounded(px(radius)),
            None => div.rounded_full(),
        }
    };
    layer(div())
        .child(layer(div()).border_t_1().border_color(theme.glass.rim_top))
        .child(layer(div()).border_b_1().border_color(theme.glass.rim_bottom))
}

pub fn glass_surface(row: Div, theme: &Theme) -> Div {
    row.relative()
        .items_center()
        .bg(theme.glass_body(theme.chrome.glass))
        .border_1()
        .border_color(theme.chrome.glass_border)
        .shadow(theme.slab_shadow(Rise::Rest))
        .child(rim(theme, None))
}

/// The floating glass surface of every menu, popover and selection bar. The rim is an absolute child, so a
/// scroll area goes inside the panel, not on it.
pub fn glass_panel(theme: &Theme) -> Div {
    div()
        .relative()
        .rounded(px(OVERLAY_RADIUS))
        .bg(theme.glass_body(theme.chrome.glass))
        .border_1()
        .border_color(theme.chrome.glass_border)
        .shadow(theme.overlay_shadow())
        .child(rim(theme, Some(OVERLAY_RADIUS)))
}

pub fn capsule(theme: &Theme, height: f32) -> Div {
    glass_surface(h_flex().h(px(height)).flex_shrink_0().rounded_full(), theme)
}

pub fn filter_field(theme: &Theme, input: impl IntoElement) -> Div {
    glass_surface(
        h_flex()
            .flex_1()
            .min_w_0()
            .h(px(CONTROL_HEIGHT))
            .px(px(12.))
            .gap(px(7.))
            .rounded_full(),
        theme,
    )
    .child(
        Icon::new(Glyph::ListFilter)
            .size(px(14.))
            .text_color(theme.chrome.secondary),
    )
    .child(div().flex_1().min_w_0().child(input))
}

pub fn glass_button(
    id: &'static str,
    glyph: Glyph,
    tooltip: &'static str,
    on: bool,
    state: State,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let shell = h_flex()
        .id(id)
        .relative()
        .child(rim(theme, None))
        .size(px(CONTROL_HEIGHT))
        .flex_shrink_0()
        .justify_center()
        .rounded_full()
        .border_1()
        .cursor_pointer();
    let (border, fills) = if on {
        let accent = Fills {
            rest: theme.accent,
            hover: theme.lifted(theme.accent, false),
            press: theme.lifted(theme.accent, true),
        };
        (theme.accent, accent)
    } else {
        (theme.chrome.glass_border, glass_fills(theme, theme.chrome.glass))
    };
    slab(shell.border_color(border), theme, fills, state)
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .on_click(click)
        .child(
            Icon::new(glyph)
                .size(px(16.))
                .text_color(if on { theme.on_accent } else { theme.chrome.label }),
        )
}

pub fn segmented(
    id: &'static str,
    titles: &[&'static str],
    selected: usize,
    height: f32,
    state: State,
    theme: &Theme,
    on_select: OnSelect,
) -> Stateful<Div> {
    let segments = titles.iter().enumerate().map(|(index, title)| {
        let on_select = on_select.clone();
        let chosen = index == selected;
        let item = div()
            .id(*title)
            .h(px(height - 8.))
            .px(px(13.))
            .flex()
            .items_center()
            .rounded_full()
            .text_size(crate::theme::text_px(crate::theme::text_size::BODY))
            .text_color(theme.chrome.label)
            .cursor_pointer();
        let item = if chosen {
            thumb(item, theme, None)
        } else {
            let (hover, press) = (theme.hover, theme.press);
            match state {
                State::Normal => item.hover(move |s| s.bg(hover)).active(move |s| s.bg(press)),
                State::Hover => item.bg(hover),
                State::Pressed => item.bg(press),
                State::Disabled => item.opacity(DISABLED_OPACITY),
            }
        };
        item.on_click(move |_, window, cx| on_select(index, window, cx))
            .child(SharedString::from(*title))
    });
    capsule(theme, height).id(id).px(px(4.)).gap(px(2.)).children(segments)
}

#[allow(clippy::too_many_arguments)]
pub fn toggle_capsule(
    id: &'static str,
    glyph: Glyph,
    tooltip: &'static str,
    on: bool,
    height: f32,
    state: State,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let base = if on {
        theme.pressed(theme.chrome.glass)
    } else {
        theme.chrome.glass
    };
    slab(
        h_flex()
            .id(id)
            .size(px(height))
            .flex_shrink_0()
            .justify_center()
            .relative()
            .rounded_full()
            .border_1()
            .border_color(theme.chrome.glass_border)
            .cursor_pointer()
            .child(rim(theme, None)),
        theme,
        glass_fills(theme, base),
        state,
    )
    .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
    .on_click(click)
    .child(Icon::new(glyph).size(px(18.)).text_color(theme.chrome.label))
}
