use super::{accounts, claude_tab, general};
use diffpilot_core::connection::{self, ClaudeStatus, McpState};
use diffpilot_core::editor::{self, Editor};
use diffpilot_core::github::auth::{Credentials, TokenSource};
use diffpilot_core::github::client::AuthStatus;
use diffpilot_core::github::http::{Http, UreqHttp};
use diffpilot_core::github::{Github, GithubError};
use diffpilot_core::settings::Settings;
use diffpilot_ui_kit::components::{glyph_icon, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, IntoElement, PathPromptOptions, Render, Subscription, Window, WindowControlArea, div, px,
};
use gpui_component::ActiveTheme;
use gpui_component::input::{InputEvent, InputState};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Appearance,
    Diff,
    Claude,
    Accounts,
    KeyBindings,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::General,
        Tab::Appearance,
        Tab::Diff,
        Tab::Claude,
        Tab::Accounts,
        Tab::KeyBindings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Diff => "Diff",
            Self::Claude => "Claude",
            Self::Accounts => "Accounts",
            Self::KeyBindings => "Key Bindings",
        }
    }

    fn icon(self) -> Glyph {
        match self {
            Self::General => Glyph::Sun,
            Self::Appearance => Glyph::Contrast,
            Self::Diff => Glyph::Columns2,
            Self::Claude => Glyph::Sparkles,
            Self::Accounts => Glyph::Users,
            Self::KeyBindings => Glyph::Keyboard,
        }
    }

    /// The value of `DIFFPILOT_SETTINGS`. Any other non-empty text opens General.
    pub fn parse(text: &str) -> Option<Tab> {
        let text = text.trim().to_lowercase().replace([' ', '-', '_'], "");
        if text.is_empty() {
            return None;
        }
        Some(
            Self::ALL
                .into_iter()
                .find(|tab| tab.title().to_lowercase().replace(' ', "") == text)
                .unwrap_or(Tab::General),
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

/// What the window talks to outside the settings file. Tests pass fakes.
#[derive(Clone)]
pub struct Services {
    pub credentials: Credentials,
    pub http: Arc<dyn Http>,
    pub host: String,
    pub mcp_server: Option<PathBuf>,
}

impl Services {
    pub fn system() -> Self {
        Self {
            credentials: Credentials::system(),
            http: Arc::new(UreqHttp::new()),
            host: "github.com".into(),
            mcp_server: None,
        }
    }

    fn mcp_server(&self) -> Option<PathBuf> {
        self.mcp_server.clone().or_else(connection::mcp_server_binary)
    }
}

pub struct SettingsWindow {
    pub(super) settings: Arc<Settings>,
    services: Services,
    pub(super) tab: Tab,
    pub(super) claude: Load<ClaudeStatus>,
    pub(super) mcp: Load<McpState>,
    pub(super) path_op: Op,
    pub(super) install_op: Op,
    pub(super) github: Load<AuthStatus>,
    pub(super) github_op: Op,
    pub(super) token: Entity<InputState>,
    pub(super) custom_editor: Entity<InputState>,
    pub(super) detected_editor: Editor,
    claude_epoch: u64,
    _subscriptions: Vec<Subscription>,
}

impl SettingsWindow {
    pub fn new(
        settings: Arc<Settings>,
        services: Services,
        tab: Tab,
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
        let subscriptions = vec![
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
            tab,
            claude: Load::Idle,
            mcp: Load::Idle,
            path_op: Op::Idle,
            install_op: Op::Idle,
            github: Load::Idle,
            github_op: Op::Idle,
            token,
            custom_editor,
            detected_editor: editor::detect(),
            claude_epoch: 0,
            _subscriptions: subscriptions,
        };
        cx.observe_global::<super::SettingsChanged>(|_, cx| cx.notify())
            .detach();
        window.set_window_title(tab.title());
        this.refresh_claude(cx);
        this.load_for_tab(cx);
        this
    }

    pub fn select_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        window.set_window_title(tab.title());
        self.load_for_tab(cx);
        cx.notify();
    }

    fn load_for_tab(&mut self, cx: &mut Context<Self>) {
        if self.tab == Tab::Accounts && self.github == Load::Idle {
            self.refresh_github(cx);
        }
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

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::of(cx);
        let body = match self.tab {
            Tab::General => general::general(self, &t, cx).into_any_element(),
            Tab::Appearance => general::appearance(self, &t, cx).into_any_element(),
            Tab::Diff => general::diff(self, &t, cx).into_any_element(),
            Tab::Claude => claude_tab::render(self, &t, cx).into_any_element(),
            Tab::Accounts => accounts::render(self, &t, cx).into_any_element(),
            Tab::KeyBindings => general::key_bindings(&t, cx).into_any_element(),
        };
        div()
            .id("settings")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(t.chrome.panel)
            .text_color(t.ui.text)
            .font_family(cx.theme().font_family.clone())
            .text_size(px(13.))
            .child(self.toolbar(&t, cx))
            .child(
                div().id("settings-body").flex_1().min_h_0().overflow_y_scroll().child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .px(px(28.))
                        .pt(px(18.))
                        .pb(px(28.))
                        .child(body),
                ),
            )
    }
}

impl SettingsWindow {
    fn toolbar(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = Tab::ALL.into_iter().map(|tab| {
            let selected = tab == self.tab;
            let color = if selected { t.accent_text } else { t.ui.text };
            let (press, cap) = (t.pressed(t.background), t.ui.cap);
            tag(
                div()
                    .id(("settings-tab", tab as usize))
                    .flex()
                    .flex_col()
                    .items_center()
                    .flex_none()
                    .w(px(76.))
                    .h(px(52.))
                    .pt(px(8.))
                    .gap(px(8.))
                    .rounded(px(8.))
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(color)
                    .cursor_pointer()
                    .when(selected, |this| this.bg(press))
                    .when(!selected, |this| this.hover(move |style| style.bg(cap)))
                    .on_click(cx.listener(move |this, _, window, cx| this.select_tab(tab, window, cx)))
                    .child(glyph_icon(tab.icon(), 16., color))
                    .child(tab.title()),
                format!("tab:{}", tab.title()),
            )
        });
        div()
            .flex()
            .flex_col()
            .flex_none()
            .h(px(92.))
            .bg(t.chrome.glass)
            .border_b_1()
            .border_color(t.ui.line)
            .window_control_area(WindowControlArea::Drag)
            .child(
                div()
                    .h(px(35.))
                    .pt(px(8.))
                    .flex()
                    .justify_center()
                    .text_size(px(13.))
                    .line_height(px(24.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(t.ui.text)
                    .child(self.tab.title()),
            )
            .child(div().flex().justify_center().gap(px(4.)).children(tabs))
    }
}

pub fn open_window_options(cx: &App) -> gpui::WindowOptions {
    let bounds = gpui::Bounds::centered(None, gpui::size(px(780.), px(800.)), cx);
    gpui::WindowOptions {
        window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Settings".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(14.), px(14.))),
        }),
        window_min_size: Some(gpui::size(px(780.), px(480.))),
        ..Default::default()
    }
}
