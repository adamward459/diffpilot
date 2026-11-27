use diffpilot_core::guide::linked::{blocker, complete};
use diffpilot_ui_kit::components::{glyph_icon, tag};
use diffpilot_ui_kit::{Glyph, Theme, text_px, text_size};
use gpui::prelude::*;
use gpui::{App, Context, KeyBinding, SharedString, Task, actions, div, px};
use gpui_component::{h_flex, v_flex};
use std::path::PathBuf;

const LIMIT: usize = 6;
pub const ACTIVE: &str = "Hinting";

actions!(diffpilot_path_hints, [HintNext, HintPrev, HintAccept]);

pub fn init(cx: &mut App) {
    let context = Some("Hinting > Input");
    cx.bind_keys([
        KeyBinding::new("up", HintPrev, context),
        KeyBinding::new("down", HintNext, context),
        KeyBinding::new("tab", HintAccept, context),
    ]);
}

struct Hint {
    name: String,
    text: String,
    reason: Option<&'static str>,
}

#[derive(Default)]
pub struct PathHints {
    items: Vec<Hint>,
    highlight: Option<usize>,
    task: Option<Task<()>>,
}

impl PathHints {
    pub fn is_active(&self) -> bool {
        !self.items.is_empty()
    }

    /// Lists the sub-folders of what was typed. With `linking` set, each one that cannot be linked says why.
    pub fn set_text(&mut self, text: &str, linking: Option<(PathBuf, Vec<PathBuf>)>, cx: &mut Context<Self>) {
        self.highlight = None;
        if !diffpilot_core::guide::linked::is_path(text) {
            self.items.clear();
            self.task = None;
            cx.notify();
            return;
        }
        let text = text.to_string();
        let work = cx.background_spawn(async move {
            complete(&text, LIMIT)
                .into_iter()
                .map(|found| Hint {
                    reason: linking
                        .as_ref()
                        .and_then(|(repo, linked)| blocker(repo, linked, &found.path)),
                    name: found.name,
                    text: found.text,
                })
                .collect::<Vec<_>>()
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let items = work.await;
            this.update(cx, |this, cx| {
                this.items = items;
                this.highlight = None;
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let last = self.items.len().checked_sub(1);
        self.highlight = match (self.highlight, forward) {
            (None, true) => last.map(|_| 0),
            (Some(at), true) => last.map(|last| (at + 1).min(last)),
            (Some(at), false) => at.checked_sub(1),
            (None, false) => None,
        };
        cx.notify();
    }

    /// The text of the highlighted hint.
    pub fn highlighted(&self) -> Option<String> {
        Some(self.items.get(self.highlight?)?.text.clone())
    }

    /// The highlighted hint, or the first one when none is highlighted.
    pub fn pick(&self) -> Option<String> {
        self.highlighted().or_else(|| Some(self.items.first()?.text.clone()))
    }

    pub fn list(&self, tokens: &Theme) -> impl IntoElement + use<> {
        let rows = self.items.iter().enumerate().map(|(index, hint)| {
            let (name, shown) = (hint.name.clone(), hint.text.clone());
            let reason = hint.reason;
            let highlighted = self.highlight == Some(index);
            tag(
                h_flex()
                    .h(px(24.))
                    .px(px(8.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(5.))
                    .when(highlighted, |row| row.bg(tokens.press))
                    .child(glyph_icon(Glyph::Folder, 13., tokens.ui.text2))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family("Menlo")
                            .text_size(text_px(text_size::SMALL))
                            .text_color(tokens.ui.text2)
                            .child(SharedString::from(shown)),
                    )
                    .children(reason.map(|reason| {
                        tag(
                            div()
                                .flex_none()
                                .text_size(text_px(text_size::SMALL))
                                .text_color(tokens.amber)
                                .child(reason),
                            {
                                let name = name.clone();
                                move || format!("hint-reason:{name}:{reason}")
                            },
                        )
                    })),
                move || format!("hint:{name}"),
            )
        });
        v_flex()
            .w_full()
            .p(px(4.))
            .gap(px(1.))
            .rounded(px(7.))
            .border_1()
            .border_color(tokens.ui.line)
            .bg(tokens.ui.pop)
            .children(rows)
    }
}
