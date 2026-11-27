use super::form::glyph_icon;
use super::interaction::{Fills, State, interactive};
use super::motion::Transition;
use crate::icons::Glyph;
use crate::theme::{Rise, Theme};
use gpui::prelude::*;
use gpui::{App, Div, ElementId, Hsla, RenderOnce, Stateful, Window, div, px};

/// A filled checkbox with a "Viewed" label. For file headers.
pub fn viewed_toggle(theme: &Theme, id: impl Into<ElementId>, viewed: bool, state: State) -> Stateful<Div> {
    let id = id.into();
    let mut checkbox = div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(16.0))
        .rounded(px(4.0))
        .border_1()
        .shadow(theme.slab_shadow(Rise::Pressed));
    checkbox = if viewed {
        checkbox.border_color(theme.accent).bg(theme.glass_body(theme.accent))
    } else {
        checkbox
            .border_color(theme.ui.faint)
            .bg(theme.glass_body(theme.ui.raised))
    }
    .child(CheckMark {
        key: ElementId::from((id.clone(), "mark")),
        shown: viewed,
        color: theme.on_accent,
    });
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
            .text_size(crate::theme::text_px(crate::theme::text_size::BODY))
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

#[derive(IntoElement)]
struct CheckMark {
    key: ElementId,
    shown: bool,
    color: Hsla,
}

impl RenderOnce for CheckMark {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let transition = Transition::follow(self.key, if self.shown { 1. } else { 0. }, window, cx);
        let color = self.color;
        transition.animate("check-mark", div(), move |holder, shown| {
            if !self.shown {
                return holder;
            }
            holder.opacity(shown).child(glyph_icon(Glyph::CheckBold, 11.0, color))
        })
    }
}
