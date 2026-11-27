use super::focus::{flow_focus, next_step, previous_step};
use super::paint::{Drawing, Palette, TextMeasure};
use diffpilot_core::guide::layout::{Layout, layout};
use diffpilot_core::guide::{Diagram, DiagramKind};
use diffpilot_ui_kit::components::{ChipTone, chip_label, icon_button, segmented_control, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, font, px,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use std::rc::Rc;

const MAX_HEIGHT: f32 = 340.0;
const PARSE_NOTE: &str = "This diagram could not be read, so its source is shown.";
const KIND_NOTE: &str = "This kind of diagram is not drawn yet, so its source is shown.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Diagram,
    Source,
}

enum Parsed {
    Drawn(Rc<Layout>),
    SourceOnly(&'static str),
}

#[derive(PartialEq)]
struct Key {
    source: String,
    family: SharedString,
    size: f32,
}

struct CardState {
    view: View,
    step: Option<usize>,
    key: Option<Key>,
    parsed: Parsed,
}

impl CardState {
    fn new() -> Self {
        Self {
            view: View::Diagram,
            step: None,
            key: None,
            parsed: Parsed::SourceOnly(PARSE_NOTE),
        }
    }

    fn refresh(&mut self, diagram: &Diagram, key: Key, measure: &TextMeasure) {
        if self.key.as_ref() == Some(&key) {
            return;
        }
        if self.key.as_ref().is_none_or(|old| old.source != key.source) {
            self.step = None;
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

/// A card that draws one guide diagram, with a Diagram and Source switch and, for a flowchart, a walkthrough.
/// `id` must be unique among the cards on screen, since the card keeps its own view and step under it.
pub fn diagram_card(id: &str, diagram: &Diagram, window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = Theme::of(cx);
    let family = cx.theme().font_family.clone();
    let font_size = f32::from(cx.theme().font_size);
    let state = window.use_keyed_state(SharedString::from(format!("{id}-state")), cx, |_, _| CardState::new());
    let measure = TextMeasure::new(window, family.clone(), font_size);
    let key = Key {
        source: diagram.source.clone(),
        family: family.clone(),
        size: font_size,
    };
    state.update(cx, |state, _| state.refresh(diagram, key, &measure));

    let (view, step, parsed) = {
        let state = state.read(cx);
        let parsed = match &state.parsed {
            Parsed::Drawn(layout) => Ok(layout.clone()),
            Parsed::SourceOnly(note) => Err(*note),
        };
        (state.view, state.step, parsed)
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
    if drawn.is_some() {
        let switch = state.clone();
        header = header.child(div().flex_1()).child(segmented_control(
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

    match (parsed, view) {
        (Ok(layout), View::Diagram) => {
            let (width, height) = layout.size();
            let steps: Vec<String> = diagram.flow.iter().map(|step| step.node.clone()).collect();
            let focus = match &*layout {
                Layout::Flow(_) => flow_focus(&steps, step),
                Layout::Sequence(_) => None,
            };
            let drawing = Drawing {
                layout,
                palette: Palette::of(&theme),
                focus,
                font: font(family),
                font_size,
            };
            let mut surface = div().w_full().max_w(px(width)).mx_auto().child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| drawing.paint(bounds, window, cx),
                )
                .size_full(),
            );
            if width > 0.0 && height > 0.0 {
                surface.style().aspect_ratio = Some(width / height);
            }
            card = card.child(
                div()
                    .id(SharedString::from(format!("{id}-scroll")))
                    .w_full()
                    .max_h(px(MAX_HEIGHT))
                    .overflow_y_scroll()
                    .child(tag(surface, || "guide-diagram-canvas")),
            );
            if matches!(diagram.kind, DiagramKind::Flowchart) && !steps.is_empty() {
                card = card.child(stepper(id, &theme, diagram, step, &state));
            }
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

fn stepper(
    id: &str,
    theme: &Theme,
    diagram: &Diagram,
    step: Option<usize>,
    state: &gpui::Entity<CardState>,
) -> AnyElement {
    let total = diagram.flow.len();
    let label = match step {
        Some(at) => format!("{} / {total}", at + 1),
        None => match total {
            1 => "1 step".to_string(),
            total => format!("{total} steps"),
        },
    };
    let (previous, next) = (state.clone(), state.clone());
    let controls = h_flex()
        .gap(px(4.))
        .child(tag(
            icon_button(
                SharedString::from(format!("{id}-previous")),
                Glyph::ChevronLeft,
                theme.ui.text2,
            )
            .on_click(move |_, _, cx| {
                previous.update(cx, |state, cx| {
                    state.step = previous_step(state.step);
                    cx.notify();
                })
            }),
            || "guide-diagram-previous",
        ))
        .child(
            div()
                .min_w(px(40.))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                .text_color(theme.ui.text2)
                .flex()
                .justify_center()
                .child(SharedString::from(label)),
        )
        .child(tag(
            icon_button(
                SharedString::from(format!("{id}-next")),
                Glyph::ChevronRight,
                theme.ui.text2,
            )
            .on_click(move |_, _, cx| {
                next.update(cx, |state, cx| {
                    state.step = Some(next_step(state.step, total));
                    cx.notify();
                })
            }),
            || "guide-diagram-next",
        ));
    v_flex()
        .gap(px(6.))
        .child(controls)
        .when_some(step.and_then(|at| diagram.flow.get(at)), |column, current| {
            column.child(
                tag(div().id("guide-diagram-step-text"), || "guide-diagram-step-text")
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .text_color(theme.ui.text2)
                    .child(SharedString::from(current.text.clone())),
            )
        })
        .into_any_element()
}
