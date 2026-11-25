use crate::assets;
use crate::palette::{Palette, accent_variant};
use crate::session::ReviewSession;
use diffpilot_core::review::{Effort, ModelName, ReviewModel, ReviewRun, ReviewScope, focus};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AppContext, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::radio::{Radio, RadioGroup};
use gpui_component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};

pub const INSTRUCTIONS_HINT: &str = "Optional. For example “Check error handling in the new API routes”.";
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
            let mut state = InputState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder(INSTRUCTIONS_HINT);
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

    fn chip(&self, name: &'static str, on: bool, palette: Palette, cx: &Context<Self>) -> Button {
        let style = ButtonCustomVariant::new(cx)
            .color(if on {
                palette.accent.opacity(0.14)
            } else {
                cx.theme().transparent
            })
            .foreground(if on { palette.accent } else { palette.text })
            .border(if on { palette.accent.opacity(0.6) } else { palette.line })
            .hover(palette.accent.opacity(0.2))
            .active(palette.accent.opacity(0.28));
        let button = Button::new(name).label(name).small().rounded(px(999.)).custom(style);
        if on {
            button.icon(Icon::empty().path(assets::CHECK).size(px(11.)))
        } else {
            button
        }
    }

    fn model_menu(&self, model: ReviewModel) -> AnyElement {
        let session = self.session.clone();
        Button::new("review-model")
            .icon(Icon::empty().path(assets::SPARKLES).size_3())
            .label(model.title())
            .small()
            .outline()
            .dropdown_menu(move |menu, _, _| {
                let pick = |model: ReviewModel| {
                    let session = session.clone();
                    move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                        session.update(cx, |session, cx| session.update_setup(|s| s.model = Some(model), cx));
                    }
                };
                let names = ModelName::ALL.map(|name| {
                    let next = ReviewModel::new(name, model.effort);
                    PopupMenuItem::new(title_case(name.as_str()))
                        .checked(name == model.name)
                        .on_click(pick(next))
                });
                let efforts = Effort::ALL.map(|effort| {
                    let next = ReviewModel::new(model.name, effort);
                    PopupMenuItem::new(title_case(effort.as_str()))
                        .checked(effort == model.effort)
                        .on_click(pick(next))
                });
                let menu = menu.label("Model");
                let menu = names.into_iter().fold(menu, |menu, item| menu.item(item));
                efforts
                    .into_iter()
                    .fold(menu.separator().label("Effort"), |menu, item| menu.item(item))
            })
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
        let palette = Palette::of(cx);
        let session = self.session.read(cx);
        let run = session.run().clone();
        let setup = session.setup().clone();
        let model = session.model();
        let files = session.file_count();
        let visible = session.host().visible_path.clone();
        let title = session.title();
        let start_title = match run {
            ReviewRun::Failed(_) => "Try Again",
            ReviewRun::Done { .. } => "Review Again",
            _ => "Start Review",
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
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .child(text)
        };
        v_flex()
            .w(px(WIDTH - 24.))
            .gap(px(0.))
            .text_size(px(13.))
            .text_color(palette.text)
            .child(
                h_flex()
                    .gap_2()
                    .mb(px(12.))
                    .items_center()
                    .child(
                        Icon::empty()
                            .path(assets::SPARKLES)
                            .size(px(15.))
                            .text_color(palette.claude),
                    )
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Ask Claude"),
                    )
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(palette.muted).child("Claude Code")),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .mb(px(14.))
                    .items_center()
                    .text_color(palette.muted)
                    .child(Icon::empty().path(assets::BRANCH).size_3p5())
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(title),
                    ),
            )
            .when_some(
                match &run {
                    ReviewRun::Failed(message) => Some(SharedString::from(message.clone())),
                    _ => None,
                },
                |col, message| {
                    col.child(
                        h_flex()
                            .gap_2()
                            .mb(px(12.))
                            .items_start()
                            .text_color(palette.red)
                            .child(Icon::empty().path(assets::WARNING).size_4().flex_none())
                            .child(div().min_w_0().child(message)),
                    )
                },
            )
            .child(section("What should Claude focus on?").mb(px(6.)))
            .child(div().mb(px(10.)).child(Input::new(&self.instructions)))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_1p5()
                    .mb(px(16.))
                    .children(focus::PRESETS.map(|name| {
                        let on = setup.focuses.contains(name);
                        self.chip(name, on, palette, cx)
                            .on_click(cx.listener(move |this, _, _, cx| {
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
                    .child(Radio::new("scope-all").label(format!("All changes ({files} {plural})")))
                    .child(Radio::new("scope-open").label(format!("Only the open file  {open_name}"))),
            )
            .child(div().h(px(1.)).bg(palette.line).my(px(12.)))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(self.model_menu(model))
                    .child(div().flex_1())
                    .child(
                        Button::new("cancel-review")
                            .label("Cancel")
                            .small()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(PopoverEvent::Dismiss))),
                    )
                    .child(
                        Button::new("start-review")
                            .label(format!("{start_title}  ⌘↩"))
                            .small()
                            .custom(accent_variant(cx))
                            .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
                    ),
            )
    }
}
