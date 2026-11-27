use gpui::ParentElement;
#[cfg(feature = "regions")]
use gpui::{Bounds, Pixels};

#[cfg(feature = "regions")]
thread_local! {
    static REGIONS: std::cell::RefCell<std::collections::HashMap<String, Bounds<Pixels>>> = Default::default();
}

/// Names a part of the window so a screenshot can be cropped to it.
#[cfg(feature = "regions")]
pub fn region<E: ParentElement>(element: E, name: impl FnOnce() -> String) -> E {
    use gpui::{Styled, canvas};
    let name = name();
    element.child(
        canvas(
            move |bounds, _, _| {
                REGIONS.with_borrow_mut(|regions| regions.insert(name, bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

#[cfg(not(feature = "regions"))]
#[inline]
pub fn region<E: ParentElement>(element: E, _: impl FnOnce() -> String) -> E {
    element
}

/// Where the region was in the last frame that painted it.
#[cfg(feature = "regions")]
pub fn region_bounds(name: &str) -> Option<Bounds<Pixels>> {
    REGIONS.with_borrow(|regions| regions.get(name).copied())
}
