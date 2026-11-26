mod chrome;
pub mod highlight;
pub mod layout;
mod lines;
pub mod rows;
pub mod state;
mod view;
mod word;

#[cfg(test)]
mod testing;

pub use view::{DiffPane, DiffPaneEvent, PaneNote, Probe};
