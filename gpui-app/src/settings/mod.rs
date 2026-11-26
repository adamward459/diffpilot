//! The Settings window (F1 in the v2 design): ⌘, or the app menu opens one window, with icon tabs for General,
//! Appearance, Diff, Claude, Accounts and Key Bindings.

mod accounts;
mod appearance;
mod claude_tab;
mod general;
mod window;

pub use appearance::install as install_theme;
pub use window::{Services, SettingsWindow, Tab};

use diffpilot_core::settings::Settings;
use gpui::{AnyWindowHandle, App, AppContext, Entity, Global, KeyBinding, WindowHandle, actions};
use gpui_component::Root;
use std::sync::Arc;

actions!(settings, [OpenSettings]);

/// Updated after any settings value changes, from any window or thread. Observe it to stay in sync.
#[derive(Default)]
pub struct SettingsChanged;

impl Global for SettingsChanged {}

struct Registry {
    settings: Arc<Settings>,
    services: Services,
    open: Option<(AnyWindowHandle, Entity<SettingsWindow>)>,
}

impl Global for Registry {}

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
    cx.set_global(Registry {
        settings,
        services,
        open: None,
    });
    cx.bind_keys([KeyBinding::new("cmd-,", OpenSettings, None)]);
    cx.on_action(|_: &OpenSettings, cx| {
        open(None, cx);
    });
}

/// Opens the window, or brings the open one to the front. `tab` selects a tab in either case.
pub fn open(tab: Option<Tab>, cx: &mut App) -> Option<WindowHandle<Root>> {
    let registry = cx.try_global::<Registry>()?;
    if let Some((handle, view)) = registry.open.clone()
        && cx.windows().contains(&handle)
    {
        if !show(handle, &view, tab, cx) {
            cx.defer(move |cx| {
                show(handle, &view, tab, cx);
            });
        }
        return handle.downcast::<Root>();
    }
    let (settings, services) = (registry.settings.clone(), registry.services.clone());
    let options = window::open_window_options(cx);
    let mut view = None;
    let handle = cx
        .open_window(options, |window, cx| {
            appearance::install(window, cx);
            let state = cx.new(|cx| SettingsWindow::new(settings, services, tab.unwrap_or(Tab::General), window, cx));
            view = Some(state.clone());
            cx.new(|cx| Root::new(state, window, cx))
        })
        .ok()?;
    let view = view?;
    cx.global_mut::<Registry>().open = Some((handle.into(), view));
    Some(handle)
}

/// False when the window is busy handling an event, as it is when ⌘, is pressed inside it.
fn show(handle: AnyWindowHandle, view: &Entity<SettingsWindow>, tab: Option<Tab>, cx: &mut App) -> bool {
    handle
        .update(cx, |_, window, cx| {
            if let Some(tab) = tab {
                view.update(cx, |view, cx| view.select_tab(tab, window, cx));
            }
            window.activate_window();
        })
        .is_ok()
}

#[cfg(test)]
mod tests;
