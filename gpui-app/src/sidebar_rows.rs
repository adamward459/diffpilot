use diffpilot_core::tree::{self, FileEntry, FileFilter, FileNode, Folder, SidebarMode};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    Folder { name: String, expanded: bool },
    File { entry: Arc<FileEntry>, show_parent: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub path: String,
    pub depth: usize,
    pub kind: RowKind,
}

impl Row {
    pub fn is_folder(&self) -> bool {
        matches!(self.kind, RowKind::Folder { .. })
    }
}

pub struct RowInput<'a> {
    pub root: &'a Folder,
    pub files: &'a [Arc<FileEntry>],
    pub filter: &'a FileFilter,
    pub mode: SidebarMode,
    pub viewed: &'a HashSet<String>,
    pub collapsed: &'a HashSet<String>,
}

pub fn build_rows(input: &RowInput) -> Vec<Row> {
    match input.mode {
        SidebarMode::List => tree::list(input.files, input.filter, input.viewed)
            .into_iter()
            .map(|entry| Row {
                path: entry.path().to_string(),
                depth: 0,
                kind: RowKind::File {
                    entry,
                    show_parent: true,
                },
            })
            .collect(),
        SidebarMode::Tree => {
            let shown = tree::filter(input.root, input.filter, input.viewed);
            let expand_all = input.filter.has_query();
            let mut rows = Vec::new();
            flatten(&shown.children, 0, expand_all, input.collapsed, &mut rows);
            rows
        }
    }
}

fn flatten(nodes: &[FileNode], depth: usize, expand_all: bool, collapsed: &HashSet<String>, rows: &mut Vec<Row>) {
    for node in nodes {
        match node {
            FileNode::Folder(folder) => {
                let expanded = expand_all || !collapsed.contains(&folder.path);
                rows.push(Row {
                    path: folder.path.clone(),
                    depth,
                    kind: RowKind::Folder {
                        name: folder.name.clone(),
                        expanded,
                    },
                });
                if expanded {
                    flatten(&folder.children, depth + 1, expand_all, collapsed, rows);
                }
            }
            FileNode::File(entry) => rows.push(Row {
                path: entry.path().to_string(),
                depth,
                kind: RowKind::File {
                    entry: entry.clone(),
                    show_parent: false,
                },
            }),
        }
    }
}

pub fn index_of(rows: &[Row], path: &str) -> Option<usize> {
    rows.iter().position(|row| row.path == path)
}

pub fn step(rows: &[Row], selected: Option<&str>, delta: isize) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let last = rows.len() - 1;
    let current = selected.and_then(|path| index_of(rows, path));
    Some(match (current, delta.signum()) {
        (None, 1) => 0,
        (None, _) => last,
        (Some(index), 1) => (index + 1).min(last),
        (Some(index), _) => index.saturating_sub(1),
    })
}

pub fn parent_index(rows: &[Row], index: usize) -> Option<usize> {
    let depth = rows.get(index)?.depth;
    if depth == 0 {
        return None;
    }
    rows[..index].iter().rposition(|row| row.depth < depth)
}

pub fn scroll_target(index: usize, row_height: f32, scrolled: f32, viewport: f32) -> Option<f32> {
    let top = index as f32 * row_height;
    let bottom = top + row_height;
    if top < scrolled {
        Some(top)
    } else if bottom > scrolled + viewport {
        Some((bottom - viewport).max(0.0))
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrowOutcome {
    Collapse(String),
    Expand(String),
    Select(usize),
    Nothing,
}

pub fn arrow(rows: &[Row], selected: Option<&str>, key: Arrow, list_mode: bool) -> ArrowOutcome {
    let Some(index) = selected.and_then(|path| index_of(rows, path)) else {
        return ArrowOutcome::Nothing;
    };
    if list_mode {
        return ArrowOutcome::Nothing;
    }
    let row = &rows[index];
    match (&row.kind, key) {
        (RowKind::Folder { expanded: true, .. }, Arrow::Left) => ArrowOutcome::Collapse(row.path.clone()),
        (RowKind::Folder { expanded: false, .. }, Arrow::Right) => ArrowOutcome::Expand(row.path.clone()),
        (RowKind::Folder { expanded: true, .. }, Arrow::Right) => {
            if rows.get(index + 1).is_some_and(|next| next.depth > row.depth) {
                ArrowOutcome::Select(index + 1)
            } else {
                ArrowOutcome::Nothing
            }
        }
        (_, Arrow::Left) => parent_index(rows, index).map_or(ArrowOutcome::Nothing, ArrowOutcome::Select),
        _ => ArrowOutcome::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffpilot_core::changes::{FileChange, FileStatus};

    fn change(path: &str, uncommitted: bool) -> FileChange {
        FileChange {
            path: path.into(),
            status: FileStatus::Modified,
            additions: 1,
            deletions: 0,
            is_uncommitted: uncommitted,
        }
    }

    struct Fixture {
        root: Folder,
        files: Vec<Arc<FileEntry>>,
    }

    fn fixture() -> Fixture {
        let changes = [
            change("docs/archive.md", false),
            change("docs/Cache.md", false),
            change("src/api/client.ts", true),
            change("src/api/archive.ts", false),
            change("src/components/inbox/ArchiveToast.tsx", true),
            change("package.json", false),
        ];
        let root = tree::build(&changes);
        let files = tree::files(&root);
        Fixture { root, files }
    }

    fn rows(
        fixture: &Fixture,
        filter: &FileFilter,
        mode: SidebarMode,
        viewed: &[&str],
        collapsed: &[&str],
    ) -> Vec<Row> {
        let viewed: HashSet<String> = viewed.iter().map(|s| s.to_string()).collect();
        let collapsed: HashSet<String> = collapsed.iter().map(|s| s.to_string()).collect();
        build_rows(&RowInput {
            root: &fixture.root,
            files: &fixture.files,
            filter,
            mode,
            viewed: &viewed,
            collapsed: &collapsed,
        })
    }

    fn paths(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.path.as_str()).collect()
    }

    #[test]
    fn tree_lists_folders_first_with_every_folder_expanded() {
        let fixture = fixture();
        let rows = rows(&fixture, &FileFilter::default(), SidebarMode::Tree, &[], &[]);
        assert_eq!(
            paths(&rows),
            [
                "docs",
                "docs/archive.md",
                "docs/Cache.md",
                "src",
                "src/api",
                "src/api/archive.ts",
                "src/api/client.ts",
                "src/components/inbox",
                "src/components/inbox/ArchiveToast.tsx",
                "package.json",
            ]
        );
        let depths: Vec<usize> = rows.iter().map(|row| row.depth).collect();
        assert_eq!(depths, [0, 1, 1, 0, 1, 2, 2, 1, 2, 0]);
    }

    #[test]
    fn collapsed_folders_hide_their_children() {
        let fixture = fixture();
        let rows = rows(
            &fixture,
            &FileFilter::default(),
            SidebarMode::Tree,
            &[],
            &["docs", "src/api"],
        );
        assert_eq!(
            paths(&rows),
            [
                "docs",
                "src",
                "src/api",
                "src/components/inbox",
                "src/components/inbox/ArchiveToast.tsx",
                "package.json"
            ]
        );
        assert!(matches!(rows[0].kind, RowKind::Folder { expanded: false, .. }));
    }

    #[test]
    fn a_query_expands_everything_and_keeps_only_matches() {
        let fixture = fixture();
        let filter = FileFilter {
            query: "archive".into(),
            ..FileFilter::default()
        };
        let rows = rows(&fixture, &filter, SidebarMode::Tree, &[], &["docs", "src/api"]);
        assert_eq!(
            paths(&rows),
            [
                "docs",
                "docs/archive.md",
                "src",
                "src/api",
                "src/api/archive.ts",
                "src/components/inbox",
                "src/components/inbox/ArchiveToast.tsx",
            ]
        );
    }

    #[test]
    fn only_uncommitted_and_hide_viewed_filter_files() {
        let fixture = fixture();
        let uncommitted = FileFilter {
            only_uncommitted: true,
            ..FileFilter::default()
        };
        let rows_a = rows(&fixture, &uncommitted, SidebarMode::List, &[], &[]);
        assert_eq!(
            paths(&rows_a),
            ["src/api/client.ts", "src/components/inbox/ArchiveToast.tsx"]
        );

        let hide = FileFilter {
            hide_viewed: true,
            ..FileFilter::default()
        };
        let rows_b = rows(
            &fixture,
            &hide,
            SidebarMode::List,
            &["docs/archive.md", "package.json"],
            &[],
        );
        assert_eq!(rows_b.len(), 4);
        assert!(!paths(&rows_b).contains(&"package.json"));
    }

    #[test]
    fn list_mode_is_flat_ranked_and_shows_the_parent() {
        let fixture = fixture();
        let filter = FileFilter {
            query: "archive".into(),
            ..FileFilter::default()
        };
        let rows = rows(&fixture, &filter, SidebarMode::List, &[], &[]);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.depth == 0 && !row.is_folder()));
        assert!(
            rows.iter()
                .all(|row| matches!(row.kind, RowKind::File { show_parent: true, .. }))
        );
        assert_eq!(rows[0].path, "docs/archive.md");
    }

    #[test]
    fn stepping_clamps_at_both_ends() {
        let fixture = fixture();
        let rows = rows(&fixture, &FileFilter::default(), SidebarMode::Tree, &[], &[]);
        assert_eq!(step(&rows, None, 1), Some(0));
        assert_eq!(step(&rows, None, -1), Some(rows.len() - 1));
        assert_eq!(step(&rows, Some("docs"), 1), Some(1));
        assert_eq!(step(&rows, Some("docs"), -1), Some(0));
        assert_eq!(step(&rows, Some("package.json"), 1), Some(rows.len() - 1));
        assert_eq!(step(&[], None, 1), None);
    }

    #[test]
    fn scrolls_only_when_the_row_is_out_of_view() {
        assert_eq!(scroll_target(2, 24.0, 0.0, 240.0), None);
        assert_eq!(scroll_target(9, 24.0, 0.0, 240.0), None);
        assert_eq!(scroll_target(10, 24.0, 0.0, 240.0), Some(24.0));
        assert_eq!(scroll_target(3, 24.0, 120.0, 240.0), Some(72.0));
    }

    #[test]
    fn arrows_collapse_expand_and_climb() {
        let fixture = fixture();
        let all = rows(&fixture, &FileFilter::default(), SidebarMode::Tree, &[], &[]);
        assert_eq!(
            arrow(&all, Some("docs"), Arrow::Left, false),
            ArrowOutcome::Collapse("docs".into())
        );
        assert_eq!(arrow(&all, Some("docs"), Arrow::Right, false), ArrowOutcome::Select(1));
        assert_eq!(
            arrow(&all, Some("docs/archive.md"), Arrow::Left, false),
            ArrowOutcome::Select(0)
        );
        assert_eq!(
            arrow(&all, Some("package.json"), Arrow::Left, false),
            ArrowOutcome::Nothing
        );
        let closed = rows(&fixture, &FileFilter::default(), SidebarMode::Tree, &[], &["docs"]);
        assert_eq!(
            arrow(&closed, Some("docs"), Arrow::Right, false),
            ArrowOutcome::Expand("docs".into())
        );
        assert_eq!(arrow(&all, Some("docs"), Arrow::Left, true), ArrowOutcome::Nothing);
    }
}
