mod chrome;
pub mod highlight;
pub mod layout;
mod lines;
mod markdown;
mod preview;
pub mod rows;
mod selection;
mod stage_lines;
pub mod state;
mod view;
mod word;

#[cfg(test)]
mod testing;

pub use view::{DiffPane, DiffPaneEvent, PaneNote, Probe, bind_keys};
