use crate::theme;
use diffpilot_core::settings::{Appearance, AppearanceChoice, appearance_override};
use gpui::{App, Global, Window};
use gpui_component::ThemeMode;

struct Choice(AppearanceChoice);

impl Global for Choice {}

pub fn init(choice: AppearanceChoice, cx: &mut App) {
    cx.set_global(Choice(choice));
}

fn mode_of(appearance: Appearance) -> ThemeMode {
    match appearance {
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    }
}

/// `DIFFPILOT_APPEARANCE` wins over the saved choice, as before the setting existed.
fn mode(window: &Window, cx: &App) -> ThemeMode {
    let choice = cx
        .try_global::<Choice>()
        .map_or(AppearanceChoice::System, |choice| choice.0);
    let forced = appearance_override().map(mode_of).or(match choice {
        AppearanceChoice::System => None,
        AppearanceChoice::Light => Some(ThemeMode::Light),
        AppearanceChoice::Dark => Some(ThemeMode::Dark),
    });
    forced.unwrap_or_else(|| ThemeMode::from(window.appearance()))
}

fn apply(window: &mut Window, cx: &mut App) {
    let mode = mode(window, cx);
    theme::set_mode(mode, Some(window), cx);
}

/// Sets the theme for a new window and follows the system while the choice is System.
pub fn install(window: &mut Window, cx: &mut App) {
    apply(window, cx);
    window.observe_window_appearance(apply).detach();
}

/// Applies a new choice. `window` is the one handling the click, so it cannot be updated through its handle.
pub fn set_choice(choice: AppearanceChoice, window: &mut Window, cx: &mut App) {
    cx.set_global(Choice(choice));
    apply(window, cx);
    cx.refresh_windows();
}
