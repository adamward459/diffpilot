use super::icons;
use super::ui::{self, Row, Tokens, tag};
use super::window::{Load, SettingsWindow};
use diffpilot_core::github::auth::TokenSource;
use gpui::prelude::*;
use gpui::{Context, Div, IntoElement, div, px};
use gpui_component::Sizable;
use gpui_component::input::Input;

pub fn render(window: &SettingsWindow, t: &Tokens, cx: &mut Context<SettingsWindow>) -> impl IntoElement {
    let working = window.github_op.running();
    let status = match &window.github {
        Load::Ready(status) => Some(status.clone()),
        _ => None,
    };
    let sub = match &window.github {
        Load::Ready(status) => match (&status.login, status.source) {
            (Some(login), source) => format!("Signed in as {login} · {}", SettingsWindow::github_source_text(source)),
            _ => "Not signed in".to_string(),
        },
        _ => "Checking…".to_string(),
    };
    let from_keychain = status
        .as_ref()
        .is_some_and(|status| status.source == Some(TokenSource::Keychain));
    let control = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .when(working || window.github == Load::Loading, |this| {
            this.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(12.))
                    .text_color(t.muted)
                    .child(ui::spinner("github-busy", 12., t.muted))
                    .child(if working { "Working…" } else { "Checking…" }),
            )
        })
        .child(
            tag(
                ui::button(t, "github-import", "Import from gh", !working),
                "github:import",
            )
            .when(!working, |this| {
                this.on_click(cx.listener(|this, _, _, cx| this.import_from_gh(cx)))
            }),
        )
        .when(from_keychain, |this| {
            this.child(
                tag(
                    ui::button(t, "github-sign-out", "Sign Out", !working),
                    "github:sign-out",
                )
                .when(!working, |this| {
                    this.on_click(cx.listener(|this, _, _, cx| this.sign_out(cx)))
                }),
            )
        });
    let token_field = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .w(px(220.))
                .h(px(22.))
                .rounded(px(5.))
                .border_1()
                .border_color(t.cap_border)
                .bg(t.cap)
                .overflow_hidden()
                .child(Input::new(&window.token).small().appearance(false).bordered(false)),
        )
        .child(
            tag(ui::button(t, "github-save", "Save", !working), "github:save").when(!working, |this| {
                this.on_click(cx.listener(|this, _, window, cx| this.save_token(window, cx)))
            }),
        );
    let mut rows = vec![
        Row::new(t, "github.com")
            .leading(icons::GITHUB)
            .sub(ui::sub_text(t, sub))
            .control(control)
            .into_any_element(),
        Row::new(t, "Personal access token")
            .sub(ui::sub_text(
                t,
                "Paste a token. diffpilot checks it with GitHub, then saves it in your keychain.",
            ))
            .control(token_field)
            .into_any_element(),
    ];
    if let Some(message) = window.github_op.failure() {
        rows.push(ui::note(t, t.err, message.to_string()).into_any_element());
    }
    let hint: Div = ui::sub_text(
        t,
        "Without a token saved here, diffpilot uses the one the gh CLI holds. Sign out of gh with `gh auth logout`.",
    );
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(ui::section(t, "GitHub", ui::card(t, rows)))
        .child(hint)
}
