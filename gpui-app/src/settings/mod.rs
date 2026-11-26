//! Settings (F1 in the v2 design): ⌘, or the app menu opens an inline page in the main window, like the
//! VS Code Settings editor. One list shows every section, and a search box filters the rows.

mod accounts;
mod appearance;
mod claude_tab;
mod entries;
mod general;
mod page;

pub use appearance::install as install_theme;
pub use page::{PageEvent, Section, Services, SettingsPage};

use diffpilot_core::settings::Settings;
use gpui::{App, Global, KeyBinding, actions};
use std::sync::Arc;

actions!(settings, [OpenSettings, CloseSettings]);

/// Updated after any settings value changes, from any window or thread. Observe it to stay in sync.
#[derive(Default)]
pub struct SettingsChanged;

impl Global for SettingsChanged {}

struct Registry {
    services: Services,
}

impl Global for Registry {}

pub const KEY_CONTEXT: &str = "SettingsPage";

pub fn init(settings: Arc<Settings>, services: Services, cx: &mut App) {
    appearance::init(settings.appearance(), cx);
    let (changed, heard) = async_channel::unbounded::<()>();
    settings.on_change(move || {
        let _ = changed.try_send(());
    });
    cx.spawn(async move |cx| {
        while heard.recv().await.is_ok() {
            while heard.try_recv().is_ok() {}
            if cx.update(|cx| cx.set_global(SettingsChanged)).is_err() {
                break;
            }
        }
    })
    .detach();
    cx.set_global(Registry { services });
    cx.bind_keys([
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("escape", CloseSettings, Some(KEY_CONTEXT)),
    ]);
}

pub fn services(cx: &App) -> Option<Services> {
    cx.try_global::<Registry>().map(|registry| registry.services.clone())
}

#[cfg(test)]
mod tests;
