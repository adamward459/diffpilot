use crate::theme::Theme;
use gpui::{BoxShadow, Div, Hsla, Styled, px};
use gpui_component::v_flex;

pub fn card_shell(theme: &Theme, border: Hsla) -> Div {
    v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(px(12.))
        .border_1()
        .border_color(border)
        .bg(theme.background)
        .text_color(theme.ui.text)
        .shadow(vec![BoxShadow {
            color: gpui::black().opacity(if theme.dark { 0.45 } else { 0.12 }),
            offset: gpui::point(px(0.), px(4.)),
            blur_radius: px(18.),
            spread_radius: px(0.),
        }])
}
