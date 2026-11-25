use gpui::{Hsla, StatefulInteractiveElement, Styled};

/// A control's look in one interaction state. `Normal` reacts to the real pointer. The others
/// hold the look still, so a gallery can show every state at once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Normal,
    Hover,
    Pressed,
    Disabled,
}

#[derive(Debug, Clone, Copy)]
pub struct Fills {
    pub rest: Hsla,
    pub hover: Hsla,
    pub press: Hsla,
}

pub const DISABLED_OPACITY: f32 = 0.45;

/// Paints the element's fill. The hover and press fills are colors, so a state change repaints
/// the same quad and never changes layout.
pub fn interactive<E: Styled + StatefulInteractiveElement>(element: E, fills: Fills, state: State) -> E {
    match state {
        State::Normal => element
            .bg(fills.rest)
            .hover(move |style| style.bg(fills.hover))
            .active(move |style| style.bg(fills.press)),
        State::Hover => element.bg(fills.hover),
        State::Pressed => element.bg(fills.press),
        State::Disabled => element.bg(fills.rest).opacity(DISABLED_OPACITY),
    }
}
