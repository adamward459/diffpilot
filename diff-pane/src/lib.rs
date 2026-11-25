mod chrome;
pub mod highlight;
mod icons;
pub mod layout;
mod lines;
pub mod rows;
pub mod state;
pub mod theme;
mod view;
mod word;

#[cfg(test)]
mod testing;

pub use view::{DiffPane, DiffPaneEvent, Probe};
