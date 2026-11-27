use super::data_tab::Project;
use super::entries::{self, ENTRIES};
use super::{CloseSettings, KEY_CONTEXT, general};
use diffpilot_core::connection::{self, ClaudeStatus, McpState};
use diffpilot_core::editor::{self, Editor};
use diffpilot_core::github::auth::{Credentials, TokenSource};
use diffpilot_core::github::client::AuthStatus;
use diffpilot_core::github::http::{Http, UreqHttp};
use diffpilot_core::github::{Github, GithubError};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::{DataScope, Store};
use diffpilot_ui_kit::components::{filter_field, glyph_icon, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, IntoElement, PathPromptOptions, Render, ScrollHandle, Subscription, Window, div,
    point, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme, Sizable};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Appearance,
    Diff,
    Claude,
    Accounts,
    KeyBindings,
    Data,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::General,
        Section::Appearance,
        Section::Diff,
        Section::Claude,
        Section::Accounts,
        Section::KeyBindings,
        Section::Data,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Diff => "Diff",
            Self::Claude => "Claude",
            Self::Accounts => "Accounts",
            Self::KeyBindings => "Key Bindings",
            Self::Data => "Data",
        }
    }

    pub fn icon(self) -> Glyph {
        match self {
            Self::General => Glyph::Sun,
            Self::Appearance => Glyph::Contrast,
            Self::Diff => Glyph::Columns2,
            Self::Claude => Glyph::Sparkles,
            Self::Accounts => Glyph::Users,
            Self::KeyBindings => Glyph::Keyboard,
            Self::Data => Glyph::Database,
        }
    }

    /// The value of `DIFFPILOT_SETTINGS`. Any other non-empty text opens General.
    pub fn parse(text: &str) -> Option<Section> {
        let text = text.trim().to_lowercase().replace([' ', '-', '_'], "");
        if text.is_empty() {
            return None;
        }
        Some(
            Self::ALL
                .into_iter()
                .find(|section| section.title().to_lowercase().replace(' ', "") == text)
                .unwrap_or(Section::General),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Op {
    #[default]
    Idle,
    Running,
    Failed(String),
}

impl Op {
    pub fn running(&self) -> bool {
        matches!(self, Self::Running)
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(message) => Some(message),
            _ => None,
        }
    }
}

/// What the page talks to outside the settings file. Tests pass fakes.
#[derive(Clone)]
pub struct Services {
    pub credentials: Credentials,
    pub http: Arc<dyn Http>,
    pub host: String,
    pub mcp_server: Option<PathBuf>,
    pub store: Arc<Store>,
}

impl Services {
    pub fn system(store: Arc<Store>) -> Self {
        Self {
            credentials: Credentials::system(),
            http: Arc::new(UreqHttp::new()),
            host: "github.com".into(),
            mcp_server: None,
            store,
        }
    }

    fn mcp_server(&self) -> Option<PathBuf> {
        self.mcp_server.clone().or_else(connection::mcp_server_binary)
    }
}

pub struct SettingsPage {
    pub(super) settings: Arc<Settings>,
    pub(super) services: Services,
    pub(super) search: Entity<InputState>,
    pub(super) scroll: ScrollHandle,
    scroll_target: Option<Section>,
    pub(super) claude: Load<ClaudeStatus>,
    pub(super) mcp: Load<McpState>,
    pub(super) path_op: Op,
    pub(super) install_op: Op,
    pub(super) github: Load<AuthStatus>,
    pub(super) github_op: Op,
    pub(super) token: Entity<InputState>,
    pub(super) custom_editor: Entity<InputState>,
    pub(super) detected_editor: Editor,
    pub(super) data: Load<Vec<Project>>,
    pub(super) data_open: HashSet<String>,
    pub(super) data_confirm: Option<DataScope>,
    pub(super) data_op: Op,
    claude_epoch: u64,
    _subscriptions: Vec<Subscription>,
}

impl SettingsPage {
    pub fn new(
        settings: Arc<Settings>,
        services: Services,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("ghp_… or github_pat_…")
                .masked(true)
        });
        let saved_template = match settings.editor() {
            Some(Editor::Custom(template)) => template,
            _ => String::new(),
        };
        let custom_editor = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("code -g {file}:{line}:{column}");
            state.set_value(saved_template, window, cx);
            state
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        let subscriptions = vec![
            cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.scroll.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                }
            }),
            cx.subscribe(&custom_editor, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) && matches!(this.settings.editor(), Some(Editor::Custom(_))) {
                    let template = input.read(cx).value().to_string();
                    this.settings.set_editor(Some(Editor::Custom(template)));
                    cx.notify();
                }
            }),
            cx.subscribe_in(&token, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.save_token(window, cx);
                }
            }),
        ];
        let mut this = Self {
            settings,
            services,
            search,
            scroll: ScrollHandle::new(),
            scroll_target: Some(section),
            claude: Load::Idle,
            mcp: Load::Idle,
            path_op: Op::Idle,
            install_op: Op::Idle,
            github: Load::Idle,
            github_op: Op::Idle,
            token,
            custom_editor,
            detected_editor: editor::detect(),
            data: Load::Idle,
            data_open: HashSet::new(),
            data_confirm: None,
            data_op: Op::Idle,
            claude_epoch: 0,
            _subscriptions: subscriptions,
        };
        cx.observe_global::<super::SettingsChanged>(|_, cx| cx.notify())
            .detach();
        this.refresh_claude(cx);
        this.refresh_github(cx);
        this.refresh_data(cx);
        this.focus_search(window, cx);
        this
    }

    pub fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn scroll_to_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if let Some(ix) = self.visible_sections(cx).iter().position(|shown| *shown == section) {
            self.scroll.scroll_to_top_of_item(ix);
            cx.notify();
        }
    }

    pub fn query(&self, cx: &gpui::App) -> String {
        self.search.read(cx).value().trim().to_string()
    }

    pub fn visible_sections(&self, cx: &gpui::App) -> Vec<Section> {
        let query = self.query(cx);
        let shortcuts = general::bindings(cx);
        Section::ALL
            .into_iter()
            .filter(|section| {
                let listed = entries::in_section(*section, &query).next().is_some();
                let shortcut = *section == Section::KeyBindings
                    && shortcuts
                        .iter()
                        .any(|(title, _)| entries::matches(&query, &[section.title(), title]));
                listed || shortcut
            })
            .collect()
    }

    pub(super) fn refresh_claude(&mut self, cx: &mut Context<Self>) {
        self.claude_epoch += 1;
        let epoch = self.claude_epoch;
        self.claude = Load::Loading;
        self.mcp = Load::Idle;
        let custom = self.settings.claude_path();
        cx.spawn(async move |this, cx| {
            let status = cx
                .background_spawn(async move { connection::status(custom.as_deref()) })
                .await;
            this.update(cx, |this, cx| {
                if this.claude_epoch != epoch {
                    return;
                }
                let found = status.path().map(ToOwned::to_owned);
                this.claude = Load::Ready(status);
                if let Some(path) = found {
                    this.refresh_mcp(path, epoch, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn refresh_mcp(&mut self, binary: PathBuf, epoch: u64, cx: &mut Context<Self>) {
        self.mcp = Load::Loading;
        cx.spawn(async move |this, cx| {
            let state = cx.background_spawn(async move { connection::mcp_state(&binary) }).await;
            this.update(cx, |this, cx| {
                if this.claude_epoch == epoch {
                    this.mcp = Load::Ready(state);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn choose_claude_path(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            this.update(cx, |this, cx| this.check_claude_path(path, cx)).ok();
        })
        .detach();
    }

    pub(super) fn check_claude_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.path_op = Op::Running;
        cx.notify();
        let checked = path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { connection::validate_binary(&checked) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(_) => {
                    this.settings.set_claude_path(Some(&path));
                    this.path_op = Op::Idle;
                    this.refresh_claude(cx);
                }
                Err(message) => {
                    this.path_op = Op::Failed(message);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn use_default_claude_path(&mut self, cx: &mut Context<Self>) {
        self.settings.set_claude_path(None);
        self.path_op = Op::Idle;
        self.refresh_claude(cx);
    }

    pub(super) fn install_mcp(&mut self, cx: &mut Context<Self>) {
        let Some(binary) = self.claude_found() else { return };
        let Some(server) = self.services.mcp_server() else {
            self.install_op = Op::Failed("diffpilot-mcp was not found next to the app.".into());
            cx.notify();
            return;
        };
        self.install_op = Op::Running;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { connection::install_mcp(&binary, &server) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(state) => {
                        this.mcp = Load::Ready(state);
                        this.install_op = Op::Idle;
                    }
                    Err(message) => this.install_op = Op::Failed(message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn claude_found(&self) -> Option<PathBuf> {
        match &self.claude {
            Load::Ready(status) => status.path().map(ToOwned::to_owned),
            _ => None,
        }
    }

    pub(super) fn set_allow_terminal_agents(&mut self, allow: bool, cx: &mut Context<Self>) {
        self.settings.set_allow_terminal_agents(allow);
        cx.notify();
    }

    pub(super) fn refresh_github(&mut self, cx: &mut Context<Self>) {
        self.github = Load::Loading;
        let services = self.services.clone();
        cx.spawn(async move |this, cx| {
            let status = cx
                .background_spawn(async move {
                    Github::auth_status(services.http.clone(), &services.credentials, &services.host)
                })
                .await;
            this.update(cx, |this, cx| {
                this.github = Load::Ready(status);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn import_from_gh(&mut self, cx: &mut Context<Self>) {
        let services = self.services.clone();
        self.run_github(cx, move || {
            services.credentials.import_from_gh(&services.host).map(|_| ())
        });
    }

    pub(super) fn sign_out(&mut self, cx: &mut Context<Self>) {
        let services = self.services.clone();
        self.run_github(cx, move || services.credentials.sign_out(&services.host));
    }

    pub(super) fn save_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.token.read(cx).value().trim().to_string();
        if text.is_empty() || self.github_op.running() {
            return;
        }
        let services = self.services.clone();
        self.token.update(cx, |input, cx| input.set_value("", window, cx));
        self.run_github(cx, move || {
            Github::new(services.http.clone(), &services.host, &text).viewer_login()?;
            services.credentials.save(&services.host, &text)
        });
    }

    fn run_github(&mut self, cx: &mut Context<Self>, work: impl FnOnce() -> Result<(), GithubError> + Send + 'static) {
        self.github_op = Op::Running;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { work() }).await;
            this.update(cx, |this, cx| {
                this.github_op = match result {
                    Ok(()) => Op::Idle,
                    Err(error) => Op::Failed(error.to_string()),
                };
                this.refresh_github(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn github_source_text(source: Option<TokenSource>) -> &'static str {
        match source {
            Some(TokenSource::Keychain) => "token saved in your keychain",
            Some(TokenSource::Gh) => "token from the gh CLI",
            None => "",
        }
    }
}

pub enum PageEvent {
    Close,
    DataDeleted(DataScope),
}

impl EventEmitter<PageEvent> for SettingsPage {}

const TOC_WIDTH: f32 = 176.;

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // gpui scrolls to an item using the scroll area's bounds from the last frame, which are empty
        // before the first one; scrolling then overshoots by the height of the header.
        if let Some(section) = self.scroll_target {
            if self.scroll.bounds().size.height > px(0.) {
                self.scroll_target = None;
                self.scroll_to_section(section, cx);
            } else {
                window.request_animation_frame();
            }
        }
        let t = Theme::of(cx);
        let query = self.query(cx);
        let sections = self.visible_sections(cx);
        let current = sections.get(self.scroll.top_item()).copied();
        let viewport = self.scroll.bounds().size.height;
        let last = sections.len().saturating_sub(1);
        let shortcuts = general::bindings(cx);
        let body = sections
            .iter()
            .enumerate()
            .map(|(ix, section)| {
                let content = self.section_content(*section, &query, &shortcuts, &t, cx);
                tag(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .pb(px(32.))
                        .when(ix == last, |this| this.min_h(viewport))
                        .child(
                            div()
                                .text_size(px(20.))
                                .line_height(px(28.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(section.title()),
                        )
                        .children(content),
                    || format!("section:{}", section.title()),
                )
            })
            .collect::<Vec<_>>();
        let toc = sections
            .iter()
            .enumerate()
            .map(|(ix, section)| {
                let section = *section;
                let selected = current == Some(section);
                let color = if selected { t.accent_text } else { t.ui.text };
                let (press, cap) = (t.pressed(t.background), t.ui.cap);
                tag(
                    div()
                        .id(("settings-toc", section as usize))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .h(px(32.))
                        .px(px(10.))
                        .rounded(px(8.))
                        .text_color(color)
                        .cursor_pointer()
                        .when(selected, |this| this.bg(press))
                        .when(!selected, |this| this.hover(move |style| style.bg(cap)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.scroll.scroll_to_top_of_item(ix);
                            cx.notify();
                        }))
                        .child(glyph_icon(section.icon(), 16., color))
                        .child(section.title()),
                    move || format!("toc:{}", section.title()),
                )
            })
            .collect::<Vec<_>>();
        let header = self.header(&t, cx);
        let shown = sections
            .iter()
            .map(|section| section.title())
            .collect::<Vec<_>>()
            .join(",");
        div()
            .id("settings")
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(|_, _: &CloseSettings, _, cx| cx.emit(PageEvent::Close)))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .text_color(t.ui.text)
            .font_family(cx.theme().font_family.clone())
            .text_size(px(13.))
            .child(header)
            .child(
                div().flex_none().flex().px(px(20.)).pb(px(12.)).child(tag(
                    filter_field(
                        &t,
                        Input::new(&self.search)
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .small(),
                    ),
                    || "settings:search",
                )),
            )
            .child(tag(div().size_0(), move || format!("settings:shows:{shown}")))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .w(px(TOC_WIDTH))
                            .pl(px(12.))
                            .pr(px(8.))
                            .pt(px(4.))
                            .children(toc)
                            .child(div().flex_1())
                            .child(tag(
                                div()
                                    .px(px(10.))
                                    .pb(px(16.))
                                    .text_size(px(12.))
                                    .text_color(t.ui.muted)
                                    .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                                || format!("settings:version:{}", env!("CARGO_PKG_VERSION")),
                            )),
                    )
                    .child(tag(
                        div()
                            .id("settings-body")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .pr(px(20.))
                            .pt(px(4.))
                            .children(body)
                            .when(sections.is_empty(), |this| {
                                this.child(div().text_color(t.ui.muted).child("No settings found"))
                            }),
                        || "settings:body",
                    )),
            )
    }
}

impl SettingsPage {
    fn section_content(
        &self,
        section: Section,
        query: &str,
        shortcuts: &[(gpui::SharedString, String)],
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        if section == Section::KeyBindings {
            let found: Vec<_> = shortcuts
                .iter()
                .filter(|(title, _)| entries::matches(query, &[section.title(), title]))
                .cloned()
                .collect();
            return vec![general::key_bindings(t, &found, query.is_empty(), cx).into_any_element()];
        }
        ENTRIES
            .iter()
            .filter(|entry| entry.section == section && entry.matches(query))
            .map(|entry| (entry.render)(self, t, cx))
            .collect()
    }

    fn header(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let cap = t.ui.cap;
        div()
            .flex()
            .items_center()
            .flex_none()
            .h(px(48.))
            .pl(px(20.))
            .pr(px(12.))
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .child(tag(
                div()
                    .id("settings-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(move |style| style.bg(cap))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(PageEvent::Close)))
                    .child(glyph_icon(Glyph::X, 14., t.ui.text2)),
                || "settings:close",
            ))
            .into_any_element()
    }
}
