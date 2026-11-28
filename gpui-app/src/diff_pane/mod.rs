mod chrome;
pub mod highlight;
pub mod layout;
mod lines;
mod linked;
mod markdown;
mod preview;
pub mod rows;
mod selection;
mod stage_lines;
pub mod state;
mod text_select;
mod view;
mod word;

#[cfg(test)]
mod testing;

pub use linked::LinkedStop;
pub use view::{DiffPane, DiffPaneEvent, PaneNote, Probe, StopDone, bind_keys};
