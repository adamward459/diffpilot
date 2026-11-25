use diffpilot_core::settings::{Appearance, AppearanceChoice, appearance_override};
use diffpilot_ui_kit::theme;
use gpui::{App, Global, Window};

struct Choice(AppearanceChoice);

impl Global for Choice {}

pub fn init(choice: AppearanceChoice, cx: &mut App) {
    cx.set_global(Choice(choice));
}

/// `DIFFPILOT_APPEARANCE` wins over the saved choice, as before the setting existed.
fn dark(window: &Window, cx: &App) -> bool {
    let choice = cx
        .try_global::<Choice>()
        .map_or(AppearanceChoice::System, |choice| choice.0);
    let forced = appearance_override()
        .map(|appearance| appearance == Appearance::Dark)
        .or(match choice {
            AppearanceChoice::System => None,
            AppearanceChoice::Light => Some(false),
            AppearanceChoice::Dark => Some(true),
        });
    forced.unwrap_or_else(|| theme::is_dark(window.appearance()))
}

fn apply(window: &mut Window, cx: &mut App) {
    let dark = dark(window, cx);
    theme::set_dark(dark, Some(window), cx);
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
