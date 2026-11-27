use super::page::{Section, SettingsPage};
use super::{accounts, claude_tab, data_tab, general};
use diffpilot_ui_kit::Theme;
use gpui::prelude::*;
use gpui::{AnyElement, Context};

pub struct Entry {
    pub section: Section,
    pub title: &'static str,
    pub detail: &'static str,
    pub keywords: &'static str,
    pub render: fn(&SettingsPage, &Theme, &mut Context<SettingsPage>) -> AnyElement,
}

impl Entry {
    pub fn matches(&self, query: &str) -> bool {
        matches(query, &[self.section.title(), self.title, self.detail, self.keywords])
    }
}

pub const ENTRIES: &[Entry] = &[
    Entry {
        section: Section::General,
        title: "Open the last repository on launch",
        detail: "Without it, Diffpilot starts with no folder until you press ⌘O.",
        keywords: "startup start restore folder",
        render: general::open_last,
    },
    Entry {
        section: Section::General,
        title: "Open files in",
        detail: "Used by Open in Editor in the diff. Command uses {file}, {line} and {column}.",
        keywords: "editor command custom vscode zed cursor xcode",
        render: general::editor,
    },
    Entry {
        section: Section::Appearance,
        title: "Appearance",
        detail: "System follows macOS. Every open window changes at once.",
        keywords: "theme dark light system color mode",
        render: general::appearance,
    },
    Entry {
        section: Section::Diff,
        title: "Wrap lines",
        detail: "Same as the wrap button in the toolbar.",
        keywords: "word wrap long lines",
        render: general::wrap_lines,
    },
    Entry {
        section: Section::Diff,
        title: "Layout",
        detail: "Same as the Unified and Split switch in the toolbar.",
        keywords: "unified split side by side mode",
        render: general::layout,
    },
    Entry {
        section: Section::Claude,
        title: "Claude",
        detail: "Connection, models, editing and terminal agents.",
        keywords: "claude path cli mcp install terminal agents model effort opus sonnet haiku permission login",
        render: claude,
    },
    Entry {
        section: Section::Claude,
        title: "Ask before stopping Claude",
        detail: "When you switch folders while Claude is running",
        keywords: "switch folder stop run alert confirm",
        render: general::ask_before_stopping,
    },
    Entry {
        section: Section::Accounts,
        title: "GitHub",
        detail: "Sign in to read and reply to pull requests.",
        keywords: "github token sign in sign out gh keychain account import",
        render: accounts_entry,
    },
    Entry {
        section: Section::Data,
        title: "Stored data",
        detail: "Threads, comments, runs, guides and viewed marks kept for each repo and branch.",
        keywords: "delete clear remove storage database reset review history",
        render: data_entry,
    },
];

fn data_entry(page: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> AnyElement {
    data_tab::render(page, t, cx).into_any_element()
}

fn claude(page: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> AnyElement {
    claude_tab::render(page, t, cx).into_any_element()
}

fn accounts_entry(page: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> AnyElement {
    accounts::render(page, t, cx).into_any_element()
}

/// Every whitespace-separated word of `query` must appear in the haystack, ignoring case.
pub fn matches(query: &str, haystack: &[&str]) -> bool {
    let text = haystack.join(" ").to_lowercase();
    query.split_whitespace().all(|word| text.contains(&word.to_lowercase()))
}

pub fn in_section(section: Section, query: &str) -> impl Iterator<Item = &'static Entry> + '_ {
    ENTRIES
        .iter()
        .filter(move |entry| entry.section == section && entry.matches(query))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_matches_everything() {
        assert!(matches("", &["Wrap lines"]));
        assert!(matches("   ", &[]));
    }

    #[test]
    fn matching_ignores_case() {
        assert!(matches("WRAP", &["Wrap lines"]));
        assert!(matches("wrap", &["WRAP LINES"]));
    }

    #[test]
    fn every_word_must_appear_in_the_haystack_in_any_order() {
        assert!(matches("lines wrap", &["Wrap lines"]));
        assert!(!matches("wrap tabs", &["Wrap lines"]));
    }

    #[test]
    fn words_may_come_from_different_fields() {
        assert!(matches(
            "diff wrap",
            &["Diff", "Wrap lines", "Same as the toolbar", "word"]
        ));
    }

    #[test]
    fn a_word_can_match_inside_a_longer_word() {
        assert!(matches("lin", &["Wrap lines"]));
    }

    #[test]
    fn every_section_has_an_entry_except_key_bindings() {
        for section in Section::ALL {
            let has = ENTRIES.iter().any(|entry| entry.section == section);
            assert_eq!(has, section != Section::KeyBindings, "{section:?}");
        }
    }

    #[test]
    fn keywords_find_the_claude_and_accounts_panels() {
        assert_eq!(in_section(Section::Claude, "mcp install").count(), 1);
        assert_eq!(in_section(Section::Accounts, "gh keychain").count(), 1);
        assert_eq!(in_section(Section::Accounts, "mcp").count(), 0);
    }

    #[test]
    fn the_section_title_is_searchable() {
        assert_eq!(in_section(Section::Diff, "diff").count(), 2);
    }
}
