#[cfg(test)]
use crate::features::listed;
use crate::features::{FEATURES, Feature, Usage};
use crate::workspace::Workspace;
use std::collections::{BTreeMap, BTreeSet};

pub struct Pick {
    pub feature: &'static Feature,
    pub packages: BTreeSet<&'static str>,
}

/// The features a change can affect, each with the packages whose tests the change can reach:
/// the owner of each changed file and every feature that uses the owner. A file that no feature
/// lists selects every feature in the crates it builds into.
pub fn select(ws: &Workspace, usage: &Usage, files: &[String]) -> Vec<Pick> {
    let mut picked: BTreeMap<usize, BTreeSet<&'static str>> = BTreeMap::new();
    for file in files {
        let affected = ws.affected(file);
        let owners: Vec<usize> = (0..FEATURES.len()).filter(|&i| FEATURES[i].lists(file)).collect();
        let candidates: BTreeSet<usize> = if owners.is_empty() {
            (0..FEATURES.len()).collect()
        } else {
            owners
                .iter()
                .flat_map(|&o| usage.users(o).iter().copied().chain([o]))
                .collect()
        };
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

            if files.iter().any(|f| !listed(f) && !ws.affected(f).is_empty()) {
                assert_eq!(
                    run_in, old,
                    "{files:?} has an unlisted file, so the selections must be equal"
                );
            }
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
        assert!(!names.contains("github") && !names.contains("guide"), "{names:?}");
    }

    #[test]
    fn an_unlisted_core_file_selects_every_feature_with_tests_in_its_dependents() {
        let ws = Workspace::load().unwrap();
        let picks = select(&ws, &usage(), &["core/src/git.rs".to_owned()]);
        let names: BTreeSet<_> = picks.iter().map(|p| p.feature.name).collect();
        let all: BTreeSet<_> = FEATURES.iter().map(|f| f.name).filter(|n| *n != "xtask").collect();
        assert_eq!(names, all);
    }

    #[test]
    fn xtask_changes_select_only_xtask() {
        let ws = Workspace::load().unwrap();
        let picks = select(&ws, &usage(), &["xtask/src/main.rs".to_owned()]);
        let names: Vec<_> = picks.iter().map(|p| p.feature.name).collect();
        assert_eq!(names, ["xtask"]);
    }
}
