use super::interaction::{Fills, State, interactive};
use crate::painted;
use crate::theme::{Rise, Theme};
use gpui::prelude::*;
use gpui::{Div, ElementId, Stateful, div, px};

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
