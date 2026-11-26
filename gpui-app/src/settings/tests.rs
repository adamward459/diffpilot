use super::window::{Load, Op};
use super::*;
use diffpilot_core::connection::{ClaudeStatus, Login, McpState};
use diffpilot_core::diff::DiffMode;
use diffpilot_core::github::auth::{Credentials, GhCli, MemoryStore, TokenSource, TokenStore};
use diffpilot_core::github::http::{Http, HttpError, Request, Response};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::{Effort, ModelName};
use diffpilot_core::settings::{AppearanceChoice, appearance_override};
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px, size};
use gpui_component::ActiveTheme;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const LOGGED_IN: &str = r#"{"loggedIn": true, "authMethod": "claude.ai"}"#;
const LOGGED_OUT: &str = r#"{"loggedIn": false, "authMethod": "none"}"#;
const MCP_SERVER: &str = "/bin/diffpilot-mcp";

fn fake_claude(dir: &Path, name: &str, auth: &str) -> PathBuf {
    std::fs::write(dir.join(format!("{name}.auth")), auth).unwrap();
    let script = format!(
        r#"#!/bin/sh
echo "$@" >> '{dir}/{name}.calls'
case "$1" in
  --version) echo '2.1.0 (Claude Code)' ;;
  auth) cat '{dir}/{name}.auth'; grep -q true '{dir}/{name}.auth' || exit 1 ;;
  mcp)
    case "$2" in
      get) if [ -f '{dir}/{name}.installed' ]; then echo '  Status: ✓ Connected'; else echo 'No MCP server named "diffpilot".'; exit 1; fi ;;
      add) touch '{dir}/{name}.installed' ;;
      remove) rm -f '{dir}/{name}.installed' ;;
    esac ;;
esac
"#,
        dir = dir.display()
    );
    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn calls(claude: &Path) -> Vec<String> {
    let file = format!("{}.calls", claude.display());
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

struct FakeGh(Option<&'static str>);

impl GhCli for FakeGh {
    fn token(&self, _host: &str) -> Option<String> {
        self.0.map(String::from)
    }
}

#[derive(Default)]
struct FakeHttp {
    requests: Mutex<Vec<String>>,
}

impl Http for FakeHttp {
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        self.requests.lock().unwrap().push(request.url.clone());
        let good = request.headers.iter().any(|(_, value)| value.contains("good"));
        Ok(if good {
            Response {
                status: 200,
                body: r#"{"login": "octocat"}"#.into(),
            }
        } else {
            Response {
                status: 401,
                body: r#"{"message": "Bad credentials"}"#.into(),
            }
        })
    }
}

struct Rig {
    dir: tempfile::TempDir,
    claude: PathBuf,
    settings: Arc<Settings>,
    store: Arc<MemoryStore>,
    root: Entity<Root>,
    view: Entity<SettingsWindow>,
    handle: WindowHandle<Root>,
}

struct Options {
    tab: Tab,
    auth: &'static str,
    gh: Option<&'static str>,
    claude_missing: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            tab: Tab::Claude,
            auth: LOGGED_IN,
            gh: None,
            claude_missing: false,
        }
    }
}

fn rig(cx: &mut TestAppContext, options: Options) -> (Rig, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let claude = fake_claude(dir.path(), "claude", options.auth);
    let settings = Arc::new(Settings::at(dir.path().join("settings.json")));
    settings.set_claude_path(Some(&if options.claude_missing {
        dir.path().join("missing")
    } else {
        claude.clone()
    }));
    let store = Arc::new(MemoryStore::default());
    let services = Services {
        credentials: Credentials::new(store.clone(), Arc::new(FakeGh(options.gh))),
        http: Arc::new(FakeHttp::default()),
        host: "github.com".into(),
        mcp_server: Some(PathBuf::from(MCP_SERVER)),
    };
    let handle = cx.update(|cx| {
        gpui_component::init(cx);
        init(settings.clone(), services, cx);
        open(Some(options.tab), cx).expect("the window opens")
    });
    let root = handle.root(cx).unwrap();
    let view = cx.update(|cx| cx.global::<Registry>().open.clone().unwrap().1);
    let vcx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    let rig = Rig {
        dir,
        claude,
        settings,
        store,
        root,
        view,
        handle,
    };
    rig.settle(vcx);
    (rig, vcx)
}

impl Rig {
    fn settle(&self, cx: &mut VisualTestContext) {
        let root = self.root.clone();
        cx.run_until_parked();
        cx.draw(point(px(0.), px(0.)), size(px(780.), px(800.)), move |_, _| root);
        cx.run_until_parked();
    }

    fn click(&self, cx: &mut VisualTestContext, selector: &'static str) {
        self.settle(cx);
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("no element tagged {selector}"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        self.settle(cx);
    }

    fn state<T>(&self, cx: &mut VisualTestContext, read: impl FnOnce(&SettingsWindow) -> T) -> T {
        self.view.read_with(cx, |view, _| read(view))
    }
}

#[gpui::test]
async fn the_claude_tab_finds_the_cli_reads_the_login_and_the_mcp_install(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    let (claude, mcp) = rig.state(cx, |view| (view.claude.clone(), view.mcp.clone()));
    assert_eq!(
        claude,
        Load::Ready(ClaudeStatus::Found {
            path: rig.claude.clone(),
            login: Login::LoggedIn
        })
    );
    assert_eq!(mcp, Load::Ready(McpState::NotInstalled));
    assert!(calls(&rig.claude).contains(&"auth status --json".to_string()));
    assert!(calls(&rig.claude).contains(&"mcp get diffpilot".to_string()));
}

#[gpui::test]
async fn a_logged_out_cli_and_a_missing_cli_each_show_their_state(cx: &mut TestAppContext) {
    let (rig, cx) = rig(
        cx,
        Options {
            auth: LOGGED_OUT,
            ..Options::default()
        },
    );
    let claude = rig.state(cx, |view| view.claude.clone());
    assert!(matches!(
        claude,
        Load::Ready(ClaudeStatus::Found {
            login: Login::LoggedOut,
            ..
        })
    ));
    std::fs::write(format!("{}.auth", rig.claude.display()), LOGGED_IN).unwrap();
    rig.click(cx, "claude:recheck");
    assert!(matches!(
        rig.state(cx, |view| view.claude.clone()),
        Load::Ready(ClaudeStatus::Found {
            login: Login::LoggedIn,
            ..
        })
    ));
}

#[gpui::test]
async fn without_the_cli_the_tab_says_so_and_never_runs_mcp_commands(cx: &mut TestAppContext) {
    let (rig, cx) = rig(
        cx,
        Options {
            claude_missing: true,
            ..Options::default()
        },
    );
    assert_eq!(
        rig.state(cx, |view| view.claude.clone()),
        Load::Ready(ClaudeStatus::NotFound)
    );
    assert_eq!(rig.state(cx, |view| view.mcp.clone()), Load::Idle);
    assert!(calls(&rig.claude).is_empty());
}

#[gpui::test]
async fn clicking_a_tab_switches_the_page(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    for tab in Tab::ALL {
        let selector: &'static str = Box::leak(format!("tab:{}", tab.title()).into_boxed_str());
        rig.click(cx, selector);
        assert_eq!(rig.state(cx, |view| view.tab), tab);
    }
}

#[gpui::test]
async fn a_model_picker_changes_the_name_and_the_effort_in_settings(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    assert_eq!(rig.settings.reviews_model().title(), "Opus · High");
    rig.click(cx, "model-reviews");
    rig.click(cx, "model-reviews:Sonnet");
    assert_eq!(rig.settings.reviews_model().title(), "Sonnet · High");
    rig.click(cx, "model-reviews:Haiku");
    assert_eq!(
        rig.settings.reviews_model().name,
        ModelName::Sonnet,
        "picking closes the menu"
    );
    rig.click(cx, "model-reviews");
    rig.click(cx, "model-reviews:Low");
    assert_eq!(
        (rig.settings.reviews_model().name, rig.settings.reviews_model().effort),
        (ModelName::Sonnet, Effort::Low)
    );
    assert_eq!(rig.settings.fixes_model().title(), "Sonnet · Medium");

    rig.click(cx, "model-fixes");
    rig.click(cx, "model-fixes:Haiku");
    assert_eq!(rig.settings.fixes_model().name, ModelName::Haiku);
    assert_eq!(rig.settings.reviews_model().name, ModelName::Sonnet);
}

#[gpui::test]
async fn the_edit_permission_picker_writes_the_setting(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    assert_eq!(rig.settings.edit_permission(), EditPermission::DontAsk);
    rig.click(cx, "edit-permission");
    rig.click(cx, "edit-permission:Ask for every edit");
    assert_eq!(rig.settings.edit_permission(), EditPermission::AskEach);
    let reopened = Settings::at(rig.dir.path().join("settings.json"));
    assert_eq!(reopened.edit_permission(), EditPermission::AskEach);
}

#[gpui::test]
async fn clicking_outside_an_open_menu_closes_it_without_changing_anything(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    rig.click(cx, "model-reviews");
    cx.simulate_click(point(px(5.), px(780.)), Modifiers::default());
    rig.settle(cx);
    rig.click(cx, "model-reviews:Sonnet");
    assert_eq!(rig.settings.reviews_model().title(), "Opus · High");
}

#[gpui::test]
async fn install_runs_claude_mcp_add_and_reinstall_replaces_it(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    rig.click(cx, "mcp:install");
    assert_eq!(rig.state(cx, |view| view.mcp.clone()), Load::Ready(McpState::Connected));
    assert_eq!(rig.state(cx, |view| view.install_op.clone()), Op::Idle);
    let add = format!("mcp add --scope user diffpilot -- {MCP_SERVER} --terminal");
    assert_eq!(calls(&rig.claude).iter().filter(|call| **call == add).count(), 1);

    rig.click(cx, "mcp:install");
    let all = calls(&rig.claude);
    assert_eq!(all.iter().filter(|call| **call == add).count(), 2);
    assert!(all.contains(&"mcp remove --scope user diffpilot".to_string()));
}

#[gpui::test]
async fn a_failing_install_shows_its_message_inline(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    let script = std::fs::read_to_string(&rig.claude)
        .unwrap()
        .replace("add) touch", "add) echo 'cannot write config' >&2; exit 1; touch");
    std::fs::write(&rig.claude, script).unwrap();
    rig.click(cx, "mcp:install");
    assert_eq!(
        rig.state(cx, |view| view.install_op.clone()),
        Op::Failed("cannot write config".into())
    );
    assert_eq!(
        rig.state(cx, |view| view.mcp.clone()),
        Load::Ready(McpState::NotInstalled)
    );
}

#[gpui::test]
async fn the_terminal_switch_is_saved_and_blocks_install(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    assert!(rig.settings.allow_terminal_agents());
    rig.click(cx, "allow-terminal");
    assert!(!rig.settings.allow_terminal_agents());
    rig.click(cx, "mcp:install");
    assert!(!calls(&rig.claude).iter().any(|call| call.starts_with("mcp add")));
    rig.click(cx, "allow-terminal");
    assert!(rig.settings.allow_terminal_agents());
}

#[gpui::test]
async fn a_chosen_path_is_validated_by_running_it_before_it_is_saved(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    let other = fake_claude(rig.dir.path(), "claude-other", LOGGED_OUT);
    let not_claude = rig.dir.path().join("not-claude");
    std::fs::write(&not_claude, "#!/bin/sh\necho hello\n").unwrap();
    std::fs::set_permissions(&not_claude, std::fs::Permissions::from_mode(0o755)).unwrap();

    rig.settle(cx);
    let check = |rig: &Rig, cx: &mut VisualTestContext, path: &Path| {
        let path = path.to_path_buf();
        rig.view.update(cx, |view, cx| view.check_claude_path(path, cx));
        rig.settle(cx);
    };
    check(&rig, cx, &not_claude);
    assert!(rig.state(cx, |view| view.path_op.failure().is_some()));
    assert_eq!(rig.settings.claude_path(), Some(rig.claude.clone()));

    check(&rig, cx, &other);
    assert_eq!(rig.settings.claude_path(), Some(other.clone()));
    assert_eq!(rig.state(cx, |view| view.path_op.clone()), Op::Idle);
    assert!(matches!(
        rig.state(cx, |view| view.claude.clone()),
        Load::Ready(ClaudeStatus::Found {
            login: Login::LoggedOut,
            ..
        })
    ));

    rig.click(cx, "claude:default");
    assert_eq!(rig.settings.claude_path(), None);
}

#[gpui::test]
async fn the_appearance_choice_is_saved_and_applied_to_the_theme(cx: &mut TestAppContext) {
    let (rig, cx) = rig(
        cx,
        Options {
            tab: Tab::Appearance,
            ..Options::default()
        },
    );
    if appearance_override().is_some() {
        return;
    }
    rig.click(cx, "appearance:Dark");
    assert_eq!(rig.settings.appearance(), AppearanceChoice::Dark);
    assert!(cx.update(|_, cx| cx.theme().is_dark()));
    rig.click(cx, "appearance:Light");
    assert_eq!(rig.settings.appearance(), AppearanceChoice::Light);
    assert!(!cx.update(|_, cx| cx.theme().is_dark()));
}

#[gpui::test]
async fn the_general_and_diff_switches_write_their_settings(cx: &mut TestAppContext) {
    let (rig, cx) = rig(
        cx,
        Options {
            tab: Tab::General,
            ..Options::default()
        },
    );
    assert!(rig.settings.open_last_repository());
    rig.click(cx, "open-last");
    assert!(!rig.settings.open_last_repository());

    rig.click(cx, "tab:Diff");
    assert!(!rig.settings.wrap_lines());
    rig.click(cx, "wrap-lines");
    assert!(rig.settings.wrap_lines());

    assert_eq!(rig.settings.diff_mode(), DiffMode::Unified);
    rig.click(cx, "layout:Split");
    assert_eq!(rig.settings.diff_mode(), DiffMode::Split);
    rig.click(cx, "layout:Unified");
    assert_eq!(rig.settings.diff_mode(), DiffMode::Unified);
    let reopened = Settings::at(rig.dir.path().join("settings.json"));
    assert!(reopened.wrap_lines() && !reopened.open_last_repository());
}

#[gpui::test]
async fn opening_twice_reuses_the_window_and_selects_the_tab(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    let second = cx.update(|_, cx| open(Some(Tab::Accounts), cx)).unwrap();
    assert_eq!(second.window_id(), rig.handle.window_id());
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 1);
    rig.settle(cx);
    assert_eq!(rig.state(cx, |view| view.tab), Tab::Accounts);
}

#[gpui::test]
async fn accounts_reads_the_github_state_and_imports_and_signs_out(cx: &mut TestAppContext) {
    let (rig, cx) = rig(
        cx,
        Options {
            tab: Tab::Accounts,
            gh: Some("gho_good"),
            ..Options::default()
        },
    );
    let status = |rig: &Rig, cx: &mut VisualTestContext| match rig.state(cx, |view| view.github.clone()) {
        Load::Ready(status) => (status.login, status.source),
        other => panic!("{other:?}"),
    };
    assert_eq!(status(&rig, cx), (Some("octocat".into()), Some(TokenSource::Gh)));
    assert_eq!(rig.store.read("github.com").unwrap(), None);

    rig.click(cx, "github:import");
    assert_eq!(rig.store.read("github.com").unwrap().as_deref(), Some("gho_good"));
    assert_eq!(status(&rig, cx), (Some("octocat".into()), Some(TokenSource::Keychain)));

    rig.click(cx, "github:sign-out");
    assert_eq!(rig.store.read("github.com").unwrap(), None);
    assert_eq!(status(&rig, cx).1, Some(TokenSource::Gh));
}

#[gpui::test]
async fn a_pasted_token_is_checked_with_github_before_it_is_saved(cx: &mut TestAppContext) {
    let (rig, cx) = rig(
        cx,
        Options {
            tab: Tab::Accounts,
            ..Options::default()
        },
    );
    let paste = |rig: &Rig, cx: &mut VisualTestContext, text: &'static str| {
        let view = rig.view.clone();
        cx.update(|window, cx| {
            view.read(cx)
                .token
                .clone()
                .update(cx, |input, cx| input.set_value(text, window, cx))
        });
    };
    paste(&rig, cx, "ghp_bad");
    rig.click(cx, "github:save");
    assert_eq!(rig.store.read("github.com").unwrap(), None);
    assert_eq!(
        rig.state(cx, |view| view.github_op.failure().map(String::from)),
        Some("GitHub rejected the token".into())
    );

    paste(&rig, cx, " ghp_good ");
    rig.click(cx, "github:save");
    assert_eq!(rig.store.read("github.com").unwrap().as_deref(), Some("ghp_good"));
    assert_eq!(rig.state(cx, |view| view.github_op.clone()), Op::Idle);
    assert!(matches!(
        rig.state(cx, |view| view.github.clone()),
        Load::Ready(status) if status.login.as_deref() == Some("octocat") && status.source == Some(TokenSource::Keychain)
    ));
}

#[gpui::test]
async fn key_bindings_lists_the_shortcuts_the_app_registers(cx: &mut TestAppContext) {
    let (_rig, cx) = rig(
        cx,
        Options {
            tab: Tab::KeyBindings,
            ..Options::default()
        },
    );
    cx.update(|_, cx| crate::workspace::init(cx));
    let list = cx.update(|_, cx| general::bindings(cx));
    let has = |title: &str, keys: &str| list.iter().any(|(t, k)| t.as_ref() == title && k == keys);
    assert!(has("Open Folder…", "⌘O"), "{list:?}");
    assert!(has("Settings…", "⌘,"), "{list:?}");
    assert!(has("Toggle Sidebar", "⌃⌘S"), "{list:?}");
    assert!(list.iter().all(|(title, keys)| !title.is_empty() && !keys.is_empty()));
}

#[test]
fn the_environment_flag_names_a_tab() {
    assert_eq!(Tab::parse("claude"), Some(Tab::Claude));
    assert_eq!(Tab::parse("Key Bindings"), Some(Tab::KeyBindings));
    assert_eq!(Tab::parse("key-bindings"), Some(Tab::KeyBindings));
    assert_eq!(Tab::parse("1"), Some(Tab::General));
    assert_eq!(Tab::parse(" "), None);
}
