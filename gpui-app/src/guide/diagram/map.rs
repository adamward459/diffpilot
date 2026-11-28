use super::card::{Parsed, card_state};
use super::focus::{Focus, fractions};
use super::paint::{Drawing, Palette};
use diffpilot_core::guide::Diagram;
use diffpilot_core::guide::layout::Layout;
use diffpilot_ui_kit::Theme;
use diffpilot_ui_kit::components::{PULSE_PERIOD, loop_phase, reduced_motion, tag};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, font, px, relative,
};
use gpui_component::{ActiveTheme, h_flex};
use std::rc::Rc;

pub type OnPick = Rc<dyn Fn(usize, &mut App)>;

fn swatch(
    theme: &Theme,
    label: &'static str,
    border: gpui::Hsla,
    fill: Option<gpui::Hsla>,
    dashed: bool,
) -> impl IntoElement {
    h_flex()
        .gap(px(4.))
        .child(
            div()
                .size(px(10.))
                .rounded(px(3.))
                .border_1()
                .when(dashed, |swatch| swatch.border_dashed())
                .border_color(border)
                .when_some(fill, |swatch, fill| swatch.bg(fill)),
        )
        .child(div().text_color(theme.ui.muted).child(label))
}

/// The chapter's flowchart with the current stop's node lit. A node with stops is a button that picks its first stop:
/// `targets` pairs a node id with that note index.
pub fn chapter_map(
    id: &str,
    diagram: &Diagram,
    focus: Focus,
    targets: &[(String, usize)],
    on_pick: OnPick,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let family = cx.theme().font_family.clone();
    let (state, font_size) = card_state(id, diagram, window, cx);
    let layout = match &state.read(cx).parsed {
        Parsed::Drawn(layout) => Some(layout.clone()),
        Parsed::SourceOnly(_) => None,
    };
    let small = diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL);
    let header = h_flex()
        .px(px(4.))
        .gap(px(8.))
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
        .child(div().font_weight(FontWeight::SEMIBOLD).child("Chapter map"))
        .child(div().flex_1())
        .child(div().text_color(theme.ui.muted).child("Click a box to jump"));
    let mut map = tag(div().id(SharedString::from(id.to_string())), || "guide-map")
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(8.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme.ui.line)
        .bg(theme.ui.raised)
        .child(header);
    if let Some(layout) = layout {
        let (width, height) = layout.size();
        let flow = match &*layout {
            Layout::Flow(flow) => Some(flow.clone()),
            Layout::Sequence(_) => None,
        };
        let animated = focus.node.is_some() && !reduced_motion();
        let drawing = Drawing {
            layout,
            palette: Palette::of(&theme),
            focus: Some(focus),
            phase: None,
            font: font(family),
            font_size,
        };
        let mut surface = div().relative().w_full().max_w(px(width)).mx_auto().child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, cx| {
                    let mut drawing = drawing;
                    if animated {
                        drawing.phase = Some(loop_phase(PULSE_PERIOD));
                        window.request_animation_frame();
                    }
                    drawing.paint(bounds, window, cx);
                },
            )
            .size_full(),
        );
        if width > 0.0 && height > 0.0 {
            surface.style().aspect_ratio = Some(width / height);
        }
        if let Some(flow) = flow {
            for (node, note) in targets {
                let Some(geom) = flow.node(node) else { continue };
                let rect = geom.rect;
                let (left, top, w, h) = fractions((rect.x, rect.y, rect.w, rect.h), (width, height));
                let (pick, note, name) = (on_pick.clone(), *note, node.clone());
                surface = surface.child(
                    tag(div().id(SharedString::from(format!("{id}-node-{node}"))), move || {
                        format!("guide-map-node:{name}")
                    })
                    .absolute()
                    .left(relative(left))
                    .top(relative(top))
                    .w(relative(w))
                    .h(relative(h))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| pick(note, cx)),
                );
            }
        }
        map = map.child(surface);
    }
    map.child(
        h_flex()
            .px(px(4.))
            .gap(px(10.))
            .text_size(small)
            .child(swatch(&theme, "This stop", theme.ui.claude, None, false))
            .child(swatch(
                &theme,
                "All reviewed",
                theme.success,
                Some(theme.success.opacity(0.14)),
                false,
            ))
            .child(swatch(&theme, "Linked", theme.ui.line2, None, true)),
    )
    .into_any_element()
}
