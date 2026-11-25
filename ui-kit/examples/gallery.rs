//! Shows every ui-kit component. `DIFFPILOT_APPEARANCE=light|dark` picks the theme.

use diffpilot_core::review::Severity;
use diffpilot_ui_kit::components::{
    BadgeTone, Button, Fills, Look, State, avatar, capsule, card_shell, chip, filled_check, filter_field, glass_button,
    icon_button, interactive, segmented, severity_badge, status_badge, toggle_capsule, viewed_check, viewed_toggle,
};
use diffpilot_ui_kit::{Assets, Glyph, Theme, painted};
use gpui::{
    AnyElement, App, Application, Bounds, Context, IntoElement, ParentElement, Render, Styled, Window, WindowBounds,
    WindowOptions, div, font, point, prelude::*, px, size,
};
use gpui_component::{Icon, Root, Selectable, h_flex, v_flex};
use std::rc::Rc;

struct Gallery;

fn section(theme: &Theme, title: &'static str, body: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap(px(10.))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.ui.muted)
                .child(title.to_uppercase()),
        )
        .child(body)
}

fn buttons() -> impl IntoElement {
    let looks = [
        ("plain", Look::Plain),
        ("claude", Look::Claude),
        ("primary", Look::Primary),
        ("ghost", Look::Ghost),
        ("tint", Look::Tint),
    ];
    let compact = looks.map(|(name, look)| {
        Button::new((name, 0usize))
            .label(name)
            .look(look)
            .icon(Glyph::Sparkles, 12.)
            .on_click(|_, _, _| {})
    });
    let regular = looks.map(|(name, look)| {
        Button::new((name, 1usize))
            .label(name)
            .regular()
            .look(look)
            .on_click(|_, _, _| {})
    });
    v_flex()
        .gap(px(10.))
        .child(h_flex().gap(px(8.)).children(compact))
        .child(h_flex().gap(px(8.)).children(regular))
        .child(
            h_flex()
                .gap(px(8.))
                .child(Button::new("disabled").label("disabled").disabled(true))
                .child(
                    Button::new("selected")
                        .label("selected")
                        .look(Look::Ghost)
                        .selected(true),
                )
                .child(Button::new("hint").label("Comment").look(Look::Primary).hint("⌘↩")),
        )
}

fn small_controls(theme: &Theme) -> impl IntoElement {
    h_flex()
        .gap(px(10.))
        .items_center()
        .child(icon_button("copy", Glyph::Copy, theme.ui.muted))
        .child(icon_button("up", Glyph::ChevronUp, theme.ui.muted))
        .child(chip("Off", false))
        .child(chip("On", true))
}

fn glass(theme: &Theme) -> impl IntoElement {
    v_flex()
        .gap(px(12.))
        .child(
            h_flex()
                .gap(px(10.))
                .items_center()
                .child(glass_button(
                    "g-off",
                    Glyph::Pencil,
                    "off",
                    false,
                    State::Normal,
                    theme,
                    |_, _, _| {},
                ))
                .child(glass_button(
                    "g-on",
                    Glyph::Eye,
                    "on",
                    true,
                    State::Normal,
                    theme,
                    |_, _, _| {},
                ))
                .child(toggle_capsule(
                    "t-off",
                    Glyph::TextWrap,
                    "off",
                    false,
                    35.,
                    State::Normal,
                    theme,
                    |_, _, _| {},
                ))
                .child(toggle_capsule(
                    "t-on",
                    Glyph::TextWrap,
                    "on",
                    true,
                    35.,
                    State::Normal,
                    theme,
                    |_, _, _| {},
                ))
                .child(
                    capsule(theme, 35.)
                        .px(px(14.))
                        .gap(px(6.))
                        .text_color(theme.chrome.label)
                        .child(Icon::new(Glyph::GitBranch).size(px(16.)))
                        .child("feature"),
                ),
        )
        .child(
            h_flex()
                .gap(px(10.))
                .items_center()
                .child(segmented(
                    "seg",
                    &["Diff", "Edit"],
                    0,
                    35.,
                    State::Normal,
                    theme,
                    Rc::new(|_, _, _| {}),
                ))
                .child(div().w(px(220.)).child(filter_field(
                    theme,
                    div().text_color(theme.chrome.secondary).child("Filter"),
                ))),
        )
}

fn badges(theme: &Theme) -> impl IntoElement {
    let mono = || font("Menlo");
    let severities = [
        Severity::MustFix,
        Severity::Suggestion,
        Severity::Nit,
        Severity::Question,
    ]
    .map(|severity| severity_badge(severity, theme, 15.));
    v_flex()
        .gap(px(10.))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(status_badge("M", BadgeTone::Plain, mono(), theme))
                .child(status_badge("A", BadgeTone::Uncommitted, mono(), theme))
                .child(div().p(px(4.)).rounded(px(6.)).bg(theme.accent).child(status_badge(
                    "D",
                    BadgeTone::OnAccent,
                    mono(),
                    theme,
                ))),
        )
        .child(h_flex().gap(px(8.)).items_center().children(severities))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(filled_check(theme.ui.green, 16.))
                .child(avatar("Ada Lovelace", false, theme, 22.))
                .child(avatar("Claude", true, theme, 22.)),
        )
}

fn checks(theme: &Theme) -> impl IntoElement {
    h_flex()
        .gap(px(16.))
        .items_center()
        .child(
            h_flex()
                .group("row")
                .gap(px(6.))
                .items_center()
                .child(viewed_check(
                    "v-on",
                    true,
                    theme.chrome.secondary,
                    theme.ui.line,
                    "row",
                    |_, _, _| {},
                ))
                .child(viewed_check(
                    "v-off",
                    false,
                    theme.chrome.secondary,
                    theme.ui.line,
                    "row",
                    |_, _, _| {},
                )),
        )
        .child(viewed_toggle(theme, "toggle-off", false, State::Normal).ml_0())
        .child(viewed_toggle(theme, "toggle-on", true, State::Normal).ml_0())
}

fn painted_icons(theme: &Theme) -> impl IntoElement {
    let color = theme.diff.muted;
    h_flex()
        .gap(px(12.))
        .items_center()
        .child(painted::chevron(false, 16., color))
        .child(painted::chevron(true, 16., color))
        .child(painted::check(16., color))
        .child(painted::document(16., color))
        .child(painted::bubble(16., color))
        .child(painted::plus(16., color))
}

fn glyphs(theme: &Theme) -> impl IntoElement {
    h_flex().flex_wrap().gap(px(12.)).children(
        Glyph::ALL
            .iter()
            .map(|glyph| Icon::new(*glyph).size(px(18.)).text_color(theme.ui.text)),
    )
}

fn card(theme: &Theme) -> impl IntoElement {
    card_shell(theme, theme.ui.line).child(
        div()
            .p(px(12.))
            .text_size(px(13.))
            .child("A card shell with a border and a shadow."),
    )
}

const STATES: [(&str, State); 4] = [
    ("Normal", State::Normal),
    ("Hover", State::Hover),
    ("Pressed", State::Pressed),
    ("Disabled", State::Disabled),
];

fn button_cell(look: Look) -> impl Fn(&Theme, State, usize) -> AnyElement {
    move |_, state, column| {
        Button::new(("state-button", column + 10 * look as usize))
            .label("Label")
            .look(look)
            .state(state)
            .on_click(|_, _, _| {})
            .into_any_element()
    }
}

type Cell = Box<dyn Fn(&Theme, State, usize) -> AnyElement>;

fn state_rows() -> Vec<(&'static str, Cell)> {
    let mut rows: Vec<(&'static str, Cell)> = vec![
        ("Button plain", Box::new(button_cell(Look::Plain))),
        ("Button claude", Box::new(button_cell(Look::Claude))),
        ("Button primary", Box::new(button_cell(Look::Primary))),
        ("Button ghost", Box::new(button_cell(Look::Ghost))),
        ("Button tint", Box::new(button_cell(Look::Tint))),
    ];
    rows.push((
        "Pill (ghost capsule)",
        Box::new(|_, state, column| {
            Button::new(("state-pill", column))
                .look(Look::Ghost)
                .regular()
                .rounded_full()
                .h(px(24.))
                .icon(Glyph::Sparkles, 16.)
                .label("Ask Claude")
                .state(state)
                .on_click(|_, _, _| {})
                .into_any_element()
        }),
    ));
    rows.push((
        "Icon button",
        Box::new(|theme, state, column| {
            icon_button(("state-icon", column), Glyph::Copy, theme.ui.muted)
                .state(state)
                .on_click(|_, _, _| {})
                .into_any_element()
        }),
    ));
    rows.push((
        "Chip off",
        Box::new(|_, state, _| {
            chip("Off", false)
                .state(state)
                .on_click(|_, _, _| {})
                .into_any_element()
        }),
    ));
    rows.push((
        "Chip on",
        Box::new(|_, state, _| chip("On", true).state(state).on_click(|_, _, _| {}).into_any_element()),
    ));
    rows.push((
        "Glass button",
        Box::new(|theme, state, column| {
            glass_button(
                ["sg0", "sg1", "sg2", "sg3"][column],
                Glyph::Pencil,
                "t",
                false,
                state,
                theme,
                |_, _, _| {},
            )
            .into_any_element()
        }),
    ));
    rows.push((
        "Glass button on",
        Box::new(|theme, state, column| {
            glass_button(
                ["so0", "so1", "so2", "so3"][column],
                Glyph::Eye,
                "t",
                true,
                state,
                theme,
                |_, _, _| {},
            )
            .into_any_element()
        }),
    ));
    rows.push((
        "Toggle capsule",
        Box::new(|theme, state, column| {
            toggle_capsule(
                ["tc0", "tc1", "tc2", "tc3"][column],
                Glyph::TextWrap,
                "t",
                false,
                35.,
                state,
                theme,
                |_, _, _| {},
            )
            .into_any_element()
        }),
    ));
    rows.push((
        "Toggle capsule on",
        Box::new(|theme, state, column| {
            toggle_capsule(
                ["tn0", "tn1", "tn2", "tn3"][column],
                Glyph::TextWrap,
                "t",
                true,
                35.,
                state,
                theme,
                |_, _, _| {},
            )
            .into_any_element()
        }),
    ));
    rows.push((
        "Segmented (right item)",
        Box::new(|theme, state, _| {
            segmented("ss", &["Diff", "Edit"], 0, 35., state, theme, Rc::new(|_, _, _| {})).into_any_element()
        }),
    ));
    rows.push((
        "Viewed toggle",
        Box::new(|theme, state, column| {
            viewed_toggle(theme, ("sv", column), false, state)
                .ml_0()
                .into_any_element()
        }),
    ));
    rows.push((
        "List row (inset)",
        Box::new(|theme, state, column| {
            let clear = gpui::transparent_black();
            interactive(
                h_flex()
                    .id(("sr", column))
                    .w(px(120.))
                    .h(px(26.))
                    .px(px(8.))
                    .rounded(px(8.))
                    .items_center()
                    .text_size(px(12.))
                    .child("archive.md"),
                Fills {
                    rest: clear,
                    hover: theme.hover,
                    press: theme.press,
                },
                state,
            )
            .into_any_element()
        }),
    ));
    rows
}

fn states(theme: &Theme) -> impl IntoElement {
    let header = h_flex()
        .gap(px(14.))
        .child(div().w(px(150.)))
        .children(STATES.map(|(name, _)| {
            div()
                .w(px(120.))
                .text_size(px(11.))
                .text_color(theme.ui.muted)
                .child(name.to_uppercase())
        }));
    let rows = state_rows().into_iter().map(|(label, cell)| {
        h_flex()
            .gap(px(14.))
            .items_center()
            .child(
                div()
                    .w(px(150.))
                    .text_size(px(12.))
                    .text_color(theme.ui.muted)
                    .child(label),
            )
            .children(
                STATES
                    .iter()
                    .enumerate()
                    .map(|(column, (_, state))| div().w(px(120.)).child(cell(theme, *state, column))),
            )
    });
    v_flex().gap(px(10.)).child(header).children(rows)
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let left = v_flex()
            .w(px(680.))
            .gap(px(18.))
            .child(section(&theme, "Button", buttons()))
            .child(section(&theme, "Icon button and chip", small_controls(&theme)))
            .child(div().p(px(14.)).rounded(px(10.)).bg(theme.chrome.panel).child(section(
                &theme,
                "Glass",
                glass(&theme),
            )))
            .child(section(&theme, "Badges", badges(&theme)))
            .child(section(&theme, "Viewed", checks(&theme)))
            .child(section(&theme, "Painted icons", painted_icons(&theme)))
            .child(section(&theme, "Glyphs", glyphs(&theme)))
            .child(section(&theme, "Card", card(&theme)));
        let right = div().p(px(14.)).rounded(px(10.)).bg(theme.chrome.panel).child(section(
            &theme,
            "Interaction states",
            states(&theme),
        ));
        h_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.ui.text)
            .text_size(px(13.))
            .p(px(20.))
            .gap(px(24.))
            .items_start()
            .child(left)
            .child(right)
    }
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        diffpilot_ui_kit::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(80.), px(80.)),
                size(px(1500.), px(900.)),
            ))),
            ..WindowOptions::default()
        };
        cx.open_window(options, |window, cx| {
            diffpilot_ui_kit::theme::install(window, cx);
            let gallery = cx.new(|_| Gallery);
            cx.new(|cx| Root::new(gallery, window, cx))
        })
        .expect("open the window");
        cx.activate(true);
    });
}
