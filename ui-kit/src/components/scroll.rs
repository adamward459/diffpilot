use gpui::{Div, ParentElement as _, ScrollHandle, Stateful, StatefulInteractiveElement as _, Styled as _, div};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};

/// Scrolls `content` with a scrollbar over its right edge that shows while scrolling, like the diff pane's.
/// Put the outer size (`flex_1`, `max_h`, width, border) on the returned wrapper, not on `content`.
pub fn scroll_y(handle: &ScrollHandle, content: Stateful<Div>) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .min_h_0()
        .child(content.track_scroll(handle).overflow_y_scroll().flex_auto().min_h_0())
        .child(div().absolute().inset_0().child(Scrollbar::vertical(handle)))
}

/// Like `scroll_y`, for content that can be wider and taller than the wrapper. Each axis that overflows keeps its
/// scrollbar on screen, so the reader can see there is more.
pub fn scroll_xy(handle: &ScrollHandle, content: Stateful<Div>) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .min_h_0()
        .min_w_0()
        .child(
            content
                .track_scroll(handle)
                .overflow_scroll()
                .flex_auto()
                .min_h_0()
                .min_w_0(),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .child(Scrollbar::new(handle).scrollbar_show(ScrollbarShow::Always)),
        )
}
