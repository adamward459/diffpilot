use super::page::{Load, SettingsPage};
use diffpilot_core::connection::{ClaudeStatus, Login, McpState};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::{Effort, ModelName, ReviewModel};
use diffpilot_core::settings::Settings;
use diffpilot_ui_kit::components::menu::{MenuItem, MenuRow, MenuSection, menu_button};
use diffpilot_ui_kit::components::{
    Button, Row, caption, caption_code, glyph_icon, grouped, note, picker, section, spinner, switch, tag,
};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Corner, Div, HighlightStyle, Hsla, IntoElement, SharedString, StyledText, div, px};

pub fn render(page: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(intro(t))
        .child(section(t, "Connection", connection(page, t, cx)))
        .child(section(t, "Models", models(page, t, cx)))
        .child(section(t, "Editing", editing(page, t, cx)))
        .child(section(t, "Terminal agents", terminal_agents(page, t, cx)))
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
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
        .line_height(px(18.))
        .text_color(t.ui.text2)
        .child(StyledText::new(TEXT).with_highlights([(start..start + "@claude".len(), mention)]))
}

fn status_chip(icon: Glyph, color: Hsla, text: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
        .text_color(color)
        .child(glyph_icon(icon, 12., color))
        .child(text.into())
}

fn checking(t: &Theme, id: &'static str, text: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
        .text_color(t.ui.muted)
        .child(spinner(id, 12., t.ui.muted))
        .child(text)
}

fn connection(page: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> Div {
    let busy = page.path_op.running();
    let status = match &page.claude {
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
    let sub: AnyElement = match &page.claude {
        Load::Ready(ClaudeStatus::Found { path, .. }) => tag(
            caption_code(t, cx, "Uses your Claude login. Found", path.display().to_string()),
            || "claude:path",
        )
        .into_any_element(),
        Load::Ready(ClaudeStatus::NotFound) => caption(
            t,
            "Claude Code was not found. Install it, or choose where it is with Set Path…",
        )
        .into_any_element(),
        _ => caption(t, "Looking for Claude Code…").into_any_element(),
    };
    let needs_login = matches!(
        &page.claude,
        Load::Ready(ClaudeStatus::Found {
            login: Login::LoggedOut | Login::Unknown(_),
            ..
        })
    );
    let custom = page.settings.claude_path().is_some();
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
                    || "claude:recheck",
                )
                .on_click(cx.listener(|this, _, _, cx| this.refresh_claude(cx))),
            )
        })
        .when(custom && !busy, |this| {
            this.child(
                tag(
                    Button::new("claude-default").label("Use Default").disabled(!(true)),
                    || "claude:default",
                )
                .on_click(cx.listener(|this, _, _, cx| this.use_default_claude_path(cx))),
            )
        })
        .child(
            tag(
                Button::new("claude-set-path").label("Set Path…").disabled(!(!busy)),
                || "claude:set-path",
            )
            .when(!busy, |this| {
                this.on_click(cx.listener(|this, _, _, cx| this.choose_claude_path(cx)))
            }),
        );
    let mut rows = vec![
        Row::new(t, "Claude Code on this Mac")
            .leading(Glyph::Laptop)
            .subtitle(sub)
            .control(control)
            .into_any_element(),
    ];
    match &page.claude {
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
    if let Some(message) = page.path_op.failure() {
        rows.push(note(t, t.danger, message.to_string()).into_any_element());
    }
    grouped(t, rows)
}

fn model_picker(
    t: &Theme,
    cx: &mut Context<SettingsPage>,
    id: &'static str,
    model: ReviewModel,
    save: fn(&Settings, ReviewModel),
) -> impl IntoElement {
    let this = cx.entity();
    menu_button(id, tag(picker(t, id, model.title(), true), || id), move |_, _| {
        let set = |label: &str, next: ReviewModel, checked: bool| -> MenuRow {
            let this = this.clone();
            MenuItem::new(title_case(label), move |_, cx| {
                this.update(cx, |this, cx| {
                    save(&this.settings, next);
                    cx.notify();
                })
            })
            .checked(checked)
            .into()
        };
        let names = ModelName::ALL
            .map(|name| set(name.as_str(), ReviewModel::new(name, model.effort), name == model.name))
            .into();
        let efforts = Effort::ALL
            .map(|effort| {
                set(
                    effort.as_str(),
                    ReviewModel::new(model.name, effort),
                    effort == model.effort,
                )
            })
            .into();
        vec![
            MenuSection::titled("Model", names),
            MenuSection::titled("Effort", efforts),
        ]
    })
    .anchor(Corner::TopRight)
    .min_width(150.)
}

fn models(page: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> Div {
    let reviews = page.settings.reviews_model();
    let fixes = page.settings.fixes_model();
    grouped(
        t,
        vec![
            Row::new(t, "Reviews and guides")
                .subtitle(caption(t, "Review, Guide"))
                .control(model_picker(
                    t,
                    cx,
                    "model-reviews",
                    reviews,
                    Settings::set_reviews_model,
                ))
                .into_any_element(),
            Row::new(t, "Fixes and replies")
                .subtitle(caption(
                    t,
                    "Fix Comments, Make Changes, Ask Claude, @claude, Finish Review",
                ))
                .control(model_picker(t, cx, "model-fixes", fixes, Settings::set_fixes_model))
                .into_any_element(),
        ],
    )
}

fn editing(page: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> Div {
    let current = page.settings.edit_permission();
    let this = cx.entity();
    let picker = menu_button(
        "edit-permission",
        tag(
            picker(t, "edit-permission", current.title(), true),
            || "edit-permission",
        ),
        move |_, _| {
            let rows = EditPermission::ALL
                .into_iter()
                .map(|permission| {
                    let this = this.clone();
                    MenuItem::new(permission.title(), move |_, cx| {
                        this.update(cx, |this, cx| {
                            this.settings.set_edit_permission(permission);
                            cx.notify();
                        })
                    })
                    .checked(permission == current)
                    .into()
                })
                .collect();
            vec![MenuSection::new(rows)]
        },
    )
    .anchor(Corner::TopRight);
    grouped(
        t,
        vec![
            Row::new(t, "When Claude edits files")
                .subtitle(caption(
                    t,
                    "Applies to Fix Comments, Make Changes and @claude replies. Reviews never edit.",
                ))
                .control(picker)
                .into_any_element(),
        ],
    )
}

fn terminal_agents(page: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> Div {
    let allowed = page.settings.allow_terminal_agents();
    let found = page.claude_found().is_some();
    let installing = page.install_op.running();
    let state = match (&page.claude, &page.mcp) {
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
        page.mcp,
        Load::Ready(McpState::Connected | McpState::InstalledNotConnected(_))
    );
    let can_install = allowed && found && !installing && matches!(page.mcp, Load::Ready(_));
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
                || "mcp:install",
            )
            .when(can_install, |this| {
                this.on_click(cx.listener(|this, _, _, cx| this.install_mcp(cx)))
            }),
        );
    let mut rows = vec![
        Row::new(t, "Allow terminal agents")
            .subtitle(caption(
                t,
                "Claude Code in your terminal can read and answer the comments you leave here.",
            ))
            .control(
                tag(switch(t, "allow-terminal", allowed, true), || "allow-terminal")
                    .on_click(cx.listener(move |this, _, _, cx| this.set_allow_terminal_agents(!allowed, cx))),
            )
            .into_any_element(),
        Row::new(t, "Claude Code")
            .leading(Glyph::SquareTerminal)
            .subtitle(caption_code(
                t,
                cx,
                "Adds the diffpilot MCP server with",
                "claude mcp add --scope user",
            ))
            .control(control)
            .dim(!allowed)
            .into_any_element(),
    ];
    if let Load::Ready(McpState::InstalledNotConnected(detail) | McpState::Unknown(detail)) = &page.mcp {
        rows.push(note(t, t.amber, detail.clone()).into_any_element());
    }
    if let Some(message) = page.install_op.failure() {
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
