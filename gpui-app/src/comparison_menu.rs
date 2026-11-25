use diffpilot_core::comparison::Comparison;
use diffpilot_core::refs::Refs;
use diffpilot_ui_kit::Glyph;

#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    pub title: String,
    pub detail: Option<String>,
    pub checked: bool,
    pub target: Comparison,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Item(MenuItem),
    Submenu { title: &'static str, items: Vec<MenuItem> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub title: &'static str,
    pub enabled: bool,
    pub entries: Vec<Entry>,
}

fn item(title: &str, detail: Option<&str>, checked: bool, target: Comparison) -> MenuItem {
    MenuItem {
        title: title.to_string(),
        detail: detail.map(str::to_string),
        checked,
        target,
    }
}

pub fn branch_title(refs: &Refs, comparison: &Comparison) -> String {
    comparison
        .branch_name()
        .or(refs.checked_out.as_deref())
        .unwrap_or("Detached HEAD")
        .to_string()
}

pub fn branch_glyph(comparison: &Comparison) -> Glyph {
    if comparison.branch_name().is_none() {
        Glyph::GitBranch
    } else {
        Glyph::Eye
    }
}

pub fn branch_tooltip(comparison: &Comparison) -> &'static str {
    if comparison.branch_name().is_none() {
        "Checked-out branch"
    } else {
        "Read only. This branch is not checked out."
    }
}

pub fn base_title(comparison: &Comparison) -> String {
    match comparison {
        Comparison::WorkingTree => "Working Tree Changes".to_string(),
        Comparison::Staged => "Staged Changes".to_string(),
        Comparison::CheckedOut {
            base,
            includes_uncommitted: false,
        } => format!("{base}, committed only"),
        Comparison::CheckedOut { base, .. } | Comparison::Branch { base, .. } => base.clone(),
    }
}

pub fn base_glyph(comparison: &Comparison) -> Glyph {
    match comparison {
        Comparison::WorkingTree => Glyph::Pencil,
        Comparison::Staged => Glyph::CircleCheck,
        Comparison::CheckedOut { .. } | Comparison::Branch { .. } => Glyph::ArrowLeftRight,
    }
}

pub fn branch_sections(refs: &Refs, comparison: &Comparison) -> Vec<Section> {
    let default_base = refs.detected_base.as_str();
    let branch_items = |names: &[String]| -> Vec<Entry> {
        names
            .iter()
            .map(|name| {
                Entry::Item(item(
                    name,
                    None,
                    comparison.branch_name() == Some(name),
                    comparison.with_branch(Some(name), default_base),
                ))
            })
            .collect()
    };
    let mut sections = vec![Section {
        title: "Current",
        enabled: true,
        entries: vec![Entry::Item(item(
            refs.checked_out.as_deref().unwrap_or("Detached HEAD"),
            None,
            comparison.branch_name().is_none(),
            comparison.with_branch(None, default_base),
        ))],
    }];
    for (title, names) in [("Local Branches", &refs.local), ("Remote Branches", &refs.remote)] {
        if !names.is_empty() {
            sections.push(Section {
                title,
                enabled: true,
                entries: branch_items(names),
            });
        }
    }
    sections
}

pub fn base_sections(refs: &Refs, comparison: &Comparison) -> Vec<Section> {
    let default_base = refs.detected_base.as_str();
    let reviewing_other_branch = comparison.branch_name().is_some();
    let base_item = |name: &str, detail: Option<&str>| {
        item(
            name,
            detail,
            comparison.base() == Some(name),
            comparison.with_base(name),
        )
    };

    let suggested = refs
        .suggested()
        .iter()
        .map(|name| {
            let detail = (name == &refs.detected_base).then_some("Detected base");
            Entry::Item(base_item(name, detail))
        })
        .collect();

    let uncommitted_only = vec![
        Entry::Item(item(
            "Working Tree Changes Only",
            Some("Like git diff HEAD"),
            *comparison == Comparison::WorkingTree,
            Comparison::WorkingTree,
        )),
        Entry::Item(item(
            "Staged Changes Only",
            Some("Like git diff --staged"),
            *comparison == Comparison::Staged,
            Comparison::Staged,
        )),
    ];

    let has_base = comparison.base().is_some();
    let include = vec![
        Entry::Item(item(
            "Committed and Uncommitted",
            None,
            has_base && comparison.includes_uncommitted(),
            comparison.with_uncommitted(true, default_base),
        )),
        Entry::Item(item(
            "Committed Only",
            None,
            has_base && !comparison.includes_uncommitted(),
            comparison.with_uncommitted(false, default_base),
        )),
    ];

    let mut local: Vec<String> = refs.local.clone();
    local.extend(refs.checked_out.clone());
    let mut refs_entries = Vec::new();
    for (title, names) in [
        ("Local Branches", local),
        ("Remote Branches", refs.remote.clone()),
        ("Tags", refs.tags.clone()),
    ] {
        if !names.is_empty() {
            let items = names.iter().map(|name| base_item(name, None)).collect();
            refs_entries.push(Entry::Submenu { title, items });
        }
    }
    if !refs.commits.is_empty() {
        let items = refs
            .commits
            .iter()
            .map(|commit| base_item(&commit.sha, Some(&commit.subject)))
            .collect();
        refs_entries.push(Entry::Submenu {
            title: "Recent Commits",
            items,
        });
    }

    vec![
        Section {
            title: "Suggested",
            enabled: true,
            entries: suggested,
        },
        Section {
            title: "Only What Is Not Committed",
            enabled: !reviewing_other_branch,
            entries: uncommitted_only,
        },
        Section {
            title: "Include",
            enabled: !reviewing_other_branch,
            entries: include,
        },
        Section {
            title: "Branches, Tags and Commits",
            enabled: true,
            entries: refs_entries,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffpilot_core::refs::Commit;

    fn refs() -> Refs {
        Refs {
            checked_out: Some("feature".into()),
            detected_base: "origin/main".into(),
            local: vec!["main".into(), "other".into()],
            remote: vec!["origin/main".into(), "origin/other".into()],
            tags: vec!["v1".into()],
            commits: vec![Commit {
                sha: "abc1234".into(),
                subject: "First".into(),
            }],
        }
    }

    fn titles(section: &Section) -> Vec<&str> {
        section
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Item(item) => item.title.as_str(),
                Entry::Submenu { title, .. } => title,
            })
            .collect()
    }

    fn item_at(section: &Section, index: usize) -> &MenuItem {
        match &section.entries[index] {
            Entry::Item(item) => item,
            Entry::Submenu { .. } => panic!("submenu"),
        }
    }

    #[test]
    fn titles_and_icons_follow_the_comparison() {
        let refs = refs();
        let checked_out = Comparison::checked_out("origin/main", true);
        assert_eq!(branch_title(&refs, &checked_out), "feature");
        assert_eq!(branch_glyph(&checked_out), Glyph::GitBranch);
        assert_eq!(branch_tooltip(&checked_out), "Checked-out branch");
        assert_eq!(base_title(&checked_out), "origin/main");
        assert_eq!(base_glyph(&checked_out), Glyph::ArrowLeftRight);

        let other = Comparison::branch("other", "main");
        assert_eq!(branch_title(&refs, &other), "other");
        assert_eq!(branch_glyph(&other), Glyph::Eye);
        assert_eq!(branch_tooltip(&other), "Read only. This branch is not checked out.");
        assert_eq!(base_title(&other), "main");

        assert_eq!(
            base_title(&Comparison::checked_out("main", false)),
            "main, committed only"
        );
        assert_eq!(base_title(&Comparison::WorkingTree), "Working Tree Changes");
        assert_eq!(base_glyph(&Comparison::WorkingTree), Glyph::Pencil);
        assert_eq!(base_title(&Comparison::Staged), "Staged Changes");
        assert_eq!(base_glyph(&Comparison::Staged), Glyph::CircleCheck);
    }

    #[test]
    fn detached_head_has_a_title() {
        let mut refs = refs();
        refs.checked_out = None;
        let comparison = Comparison::checked_out("main", true);
        assert_eq!(branch_title(&refs, &comparison), "Detached HEAD");
        let sections = branch_sections(&refs, &comparison);
        assert_eq!(item_at(&sections[0], 0).title, "Detached HEAD");
    }

    #[test]
    fn branch_menu_checks_the_current_choice_and_skips_empty_sections() {
        let refs = refs();
        let comparison = Comparison::checked_out("origin/main", true);
        let sections = branch_sections(&refs, &comparison);
        let names: Vec<&str> = sections.iter().map(|section| section.title).collect();
        assert_eq!(names, ["Current", "Local Branches", "Remote Branches"]);
        assert!(item_at(&sections[0], 0).checked);
        assert!(!item_at(&sections[1], 0).checked);
        assert_eq!(titles(&sections[1]), ["main", "other"]);

        let on_other = Comparison::branch("other", "origin/main");
        let sections = branch_sections(&refs, &on_other);
        assert!(!item_at(&sections[0], 0).checked);
        assert!(item_at(&sections[1], 1).checked);
        assert_eq!(
            item_at(&sections[1], 1).target,
            Comparison::branch("other", "origin/main")
        );
        assert_eq!(
            item_at(&sections[0], 0).target,
            Comparison::checked_out("origin/main", true)
        );

        let mut bare = refs.clone();
        bare.local.clear();
        bare.remote.clear();
        assert_eq!(branch_sections(&bare, &comparison).len(), 1);
    }

    #[test]
    fn base_menu_has_the_four_sections() {
        let refs = refs();
        let comparison = Comparison::checked_out("origin/main", true);
        let sections = base_sections(&refs, &comparison);
        let names: Vec<&str> = sections.iter().map(|section| section.title).collect();
        assert_eq!(
            names,
            [
                "Suggested",
                "Only What Is Not Committed",
                "Include",
                "Branches, Tags and Commits"
            ]
        );
        assert!(sections.iter().all(|section| section.enabled));

        assert_eq!(titles(&sections[0]), ["origin/main", "main"]);
        let suggested = item_at(&sections[0], 0);
        assert_eq!(suggested.detail.as_deref(), Some("Detected base"));
        assert!(suggested.checked);

        assert_eq!(
            titles(&sections[1]),
            ["Working Tree Changes Only", "Staged Changes Only"]
        );
        assert_eq!(item_at(&sections[1], 0).detail.as_deref(), Some("Like git diff HEAD"));
        assert_eq!(
            item_at(&sections[1], 1).detail.as_deref(),
            Some("Like git diff --staged")
        );
        assert_eq!(titles(&sections[2]), ["Committed and Uncommitted", "Committed Only"]);
        assert!(item_at(&sections[2], 0).checked);
        assert!(!item_at(&sections[2], 1).checked);
        assert_eq!(
            titles(&sections[3]),
            ["Local Branches", "Remote Branches", "Tags", "Recent Commits"]
        );
    }

    #[test]
    fn suggested_adds_the_local_twin_of_a_remote_base() {
        let refs = refs();
        let sections = base_sections(&refs, &Comparison::checked_out("origin/main", true));
        let suggested = sections[0]
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Item(item) => item.title.clone(),
                Entry::Submenu { .. } => String::new(),
            })
            .collect::<Vec<_>>();
        assert_eq!(suggested, ["origin/main", "main"]);
    }

    #[test]
    fn local_branch_submenu_includes_the_checked_out_branch_last() {
        let refs = refs();
        let sections = base_sections(&refs, &Comparison::checked_out("origin/main", true));
        let Entry::Submenu { items, .. } = &sections[3].entries[0] else {
            panic!("submenu")
        };
        let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["main", "other", "feature"]);
    }

    #[test]
    fn picking_a_base_keeps_the_branch_and_the_include_choice() {
        let refs = refs();
        let comparison = Comparison::checked_out("main", false);
        let sections = base_sections(&refs, &comparison);
        let Entry::Submenu { items, .. } = &sections[3].entries[1] else {
            panic!("submenu")
        };
        assert_eq!(items[0].target, Comparison::checked_out("origin/main", false));
        assert!(item_at(&sections[2], 1).checked);
        assert!(!item_at(&sections[2], 0).checked);

        let on_other = Comparison::branch("other", "main");
        let sections = base_sections(&refs, &on_other);
        let Entry::Submenu { items, .. } = &sections[3].entries[0] else {
            panic!("submenu")
        };
        assert_eq!(items[0].target, Comparison::branch("other", "main"));
        assert!(items[0].checked);
    }

    #[test]
    fn reviewing_another_branch_disables_the_uncommitted_sections() {
        let refs = refs();
        let sections = base_sections(&refs, &Comparison::branch("other", "main"));
        assert!(sections[0].enabled);
        assert!(!sections[1].enabled);
        assert!(!sections[2].enabled);
        assert!(sections[3].enabled);
        assert!(!item_at(&sections[2], 0).checked);
        assert!(item_at(&sections[2], 1).checked);
    }

    #[test]
    fn working_tree_and_staged_check_their_own_rows_only() {
        let refs = refs();
        let sections = base_sections(&refs, &Comparison::WorkingTree);
        assert!(item_at(&sections[1], 0).checked);
        assert!(!item_at(&sections[1], 1).checked);
        assert!(!item_at(&sections[2], 0).checked);
        assert!(!item_at(&sections[0], 0).checked);
        let staged = base_sections(&refs, &Comparison::Staged);
        assert!(item_at(&staged[1], 1).checked);
        assert_eq!(
            item_at(&staged[2], 0).target,
            Comparison::checked_out("origin/main", true)
        );
    }

    #[test]
    fn recent_commits_show_the_subject() {
        let refs = refs();
        let sections = base_sections(&refs, &Comparison::checked_out("abc1234", true));
        let Entry::Submenu { items, .. } = &sections[3].entries[3] else {
            panic!("submenu")
        };
        assert_eq!(items[0].title, "abc1234");
        assert_eq!(items[0].detail.as_deref(), Some("First"));
        assert!(items[0].checked);
    }
}
