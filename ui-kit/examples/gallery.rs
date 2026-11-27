//! Shows every ui-kit component. `DIFFPILOT_APPEARANCE=light|dark` picks the theme.

use diffpilot_core::review::Severity;
use diffpilot_ui_kit::components::menu::{Menu, MenuItem, MenuRow, MenuSection};
use diffpilot_ui_kit::components::{
    BadgeTone, Button, Fills, Look, Presence, State, avatar, capsule, card_shell, chip, dialog, filled_check,
    filter_field, glass_button, icon_button, interactive, popover, segmented, segmented_control, severity_badge,
    side_sheet, status_badge, toggle_capsule, tour, tour_card, viewed_toggle,
};
use diffpilot_ui_kit::{Assets, Glyph, Theme};
use gpui::{
    AnyElement, App, Application, Bounds, ClickEvent, Context, Corner, Entity, IntoElement, ParentElement, Pixels,
    Render, Styled, Window, WindowBounds, WindowOptions, canvas, div, font, point, prelude::*, px, size,
};
use gpui_component::{Icon, Root, Selectable, h_flex, v_flex};
use std::rc::Rc;

type Target = Rc<std::cell::Cell<Option<Bounds<Pixels>>>>;

struct Gallery {
    menu: Entity<Menu>,
    sheet: Presence,
    dialog: Presence,
    tour: Option<usize>,
    targets: [Target; 2],
}

fn shells(theme: &Theme, targets: &[Target; 2], cx: &mut Context<Gallery>) -> impl IntoElement {
    let button = |id: &'static str, label: &'static str, field: fn(&mut Gallery) -> &mut Presence| {
        Button::new(id)
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                let presence = *field(this);
                field(this).set_open(!presence.is_open(), cx, field);
            }))
    };
    h_flex()
        .gap(px(8.))
        .child(measured(
            &targets[0],
            button("sheet-toggle", "Toggle side sheet", |this| &mut this.sheet),
        ))
        .child(measured(
            &targets[1],
            button("dialog-toggle", "Toggle dialog", |this| &mut this.dialog),
        ))
        .child(
            Button::new("tour-start")
                .label("Start tour")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.tour = Some(0);
                    cx.notify();
                })),
        )
        .text_color(theme.ui.text)
}

fn measured(target: &Target, element: impl IntoElement) -> impl IntoElement {
    let target = target.clone();
    div().relative().child(element).child(
        canvas(move |bounds, _, _| target.set(Some(bounds)), |_, _, _, _| {})
            .absolute()
            .size_full(),
    )
}

fn section(theme: &Theme, title: &'static str, body: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap(px(10.))
        .child(
            div()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
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
                )))
                .child(segmented_control(
                    theme,
                    "appearance",
                    &["System", "Light", "Dark"],
                    0,
                    true,
                    |_, _, _| {},
                )),
        )
}

fn overlays(theme: &Theme, menu: &Entity<Menu>) -> impl IntoElement {
    let note = |theme: &Theme, text: &'static str| {
        v_flex().gap(px(4.)).w(px(210.)).child(
            div()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                .text_color(theme.ui.text)
                .child(text),
        )
    };
    h_flex()
        .h(px(230.))
        .gap(px(24.))
        .items_start()
        .child(
            popover("gallery-below")
                .open(true)
                .trigger(Button::new("gallery-below-trigger").label("Opens below"))
                .content({
                    let theme = *theme;
                    move |_, _, _| note(&theme, "Popover that opens below its trigger.")
                }),
        )
        .child(
            div().h_full().flex().items_end().child(
                popover("gallery-above")
                    .anchor(Corner::BottomLeft)
                    .open(true)
                    .trigger(Button::new("gallery-above-trigger").label("Opens above"))
                    .content({
                        let theme = *theme;
                        move |_, _, _| note(&theme, "Popover that opens above its trigger.")
                    }),
            ),
        )
        .child(menu.clone())
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
        .child(viewed_toggle(theme, "toggle-off", false, State::Normal).ml_0())
        .child(viewed_toggle(theme, "toggle-on", true, State::Normal).ml_0())
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
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
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
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
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
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
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
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
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
            .child(section(&theme, "Glyphs", glyphs(&theme)))
            .child(section(&theme, "Card", card(&theme)));
        let right = v_flex()
            .gap(px(18.))
            .child(div().p(px(14.)).rounded(px(10.)).bg(theme.chrome.panel).child(section(
                &theme,
                "Interaction states",
                states(&theme),
            )))
            .child(section(&theme, "Overlays", overlays(&theme, &self.menu)))
            .child(section(
                &theme,
                "Side sheet and dialog",
                shells(&theme, &self.targets, cx),
            ));
        let panel_text = |text: &'static str| {
            div()
                .p(px(16.))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                .text_color(theme.ui.text)
                .child(text)
        };
        let close = cx.listener(|this, _, _, cx| {
            this.sheet.set_open(false, cx, |this| &mut this.sheet);
            this.dialog.set_open(false, cx, |this| &mut this.dialog);
        });
        let tour_step = self.tour;
        let tour_card = tour_step.map(|step| {
            let (title, body) = [
                (
                    "The side sheet",
                    "This button slides a glass panel in from the right edge.",
                ),
                ("The dialog", "This button fades a dialog in over a dimmed backdrop."),
            ][step];
            let last = step == 1;
            tour_card(
                title,
                body,
                format!("{} of 2", step + 1),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.tour = this.tour.map(|step| step.saturating_sub(1));
                    cx.notify();
                }),
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.tour = (!last).then(|| step + 1);
                    cx.notify();
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.tour = None;
                    cx.notify();
                }),
            )
            .can_go_back(step > 0)
            .next_label(if last { "Done" } else { "Next" })
        });
        let tour_target = self.targets.clone();
        let dialog_close = cx.listener(|this, _, _, cx| this.dialog.set_open(false, cx, |this| &mut this.dialog));
        h_flex()
            .relative()
            .size_full()
            .bg(theme.background)
            .text_color(theme.ui.text)
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
            .p(px(20.))
            .gap(px(24.))
            .items_start()
            .child(left)
            .child(right)
            .children(self.sheet.is_mounted().then(|| {
                side_sheet("gallery-sheet", &self.sheet)
                    .width(320.)
                    .on_backdrop_click(close)
                    .child(panel_text("A side sheet slides in from the right edge."))
            }))
            .children(self.dialog.is_mounted().then(|| {
                dialog("gallery-dialog", &self.dialog)
                    .size(420., 220.)
                    .on_backdrop_click(dialog_close)
                    .child(panel_text("A dialog fades in over a dimmed backdrop."))
            }))
            .children(tour_step.zip(tour_card).map(|(step, card)| {
                tour("gallery-tour", step)
                    .target(move |_, _| tour_target[step].get())
                    .radius(8.)
                    .card(card)
            }))
    }
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        diffpilot_ui_kit::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(80.), px(80.)),
                size(px(1500.), px(1060.)),
            ))),
            ..WindowOptions::default()
        };
        cx.open_window(options, |window, cx| {
            diffpilot_ui_kit::theme::install(window, cx);
            let gallery = cx.new(|cx| Gallery {
                menu: cx.new(|cx| {
                    let item = |label: &'static str| MenuItem::new(label, |_, _| {});
                    Menu::new(
                        "gallery-menu",
                        vec![
                            MenuSection::new(vec![
                                MenuRow::Item(item("Open").shortcut("⌘O")),
                                MenuRow::Item(item("Copy Path").shortcut("⌘C")),
                                MenuRow::Item(item("Reveal in Finder").checked(true)),
                            ]),
                            MenuSection::titled("More", vec![MenuRow::Item(item("Settings").detail("⌘,"))]),
                        ],
                        cx,
                    )
                }),
                sheet: Presence::new(false),
                dialog: Presence::new(false),
                tour: None,
                targets: [Target::default(), Target::default()],
            });
            cx.new(|cx| Root::new(gallery, window, cx))
        })
        .expect("open the window");
        cx.activate(true);
    });
}
