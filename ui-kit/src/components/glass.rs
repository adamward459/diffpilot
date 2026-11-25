use super::interaction::{Fills, State, interactive};
use crate::icons::Glyph;
use crate::theme::Theme;
use gpui::prelude::*;
use gpui::{App, ClickEvent, Div, IntoElement, SharedString, Stateful, Styled, Window, div, px};
use gpui_component::tooltip::Tooltip;
use gpui_component::{Icon, h_flex};
use std::rc::Rc;

pub type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

fn glass_fills(theme: &Theme, rest: gpui::Hsla) -> Fills {
    Fills {
        rest,
        hover: theme.hovered(rest),
        press: theme.pressed(rest),
    }
}

pub fn glass_surface(row: Div, theme: &Theme) -> Div {
    row.items_center()
        .bg(theme.ui.cap)
        .border_1()
        .border_color(theme.ui.cap_border)
}

pub fn capsule(theme: &Theme, height: f32) -> Div {
    glass_surface(h_flex().h(px(height)).flex_shrink_0().rounded(px(6.)), theme)
}

pub fn filter_field(theme: &Theme, input: impl IntoElement) -> Div {
    glass_surface(
        h_flex()
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .px(px(10.))
            .gap(px(6.))
            .rounded(px(6.)),
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
    state: State,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let fills = if on {
        Fills {
            rest: theme.accent,
            hover: theme.lifted(theme.accent, false),
            press: theme.lifted(theme.accent, true),
        }
    } else {
        glass_fills(theme, theme.ui.cap)
    };
    interactive(
        glass_surface(
            h_flex().size(px(28.)).flex_shrink_0().justify_center().rounded(px(6.)),
            theme,
        )
        .id(id)
        .cursor_pointer()
        .when(on, |this| this.border_color(theme.accent)),
        fills,
        state,
    )
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
    state: State,
    theme: &Theme,
    on_select: OnSelect,
) -> Stateful<Div> {
    let segments = titles.iter().enumerate().map(|(index, title)| {
        let on_select = on_select.clone();
        let fills = if index == selected {
            glass_fills(theme, theme.chrome.segment)
        } else {
            glass_fills(theme, gpui::transparent_black())
        };
        interactive(
            div()
                .id(*title)
                .h(px(height - 8.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .text_size(px(12.))
                .text_color(theme.chrome.label)
                .cursor_pointer(),
            fills,
            if index == selected { State::Normal } else { state },
        )
        .on_click(move |_, window, cx| on_select(index, window, cx))
        .child(SharedString::from(*title))
    });
    capsule(theme, height).id(id).px(px(2.)).children(segments)
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
    let rest = if on { theme.chrome.segment } else { theme.ui.cap };
    interactive(
        capsule(theme, height)
            .id(id)
            .w(px(height))
            .justify_center()
            .cursor_pointer(),
        glass_fills(theme, rest),
        state,
    )
    .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
    .on_click(click)
    .child(Icon::new(glyph).size_4().text_color(theme.chrome.label))
}
