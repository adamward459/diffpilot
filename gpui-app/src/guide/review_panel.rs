use crate::folder_menu::folder_name;
use crate::guide::diagram::chapter_map;
use crate::guide::diagram::focus::Focus;
use crate::guide::inspector::GuideInspector;
use crate::guide::path::{self, Counts};
use crate::guide::session::{GuideSession, Step};
use diffpilot_core::guide::{Note, StopKind};
use diffpilot_ui_kit::components::{Ease, POP_DURATION, Transition, glyph_icon, icon_button, pop_scale, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_component::{h_flex, v_flex};
use std::rc::Rc;

pub struct Panel {
    pub top: AnyElement,
    pub footer: AnyElement,
    /// Index of the chapter diagram the panel draws as its map.
    pub map: Option<usize>,
}

fn kind_color(tokens: &Theme, kind: StopKind) -> Hsla {
    match kind {
        StopKind::Read => tokens.ui.claude,
        StopKind::Check => tokens.accent,
        StopKind::Skim => tokens.ui.muted,
    }
}

fn site(note: &Note) -> String {
    let file = note.path.rsplit('/').next().unwrap_or(&note.path);
    let place = match &note.span {
        Some(span) => format!("{file}:{}", span.line),
        None => file.to_string(),
    };
    match &note.folder {
        Some(folder) => format!("{} \u{b7} {place}", folder_name(folder)),
        None => place,
    }
}

fn key_hint(tokens: &Theme, key: &'static str) -> impl IntoElement {
    div()
        .px(px(5.))
        .rounded(px(4.))
        .border_1()
        .border_color(tokens.ui.line2)
        .font_family("Menlo")
        .child(key)
}

struct Row<'a> {
    chapter: usize,
    number: usize,
    note_index: usize,
    note: &'a Note,
    reviewed: bool,
    current: bool,
}

fn row(guide: &Entity<GuideSession>, tokens: &Theme, row: Row, window: &mut Window, cx: &mut App) -> AnyElement {
    let Row {
        chapter,
        number,
        note_index,
        note,
        reviewed,
        current,
    } = row;
    let key = chapter * 10_000 + number;
    let pop = Transition::follow_over(
        ("guide-stop-pop", key),
        f32::from(u8::from(reviewed)),
        POP_DURATION,
        Ease::Linear,
        window,
        cx,
    );
    let lit = Transition::follow(("guide-stop-lit", key), f32::from(u8::from(current)), window, cx);
    let small = diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL);
    let secondary = diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY);
    let circle = div()
        .size(px(20.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .text_size(small)
        .font_weight(FontWeight::SEMIBOLD)
        .map(|circle| {
            if reviewed {
                circle.bg(tokens.success)
            } else if current {
                circle.bg(tokens.ui.claude).text_color(tokens.on_accent)
            } else {
                circle.bg(tokens.ui.raised).text_color(tokens.ui.text2)
            }
        })
        .child(if reviewed {
            pop.animate(
                "guide-stop-pop",
                tag(div(), move || format!("guide-stop-done:{number}")),
                |check, scale| check.child(glyph_icon(Glyph::Check, 11. * pop_scale(scale), gpui::white())),
            )
        } else {
            div().child(number.to_string()).into_any_element()
        });
    let mut place = h_flex()
        .gap(px(4.))
        .font_family("Menlo")
        .text_size(small)
        .text_color(tokens.ui.muted)
        .child(SharedString::from(site(note)));
    if note.is_linked() {
        place = place.child(glyph_icon(Glyph::Link, 11., tokens.ui.muted));
    }
    let mut body = v_flex()
        .flex_1()
        .min_w_0()
        .gap(px(2.))
        .child(
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(
                    div()
                        .size(px(7.))
                        .flex_none()
                        .rounded_full()
                        .bg(kind_color(tokens, note.kind)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(secondary)
                        .font_weight(FontWeight::MEDIUM)
                        .child(SharedString::from(path::title(&note.text))),
                )
                .when(note.critical, |title| {
                    title.child(
                        tag(div().flex_none(), move || format!("guide-stop-critical:{number}")).child(glyph_icon(
                            Glyph::TriangleAlert,
                            12.,
                            tokens.danger,
                        )),
                    )
                }),
        )
        .child(div().pl(px(13.)).child(place));
    if let Some(check) = note.check.as_deref().filter(|_| current) {
        let name = check.to_string();
        body = body.child(
            tag(div(), move || format!("guide-stop-check:{name}"))
                .pl(px(13.))
                .text_size(secondary)
                .text_color(tokens.ui.text2)
                .child(SharedString::from(check.to_string())),
        );
    }
    let guide = guide.clone();
    let (fill, border) = (tokens.ui.claude_bg, tokens.ui.claude_border);
    let row = tag(h_flex().id(("guide-stop-row", number)), move || {
        format!("guide-stop-row:{number}")
    })
    .w_full()
    .gap(px(8.))
    .items_start()
    .p(px(8.))
    .rounded(px(8.))
    .cursor_pointer()
    .border_1()
    .when(!current, |row| row.hover(|style| style.bg(tokens.hover)))
    .on_click(move |_, _, cx| guide.update(cx, |guide, cx| guide.go_stop(note_index, cx)))
    .child(circle)
    .child(body);
    lit.animate(format!("guide-stop-lit-{key}"), row, move |row, lit| {
        row.bg(fill.opacity(lit)).border_color(border.opacity(lit))
    })
}

/// The review panel of the open chapter: counts, the lit map, the stop list and a footer with the stop keys.
/// `None` when the chapter has no stops.
pub fn review_panel(
    guide: &Entity<GuideSession>,
    step_key: &str,
    window: &mut Window,
    cx: &mut Context<GuideInspector>,
) -> Option<Panel> {
    let tokens = Theme::of(cx);
    let session = guide.read(cx);
    let (written, stops) = session.path()?;
    if stops.is_empty() {
        return None;
    }
    let stored = session.stored()?;
    let chapter = stored.guide.chapters[written].clone();
    let reading = session.chapter()?;
    let Step::Chapter(open) = session.step() else {
        return None;
    };
    let current = session.current_stop();
    let open_file = session.open_file().map(str::to_string);
    let done = session.reviewed().clone();
    let reviewed = |note: usize| done.contains(&(written, note));
    let all_done = stops.all_done(reviewed);
    let counts = Counts::of(&chapter.notes, &stops);
    let marks = path::node_marks(&chapter.notes, &stops, reviewed);
    let targets = path::node_targets(&chapter.notes, &stops);
    let map_at = path::map_diagram(&chapter.diagrams, &chapter.notes, &stops);
    let node = current
        .and_then(|note| chapter.notes.get(note))
        .and_then(|note| note.node.clone());
    let off = path::off_path(&reading.files, &chapter.notes, &stops);

    let diagram = map_at.map(|at| chapter.diagrams[at].clone());
    let next_title = session.reading().chapters.get(open + 1).map(|next| next.title.clone());
    let rows: Vec<_> = stops
        .iter()
        .map(|note| (note, stops.number(note).unwrap_or(0)))
        .collect();
    let secondary = diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY);

    let mut count_line = path::stop_count(stops.len());
    if counts.linked > 0 {
        count_line.push_str(&format!(" \u{b7} {} in a linked folder", counts.linked));
    }
    let mut top = v_flex().w_full().gap(px(8.)).child(
        v_flex()
            .gap(px(2.))
            .child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Review path"),
            )
            .child(
                tag(div(), || "guide-review-count")
                    .text_size(secondary)
                    .text_color(tokens.ui.muted)
                    .child(SharedString::from(count_line)),
            ),
    );
    if let Some(diagram) = diagram {
        let pick = guide.clone();
        top = top.child(chapter_map(
            &format!("guide-chapter-map-{step_key}"),
            &diagram,
            Focus { node, marks },
            &targets,
            Rc::new(move |note, cx| pick.update(cx, |guide, cx| guide.go_stop(note, cx))),
            window,
            cx,
        ));
    }
    if all_done {
        let forward = guide.clone();
        top = top.child(
            tag(v_flex().id("guide-chapter-reviewed"), || "guide-chapter-reviewed")
                .gap(px(2.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(10.))
                .border_1()
                .border_color(tokens.success.opacity(0.4))
                .bg(tokens.success.opacity(0.12))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.success)
                        .child("Chapter reviewed"),
                )
                .child(
                    tag(
                        div().id("guide-chapter-reviewed-next"),
                        || "guide-chapter-reviewed-next",
                    )
                    .cursor_pointer()
                    .text_size(secondary)
                    .text_color(tokens.success)
                    .child(SharedString::from(match &next_title {
                        Some(title) => format!("Next: {title} \u{203a}"),
                        None => "Back to the overview \u{203a}".to_string(),
                    }))
                    .on_click(move |_, _, cx| forward.update(cx, |guide, cx| guide.next(cx))),
                ),
        );
    }
    let mut list = v_flex().w_full().gap(px(2.));
    for (note_index, number) in rows {
        let item = Row {
            chapter: written,
            number,
            note_index,
            note: &chapter.notes[note_index],
            reviewed: reviewed(note_index),
            current: current == Some(note_index),
        };
        list = list.child(row(guide, &tokens, item, window, cx));
    }
    top = top.child(tag(list, || "guide-stop-list"));
    if !off.is_empty() {
        let mut files = h_flex()
            .flex_1()
            .min_w_0()
            .flex_wrap()
            .gap_x(px(6.))
            .child("Not on the path:");
        for path in off {
            let open = guide.clone();
            let shown = open_file.as_deref() == Some(path.as_str());
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let id = path.clone();
            let target = path.clone();
            files = files.child(
                tag(
                    div().id(SharedString::from(format!("guide-off-path-{id}"))),
                    move || match shown {
                        true => format!("guide-off-path-open:{path}"),
                        false => format!("guide-off-path:{path}"),
                    },
                )
                .px(px(4.))
                .rounded(px(4.))
                .border_1()
                .cursor_pointer()
                .font_family("Menlo")
                .text_color(tokens.ui.claude)
                .map(|file| match shown {
                    true => file.bg(tokens.ui.claude_bg).border_color(tokens.ui.claude_border),
                    false => file
                        .border_color(gpui::transparent_black())
                        .hover(|style| style.underline()),
                })
                .child(SharedString::from(name))
                .on_click(move |_, _, cx| open.update(cx, |guide, cx| guide.show_file(&target, cx))),
            );
        }
        top = top.child(
            tag(h_flex().id("guide-not-on-path"), || "guide-not-on-path")
                .gap(px(10.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_dashed()
                .border_color(tokens.ui.line2)
                .text_size(secondary)
                .text_color(tokens.ui.text2)
                .child(glyph_icon(Glyph::Eye, 14., tokens.ui.muted))
                .child(files),
        );
    }

    let (previous, next) = (guide.clone(), guide.clone());
    let footer = h_flex()
        .gap(px(8.))
        .pt(px(10.))
        .mt(px(14.))
        .border_t_1()
        .border_color(tokens.ui.line)
        .child(tag(
            icon_button("guide-stop-prev", Glyph::ChevronLeft, tokens.ui.text2)
                .on_click(move |_, _, cx| previous.update(cx, |guide, cx| guide.previous_stop(cx))),
            || "guide-stop-prev",
        ))
        .child(tag(
            icon_button("guide-stop-next", Glyph::ChevronRight, tokens.ui.text2)
                .on_click(move |_, _, cx| next.update(cx, |guide, cx| guide.next_stop(cx))),
            || "guide-stop-next",
        ))
        .child(div().flex_1())
        .child(
            h_flex()
                .gap(px(4.))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                .text_color(tokens.ui.muted)
                .child(key_hint(&tokens, "["))
                .child(key_hint(&tokens, "]"))
                .child("stop")
                .child(div().ml(px(6.)).child(key_hint(&tokens, "v")))
                .child("looks good"),
        )
        .into_any_element();
    Some(Panel {
        top: top.into_any_element(),
        footer,
        map: map_at,
    })
}
