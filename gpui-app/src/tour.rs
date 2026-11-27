use gpui::{App, Bounds, Global, ParentElement, Pixels, Styled, Window, WindowId, canvas};
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stop {
    Folder,
    Comparison,
    Sidebar,
    Diff,
    Review,
    Guide,
}

pub const STOPS: &[Stop] = &[
    Stop::Folder,
    Stop::Comparison,
    Stop::Sidebar,
    Stop::Diff,
    Stop::Review,
    Stop::Guide,
];

impl Stop {
    pub fn title(self) -> &'static str {
        match self {
            Stop::Folder => "Your repository",
            Stop::Comparison => "What you compare",
            Stop::Sidebar => "Changed files",
            Stop::Diff => "The diff",
            Stop::Review => "Review with Claude",
            Stop::Guide => "Guide",
        }
    }

    pub fn body(self) -> &'static str {
        match self {
            Stop::Folder => "Pick the Git folder you want to review. Add more folders and switch between them here.",
            Stop::Comparison => {
                "Choose a branch and what it is compared to. For example, compare your branch to main, or look at staged changes only."
            }
            Stop::Sidebar => "Every changed file is listed here. Mark a file as viewed when you have read it.",
            Stop::Diff => {
                "Read the changes of each file here. You can switch between unified and split view in the View menu."
            }
            Stop::Review => "Ask Claude to review your changes. It adds comments to the diff, and it can fix them.",
            Stop::Guide => "Switch to Guide (\u{2318}2) to read the change as a story, chapter by chapter.",
        }
    }

    pub fn radius(self) -> f32 {
        match self {
            Stop::Folder => 8.,
            Stop::Comparison | Stop::Review => 19.,
            Stop::Sidebar => 20.,
            Stop::Diff => 12.,
            Stop::Guide => 16.,
        }
    }
}

#[derive(Debug, Default)]
pub struct Tour {
    step: Option<usize>,
}

impl Tour {
    pub fn is_open(&self) -> bool {
        self.step.is_some()
    }

    pub fn stop(&self) -> Option<Stop> {
        self.step.map(|step| STOPS[step])
    }

    pub fn step(&self) -> Option<usize> {
        self.step
    }

    pub fn start(&mut self, shown: &[Stop]) -> bool {
        self.step = STOPS.iter().position(|stop| shown.contains(stop));
        self.is_open()
    }

    pub fn next(&mut self, shown: &[Stop]) {
        let Some(step) = self.step else { return };
        self.step = (step + 1..STOPS.len()).find(|&index| shown.contains(&STOPS[index]));
    }

    pub fn back(&mut self, shown: &[Stop]) {
        let Some(step) = self.step else { return };
        if let Some(previous) = (0..step).rev().find(|&index| shown.contains(&STOPS[index])) {
            self.step = Some(previous);
        }
    }

    pub fn close(&mut self) {
        self.step = None;
    }

    pub fn position(&self, shown: &[Stop]) -> Option<(usize, usize)> {
        let step = self.step?;
        let before = STOPS[..step].iter().filter(|stop| shown.contains(stop)).count();
        let total = STOPS.iter().filter(|stop| shown.contains(stop)).count();
        Some((before + 1, total.max(before + 1)))
    }

    pub fn is_last(&self, shown: &[Stop]) -> bool {
        self.step
            .is_some_and(|step| !STOPS[step + 1..].iter().any(|stop| shown.contains(stop)))
    }

    pub fn is_first(&self, shown: &[Stop]) -> bool {
        self.step
            .is_some_and(|step| !STOPS[..step].iter().any(|stop| shown.contains(stop)))
    }
}

#[derive(Default)]
struct Anchors(RefCell<HashMap<(WindowId, Stop), Bounds<Pixels>>>);

impl Global for Anchors {}

pub fn anchor<E: ParentElement + Styled>(element: E, stop: Stop) -> E {
    element.child(
        canvas(
            move |bounds, window, cx| {
                if bounds.size.width >= gpui::px(1.) && bounds.size.height >= gpui::px(1.) {
                    cx.default_global::<Anchors>()
                        .0
                        .borrow_mut()
                        .insert((window.window_handle().window_id(), stop), bounds);
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0(),
    )
}

pub fn bounds_of(stop: Stop, window: &Window, cx: &App) -> Option<Bounds<Pixels>> {
    let anchors = cx.try_global::<Anchors>()?;
    let bounds = anchors
        .0
        .borrow()
        .get(&(window.window_handle().window_id(), stop))
        .copied();
    bounds
}

pub fn shown(window: &Window, cx: &App) -> Vec<Stop> {
    STOPS
        .iter()
        .copied()
        .filter(|&stop| bounds_of(stop, window, cx).is_some())
        .collect()
}

/// Forgets what the last frame painted. Call it before the views paint, so a stop whose element left the
/// window is gone.
pub fn begin_frame(window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    cx.default_global::<Anchors>()
        .0
        .borrow_mut()
        .retain(|(window, _), _| *window != id);
}
