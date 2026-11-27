use super::page::{Load, SettingsPage};
use diffpilot_core::github::auth::TokenSource;
use diffpilot_ui_kit::components::{Button, Row, caption, grouped, note, section, spinner, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{Context, Div, IntoElement, div, px};
use gpui_component::Sizable;
use gpui_component::input::Input;

pub fn render(page: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> impl IntoElement {
    let working = page.github_op.running();
    let status = match &page.github {
        Load::Ready(status) => Some(status.clone()),
        _ => None,
    };
    let sub = match &page.github {
        Load::Ready(status) => match (&status.login, status.source) {
            (Some(login), source) => format!("Signed in as {login} · {}", SettingsPage::github_source_text(source)),
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
        .when(working || page.github == Load::Loading, |this| {
            this.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(diffpilot_ui_kit::text_px(12.))
                    .text_color(t.ui.muted)
                    .child(spinner("github-busy", 12., t.ui.muted))
                    .child(if working { "Working…" } else { "Checking…" }),
            )
        })
        .child(
            tag(
                Button::new("github-import")
                    .label("Import from gh")
                    .disabled(!(!working)),
                || "github:import",
            )
            .when(!working, |this| {
                this.on_click(cx.listener(|this, _, _, cx| this.import_from_gh(cx)))
            }),
        )
        .when(from_keychain, |this| {
            this.child(
                tag(
                    Button::new("github-sign-out").label("Sign Out").disabled(!(!working)),
                    || "github:sign-out",
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
                .border_color(t.ui.cap_border)
                .bg(t.ui.cap)
                .overflow_hidden()
                .child(Input::new(&page.token).small().appearance(false).bordered(false)),
        )
        .child(
            tag(
                Button::new("github-save").label("Save").disabled(!(!working)),
                || "github:save",
            )
            .when(!working, |this| {
                this.on_click(cx.listener(|this, _, window, cx| this.save_token(window, cx)))
            }),
        );
    let mut rows = vec![
        Row::new(t, "github.com")
            .leading(Glyph::Github)
            .subtitle(caption(t, sub))
            .control(control)
            .into_any_element(),
        Row::new(t, "Personal access token")
            .subtitle(caption(
                t,
                "Paste a token. Diffpilot checks it with GitHub, then saves it in your keychain.",
            ))
            .control(token_field)
            .into_any_element(),
    ];
    if let Some(message) = page.github_op.failure() {
        rows.push(note(t, t.danger, message.to_string()).into_any_element());
    }
    let hint: Div = caption(
        t,
        "Without a token saved here, Diffpilot uses the one the gh CLI holds. Sign out of gh with `gh auth logout`.",
    );
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(section(t, "GitHub", grouped(t, rows)))
        .child(hint)
}
