use super::paint::{Drawing, Palette, TextMeasure};
use diffpilot_core::guide::layout::{Layout, layout};
use diffpilot_core::guide::{Diagram, DiagramKind};
use diffpilot_ui_kit::components::scroll_xy;
use diffpilot_ui_kit::components::{
    Button, ChipTone, Presence, chip_label, dialog, icon_button, segmented_control, tag,
};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::{
    AnyElement, App, Entity, EntityId, FocusHandle, FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, ScrollHandle, SharedString, Styled as _, Window, anchored, canvas, deferred, div, font, point,
    px,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use std::rc::Rc;

const MAX_HEIGHT: f32 = 340.0;
const MIN_SCALE: f32 = 0.6;
const SHEET_MARGIN: f32 = 32.0;
const SHEET_PADDING: f32 = 16.0;
const SHEET_HEADER: f32 = 44.0;
const PARSE_NOTE: &str = "This diagram could not be read, so its source is shown.";
const KIND_NOTE: &str = "This kind of diagram is not drawn yet, so its source is shown.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Diagram,
    Source,
}

pub(super) enum Parsed {
    Drawn(Rc<Layout>),
    SourceOnly(&'static str),
}

#[derive(PartialEq)]
struct Key {
    source: String,
    family: SharedString,
    size: f32,
}

pub(super) struct CardState {
    pub(super) scroll: ScrollHandle,
    view: View,
    key: Option<Key>,
    pub(super) parsed: Parsed,
}

impl CardState {
    fn new() -> Self {
        Self {
            scroll: ScrollHandle::new(),
            view: View::Diagram,
            key: None,
            parsed: Parsed::SourceOnly(PARSE_NOTE),
        }
    }

    fn refresh(&mut self, diagram: &Diagram, key: Key, measure: &TextMeasure) {
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.parsed = match diagram.graph() {
            Ok(graph) => match layout(&graph, measure) {
                Some(layout) => Parsed::Drawn(Rc::new(layout)),
                None => Parsed::SourceOnly(KIND_NOTE),
            },
            Err(_) => Parsed::SourceOnly(PARSE_NOTE),
        };
        self.key = Some(key);
    }
}

/// Shrinks a diagram to fit `MAX_HEIGHT`, but never below `MIN_SCALE`, so labels stay readable. A diagram that is
/// still too big scrolls inside the card.
fn card_scale(height: f32) -> f32 {
    if height <= 0. {
        return 1.;
    }
    (MAX_HEIGHT / height).clamp(MIN_SCALE, 1.)
}

fn kind_name(diagram: &Diagram, parsed: &Parsed) -> String {
    match parsed {
        Parsed::Drawn(layout) => match &**layout {
            Layout::Flow(_) => "Flow".into(),
            Layout::Sequence(_) => "Sequence".into(),
        },
        Parsed::SourceOnly(_) => match &diagram.kind {
            DiagramKind::Flowchart => "Flow".into(),
            DiagramKind::Sequence => "Sequence".into(),
            DiagramKind::Other(name) => name.clone(),
        },
    }
}

fn legend(theme: &Theme, layout: &Layout) -> Vec<AnyElement> {
    let Layout::Flow(flow) = layout else {
        return Vec::new();
    };
    let palette = Palette::of(theme);
    [("added", "New"), ("changed", "Changed"), ("removed", "Removed")]
        .into_iter()
        .filter(|(class, _)| flow.boxes.iter().any(|node| node.class.as_deref() == Some(class)))
        .filter_map(|(class, label)| Some((palette.class_color(class)?, label)))
        .map(|(color, label)| {
            h_flex()
                .gap(px(4.))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                .text_color(theme.ui.muted)
                .child(div().size(px(8.)).rounded_full().bg(color))
                .child(label)
                .into_any_element()
        })
        .collect()
}

fn source_block(theme: &Theme, diagram: &Diagram) -> impl IntoElement {
    tag(div().id("guide-diagram-source"), || "guide-diagram-source")
        .rounded(px(8.))
        .bg(theme.ui.raised)
        .p(px(10.))
        .font_family("Menlo")
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
        .text_color(theme.ui.text2)
        .child(SharedString::from(diagram.source.clone()))
}

/// The card state for `id`, with the diagram parsed and laid out for the current font.
pub(super) fn card_state(id: &str, diagram: &Diagram, window: &mut Window, cx: &mut App) -> (Entity<CardState>, f32) {
    let family = cx.theme().font_family.clone();
    let font_size = f32::from(cx.theme().font_size);
    let state = window.use_keyed_state(SharedString::from(format!("{id}-state")), cx, |_, _| CardState::new());
    let measure = TextMeasure::new(window, family.clone(), font_size);
    let key = Key {
        source: diagram.source.clone(),
        family,
        size: font_size,
    };
    state.update(cx, |state, _| state.refresh(diagram, key, &measure));
    (state, font_size)
}

/// A card that draws one guide diagram, with a Diagram and Source switch.
/// `id` must be unique among the cards on screen, since the card keeps its own view under it.
pub fn diagram_card(id: &str, diagram: &Diagram, window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = Theme::of(cx);
    let family = cx.theme().font_family.clone();
    let (state, font_size) = card_state(id, diagram, window, cx);

    let (view, parsed) = {
        let state = state.read(cx);
        let parsed = match &state.parsed {
            Parsed::Drawn(layout) => Ok(layout.clone()),
            Parsed::SourceOnly(note) => Err(*note),
        };
        (state.view, parsed)
    };
    let drawn = parsed.as_ref().ok();
    let kind = match &parsed {
        Ok(layout) => kind_name(diagram, &Parsed::Drawn(layout.clone())),
        Err(note) => kind_name(diagram, &Parsed::SourceOnly(note)),
    };

    let mut header = h_flex()
        .gap(px(8.))
        .flex_wrap()
        .child(
            div()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(
                    diagram.title.clone().unwrap_or_else(|| "Diagram".into()),
                )),
        )
        .child(chip_label(&theme, kind, ChipTone::Blue));
    if let (Some(layout), View::Diagram) = (drawn, view) {
        header = header.children(legend(&theme, layout));
    }
    let sheet = window.use_keyed_state(SharedString::from(format!("{id}-sheet")), cx, |_, _| {
        Presence::overlay(false)
    });
    let sheet_scroll = window.use_keyed_state(SharedString::from(format!("{id}-sheet-scroll")), cx, |_, _| {
        ScrollHandle::new()
    });
    let sheet_focus = window.use_keyed_state(SharedString::from(format!("{id}-sheet-focus")), cx, |_, cx| {
        cx.focus_handle()
    });
    let view_id = window.current_view();
    if drawn.is_some() {
        let switch = state.clone();
        let (open, focus) = (sheet.clone(), sheet_focus.read(cx).clone());
        header = header.child(div().flex_1()).child(tag(
            Button::new(SharedString::from(format!("{id}-expand")))
                .label("Expand")
                .on_click(move |_, window, cx| {
                    focus.focus(window);
                    Presence::drive(&open, true, view_id, cx);
                }),
            || "guide-diagram-expand",
        ));
        header = header.child(segmented_control(
            &theme,
            "guide-diagram-view",
            &["Diagram", "Source"],
            usize::from(view == View::Source),
            true,
            move |index, _, cx| {
                switch.update(cx, |state, cx| {
                    state.view = if index == 0 { View::Diagram } else { View::Source };
                    cx.notify();
                })
            },
        ));
    }

    let mut card = diffpilot_ui_kit::components::card_shell(&theme, theme.ui.line)
        .id(SharedString::from(id.to_string()))
        .gap(px(8.))
        .p(px(12.))
        .child(header);

    if let Ok(layout) = &parsed
        && sheet.read(cx).is_mounted()
    {
        let drawing = Drawing {
            layout: layout.clone(),
            palette: Palette::of(&theme),
            focus: None,
            phase: None,
            font: font(family.clone()),
            font_size,
        };
        let title = diagram.title.clone().unwrap_or_else(|| "Diagram".into());
        let focus = sheet_focus.read(cx).clone();
        card = card.child(full_size(
            id,
            title,
            drawing,
            &sheet,
            (focus, sheet_scroll.read(cx).clone()),
            view_id,
            &theme,
            window,
            cx,
        ));
    }

    match (parsed, view) {
        (Ok(layout), View::Diagram) => {
            let (width, height) = layout.size();
            let drawing = Drawing {
                layout,
                palette: Palette::of(&theme),
                focus: None,
                phase: None,
                font: font(family),
                font_size,
            };
            let scale = card_scale(height);
            let surface = div()
                .w(px(width * scale))
                .h(px(height * scale))
                .flex_none()
                .mx_auto()
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, (), window, cx| drawing.paint(bounds, window, cx),
                    )
                    .size_full(),
                );
            card = card.child(
                scroll_xy(
                    &state.read(cx).scroll,
                    div()
                        .id(SharedString::from(format!("{id}-scroll")))
                        .flex()
                        .child(tag(surface, || "guide-diagram-canvas")),
                )
                .w_full()
                .h(px((height * scale).min(MAX_HEIGHT))),
            );
        }
        (parsed, _) => {
            if let Err(note) = parsed {
                card = card.child(
                    tag(div().id("guide-diagram-note"), || "guide-diagram-note")
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                        .text_color(theme.ui.muted)
                        .child(note),
                );
            }
            card = card.child(source_block(&theme, diagram));
        }
    }
    card.into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn full_size(
    id: &str,
    title: String,
    drawing: Drawing,
    sheet: &Entity<Presence>,
    (focus, scroll): (FocusHandle, ScrollHandle),
    view: EntityId,
    theme: &Theme,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let (width, height) = drawing.layout.size();
    let viewport = window.viewport_size();
    let panel_width = (width + 2. * SHEET_PADDING).min(f32::from(viewport.width) - 2. * SHEET_MARGIN);
    let panel_height = (height + SHEET_HEADER + 2. * SHEET_PADDING).min(f32::from(viewport.height) - 2. * SHEET_MARGIN);
    let close = {
        let sheet = sheet.clone();
        move |cx: &mut App| Presence::drive(&sheet, false, view, cx)
    };
    let header = h_flex()
        .h(px(SHEET_HEADER))
        .flex_none()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(title)),
        )
        .child(tag(
            icon_button(
                SharedString::from(format!("{id}-sheet-close")),
                Glyph::X,
                theme.ui.muted,
            )
            .on_click({
                let close = close.clone();
                move |_, _, cx| close(cx)
            }),
            || "guide-diagram-sheet:close",
        ));
    let body = scroll_xy(
        &scroll,
        div().id(SharedString::from(format!("{id}-sheet-scroll"))).child(tag(
            div().w(px(width)).h(px(height)).flex_none().child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| drawing.paint(bounds, window, cx),
                )
                .size_full(),
            ),
            || "guide-diagram-sheet:canvas",
        )),
    )
    .w_full()
    .flex_1();
    let content = v_flex()
        .w(px(panel_width))
        .h(px(panel_height))
        .overflow_hidden()
        .px(px(SHEET_PADDING))
        .pb(px(SHEET_PADDING))
        .track_focus(&focus)
        .on_key_down({
            let close = close.clone();
            move |event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    close(cx);
                }
            }
        })
        .child(header)
        .child(body);
    let presence = *sheet.read(cx);
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div().w(viewport.width).h(viewport.height).child(
                dialog(SharedString::from(format!("{id}-sheet")), &presence)
                    .tags("guide-diagram-sheet", "guide-diagram-sheet:panel")
                    .size(panel_width, panel_height)
                    .margin(SHEET_MARGIN)
                    .on_backdrop_click(move |_, _, cx| close(cx))
                    .child(content),
            ),
        ),
    )
    .with_priority(1)
    .into_any_element()
}
