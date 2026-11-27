use gpui::{Div, ParentElement as _, ScrollHandle, Stateful, StatefulInteractiveElement as _, Styled as _, div};
use gpui_component::scroll::Scrollbar;

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
