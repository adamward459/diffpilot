use crate::review::session::ReviewSession;
use diffpilot_core::review::{Effort, ModelName, ReviewModel, ReviewScope, focus};
use diffpilot_core::run::RunState;
use diffpilot_ui_kit::components::menu::{MenuItem, MenuSection, menu_button};
use diffpilot_ui_kit::components::{Button, Look, chip, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AppContext, Context, Corner, Entity, EventEmitter, FontWeight, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::radio::{Radio, RadioGroup};
use gpui_component::{Icon, h_flex, v_flex};

pub const INSTRUCTIONS_HINT: &str = "Optional. For example “Check error handling in the new API routes” or “This is a perf refactor, look for regressions”.";
pub const WIDTH: f32 = 440.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopoverEvent {
    Dismiss,
}

pub struct ReviewPopover {
    session: Entity<ReviewSession>,
    instructions: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PopoverEvent> for ReviewPopover {}

impl ReviewPopover {
    pub fn new(session: Entity<ReviewSession>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = session.read(cx).setup().instructions.clone();
        let instructions = cx.new(|cx| {
            let mut state = InputState::new(window, cx).auto_grow(2, 6);
            if !text.is_empty() {
                state.set_value(text, window, cx);
            }
            state
        });
        let subscriptions = vec![
            cx.observe(&session, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &instructions,
                window,
                |this, input, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => {
                        let value = input.read(cx).value().to_string();
                        this.session
                            .update(cx, |session, cx| session.update_setup(|s| s.instructions = value, cx));
                    }
                    InputEvent::PressEnter { secondary: true } => this.start(cx),
                    _ => {}
                },
            ),
        ];
        Self {
            session,
            instructions,
            _subscriptions: subscriptions,
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        cx.emit(PopoverEvent::Dismiss);
        self.session.update(cx, |session, cx| session.start_review(cx));
    }

    fn model_menu(&self, model: ReviewModel) -> AnyElement {
        let session = self.session.clone();
        let trigger = tag(
            Button::new("review-model")
                .icon(Glyph::Sparkles, 12.)
                .label(model.title())
                .regular(),
            move || format!("review:model:{}", model.title()),
        );
        menu_button("review-model", trigger, move |_, _| {
            let pick = |label: &str, next: ReviewModel, checked: bool| {
                let session = session.clone();
                MenuItem::new(title_case(label), move |_, cx| {
                    session.update(cx, |session, cx| session.update_setup(|s| s.model = Some(next), cx));
                })
                .checked(checked)
                .into()
            };
            let names = ModelName::ALL
                .map(|name| pick(name.as_str(), ReviewModel::new(name, model.effort), name == model.name))
                .into();
            let efforts = Effort::ALL
                .map(|effort| {
                    pick(
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
        .inline()
        .anchor(Corner::BottomLeft)
        .into_any_element()
    }
}

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

impl Render for ReviewPopover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let session = self.session.read(cx);
        let problem = session.review_problem(cx);
        let done = session.review_run(cx).is_some_and(|run| run.state == RunState::Done);
        let busy = session.activity().read(cx).is_busy();
        let setup = session.setup().clone();
        let model = session.model();
        let files = session.file_count();
        let visible = session.host().visible_path.clone();
        let title = session.title();
        let start_title = if busy {
            "Queue Review"
        } else if problem.is_some() {
            "Try Again"
        } else if done {
            "Review Again"
        } else {
            "Start Review"
        };
        let open_name = visible
            .as_deref()
            .map(|path| path.rsplit('/').next().unwrap_or(path).to_string())
            .unwrap_or_default();
        let scope_index = match setup.scope {
            ReviewScope::All => 0,
            ReviewScope::OpenFile => 1,
        };
        let plural = if files == 1 { "file" } else { "files" };
        let section = |text: &'static str| {
            div()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ui.text)
                .child(text)
        };
        v_flex()
            .w(px(WIDTH - 24.))
            .p(px(4.))
            .gap(px(0.))
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
            .text_color(theme.ui.text)
            .child(
                h_flex()
                    .gap_2()
                    .mb(px(12.))
                    .items_center()
                    .child(Icon::new(Glyph::Sparkles).size(px(13.)).text_color(theme.ui.claude))
                    .child(div().font_weight(FontWeight::BOLD).child("Ask Claude"))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                            .text_color(theme.ui.muted)
                            .child("Claude Code"),
                    ),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .mb(px(14.))
                    .items_center()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .text_color(theme.ui.muted)
                    .child(Icon::new(Glyph::GitBranch).size_3p5())
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(title),
                    ),
            )
            .when_some(problem.map(SharedString::from), |col, message| {
                col.child(
                    tag(h_flex(), || format!("review:problem:{message}"))
                        .gap_2()
                        .mb(px(12.))
                        .items_start()
                        .text_color(theme.danger)
                        .child(Icon::new(Glyph::TriangleAlert).size_4().flex_none())
                        .child(div().min_w_0().child(message)),
                )
            })
            .child(section("What should Claude focus on?").mb(px(6.)))
            .child(
                div()
                    .mb(px(10.))
                    .relative()
                    .child(
                        Input::new(&self.instructions)
                            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY)),
                    )
                    .when(self.instructions.read(cx).value().is_empty(), |field| {
                        field.child(
                            div()
                                .absolute()
                                .top(px(9.))
                                .left(px(13.))
                                .right(px(13.))
                                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                                .text_color(theme.ui.muted)
                                .child(INSTRUCTIONS_HINT),
                        )
                    }),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_1p5()
                    .mb(px(16.))
                    .children(focus::PRESETS.map(|name| {
                        let on = setup.focuses.contains(name);
                        chip(name, on).on_click(cx.listener(move |this, _, _, cx| {
                            this.session.update(cx, |session, cx| {
                                session.update_setup(
                                    |s| {
                                        if !s.focuses.remove(name) {
                                            s.focuses.insert(name.to_string());
                                        }
                                    },
                                    cx,
                                )
                            });
                        }))
                    })),
            )
            .child(section("Scope").mb(px(6.)))
            .child(
                RadioGroup::vertical("review-scope")
                    .selected_index(Some(scope_index))
                    .disabled(visible.is_none())
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        let scope = if *index == 0 {
                            ReviewScope::All
                        } else {
                            ReviewScope::OpenFile
                        };
                        this.session
                            .update(cx, |session, cx| session.update_setup(|s| s.scope = scope, cx));
                    }))
                    .child(tag(
                        Radio::new("scope-all").label(format!("All changes ({files} {plural})")),
                        || "review:scope:all",
                    ))
                    .child(tag(
                        Radio::new("scope-open").label(format!("Only the open file  {open_name}")),
                        || "review:scope:open",
                    )),
            )
            .child(div().h(px(1.)).bg(theme.ui.line).my(px(12.)))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(self.model_menu(model))
                    .child(div().flex_1())
                    .child(
                        Button::new("cancel-review")
                            .label("Cancel")
                            .regular()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(PopoverEvent::Dismiss))),
                    )
                    .child(
                        tag(
                            Button::new("start-review")
                                .label(start_title)
                                .hint("⌘↩")
                                .regular()
                                .look(Look::Primary),
                            || "review:start",
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
                    ),
            )
    }
}
