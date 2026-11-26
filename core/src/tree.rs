use crate::changes::FileChange;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarMode {
    #[default]
    Tree,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileMarks {
    pub viewed: HashSet<String>,
    pub comments: std::collections::HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub change: FileChange,
    pub name: String,
    pub parent_path: String,
    lower_path: String,
    name_start: usize,
}

impl FileEntry {
    pub fn new(change: FileChange) -> Self {
        let slash = change.path.rfind('/');
        let lower_path = change.path.to_lowercase();
        let name_start = lower_path.rfind('/').map_or(0, |index| index + 1);
        Self {
            name: slash.map_or(change.path.clone(), |i| change.path[i + 1..].to_string()),
            parent_path: slash.map_or(String::new(), |i| change.path[..i].to_string()),
            lower_path,
            name_start,
            change,
        }
    }

    pub fn path(&self) -> &str {
        &self.change.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Folder {
    pub name: String,
    pub path: String,
    pub children: Vec<FileNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileNode {
    Folder(Folder),
    File(Arc<FileEntry>),
}

impl FileNode {
    pub fn name(&self) -> &str {
        match self {
            Self::Folder(folder) => &folder.name,
            Self::File(file) => &file.name,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Folder(folder) => &folder.path,
            Self::File(file) => file.path(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileFilter {
    pub query: String,
    pub only_uncommitted: bool,
    pub hide_viewed: bool,
}

impl FileFilter {
    pub fn trimmed_query(&self) -> &str {
        self.query.trim_matches(|c| c == ' ' || c == '\t')
    }

    pub fn has_query(&self) -> bool {
        !self.trimmed_query().is_empty()
    }

    pub fn is_active(&self) -> bool {
        self.has_query() || self.only_uncommitted || self.hide_viewed
    }
}

pub struct FileMatcher {
    terms: Vec<Vec<u8>>,
}

impl FileMatcher {
    pub fn new(query: &str) -> Option<Self> {
        let terms: Vec<Vec<u8>> = query
            .split_whitespace()
            .map(|term| term.to_lowercase().into_bytes())
            .collect();
        (!terms.is_empty()).then_some(Self { terms })
    }

    pub fn score(&self, entry: &FileEntry) -> Option<i64> {
        self.terms
            .iter()
            .try_fold(0, |total, term| Some(total + score_term(term, entry)?))
    }
}

fn score_term(needle: &[u8], entry: &FileEntry) -> Option<i64> {
    let path = entry.lower_path.as_bytes();
    if needle.len() > path.len() {
        return None;
    }
    let name_start = entry.name_start;
    let count = path.len() as i64;
    if let Some(index) = find(needle, path, name_start) {
        let bonus = if index == name_start { 500 } else { 0 };
        return Some(3000 - (index - name_start) as i64 + bonus - count);
    }
    if let Some(index) = find(needle, path, 0) {
        return Some(2000 - index as i64 - count);
    }
    fuzzy(needle, path, name_start)
}

fn find(needle: &[u8], path: &[u8], start: usize) -> Option<usize> {
    let last = path.len() - needle.len();
    (start..=last).find(|&index| path[index..].starts_with(needle))
}

fn fuzzy(needle: &[u8], path: &[u8], name_start: usize) -> Option<i64> {
    let mut matched = 0;
    let mut previous: i64 = -2;
    let mut score: i64 = 1000;
    for (index, &byte) in path.iter().enumerate() {
        if byte != needle[matched] {
            continue;
        }
        if index as i64 == previous + 1 {
            score += 15;
        }
        if index >= name_start {
            score += 5;
        }
        if index == 0 || is_separator(path[index - 1]) {
            score += 10;
        }
        previous = index as i64;
        matched += 1;
        if matched == needle.len() {
            return Some(score.min(1900) - path.len() as i64);
        }
    }
    None
}

fn is_separator(byte: u8) -> bool {
    matches!(byte, b'/' | b'.' | b'-' | b'_')
}

struct Builder<T> {
    folders: BTreeMap<String, Builder<T>>,
    files: Vec<T>,
}

impl<T> Default for Builder<T> {
    fn default() -> Self {
        Self {
            folders: BTreeMap::new(),
            files: Vec::new(),
        }
    }
}

impl<T> Builder<T> {
    fn new(items: impl IntoIterator<Item = T>, path: impl Fn(&T) -> &str) -> Self {
        let mut root = Self::default();
        for item in items {
            let parts: Vec<String> = path(&item)
                .split('/')
                .filter(|part| !part.is_empty())
                .map(String::from)
                .collect();
            let folders = parts.split_last().map_or(&[][..], |(_, folders)| folders);
            let mut builder = &mut root;
            for part in folders {
                builder = builder.folders.entry(part.clone()).or_default();
            }
            builder.files.push(item);
        }
        root
    }
}

pub fn build(changes: &[FileChange]) -> Folder {
    let entries = changes.iter().map(|change| Arc::new(FileEntry::new(change.clone())));
    let root = Builder::new(entries, |entry| entry.path());
    let folder = |name, path, children| FileNode::Folder(Folder { name, path, children });
    Folder {
        name: String::new(),
        path: String::new(),
        children: arrange(root, "", &|entry| entry.path(), &FileNode::File, &folder),
    }
}

/// The order `files(&build(..))` gives, for any items that have a path.
pub fn in_tree_order<T>(items: Vec<T>, path: impl Fn(&T) -> &str) -> Vec<T> {
    let root = Builder::new(items, &path);
    let file = |item| vec![item];
    let folder = |_, _, children: Vec<Vec<T>>| children.into_iter().flatten().collect();
    arrange(root, "", &path, &file, &folder).into_iter().flatten().collect()
}

pub fn files(root: &Folder) -> Vec<Arc<FileEntry>> {
    fn visit(nodes: &[FileNode], out: &mut Vec<Arc<FileEntry>>) {
        for node in nodes {
            match node {
                FileNode::Folder(folder) => visit(&folder.children, out),
                FileNode::File(file) => out.push(file.clone()),
            }
        }
    }
    let mut result = Vec::new();
    visit(&root.children, &mut result);
    result
}

pub fn filter(root: &Folder, filter: &FileFilter, viewed: &HashSet<String>) -> Folder {
    if !filter.is_active() {
        return root.clone();
    }
    let matcher = FileMatcher::new(&filter.query);
    fn prune(
        node: &FileNode,
        filter: &FileFilter,
        matcher: &Option<FileMatcher>,
        viewed: &HashSet<String>,
    ) -> Option<FileNode> {
        match node {
            FileNode::Folder(folder) => {
                let kept: Vec<FileNode> = folder
                    .children
                    .iter()
                    .filter_map(|child| prune(child, filter, matcher, viewed))
                    .collect();
                (!kept.is_empty()).then(|| {
                    FileNode::Folder(Folder {
                        name: folder.name.clone(),
                        path: folder.path.clone(),
                        children: kept,
                    })
                })
            }
            FileNode::File(file) => accepts(file, filter, matcher, viewed).map(|_| node.clone()),
        }
    }
    Folder {
        name: String::new(),
        path: String::new(),
        children: root
            .children
            .iter()
            .filter_map(|child| prune(child, filter, &matcher, viewed))
            .collect(),
    }
}

pub fn list(files: &[Arc<FileEntry>], filter: &FileFilter, viewed: &HashSet<String>) -> Vec<Arc<FileEntry>> {
    if !filter.is_active() {
        return files.to_vec();
    }
    let matcher = FileMatcher::new(&filter.query);
    let mut scored: Vec<(i64, &Arc<FileEntry>)> = files
        .iter()
        .filter_map(|file| accepts(file, filter, &matcher, viewed).map(|score| (score, file)))
        .collect();
    if matcher.is_some() {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path().cmp(b.1.path())));
    }
    scored.into_iter().map(|(_, file)| file.clone()).collect()
}

fn accepts(
    file: &FileEntry,
    filter: &FileFilter,
    matcher: &Option<FileMatcher>,
    viewed: &HashSet<String>,
) -> Option<i64> {
    if filter.only_uncommitted && !file.change.is_uncommitted {
        return None;
    }
    if filter.hide_viewed && viewed.contains(file.path()) {
        return None;
    }
    match matcher {
        Some(matcher) => matcher.score(file),
        None => Some(0),
    }
}

fn arrange<T, N>(
    builder: Builder<T>,
    parent_path: &str,
    path_of: &impl Fn(&T) -> &str,
    file: &impl Fn(T) -> N,
    folder: &impl Fn(String, String, Vec<N>) -> N,
) -> Vec<N> {
    let mut folders: Vec<(String, N)> = Vec::new();
    for (name, child) in builder.folders {
        let mut title = name.clone();
        let mut path = if parent_path.is_empty() {
            name
        } else {
            format!("{parent_path}/{name}")
        };
        let mut current = child;
        while current.files.is_empty() && current.folders.len() == 1 {
            let (next, nested) = current.folders.into_iter().next().expect("one folder");
            title = format!("{title}/{next}");
            path = format!("{path}/{next}");
            current = nested;
        }
        let children = arrange(current, &path, path_of, file, folder);
        folders.push((title.to_lowercase(), folder(title, path, children)));
    }
    folders.sort_by(|a, b| a.0.cmp(&b.0));
    let mut files: Vec<(String, T)> = builder
        .files
        .into_iter()
        .map(|item| {
            let path = path_of(&item);
            (path[path.rfind('/').map_or(0, |i| i + 1)..].to_lowercase(), item)
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| path_of(&a.1).cmp(path_of(&b.1))));
    folders
        .into_iter()
        .map(|(_, node)| node)
        .chain(files.into_iter().map(|(_, item)| file(item)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes::FileStatus;

    fn change(path: &str) -> FileChange {
        change_with(path, false)
    }

    fn change_with(path: &str, uncommitted: bool) -> FileChange {
        FileChange {
            path: path.into(),
            status: FileStatus::Modified,
            additions: 1,
            deletions: 0,
            is_uncommitted: uncommitted,
        }
    }

    fn build_paths(paths: &[&str]) -> Folder {
        build(&paths.iter().map(|p| change(p)).collect::<Vec<_>>())
    }

    fn names(folder: &Folder) -> Vec<&str> {
        folder.children.iter().map(FileNode::name).collect()
    }

    fn paths(folder: &Folder) -> Vec<String> {
        files(folder).iter().map(|f| f.path().to_string()).collect()
    }

    fn child<'a>(folder: &'a Folder, name: &str) -> &'a Folder {
        folder
            .children
            .iter()
            .find_map(|node| match node {
                FileNode::Folder(f) if f.name == name => Some(f),
                _ => None,
            })
            .expect("folder child")
    }

    fn no_viewed() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn groups_paths_folders_first_then_by_name() {
        let root = build_paths(&["src/b.ts", "README.md", "src/a.ts", "docs/x.md", "package.json"]);
        assert_eq!(names(&root), ["docs", "src", "package.json", "README.md"]);
        assert_eq!(names(child(&root, "src")), ["a.ts", "b.ts"]);
        assert_eq!(child(&root, "docs").path, "docs");
    }

    #[test]
    fn merges_single_child_folder_chains() {
        let root = build_paths(&["src/components/inbox/Toast.tsx", "src/components/inbox/Row.tsx"]);
        assert_eq!(names(&root), ["src/components/inbox"]);
        let merged = child(&root, "src/components/inbox");
        assert_eq!(merged.path, "src/components/inbox");
        assert_eq!(names(merged), ["Row.tsx", "Toast.tsx"]);
    }

    #[test]
    fn keeps_folder_when_it_also_holds_files_or_several_folders() {
        let root = build_paths(&["src/index.ts", "src/lib/a.ts", "src/api/b.ts"]);
        assert_eq!(names(&root), ["src"]);
        assert_eq!(names(child(&root, "src")), ["api", "lib", "index.ts"]);
    }

    #[test]
    fn file_nodes_expose_name_and_parent_path() {
        let root = build_paths(&["src/api/archive.ts"]);
        let all = files(&root);
        assert_eq!(all[0].name, "archive.ts");
        assert_eq!(all[0].parent_path, "src/api");
    }

    fn filter_sample() -> Folder {
        build_paths(&[
            "src/hooks/useArchiveUndo.ts",
            "src/api/client.ts",
            "tests/hooks/useArchiveUndo.test.ts",
            "docs/archive.md",
            "package.json",
        ])
    }

    fn query(text: &str) -> FileFilter {
        FileFilter {
            query: text.into(),
            ..FileFilter::default()
        }
    }

    #[test]
    fn keeps_matching_files_and_their_ancestors_only() {
        let result = filter(&filter_sample(), &query("client"), &no_viewed());
        assert_eq!(paths(&result), ["src/api/client.ts"]);
        assert_eq!(names(&result), ["src"]);
        assert_eq!(names(child(&result, "src")), ["api"]);
    }

    #[test]
    fn match_is_case_insensitive_on_the_full_path() {
        let result = filter(&filter_sample(), &query("TESTS/hooks"), &no_viewed());
        assert_eq!(paths(&result), ["tests/hooks/useArchiveUndo.test.ts"]);
    }

    #[test]
    fn each_word_must_match_in_any_order() {
        let result = filter(&filter_sample(), &query("undo tests"), &no_viewed());
        assert_eq!(paths(&result), ["tests/hooks/useArchiveUndo.test.ts"]);
        let result = filter(&filter_sample(), &query("  hooks   undo "), &no_viewed());
        assert_eq!(
            paths(&result),
            ["src/hooks/useArchiveUndo.ts", "tests/hooks/useArchiveUndo.test.ts"]
        );
    }

    #[test]
    fn empty_query_keeps_everything() {
        let result = filter(&filter_sample(), &FileFilter::default(), &no_viewed());
        assert_eq!(paths(&result).len(), 5);
    }

    #[test]
    fn no_match_leaves_an_empty_tree() {
        assert!(
            filter(&filter_sample(), &query("zzzz"), &no_viewed())
                .children
                .is_empty()
        );
    }

    #[test]
    fn fuzzy_subsequence_matches() {
        let result = filter(&filter_sample(), &query("uau"), &no_viewed());
        assert_eq!(
            paths(&result),
            ["src/hooks/useArchiveUndo.ts", "tests/hooks/useArchiveUndo.test.ts"]
        );
    }

    #[test]
    fn fuzzy_requires_characters_in_order() {
        assert!(
            !filter(&filter_sample(), &query("udo"), &no_viewed())
                .children
                .is_empty()
        );
        assert!(
            filter(&filter_sample(), &query("odu"), &no_viewed())
                .children
                .is_empty()
        );
    }

    fn toggle_sample() -> Folder {
        build(&[
            change_with("a/one.ts", true),
            change_with("a/two.ts", true),
            change_with("b/three.ts", false),
            change("b/four.ts"),
        ])
    }

    fn viewed() -> HashSet<String> {
        ["a/one.ts", "b/three.ts"].map(String::from).into()
    }

    #[test]
    fn only_uncommitted_drops_committed_files_and_empty_folders() {
        let toggle = FileFilter {
            only_uncommitted: true,
            ..FileFilter::default()
        };
        let result = filter(&toggle_sample(), &toggle, &no_viewed());
        assert_eq!(paths(&result), ["a/one.ts", "a/two.ts"]);
        assert_eq!(names(&result), ["a"]);
    }

    #[test]
    fn hide_viewed_drops_viewed_files() {
        let toggle = FileFilter {
            hide_viewed: true,
            ..FileFilter::default()
        };
        let result = filter(&toggle_sample(), &toggle, &viewed());
        assert_eq!(paths(&result), ["a/two.ts", "b/four.ts"]);
    }

    #[test]
    fn toggles_combine_with_query() {
        let combined = FileFilter {
            query: "t".into(),
            only_uncommitted: true,
            hide_viewed: true,
        };
        let result = filter(&toggle_sample(), &combined, &viewed());
        assert_eq!(paths(&result), ["a/two.ts"]);
    }

    fn listed(files: &[Arc<FileEntry>]) -> Vec<&str> {
        files.iter().map(|f| f.path()).collect()
    }

    #[test]
    fn ranks_filename_and_contiguous_matches_first() {
        let root = build_paths(&[
            "src/archive/zzz.ts",
            "src/lib/a_r_c_h.ts",
            "docs/archive.md",
            "src/api/archive.ts",
        ]);
        let result = list(&files(&root), &query("archive"), &no_viewed());
        let result = listed(&result);
        let mut top: Vec<_> = result[..2].to_vec();
        top.sort();
        assert_eq!(top, ["docs/archive.md", "src/api/archive.ts"]);
        assert_eq!(result.last(), Some(&"src/archive/zzz.ts"));
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn empty_query_keeps_order_and_applies_toggles() {
        let all = files(&build_paths(&["a.ts", "b.ts"]));
        assert_eq!(
            listed(&list(&all, &FileFilter::default(), &no_viewed())),
            ["a.ts", "b.ts"]
        );
        let hide = FileFilter {
            hide_viewed: true,
            ..FileFilter::default()
        };
        let viewed = HashSet::from(["a.ts".to_string()]);
        assert_eq!(listed(&list(&all, &hide, &viewed)), ["b.ts"]);
    }

    #[test]
    fn filtering_ten_thousand_files_stays_fast() {
        let changes: Vec<FileChange> = (0..10_000)
            .map(|i| change(&format!("src/area{}/sub{}/File{i}.ts", i % 40, i % 7)))
            .collect();
        let started = std::time::Instant::now();
        let root = build(&changes);
        let all = files(&root);
        let tree = filter(&root, &query("uau"), &no_viewed());
        let list = list(&all, &query("uau"), &no_viewed());
        assert_eq!(all.len(), 10_000);
        assert_eq!(files(&tree).len(), list.len());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
