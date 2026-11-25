use super::window::{Load, Menu, SettingsWindow};
use diffpilot_core::connection::{ClaudeStatus, Login, McpState};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::{Effort, ModelName, ReviewModel};
use diffpilot_ui_kit::components::{
    Button, MenuEntry, MenuItem, Row, Select, caption, glyph_icon, grouped, menu as dropdown, mono_text, note, picker,
    section, spinner, switch, tag,
};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, HighlightStyle, Hsla, IntoElement, SharedString, StyledText, div, px};
use std::rc::Rc;

pub fn render(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(intro(t))
        .child(section(t, "Connection", connection(window, t, cx)))
        .child(section(t, "Models", models(window, t, cx)))
        .child(section(t, "Editing", editing(window, t, cx)))
        .child(section(t, "Terminal agents", terminal_agents(window, t, cx)))
}

fn intro(t: &Theme) -> impl IntoElement {
    const TEXT: &str = "Claude reviews, writes guides, fixes comments, and answers @claude. Runs may read and edit files \
                        in this folder without asking. They never push and never post to GitHub; you post replies \
                        yourself.";
    let start = TEXT.find("@claude").unwrap_or(0);
    let mention = HighlightStyle {
        color: Some(t.ui.claude),
        ..Default::default()
    };
    div()
        .text_size(px(12.))
        .line_height(px(18.))
        .text_color(t.ui.text2)
        .child(StyledText::new(TEXT).with_highlights([(start..start + "@claude".len(), mention)]))
}

fn status_chip(icon: Glyph, color: Hsla, text: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .text_size(px(12.))
        .text_color(color)
        .child(glyph_icon(icon, 12., color))
        .child(text.into())
}

fn checking(t: &Theme, id: &'static str, text: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .text_size(px(12.))
        .text_color(t.ui.muted)
        .child(spinner(id, 12., t.ui.muted))
        .child(text)
}

fn connection(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> Div {
    let busy = window.path_op.running();
    let status = match &window.claude {
        Load::Idle | Load::Loading => checking(t, "claude-check", "Checking…").into_any_element(),
        Load::Ready(ClaudeStatus::NotFound) => {
            status_chip(Glyph::CircleAlert, t.danger, "Not found").into_any_element()
        }
        Load::Ready(ClaudeStatus::Found { login, .. }) => match login {
            Login::LoggedIn => status_chip(Glyph::CircleCheck, t.success, "Logged in").into_any_element(),
            Login::LoggedOut => status_chip(Glyph::CircleAlert, t.danger, "Not logged in").into_any_element(),
            Login::Unknown(_) => status_chip(Glyph::CircleAlert, t.amber, "Login unknown").into_any_element(),
        },
    };
    let sub: AnyElement = match &window.claude {
        Load::Ready(ClaudeStatus::Found { path, .. }) => div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(caption(t, "Uses your Claude login. Found"))
            .child(mono_text(t, cx, path.display().to_string()))
            .into_any_element(),
        Load::Ready(ClaudeStatus::NotFound) => caption(
            t,
            "Claude Code was not found. Install it, or choose where it is with Set Path…",
        )
        .into_any_element(),
        _ => caption(t, "Looking for Claude Code…").into_any_element(),
    };
    let needs_login = matches!(
        &window.claude,
        Load::Ready(ClaudeStatus::Found {
            login: Login::LoggedOut | Login::Unknown(_),
            ..
        })
    );
    let custom = window.settings.claude_path().is_some();
    let control = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(if busy {
            checking(t, "path-check", "Checking file…").into_any_element()
        } else {
            status
        })
        .when(needs_login, |this| {
            this.child(
                tag(
                    Button::new("claude-recheck").label("Check Again").disabled(!(true)),
                    "claude:recheck",
                )
                .on_click(cx.listener(|this, _, _, cx| this.refresh_claude(cx))),
            )
        })
        .when(custom && !busy, |this| {
            this.child(
                tag(
                    Button::new("claude-default").label("Use Default").disabled(!(true)),
                    "claude:default",
                )
                .on_click(cx.listener(|this, _, _, cx| this.use_default_claude_path(cx))),
            )
        })
        .child(
            tag(
                Button::new("claude-set-path").label("Set Path…").disabled(!(!busy)),
                "claude:set-path",
            )
            .when(!busy, |this| {
                this.on_click(cx.listener(|this, _, _, cx| this.choose_claude_path(cx)))
            }),
        );
    let mut rows = vec![
        Row::new(t, "Claude Code on this Mac")
            .leading(Glyph::Laptop)
            .sub(sub)
            .control(control)
            .into_any_element(),
    ];
    match &window.claude {
        Load::Ready(ClaudeStatus::Found {
            login: Login::LoggedOut,
            ..
        }) => rows.push(
            note(
                t,
                t.ui.text2,
                "To log in, open a terminal, run `claude auth login`, then choose Check Again.",
            )
            .into_any_element(),
        ),
        Load::Ready(ClaudeStatus::Found {
            login: Login::Unknown(message),
            ..
        }) => rows.push(note(t, t.amber, message.clone()).into_any_element()),
        _ => {}
    }
    if let Some(message) = window.path_op.failure() {
        rows.push(note(t, t.danger, message.to_string()).into_any_element());
    }
    grouped(t, rows)
}

fn model_picker(
    window: &SettingsWindow,
    t: &Theme,
    cx: &mut Context<SettingsWindow>,
    menu: Menu,
    id: &'static str,
    model: ReviewModel,
) -> impl IntoElement {
    let open = window.menu == Some(menu);
    let this = cx.entity();
    let set = move |model: ReviewModel| -> Select {
        let this = this.clone();
        Rc::new(move |_, cx| {
            this.update(cx, |this, cx| {
                match menu {
                    Menu::Reviews => this.settings.set_reviews_model(model),
                    Menu::Fixes => this.settings.set_fixes_model(model),
                    Menu::EditPermission => {}
                }
                this.close_menu(cx);
            })
        })
    };
    let mut entries = vec![MenuEntry::Heading("Model")];
    entries.extend(ModelName::ALL.map(|name| {
        MenuEntry::Item(MenuItem {
            label: title_case(name.as_str()).into(),
            checked: name == model.name,
            on_select: set(ReviewModel::new(name, model.effort)),
        })
    }));
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Heading("Effort"));
    entries.extend(Effort::ALL.map(|effort| {
        MenuEntry::Item(MenuItem {
            label: title_case(effort.as_str()).into(),
            checked: effort == model.effort,
            on_select: set(ReviewModel::new(model.name, effort)),
        })
    }));
    div()
        .relative()
        .child(
            tag(picker(t, id, model.title(), true), id)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(menu, cx))),
        )
        .when(open, |this| this.child(dropdown(t, id, entries)))
}

fn models(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> Div {
    let reviews = window.settings.reviews_model();
    let fixes = window.settings.fixes_model();
    grouped(
        t,
        vec![
            Row::new(t, "Reviews and guides")
                .sub(caption(t, "Review, Guide"))
                .control(model_picker(window, t, cx, Menu::Reviews, "model-reviews", reviews))
                .into_any_element(),
            Row::new(t, "Fixes and replies")
                .sub(caption(
                    t,
                    "Fix Comments, Make Changes, Ask Claude, @claude, Finish Review",
                ))
                .control(model_picker(window, t, cx, Menu::Fixes, "model-fixes", fixes))
                .into_any_element(),
        ],
    )
}

fn editing(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> Div {
    let current = window.settings.edit_permission();
    let open = window.menu == Some(Menu::EditPermission);
    let this = cx.entity();
    let entries: Vec<MenuEntry> = EditPermission::ALL
        .into_iter()
        .map(|permission| {
            let this = this.clone();
            MenuEntry::Item(MenuItem {
                label: permission.title().into(),
                checked: permission == current,
                on_select: Rc::new(move |_, cx| {
                    this.update(cx, |this, cx| {
                        this.settings.set_edit_permission(permission);
                        this.close_menu(cx);
                    })
                }),
            })
        })
        .collect();
    let picker = div()
        .relative()
        .child(
            tag(picker(t, "edit-permission", current.title(), true), "edit-permission")
                .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(Menu::EditPermission, cx))),
        )
        .when(open, |this| this.child(dropdown(t, "edit-permission", entries)));
    grouped(
        t,
        vec![
            Row::new(t, "When Claude edits files")
                .sub(caption(
                    t,
                    "Applies to Fix Comments, Make Changes and @claude replies. Reviews never edit.",
                ))
                .control(picker)
                .into_any_element(),
        ],
    )
}

fn terminal_agents(window: &SettingsWindow, t: &Theme, cx: &mut Context<SettingsWindow>) -> Div {
    let allowed = window.settings.allow_terminal_agents();
    let found = window.claude_found().is_some();
    let installing = window.install_op.running();
    let state = match (&window.claude, &window.mcp) {
        (Load::Ready(ClaudeStatus::NotFound), _) => status_chip(Glyph::CircleAlert, t.ui.muted, "Needs Claude Code"),
        (_, Load::Ready(McpState::Connected)) => status_chip(Glyph::CircleCheck, t.success, "Connected"),
        (_, Load::Ready(McpState::InstalledNotConnected(_))) => {
            status_chip(Glyph::CircleAlert, t.amber, "Not connected")
        }
        (_, Load::Ready(McpState::NotInstalled)) => status_chip(Glyph::CircleAlert, t.ui.muted, "Not installed"),
        (_, Load::Ready(McpState::Unknown(_))) => status_chip(Glyph::CircleAlert, t.amber, "Unknown"),
        _ => checking(t, "mcp-check", "Checking…"),
    };
    let installed = matches!(
        window.mcp,
        Load::Ready(McpState::Connected | McpState::InstalledNotConnected(_))
    );
    let can_install = allowed && found && !installing && matches!(window.mcp, Load::Ready(_));
    let action = if installed { "Reinstall" } else { "Install" };
    let control = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(if installing {
            checking(t, "mcp-install", "Installing…")
        } else {
            state
        })
        .child(
            tag(
                Button::new("mcp-install").label(action).disabled(!(can_install)),
                "mcp:install",
            )
            .when(can_install, |this| {
                this.on_click(cx.listener(|this, _, _, cx| this.install_mcp(cx)))
            }),
        );
    let mut rows = vec![
        Row::new(t, "Allow terminal agents")
            .sub(caption(
                t,
                "Claude Code in your terminal can read and answer the comments you leave here.",
            ))
            .control(
                tag(switch(t, "allow-terminal", allowed, true), "allow-terminal")
                    .on_click(cx.listener(move |this, _, _, cx| this.set_allow_terminal_agents(!allowed, cx))),
            )
            .into_any_element(),
        Row::new(t, "Claude Code")
            .leading(Glyph::SquareTerminal)
            .sub(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(caption(t, "Adds the diffpilot MCP server with"))
                    .child(mono_text(t, cx, "claude mcp add --scope user")),
            )
            .control(control)
            .dim(!allowed)
            .into_any_element(),
    ];
    if let Load::Ready(McpState::InstalledNotConnected(detail) | McpState::Unknown(detail)) = &window.mcp {
        rows.push(note(t, t.amber, detail.clone()).into_any_element());
    }
    if let Some(message) = window.install_op.failure() {
        rows.push(note(t, t.danger, message.to_string()).into_any_element());
    }
    grouped(t, rows)
}

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}
