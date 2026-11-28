use std::collections::BTreeSet;

pub struct Feature {
    pub name: &'static str,
    pub paths: &'static [&'static str],
    pub tests: &'static [TestSel],
    pub bench: Option<Binary>,
    pub scenes: &'static [Scene],
}

/// A binary to build and run: the app, or an example of one package.
pub enum Binary {
    Example { package: &'static str, name: &'static str },
    App,
}

impl Binary {
    pub const fn app_example(name: &'static str) -> Binary {
        Binary::Example { package: APP, name }
    }

    pub fn cargo_args(&self) -> Vec<&'static str> {
        match self {
            Binary::Example { package, name } => vec!["-p", package, "--example", name],
            Binary::App => vec!["-p", APP, "--bin", APP],
        }
    }
}

/// One screenshot: `screenshots/<feature>/<name>.png`. App scenes run the `scenes` example with
/// `DIFFPILOT_SCENE` naming the scene function; it opens the real app on the fixture repo and prints
/// the part of the window to keep. Other scenes capture the whole window of `target`.
/// `env` is set on top of the fixture repo variables.
pub struct Scene {
    pub name: &'static str,
    pub target: Binary,
    pub env: &'static [(&'static str, &'static str)],
}

pub enum TestSel {
    Lib {
        package: &'static str,
        module: &'static str,
    },
    Target {
        package: &'static str,
        test: &'static str,
    },
}

impl TestSel {
    pub fn package(&self) -> &'static str {
        match self {
            TestSel::Lib { package, .. } | TestSel::Target { package, .. } => package,
        }
    }
}

const CORE: &str = "diffpilot-core";
const APP: &str = "diffpilot";
const UI: &str = "diffpilot-ui-kit";
const XTASK: &str = "xtask";

use TestSel::{Lib, Target};

pub const FEATURES: &[Feature] = &[
    Feature {
        name: "diff-pane",
        paths: &[
            "core/src/{diff,file_kind,anchor,editor,preview}.rs",
            "gpui-app/src/diff_pane/**",
            "gpui-app/src/pane.rs",
            "gpui-app/examples/diff_pane.rs",
            "gpui-app/tests/diff_pane_*.rs",
        ],
        tests: &[
            Lib {
                package: CORE,
                module: "diff::",
            },
            Lib {
                package: CORE,
                module: "file_kind::",
            },
            Lib {
                package: CORE,
                module: "anchor::",
            },
            Lib {
                package: CORE,
                module: "editor::",
            },
            Lib {
                package: CORE,
                module: "preview::",
            },
            Lib {
                package: APP,
                module: "diff_pane::",
            },
            Target {
                package: APP,
                test: "diff_pane_headless",
            },
        ],
        bench: Some(Binary::app_example("diff_pane")),
        scenes: &[
            Scene {
                name: "split-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "pane"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
            Scene {
                name: "split-dark",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "pane"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "dark"),
                ],
            },
            Scene {
                name: "unified-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "pane"),
                    ("DIFFPILOT_DIFF_MODE", "unified"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
            Scene {
                name: "split-wrap-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "pane-wrap"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
            Scene {
                name: "split-nowrap-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "pane-nowrap"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
            Scene {
                name: "preview-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "pane-preview"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
            Scene {
                name: "preview-dark",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "pane-preview"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "dark"),
                ],
            },
        ],
    },
    Feature {
        name: "review",
        paths: &[
            "core/src/{review,threads,thread_groups,fix}.rs",
            "core/src/{fix_comments,make_changes,reply_thread}.md",
            "gpui-app/src/review/**",
            "gpui-app/src/claude_{activity,area,status}.rs",
            "gpui-app/examples/review.rs",
            "gpui-app/tests/review_*.rs",
            "gpui-app/src/runs.rs",
            "gpui-app/tests/fix_threads.rs",
            "core/src/store/rows.rs",
        ],
        tests: &[
            Lib {
                package: CORE,
                module: "review::",
            },
            Lib {
                package: CORE,
                module: "threads::",
            },
            Lib {
                package: CORE,
                module: "thread_groups::",
            },
            Lib {
                package: CORE,
                module: "store::rows::",
            },
            Lib {
                package: CORE,
                module: "fix::",
            },
            Lib {
                package: APP,
                module: "review::",
            },
            Lib {
                package: APP,
                module: "claude_area::",
            },
            Target {
                package: APP,
                test: "review_session",
            },
            Target {
                package: APP,
                test: "fix_threads",
            },
        ],
        bench: Some(Binary::app_example("review")),
        scenes: &[
            Scene {
                name: "setup-light",
                target: Binary::app_example("scenes"),
                env: &[("DIFFPILOT_SCENE", "review-setup"), ("DIFFPILOT_APPEARANCE", "light")],
            },
            Scene {
                name: "setup-dark",
                target: Binary::app_example("scenes"),
                env: &[("DIFFPILOT_SCENE", "review-setup"), ("DIFFPILOT_APPEARANCE", "dark")],
            },
            Scene {
                name: "threads-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "review-threads"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
            Scene {
                name: "fix-picker-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "review-fix-picker"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
            Scene {
                name: "claude-area-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "review-claude-area"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
        ],
    },
    Feature {
        name: "sidebar",
        paths: &[
            "core/src/{tree,viewed,changes}.rs",
            "core/src/store/viewed.rs",
            "gpui-app/src/sidebar*.rs",
            "gpui-app/src/format.rs",
        ],
        tests: &[
            Lib {
                package: CORE,
                module: "tree::",
            },
            Lib {
                package: CORE,
                module: "viewed::",
            },
            Lib {
                package: CORE,
                module: "changes::",
            },
            Lib {
                package: APP,
                module: "sidebar",
            },
            Lib {
                package: APP,
                module: "format::",
            },
        ],
        bench: None,
        scenes: &[],
    },
    Feature {
        name: "comparison",
        paths: &["core/src/{comparison,refs,worktree}.rs", "gpui-app/src/comparison_*.rs"],
        tests: &[
            Lib {
                package: CORE,
                module: "comparison::",
            },
            Lib {
                package: CORE,
                module: "refs::",
            },
            Lib {
                package: CORE,
                module: "worktree::",
            },
            Lib {
                package: APP,
                module: "comparison_menu::",
            },
        ],
        bench: None,
        scenes: &[],
    },
    Feature {
        name: "settings",
        paths: &[
            "core/src/settings.rs",
            "core/src/store/data.rs",
            "gpui-app/src/settings/**",
        ],
        tests: &[
            Lib {
                package: CORE,
                module: "settings::",
            },
            Lib {
                package: CORE,
                module: "store::data::",
            },
            Lib {
                package: APP,
                module: "settings::",
            },
        ],
        bench: None,
        scenes: &[],
    },
    Feature {
        name: "claude-runs",
        paths: &[
            "core/src/{activity,claude,run,runner,stream,stage,snapshot,guard,policy,prompt,connection,process_groups}.rs",
            "core/src/{review_prompt,review_tools_step}.md",
            "core/src/mcp/**",
            "core/src/bin/**",
            "core/src/store/runs.rs",
            "core/tests/{runs,mcp,mcp_stdio,guard_hook,e2e,smoke,process_groups}.rs",
        ],
        tests: &[
            Lib {
                package: CORE,
                module: "claude::",
            },
            Lib {
                package: CORE,
                module: "run::",
            },
            Lib {
                package: CORE,
                module: "activity::",
            },
            Lib {
                package: CORE,
                module: "runner::",
            },
            Lib {
                package: CORE,
                module: "stream::",
            },
            Lib {
                package: CORE,
                module: "stage::",
            },
            Lib {
                package: CORE,
                module: "snapshot::",
            },
            Lib {
                package: CORE,
                module: "guard::",
            },
            Lib {
                package: CORE,
                module: "policy::",
            },
            Lib {
                package: CORE,
                module: "prompt::",
            },
            Lib {
                package: CORE,
                module: "connection::",
            },
            Lib {
                package: CORE,
                module: "process_groups::",
            },
            Lib {
                package: CORE,
                module: "mcp::",
            },
            Target {
                package: CORE,
                test: "runs",
            },
            Target {
                package: CORE,
                test: "mcp",
            },
            Target {
                package: CORE,
                test: "mcp_stdio",
            },
            Target {
                package: CORE,
                test: "guard_hook",
            },
            Target {
                package: CORE,
                test: "e2e",
            },
            Target {
                package: CORE,
                test: "smoke",
            },
            Target {
                package: CORE,
                test: "process_groups",
            },
        ],
        bench: None,
        scenes: &[],
    },
    Feature {
        name: "github",
        paths: &[
            "core/src/github/**",
            "core/src/forge/**",
            "core/src/sync.rs",
            "gpui-app/src/remote/**",
            "core/examples/github_smoke.rs",
            "core/tests/fixtures/github/**",
        ],
        tests: &[
            Lib {
                package: CORE,
                module: "github::",
            },
            Lib {
                package: CORE,
                module: "forge::",
            },
            Lib {
                package: CORE,
                module: "sync::",
            },
            Lib {
                package: APP,
                module: "remote::",
            },
        ],
        bench: None,
        scenes: &[],
    },
    Feature {
        name: "guide",
        paths: &[
            "core/src/guide/**",
            "core/src/store/guides.rs",
            "core/tests/guide.rs",
            "gpui-app/src/guide/**",
            "gpui-app/tests/guide.rs",
        ],
        tests: &[
            Lib {
                package: CORE,
                module: "guide::",
            },
            Lib {
                package: CORE,
                module: "store::guides::",
            },
            Target {
                package: CORE,
                test: "guide",
            },
            Lib {
                package: APP,
                module: "guide::",
            },
            Target {
                package: APP,
                test: "guide",
            },
        ],
        bench: None,
        scenes: &[
            Scene {
                name: "overview-light",
                target: Binary::app_example("scenes"),
                env: &[("DIFFPILOT_SCENE", "guide-overview"), ("DIFFPILOT_APPEARANCE", "light")],
            },
            Scene {
                name: "chapter-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "guide-chapter"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
            Scene {
                name: "chapter-dark",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "guide-chapter"),
                    ("DIFFPILOT_DIFF_MODE", "split"),
                    ("DIFFPILOT_APPEARANCE", "dark"),
                ],
            },
            Scene {
                name: "inspector-light",
                target: Binary::app_example("scenes"),
                env: &[
                    ("DIFFPILOT_SCENE", "guide-inspector"),
                    ("DIFFPILOT_APPEARANCE", "light"),
                ],
            },
            Scene {
                name: "linked-light",
                target: Binary::app_example("scenes"),
                env: &[("DIFFPILOT_SCENE", "guide-linked"), ("DIFFPILOT_APPEARANCE", "light")],
            },
            Scene {
                name: "inspector-dark",
                target: Binary::app_example("scenes"),
                env: &[("DIFFPILOT_SCENE", "guide-inspector"), ("DIFFPILOT_APPEARANCE", "dark")],
            },
        ],
    },
    Feature {
        name: "app-shell",
        paths: &[
            "gpui-app/src/{main,lib,workspace,repo_view,bench,testing,folder_menu,folder_picker,folder_prs,linked_folders,open_dialog,path_hints,switch_alert,commit_popover,tour}.rs",
            "core/src/{folder_status,commit}.rs",
            "gpui-app/tests/flows.rs",
            "gpui-app/tests/flows/**",
            "gpui-app/examples/scenes.rs",
            "ui-kit/src/regions.rs",
        ],
        tests: &[
            Lib {
                package: APP,
                module: "repo_view::",
            },
            Lib {
                package: APP,
                module: "bench::",
            },
            Lib {
                package: APP,
                module: "folder_menu::",
            },
            Lib {
                package: APP,
                module: "folder_prs::",
            },
            Lib {
                package: APP,
                module: "open_dialog::",
            },
            Lib {
                package: CORE,
                module: "folder_status::",
            },
            Lib {
                package: CORE,
                module: "commit::",
            },
        ],
        bench: Some(Binary::App),
        scenes: &[
            Scene {
                name: "sidebar-light",
                target: Binary::app_example("scenes"),
                env: &[("DIFFPILOT_SCENE", "sidebar"), ("DIFFPILOT_APPEARANCE", "light")],
            },
            Scene {
                name: "toolbar-light",
                target: Binary::app_example("scenes"),
                env: &[("DIFFPILOT_SCENE", "toolbar"), ("DIFFPILOT_APPEARANCE", "light")],
            },
            Scene {
                name: "settings-light",
                target: Binary::app_example("scenes"),
                env: &[("DIFFPILOT_SCENE", "settings"), ("DIFFPILOT_APPEARANCE", "light")],
            },
            Scene {
                name: "settings-dark",
                target: Binary::app_example("scenes"),
                env: &[("DIFFPILOT_SCENE", "settings"), ("DIFFPILOT_APPEARANCE", "dark")],
            },
        ],
    },
    Feature {
        name: "shared",
        paths: &[],
        tests: &[
            Lib {
                package: CORE,
                module: "git::",
            },
            Lib {
                package: CORE,
                module: "git_diff::",
            },
            Lib {
                package: CORE,
                module: "paths::",
            },
            Lib {
                package: CORE,
                module: "cancel::",
            },
            Lib {
                package: CORE,
                module: "watcher::",
            },
            Lib {
                package: CORE,
                module: "store::",
            },
            Target {
                package: CORE,
                test: "git_env",
            },
            Lib {
                package: UI,
                module: "",
            },
        ],
        bench: None,
        scenes: &[
            Scene {
                name: "gallery-light",
                target: Binary::Example {
                    package: UI,
                    name: "gallery",
                },
                env: &[("DIFFPILOT_APPEARANCE", "light")],
            },
            Scene {
                name: "gallery-dark",
                target: Binary::Example {
                    package: UI,
                    name: "gallery",
                },
                env: &[("DIFFPILOT_APPEARANCE", "dark")],
            },
        ],
    },
    Feature {
        name: "xtask",
        paths: &["xtask/**"],
        tests: &[Lib {
            package: XTASK,
            module: "",
        }],
        bench: None,
        scenes: &[],
    },
];

pub fn find(name: &str) -> Option<&'static Feature> {
    FEATURES.iter().find(|f| f.name == name)
}

pub fn resolve(names: &[&str]) -> crate::Result<Vec<&'static Feature>> {
    if names.is_empty() {
        return Ok(FEATURES.iter().collect());
    }
    names
        .iter()
        .map(|n| {
            find(n).ok_or_else(|| {
                let known: Vec<_> = FEATURES.iter().map(|f| f.name).collect();
                format!("unknown feature {n:?}; known: {}", known.join(", ")).into()
            })
        })
        .collect()
}

impl Feature {
    pub fn lists(&self, path: &str) -> bool {
        self.paths.iter().any(|p| matches(p, path))
    }

    pub fn packages(&self) -> impl Iterator<Item = &'static str> {
        self.tests.iter().map(TestSel::package)
    }

    pub fn tests_module(&self, home: &str, module: &str) -> bool {
        self.tests.iter().any(|t| match t {
            TestSel::Lib { package, module: m } => {
                *package == home
                    && (m.is_empty()
                        || m.strip_prefix(module)
                            .is_some_and(|r| r.is_empty() || r.starts_with("::")))
            }
            TestSel::Target { .. } => false,
        })
    }
}

pub fn listed(path: &str) -> bool {
    FEATURES.iter().any(|f| f.lists(path))
}

/// `users[i]` holds the features whose Rust files name one of feature `i`'s modules through a
/// workspace crate root, in a crate that builds on that module's crate. Only direct users count:
/// following users of users reaches every feature, because the core modules use each other.
pub struct Usage {
    users: Vec<BTreeSet<usize>>,
    named: Vec<Vec<(String, BTreeSet<String>)>>,
}

impl Usage {
    pub fn scan(ws: &crate::workspace::Workspace, files: &[String]) -> Usage {
        let root = crate::root();
        let named: Vec<Vec<(String, BTreeSet<String>)>> = FEATURES
            .iter()
            .map(|f| {
                files
                    .iter()
                    .filter(|path| path.ends_with(".rs") && f.lists(path))
                    .filter_map(|path| {
                        let package = ws.crates.iter().find(|c| c.contains(path))?.package.clone();
                        let source = std::fs::read_to_string(root.join(path)).ok()?;
                        Some((package, workspace_names(&source)))
                    })
                    .collect()
            })
            .collect();
        let users = FEATURES
            .iter()
            .enumerate()
            .map(|(owner, f)| {
                (0..FEATURES.len())
                    .filter(|&user| {
                        user != owner
                            && named[user].iter().any(|(package, names)| {
                                f.modules_by_package()
                                    .any(|(home, module)| names.contains(module) && ws.closure(package).contains(home))
                            })
                    })
                    .collect()
            })
            .collect();
        Usage { users, named }
    }

    pub fn users(&self, owner: usize) -> &BTreeSet<usize> {
        &self.users[owner]
    }

    pub fn names(&self, ws: &crate::workspace::Workspace, feature: usize, home: &str, module: &str) -> bool {
        self.named[feature]
            .iter()
            .any(|(package, names)| names.contains(module) && ws.closure(package).contains(home))
    }

    pub fn used_by(&self, user: usize) -> impl Iterator<Item = usize> + '_ {
        (0..FEATURES.len()).filter(move |&owner| self.users[owner].contains(&user))
    }
}

impl Feature {
    fn modules_by_package(&self) -> impl Iterator<Item = (&'static str, &'static str)> {
        self.tests.iter().filter_map(|t| match t {
            TestSel::Lib { package, module } => module
                .trim_end_matches("::")
                .rsplit("::")
                .next()
                .filter(|m| !m.is_empty())
                .map(|m| (*package, m)),
            TestSel::Target { .. } => None,
        })
    }
}

const ROOTS: [&str; 5] = [
    "crate::",
    "super::",
    "diffpilot_core::",
    "diffpilot::",
    "diffpilot_ui_kit::",
];

/// Identifiers that follow a workspace crate root, inside a `use` statement or in a qualified path.
/// `std::sync::Arc` names no workspace module, so it does not count as using the `sync` module.
fn workspace_names(source: &str) -> BTreeSet<String> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut names = BTreeSet::new();
    for (at, _) in ROOTS.iter().flat_map(|r| source.match_indices(r)) {
        if source[..at].ends_with(ident) {
            continue;
        }
        let after = &source[at..];
        let in_use = source[..at].trim_end().ends_with("use");
        let end = if in_use {
            after.find(';').unwrap_or(after.len())
        } else {
            after.find(|c: char| !ident(c) && c != ':').unwrap_or(after.len())
        };
        names.extend(
            after[..end]
                .split(|c: char| !ident(c))
                .filter(|w| !w.is_empty())
                .map(str::to_owned),
        );
    }
    names
}

fn matches(pattern: &str, path: &str) -> bool {
    expand(pattern).iter().any(|p| glob(p.as_bytes(), path.as_bytes()))
}

fn expand(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_owned()];
    };
    let close = open + pattern[open..].find('}').expect("unclosed brace in a feature path");
    let (head, tail) = (&pattern[..open], &pattern[close + 1..]);
    pattern[open + 1..close]
        .split(',')
        .flat_map(|alt| expand(&format!("{head}{alt}{tail}")))
        .collect()
}

fn glob(pattern: &[u8], path: &[u8]) -> bool {
    match pattern {
        [] => path.is_empty(),
        [b'*', b'*', rest @ ..] => (0..=path.len()).any(|i| glob(rest, &path[i..])),
        [b'*', rest @ ..] => {
            let end = path.iter().position(|&c| c == b'/').unwrap_or(path.len());
            (0..=end).any(|i| glob(rest, &path[i..]))
        }
        [c, rest @ ..] => path.first() == Some(c) && glob(rest, &path[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_workspace_paths_name_a_module() {
        let names = workspace_names(
            "use std::sync::Arc;\nuse crate::{threads::Id, diff};\nlet x = diffpilot_core::github::pr(); a.run();",
        );
        assert!(names.contains("threads") && names.contains("diff") && names.contains("github"));
        assert!(!names.contains("sync") && !names.contains("run"), "{names:?}");
    }

    #[test]
    fn braces_and_stars() {
        assert!(matches("core/src/{diff,anchor}.rs", "core/src/anchor.rs"));
        assert!(!matches("core/src/{diff,anchor}.rs", "core/src/git.rs"));
        assert!(matches("core/src/*review*.md", "core/src/review_prompt.md"));
        assert!(!matches("core/src/*review*.md", "core/src/a/review.md"));
        assert!(matches("gpui-app/src/review/**", "gpui-app/src/review/a/b.rs"));
        assert!(!matches("gpui-app/src/review/**", "gpui-app/src/review.rs"));
    }

    #[test]
    fn names_are_unique_and_tests_exist() {
        for (i, f) in FEATURES.iter().enumerate() {
            assert!(FEATURES[..i].iter().all(|g| g.name != f.name), "{}", f.name);
            assert!(!f.tests.is_empty(), "{} has no tests", f.name);
        }
    }

    #[test]
    fn no_file_is_listed_twice() {
        let root = crate::root();
        let files = crate::git(&["ls-files"]).unwrap();
        for file in files.iter().filter(|f| root.join(f).exists()) {
            let owners: Vec<_> = FEATURES.iter().filter(|f| f.lists(file)).map(|f| f.name).collect();
            assert!(owners.len() <= 1, "{file} is listed by {owners:?}");
        }
    }
}
