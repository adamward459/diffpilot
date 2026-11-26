use crate::features::{FEATURES, Feature, Usage};
use crate::workspace::Workspace;
use std::collections::{BTreeMap, BTreeSet};

pub struct Pick {
    pub feature: &'static Feature,
    pub packages: BTreeSet<&'static str>,
}

/// The features a change can affect, each with the packages whose tests the change can reach.
/// A Rust file under `<crate>/src/` selects its owner and the features that test, name, or hold
/// a file of its top-level module. A test file selects its owner. Any other file selects its
/// owner and every feature that uses the owner. When none of these rules finds a feature, the
/// file selects every feature in the crates it builds into.
pub fn select(ws: &Workspace, usage: &Usage, files: &[String]) -> Vec<Pick> {
    let mut picked: BTreeMap<usize, BTreeSet<&'static str>> = BTreeMap::new();
    for file in files {
        let affected = ws.affected(file);
        let owners: Vec<usize> = (0..FEATURES.len()).filter(|&i| FEATURES[i].lists(file)).collect();
        let mut candidates: BTreeSet<usize> = match Module::of(ws, file) {
            Some(module) => (0..FEATURES.len())
                .filter(|&i| {
                    let f = &FEATURES[i];
                    f.tests_module(module.home, &module.name)
                        || f.paths.iter().any(|p| p.starts_with(&module.folder))
                        || usage.names(ws, i, module.home, &module.name)
                })
                .chain(owners.iter().copied())
                .collect(),
            None if file.contains("/tests/") => owners.iter().copied().collect(),
            None => owners
                .iter()
                .flat_map(|&o| usage.users(o).iter().copied().chain([o]))
                .collect(),
        };
        if candidates.is_empty() {
            candidates = (0..FEATURES.len()).collect();
        }
        for i in candidates {
            let reached: BTreeSet<_> = FEATURES[i].packages().filter(|p| affected.contains(p)).collect();
            if owners.contains(&i) || !reached.is_empty() {
                picked.entry(i).or_default().extend(reached);
            }
        }
    }
    picked
        .into_iter()
        .map(|(i, packages)| Pick {
            feature: &FEATURES[i],
            packages,
        })
        .collect()
}

struct Module<'a> {
    home: &'a str,
    name: String,
    folder: String,
}

impl<'a> Module<'a> {
    /// The top-level module of a Rust file under `<crate>/src/`. None for a crate root, or for a
    /// module the crate root re-exports, because users then reach it without naming it.
    fn of(ws: &'a Workspace, file: &str) -> Option<Module<'a>> {
        let c = ws.crates.iter().find(|c| c.contains(file))?;
        let src = format!("{}/src/", c.folder);
        let rest = file.strip_prefix(&src)?;
        if !rest.ends_with(".rs") || rest == "lib.rs" || rest == "main.rs" {
            return None;
        }
        let name = rest.split('/').next()?.trim_end_matches(".rs").to_owned();
        let root = std::fs::read_to_string(crate::root().join(&src).join("lib.rs")).unwrap_or_default();
        if root.contains(&format!("pub use {name}::")) {
            return None;
        }
        Some(Module {
            home: &c.package,
            folder: format!("{src}{name}/"),
            name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn usage() -> Usage {
        Usage::scan(&Workspace::load().unwrap(), &crate::git(&["ls-files"]).unwrap())
    }

    fn strings(value: &Value) -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn selection_stays_inside_what_the_per_crate_rule_picked() {
        let ws = Workspace::load().unwrap();
        let golden: Value = serde_json::from_str(include_str!("../tests/golden/selection.json")).unwrap();
        let sets = golden.as_array().unwrap();
        assert_eq!(sets.len(), 10);
        for set in sets {
            let files = strings(&set["files"]);
            let old: BTreeSet<String> = strings(&set["touched"]).into_iter().collect();
            let ported: BTreeSet<String> = ws.touched(&files).into_iter().map(str::to_owned).collect();
            assert_eq!(ported, old, "touched crates differ for {files:?}");

            let picks = select(&ws, &usage(), &files);
            let run_in: BTreeSet<String> = picks
                .iter()
                .flat_map(|p| p.packages.iter().map(|s| s.to_string()))
                .collect();
            assert!(
                run_in.is_subset(&old),
                "{files:?} runs tests in {run_in:?}, old rule picked {old:?}"
            );
        }
    }

    #[test]
    fn a_threads_change_selects_review_and_the_features_that_use_threads() {
        let ws = Workspace::load().unwrap();
        let picks = select(&ws, &usage(), &["core/src/threads.rs".to_owned()]);
        let names: BTreeSet<_> = picks.iter().map(|p| p.feature.name).collect();
        for name in ["review", "diff-pane", "github", "guide", "claude-runs"] {
            assert!(names.contains(name), "{name} is missing from {names:?}");
        }
    }

    #[test]
    fn a_leaf_app_change_does_not_select_unrelated_features() {
        let ws = Workspace::load().unwrap();
        let picks = select(&ws, &usage(), &["gpui-app/src/settings/appearance.rs".to_owned()]);
        let names: BTreeSet<_> = picks.iter().map(|p| p.feature.name).collect();
        assert!(names.contains("settings"));
        assert!(!names.contains("claude-runs") && !names.contains("guide"), "{names:?}");
    }

    fn names(file: &str) -> BTreeSet<&'static str> {
        let ws = Workspace::load().unwrap();
        select(&ws, &usage(), &[file.to_owned()])
            .iter()
            .map(|p| p.feature.name)
            .collect()
    }

    #[test]
    fn an_unlisted_core_file_selects_the_features_that_name_its_module() {
        let names = names("core/src/git.rs");
        assert!(names.contains("shared") && names.contains("comparison"), "{names:?}");
        assert!(!names.contains("settings") && !names.contains("review"), "{names:?}");
    }

    #[test]
    fn a_file_in_a_module_folder_selects_the_features_that_hold_files_there() {
        let names = names("core/src/store/schema.rs");
        assert!(names.contains("guide") && names.contains("shared"), "{names:?}");
        assert!(!names.contains("settings"), "{names:?}");
    }

    #[test]
    fn a_listed_file_selects_users_of_its_module_not_of_its_whole_feature() {
        assert_eq!(names("core/src/viewed.rs"), BTreeSet::from(["sidebar", "app-shell"]));
    }

    #[test]
    fn a_crate_root_still_selects_every_feature_in_its_dependents() {
        let all: BTreeSet<_> = FEATURES.iter().map(|f| f.name).filter(|n| *n != "xtask").collect();
        assert_eq!(names("core/src/lib.rs"), all);
    }

    #[test]
    fn a_ui_kit_module_without_a_re_export_selects_its_app_users() {
        let names = names("ui-kit/src/menu.rs");
        assert!(names.contains("app-shell"), "{names:?}");
        assert!(!names.contains("guide"), "{names:?}");
    }

    #[test]
    fn a_ui_kit_file_selects_the_feature_that_tests_the_whole_crate() {
        assert!(names("ui-kit/src/components/form.rs").contains("shared"));
    }

    #[test]
    fn a_test_file_selects_only_its_owner() {
        assert_eq!(names("gpui-app/tests/flows/viewed.rs"), BTreeSet::from(["app-shell"]));
    }

    #[test]
    fn xtask_changes_select_only_xtask() {
        let ws = Workspace::load().unwrap();
        let picks = select(&ws, &usage(), &["xtask/src/main.rs".to_owned()]);
        let names: Vec<_> = picks.iter().map(|p| p.feature.name).collect();
        assert_eq!(names, ["xtask"]);
    }
}
