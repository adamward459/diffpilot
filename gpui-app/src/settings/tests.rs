use super::page::{Load, Op};
use super::*;
use crate::workspace::{self, Workspace};
use diffpilot_core::connection::{ClaudeStatus, Login, McpState};
use diffpilot_core::diff::DiffMode;
use diffpilot_core::github::auth::{Credentials, GhCli, MemoryStore, TokenSource, TokenStore};
use diffpilot_core::github::http::{Http, HttpError, Request, Response};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::{Effort, ModelName};
use diffpilot_core::settings::{AppearanceChoice, appearance_override};
use diffpilot_core::store::Store;
use diffpilot_core::viewed::ViewedStore;
use gpui::{AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
use gpui_component::ActiveTheme;
use gpui_component::Root;
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
    workspace: Entity<Workspace>,
}

struct Options {
    section: Section,
    auth: &'static str,
    gh: Option<&'static str>,
    claude_missing: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            section: Section::Claude,
            auth: LOGGED_IN,
            gh: None,
            claude_missing: false,
        }
    }
}

fn rig(cx: &mut TestAppContext, options: Options) -> (Rig, &mut VisualTestContext) {
    let (rig, vcx) = closed_rig(cx, &options);
    vcx.simulate_keystrokes("cmd-,");
    rig.settle(vcx);
    rig.wait_until_loaded(vcx);
    rig.click(vcx, section_selector(options.section));
    (rig, vcx)
}

fn section_selector(section: Section) -> &'static str {
    Box::leak(format!("toc:{}", section.title()).into_boxed_str())
}

fn closed_rig<'a>(cx: &'a mut TestAppContext, options: &Options) -> (Rig, &'a mut VisualTestContext) {
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
    cx.update(|cx| {
        gpui_component::init(cx);
        diffpilot_ui_kit::init(cx);
        workspace::init(cx);
        init(settings.clone(), services, cx);
    });
    let mut workspace = None;
    let window = cx.add_window(|window, cx| {
        appearance::install(window, cx);
        let host = cx.new(|cx| {
            Workspace::new(
                settings.clone(),
                Arc::new(ViewedStore::in_memory()),
                Arc::new(Store::open_in_memory().unwrap()),
                None,
                false,
                window,
                cx,
            )
        });
        workspace = Some(host.clone());
        Root::new(host, window, cx)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx).into_mut();
    vcx.executor().allow_parking();
    let rig = Rig {
        dir,
        claude,
        settings,
        store,
        workspace: workspace.unwrap(),
    };
    rig.settle(vcx);
    (rig, vcx)
}

impl Rig {
    fn settle(&self, cx: &mut VisualTestContext) {
        for _ in 0..3 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear();
            });
        }
        cx.run_until_parked();
    }

    fn wait_until_loaded(&self, cx: &mut VisualTestContext) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let loaded = self.state(cx, |page| {
                !matches!(page.claude, Load::Idle | Load::Loading)
                    && !matches!(page.mcp, Load::Loading)
                    && !matches!(page.github, Load::Idle | Load::Loading)
            });
            if loaded {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "the page did not finish loading");
            std::thread::sleep(std::time::Duration::from_millis(10));
            self.settle(cx);
        }
        self.settle(cx);
    }

    fn click(&self, cx: &mut VisualTestContext, selector: &'static str) {
        self.settle(cx);
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("no element tagged {selector}"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        self.settle(cx);
    }

    fn page(&self, cx: &mut VisualTestContext) -> Option<Entity<SettingsPage>> {
        self.workspace
            .read_with(cx, |workspace, _| workspace.settings_page().cloned())
    }

    fn state<T>(&self, cx: &mut VisualTestContext, read: impl FnOnce(&SettingsPage) -> T) -> T {
        let page = self.page(cx).expect("the settings page is open");
        page.read_with(cx, |page, _| read(page))
    }

    fn sections(&self, cx: &mut VisualTestContext) -> Vec<Section> {
        let page = self.page(cx).expect("the settings page is open");
        page.read_with(cx, |page, cx| page.visible_sections(cx))
    }

    fn top_section(&self, cx: &mut VisualTestContext) -> Option<Section> {
        let page = self.page(cx).expect("the settings page is open");
        page.read_with(cx, |page, cx| {
            page.visible_sections(cx).get(page.scroll.top_item()).copied()
        })
    }

    fn type_query(&self, cx: &mut VisualTestContext, text: &str) {
        self.click(cx, "settings:search");
        cx.simulate_keystrokes("cmd-a backspace");
        if !text.is_empty() {
            cx.simulate_input(text);
        }
        self.settle(cx);
    }
}

#[gpui::test]
async fn the_claude_section_finds_the_cli_reads_the_login_and_the_mcp_install(cx: &mut TestAppContext) {
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
async fn without_the_cli_the_section_says_so_and_never_runs_mcp_commands(cx: &mut TestAppContext) {
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
async fn opening_on_a_section_shows_its_heading_at_the_top(cx: &mut TestAppContext) {
    let (rig, cx) = closed_rig(cx, &Options::default());
    let workspace = rig.workspace.clone();
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_settings(Some(Section::Diff), window, cx)
        })
    });
    rig.settle(cx);
    let heading = cx.debug_bounds("section:Diff").expect("the section is tagged");
    let body = cx.debug_bounds("settings:body").expect("the list is tagged");
    let gap = (heading.top() - body.top()).abs();
    assert!(gap < px(8.), "Diff starts {gap:?} away from the top of the list");
    assert_eq!(rig.top_section(cx), Some(Section::Diff));
}

#[gpui::test]
async fn clicking_a_toc_item_scrolls_the_list_to_that_section(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    for section in Section::ALL {
        rig.click(cx, section_selector(section));
        let heading = cx
            .debug_bounds(Box::leak(format!("section:{}", section.title()).into_boxed_str()))
            .expect("the section is tagged");
        let body = cx.debug_bounds("settings:body").expect("the list is tagged");
        let gap = (heading.top() - body.top()).abs();
        assert!(gap < px(8.), "{section:?} starts {gap:?} below the top of the list");
        assert_eq!(
            rig.top_section(cx),
            Some(section),
            "the highlighted item follows the scroll"
        );
    }
}

#[gpui::test]
async fn typing_in_the_search_box_filters_the_rows_and_the_section_list(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    assert_eq!(rig.sections(cx), Section::ALL.to_vec());

    rig.type_query(cx, "WRAP");
    assert_eq!(rig.sections(cx), vec![Section::Diff]);
    assert!(testing_shows(cx, "settings:shows:Diff"));

    rig.type_query(cx, "mcp install");
    assert_eq!(rig.sections(cx), vec![Section::Claude]);

    rig.type_query(cx, "gh keychain");
    assert_eq!(rig.sections(cx), vec![Section::Accounts]);

    rig.type_query(cx, "toggle sidebar");
    assert_eq!(rig.sections(cx), vec![Section::KeyBindings]);

    rig.type_query(cx, "nothing like this");
    assert_eq!(rig.sections(cx), vec![]);
    assert!(testing_shows(cx, "settings:shows:"), "the list says nothing was found");

    rig.type_query(cx, "");
    assert_eq!(rig.sections(cx), Section::ALL.to_vec());
}

#[gpui::test]
async fn the_search_box_finds_a_setting_by_its_section_name(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    rig.type_query(cx, "diff layout");
    assert_eq!(rig.sections(cx), vec![Section::Diff]);
    rig.type_query(cx, "appearance");
    assert_eq!(rig.sections(cx), vec![Section::Appearance]);
}

#[gpui::test]
async fn escape_closes_the_page_and_the_close_button_does_too(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    assert!(rig.page(cx).is_some());
    cx.simulate_keystrokes("escape");
    rig.settle(cx);
    assert!(rig.page(cx).is_none(), "escape leaves the page");

    cx.simulate_keystrokes("cmd-,");
    rig.settle(cx);
    assert!(rig.page(cx).is_some());
    rig.click(cx, "settings:close");
    assert!(rig.page(cx).is_none(), "the close button leaves the page");
}

#[gpui::test]
async fn opening_twice_keeps_one_page_and_keeps_the_search_text(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, Options::default());
    let first = rig.page(cx).unwrap();
    rig.type_query(cx, "wrap");
    cx.simulate_keystrokes("cmd-,");
    rig.settle(cx);
    assert_eq!(rig.page(cx).unwrap().entity_id(), first.entity_id());
    assert_eq!(rig.sections(cx), vec![Section::Diff]);
    cx.simulate_input(" lines");
    rig.settle(cx);
    assert_eq!(
        rig.state(cx, |page| page.search.entity_id()),
        first.read_with(cx, |p, _| p.search.entity_id())
    );
    assert_eq!(first.read_with(cx, |page, cx| page.query(cx)), "wrap lines");
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
        rig.page(cx)
            .unwrap()
            .update(cx, |page, cx| page.check_claude_path(path, cx));
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
            section: Section::Appearance,
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
            section: Section::General,
            ..Options::default()
        },
    );
    assert!(rig.settings.open_last_repository());
    rig.click(cx, "open-last");
    assert!(!rig.settings.open_last_repository());

    rig.click(cx, "toc:Diff");
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
async fn accounts_reads_the_github_state_and_imports_and_signs_out(cx: &mut TestAppContext) {
    let (rig, cx) = rig(
        cx,
        Options {
            section: Section::Accounts,
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
            section: Section::Accounts,
            ..Options::default()
        },
    );
    let paste = |rig: &Rig, cx: &mut VisualTestContext, text: &'static str| {
        let page = rig.page(cx).unwrap();
        cx.update(|window, cx| {
            page.read(cx)
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
            section: Section::KeyBindings,
            ..Options::default()
        },
    );
    let list = cx.update(|_, cx| general::bindings(cx));
    let has = |title: &str, keys: &str| list.iter().any(|(t, k)| t.as_ref() == title && k == keys);
    assert!(has("Open Folder…", "⌘O"), "{list:?}");
    assert!(has("Settings…", "⌘,"), "{list:?}");
    assert!(has("Toggle Sidebar", "⌃⌘S"), "{list:?}");
    assert!(list.iter().all(|(title, keys)| !title.is_empty() && !keys.is_empty()));
}

#[test]
fn the_environment_flag_names_a_section() {
    assert_eq!(Section::parse("claude"), Some(Section::Claude));
    assert_eq!(Section::parse("Key Bindings"), Some(Section::KeyBindings));
    assert_eq!(Section::parse("key-bindings"), Some(Section::KeyBindings));
    assert_eq!(Section::parse("1"), Some(Section::General));
    assert_eq!(Section::parse(" "), None);
}

fn testing_shows(cx: &mut VisualTestContext, selector: &str) -> bool {
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
        .is_some()
}
