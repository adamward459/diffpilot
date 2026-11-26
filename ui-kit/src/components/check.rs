use super::interaction::{Fills, State, interactive};
use crate::icons::Glyph;
use crate::painted;
use crate::theme::{Rise, Theme};
use gpui::prelude::*;
use gpui::{App, ClickEvent, Div, ElementId, Hsla, Stateful, Window, div, px};
use gpui_component::Icon;

/// A borderless check that shows a border while its row group is hovered. For sidebar rows.
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

/// A filled checkbox with a "Viewed" label. For file headers.
pub fn viewed_toggle(theme: &Theme, id: impl Into<ElementId>, viewed: bool, state: State) -> Stateful<Div> {
    let mut checkbox = div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(16.0))
        .rounded(px(4.0))
        .border_1()
        .shadow(theme.slab_shadow(Rise::Pressed));
    checkbox = if viewed {
        checkbox
            .border_color(theme.accent)
            .bg(theme.glass_body(theme.accent))
            .child(painted::check(11.0, theme.on_accent))
    } else {
        checkbox
            .border_color(theme.ui.faint)
            .bg(theme.glass_body(theme.ui.raised))
    };
    let clear = gpui::transparent_black();
    interactive(
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .ml_auto()
            .h(px(26.0))
            .px(px(8.0))
            .rounded_full()
            .cursor_pointer()
            .text_size(px(13.0))
            .text_color(theme.ui.text2),
        Fills {
            rest: clear,
            hover: theme.hover,
            press: theme.press,
        },
        state,
    )
    .child(checkbox)
    .child("Viewed")
}
