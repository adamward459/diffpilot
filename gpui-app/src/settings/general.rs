use super::window::SettingsWindow;
use diffpilot_core::diff::DiffMode;
use diffpilot_core::editor::Editor;
use diffpilot_core::settings::AppearanceChoice;
use diffpilot_ui_kit::Theme;
use diffpilot_ui_kit::components::menu::{MenuItem, MenuRow, MenuSection, menu_button};
use diffpilot_ui_kit::components::{Row, caption, grouped, picker, section, segmented_control, switch, tag};
use gpui::prelude::*;
use gpui::{App, Context, Corner, Div, IntoElement, Keystroke, SharedString, div, px};
use gpui_component::input::Input;
use gpui_component::{ActiveTheme, Sizable};

pub fn general(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(launch(window, t, cx))
        .child(editor(window, t, cx))
}

fn launch(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    let open = window.settings.open_last_repository();
    section(
        t,
        "Launch",
        grouped(
            t,
            vec![
                Row::new(t, "Open the last repository on launch")
                    .subtitle(caption(
                        t,
                        "Without it, diffpilot starts with no folder until you press ⌘O.",
                    ))
                    .control(
                        tag(switch(t, "open-last", open, true), || "open-last").on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.settings.set_open_last_repository(!open);
                                cx.notify();
                            },
                        )),
                    )
                    .into_any_element(),
            ],
        ),
    )
}

fn editor(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    let saved = window.settings.editor();
    let detected = window.detected_editor.clone();
    let label = match &saved {
        None => format!("Automatic ({})", detected.title()),
        Some(editor) => editor.title().to_string(),
    };
    let this = cx.entity();
    let template = window.custom_editor.clone();
    let automatic = format!("Automatic ({})", detected.title());
    let current = saved.clone();
    let control = menu_button(
        "editor",
        tag(picker(t, "editor", label, true), || "editor"),
        move |_, _| {
            let choose = |title: SharedString, choice: Option<Editor>, checked: bool| -> MenuRow {
                let (this, template) = (this.clone(), template.clone());
                MenuItem::new(title, move |_, cx| {
                    let choice = match &choice {
                        Some(Editor::Custom(_)) => Some(Editor::Custom(template.read(cx).value().to_string())),
                        other => other.clone(),
                    };
                    this.update(cx, |this, cx| {
                        this.settings.set_editor(choice);
                        cx.notify();
                    })
                })
                .checked(checked)
                .into()
            };
            let mut rows = vec![choose(automatic.clone().into(), None, current.is_none())];
            rows.extend(
                Editor::PRESETS
                    .into_iter()
                    .chain([Editor::Custom(String::new())])
                    .map(|editor| {
                        let checked = match (&current, &editor) {
                            (Some(Editor::Custom(_)), Editor::Custom(_)) => true,
                            (Some(current), editor) => current == editor,
                            (None, _) => false,
                        };
                        choose(editor.title().into(), Some(editor), checked)
                    }),
            );
            vec![MenuSection::new(rows)]
        },
    )
    .anchor(Corner::TopRight);
    let mut rows = vec![
        Row::new(t, "Open files in")
            .subtitle(caption(t, "Used by Open in Editor in the diff."))
            .control(control)
            .into_any_element(),
    ];
    if matches!(saved, Some(Editor::Custom(_))) {
        let field = div()
            .w(px(280.))
            .h(px(22.))
            .rounded(px(5.))
            .border_1()
            .border_color(t.ui.cap_border)
            .bg(t.ui.cap)
            .overflow_hidden()
            .child(
                Input::new(&window.custom_editor)
                    .small()
                    .appearance(false)
                    .bordered(false),
            );
        rows.push(
            Row::new(t, "Command")
                .subtitle(caption(t, "Use {file}, {line} and {column}."))
                .control(field)
                .into_any_element(),
        );
    }
    section(t, "Editor", grouped(t, rows))
}

pub fn appearance(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    let current = window.settings.appearance();
    let selected = AppearanceChoice::ALL
        .iter()
        .position(|choice| *choice == current)
        .unwrap_or(0);
    let this = cx.entity();
    let control = segmented_control(
        t,
        "appearance",
        &AppearanceChoice::ALL.map(AppearanceChoice::title),
        selected,
        true,
        move |ix, window, cx| {
            let choice = AppearanceChoice::ALL[ix];
            this.update(cx, |this, cx| {
                this.settings.set_appearance(choice);
                cx.notify();
            });
            super::appearance::set_choice(choice, window, cx);
        },
    );
    section(
        t,
        "Theme",
        grouped(
            t,
            vec![
                Row::new(t, "Appearance")
                    .subtitle(caption(t, "System follows macOS. Every open window changes at once."))
                    .control(control)
                    .into_any_element(),
            ],
        ),
    )
}

pub fn diff(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    let wrap = window.settings.wrap_lines();
    let mode = window.settings.diff_mode();
    let this = cx.entity();
    section(
        t,
        "Diff",
        grouped(
            t,
            vec![
                Row::new(t, "Wrap lines")
                    .subtitle(caption(t, "Same as the wrap button in the toolbar."))
                    .control(
                        tag(switch(t, "wrap-lines", wrap, true), || "wrap-lines").on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.settings.set_wrap_lines(!wrap);
                                cx.notify();
                            },
                        )),
                    )
                    .into_any_element(),
                Row::new(t, "Layout")
                    .subtitle(caption(t, "Same as the Unified and Split switch in the toolbar."))
                    .control(segmented_control(
                        t,
                        "layout",
                        &["Unified", "Split"],
                        usize::from(mode == DiffMode::Split),
                        true,
                        move |ix, _, cx| {
                            let mode = if ix == 1 { DiffMode::Split } else { DiffMode::Unified };
                            this.update(cx, |this, cx| {
                                this.settings.set_diff_mode(mode);
                                cx.notify();
                            });
                        },
                    ))
                    .into_any_element(),
            ],
        ),
    )
}

pub fn key_bindings(t: &Theme, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    let rows = bindings(cx)
        .into_iter()
        .map(|(title, keys)| Row::new(t, title).control(keycaps(t, cx, &keys)).into_any_element())
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(section(t, "Shortcuts", grouped(t, rows)))
        .child(caption(
            t,
            "These shortcuts are fixed. Changing them is not supported yet.",
        ))
}

fn keycaps(t: &Theme, cx: &App, keys: &str) -> Div {
    div()
        .px(px(6.))
        .h(px(20.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(t.ui.cap)
        .border_1()
        .border_color(t.ui.cap_border)
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(px(11.))
        .text_color(t.ui.text2)
        .child(keys.to_string())
}

const NAMESPACES: [&str; 4] = ["diffpilot", "review_ui", "settings", "sidebar"];

const TITLES: [(&str, &str); 9] = [
    ("diffpilot::OpenFolder", "Open Folder…"),
    ("diffpilot::ToggleSidebar", "Toggle Sidebar"),
    ("diffpilot::Quit", "Quit diffpilot"),
    ("settings::OpenSettings", "Settings…"),
    ("review_ui::StopReview", "Stop Review"),
    ("sidebar::SelectPrevious", "Previous file"),
    ("sidebar::SelectNext", "Next file"),
    ("sidebar::ArrowLeft", "Collapse folder"),
    ("sidebar::ArrowRight", "Expand folder"),
];

pub fn bindings(cx: &App) -> Vec<(SharedString, String)> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let mut found: Vec<(SharedString, String)> = keymap
        .bindings()
        .filter_map(|binding| {
            let name = binding.action().name();
            if !NAMESPACES.iter().any(|space| name.starts_with(&format!("{space}::"))) {
                return None;
            }
            let keys: String = binding
                .keystrokes()
                .iter()
                .map(|stroke| shortcut(stroke.inner()))
                .collect::<Vec<_>>()
                .join(" ");
            Some((action_title(name).into(), keys))
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

fn action_title(name: &str) -> String {
    if let Some((_, title)) = TITLES.iter().find(|(action, _)| *action == name) {
        return (*title).to_string();
    }
    let short = name.rsplit("::").next().unwrap_or(name);
    let mut title = String::new();
    for (ix, ch) in short.chars().enumerate() {
        if ix > 0 && ch.is_uppercase() {
            title.push(' ');
        }
        title.push(ch);
    }
    title
}

pub fn shortcut(stroke: &Keystroke) -> String {
    let modifiers = &stroke.modifiers;
    let mut text = String::new();
    for (on, glyph) in [
        (modifiers.control, "⌃"),
        (modifiers.alt, "⌥"),
        (modifiers.shift, "⇧"),
        (modifiers.platform, "⌘"),
    ] {
        if on {
            text.push_str(glyph);
        }
    }
    let key = match stroke.key.as_str() {
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "enter" => "↩".to_string(),
        "escape" => "⎋".to_string(),
        "space" => "Space".to_string(),
        other => other.to_uppercase(),
    };
    text + &key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_use_the_mac_symbols_in_the_mac_order() {
        let stroke = |text: &str| Keystroke::parse(text).unwrap();
        assert_eq!(shortcut(&stroke("cmd-o")), "⌘O");
        assert_eq!(shortcut(&stroke("ctrl-cmd-s")), "⌃⌘S");
        assert_eq!(shortcut(&stroke("shift-alt-cmd-up")), "⌥⇧⌘↑");
        assert_eq!(shortcut(&stroke("cmd-.")), "⌘.");
        assert_eq!(shortcut(&stroke("down")), "↓");
    }

    #[test]
    fn action_names_become_titles() {
        assert_eq!(action_title("diffpilot::OpenFolder"), "Open Folder…");
        assert_eq!(action_title("diffpilot::SomethingNew"), "Something New");
    }
}
