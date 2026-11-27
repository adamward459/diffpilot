//! Folders outside the repo that a guide run may read for context, linked by path.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    #[error("Enter the path of a folder.")]
    Empty,
    #[error("No folder at this path.")]
    NotFound,
    #[error("This is a file, not a folder.")]
    NotAFolder,
    #[error("This folder is inside the repo. Claude can already read it.")]
    InsideRepo,
    #[error("This folder is already linked.")]
    AlreadyLinked,
}

/// Turns what the user typed or picked into the folder to link: `~` expanded, symlinks resolved.
pub fn check(repo: &Path, linked: &[PathBuf], input: &str) -> Result<PathBuf, LinkError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(LinkError::Empty);
    }
    let path = expand_home(input, dirs::home_dir().as_deref());
    let folder = path.canonicalize().map_err(|_| LinkError::NotFound)?;
    if !folder.is_dir() {
        return Err(LinkError::NotAFolder);
    }
    let repo = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    if folder.starts_with(&repo) {
        return Err(LinkError::InsideRepo);
    }
    if linked.contains(&folder) {
        return Err(LinkError::AlreadyLinked);
    }
    Ok(folder)
}

pub fn is_path(text: &str) -> bool {
    text.starts_with('/') || text.starts_with('~')
}

pub fn expand_tilde(input: &str) -> PathBuf {
    expand_home(input, dirs::home_dir().as_deref())
}

fn expand_home(input: &str, home: Option<&Path>) -> PathBuf {
    match (input, home) {
        ("~", Some(home)) => home.to_path_buf(),
        (_, Some(home)) if input.starts_with("~/") => home.join(&input[2..]),
        _ => PathBuf::from(input),
    }
}

pub struct Added {
    pub folders: Vec<PathBuf>,
    pub refused: Vec<(PathBuf, LinkError)>,
}

/// Links every picked folder that passes `check`, and says why each of the others was refused.
pub fn add_all(repo: &Path, linked: &[PathBuf], picked: &[PathBuf]) -> Added {
    let mut folders = linked.to_vec();
    let mut refused = Vec::new();
    for path in picked {
        match check(repo, &folders, &path.to_string_lossy()) {
            Ok(folder) => folders.push(folder),
            Err(error) => refused.push((path.clone(), error)),
        }
    }
    Added { folders, refused }
}

pub fn relink(repo: &Path, linked: &[PathBuf], old: &Path, new: &Path) -> Result<Vec<PathBuf>, LinkError> {
    let others: Vec<PathBuf> = linked.iter().filter(|folder| *folder != old).cloned().collect();
    let folder = check(repo, &others, &new.to_string_lossy())?;
    Ok(linked
        .iter()
        .map(|current| {
            if current == old {
                folder.clone()
            } else {
                current.clone()
            }
        })
        .collect())
}

pub struct Completion {
    pub name: String,
    pub text: String,
    pub path: PathBuf,
}

/// Sub-folders of the typed parent that start with the typed last segment, by name. Hidden ones show only when
/// the segment starts with a dot.
pub fn complete(input: &str, limit: usize) -> Vec<Completion> {
    complete_in(input, dirs::home_dir().as_deref(), limit)
}

fn complete_in(input: &str, home: Option<&Path>, limit: usize) -> Vec<Completion> {
    let (parent, segment) = match (input, input.rfind('/')) {
        ("~", _) => ("~/", ""),
        (_, Some(slash)) => (&input[..=slash], &input[slash + 1..]),
        _ => return Vec::new(),
    };
    let dir = expand_home(parent, home);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let prefix = segment.to_lowercase();
    let mut found: Vec<Completion> = entries
        .filter_map(Result::ok)
        .filter(|entry| std::fs::metadata(entry.path()).is_ok_and(|meta| meta.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let hidden = name.starts_with('.') && !segment.starts_with('.');
            (!hidden && name.to_lowercase().starts_with(&prefix)).then(|| Completion {
                text: format!("{parent}{name}/"),
                path: entry.path(),
                name,
            })
        })
        .collect();
    found.sort_by_cached_key(|found| (found.name.to_lowercase(), found.name.clone()));
    found.truncate(limit);
    found
}

/// A short reason a completed folder cannot be linked, or `None` when it can.
pub fn blocker(repo: &Path, linked: &[PathBuf], folder: &Path) -> Option<&'static str> {
    match check(repo, linked, &folder.to_string_lossy()) {
        Ok(_) => None,
        Err(LinkError::InsideRepo) => {
            let same = folder.canonicalize().ok() == repo.canonicalize().ok();
            Some(if same { "This folder" } else { "Inside the repo" })
        }
        Err(LinkError::AlreadyLinked) => Some("Already linked"),
        Err(_) => Some("Not a folder"),
    }
}

/// The linked folders a run can still read. A folder that was moved or deleted is left out.
pub fn readable(linked: &[PathBuf]) -> Vec<PathBuf> {
    linked.iter().filter(|folder| folder.is_dir()).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        let (repo, other) = (base.join("repo"), base.join("shared"));
        std::fs::create_dir_all(repo.join("core")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(base.join("notes.txt"), "x").unwrap();
        (root, repo, other)
    }

    #[test]
    fn a_folder_outside_the_repo_links_by_its_real_path() {
        let (_root, repo, other) = folders();
        let typed = format!("  {}/../shared ", repo.display());
        assert_eq!(check(&repo, &[], &typed), Ok(other));
    }

    #[test]
    fn each_bad_path_says_what_is_wrong() {
        let (_root, repo, other) = folders();
        let base = repo.parent().unwrap();
        let path = |name: &str| base.join(name).display().to_string();
        assert_eq!(check(&repo, &[], " "), Err(LinkError::Empty));
        assert_eq!(check(&repo, &[], &path("missing")), Err(LinkError::NotFound));
        assert_eq!(check(&repo, &[], &path("notes.txt")), Err(LinkError::NotAFolder));
        assert_eq!(check(&repo, &[], &path("repo/core")), Err(LinkError::InsideRepo));
        assert_eq!(check(&repo, &[], &path("repo")), Err(LinkError::InsideRepo));
        assert_eq!(
            check(&repo, std::slice::from_ref(&other), &path("shared")),
            Err(LinkError::AlreadyLinked)
        );
    }

    #[test]
    fn a_pick_of_many_links_the_good_ones_and_names_each_refusal() {
        let (_root, repo, other) = folders();
        let base = repo.parent().unwrap();
        let (missing, inside) = (base.join("missing"), repo.join("core"));
        let picked = [other.clone(), missing.clone(), other.clone(), inside.clone()];
        let added = add_all(&repo, &[], &picked);
        assert_eq!(added.folders, [other]);
        assert_eq!(
            added.refused,
            [
                (missing, LinkError::NotFound),
                (picked[2].clone(), LinkError::AlreadyLinked),
                (inside, LinkError::InsideRepo),
            ]
        );
    }

    #[test]
    fn a_moved_folder_is_replaced_in_place_by_its_new_home() {
        let (_root, repo, other) = folders();
        let base = repo.parent().unwrap();
        let (gone, first, moved) = (base.join("gone"), base.join("first"), base.join("moved"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&moved).unwrap();
        let linked = [first.clone(), gone.clone(), other.clone()];
        assert_eq!(relink(&repo, &linked, &gone, &moved), Ok(vec![first, moved, other]));
    }

    #[test]
    fn a_new_home_that_is_not_a_good_link_changes_nothing() {
        let (_root, repo, shared) = folders();
        let base = repo.parent().unwrap();
        let gone = base.join("gone");
        let linked = [gone.clone()];
        assert_eq!(
            relink(&repo, &linked, &gone, &base.join("missing")),
            Err(LinkError::NotFound)
        );
        assert_eq!(
            relink(&repo, &linked, &gone, &repo.join("core")),
            Err(LinkError::InsideRepo)
        );
        let linked = [gone.clone(), shared.clone()];
        assert_eq!(relink(&repo, &linked, &gone, &shared), Err(LinkError::AlreadyLinked));
    }

    #[test]
    fn paths_start_with_a_slash_or_a_tilde() {
        assert!(is_path("/tmp/x"));
        assert!(is_path("~/x"));
        assert!(!is_path("alpha"));
    }

    #[test]
    fn a_tilde_means_the_home_folder() {
        let home = Path::new("/Users/me");
        assert_eq!(expand_home("~", Some(home)), home);
        assert_eq!(expand_home("~/lib", Some(home)), home.join("lib"));
        assert_eq!(expand_home("~lib", Some(home)), PathBuf::from("~lib"));
        assert_eq!(expand_home("~/lib", None), PathBuf::from("~/lib"));
    }

    fn tree() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        for name in ["beta", "Alpha", "alpine", ".hidden", ".hush", "other"] {
            std::fs::create_dir_all(base.join(name)).unwrap();
        }
        std::fs::write(base.join("alfa.txt"), "x").unwrap();
        (root, base)
    }

    fn names(found: &[Completion]) -> Vec<&str> {
        found.iter().map(|found| found.name.as_str()).collect()
    }

    #[test]
    fn completion_lists_matching_folders_by_name_and_skips_files() {
        let (_root, base) = tree();
        let typed = format!("{}/al", base.display());
        let found = complete_in(&typed, None, 6);
        assert_eq!(names(&found), ["Alpha", "alpine"]);
        assert_eq!(found[0].text, format!("{}/Alpha/", base.display()));
        assert_eq!(found[0].path, base.join("Alpha"));
        let all = complete_in(&format!("{}/", base.display()), None, 6);
        assert_eq!(names(&all), ["Alpha", "alpine", "beta", "other"]);
    }

    #[test]
    fn completion_shows_hidden_folders_only_for_a_dotted_segment() {
        let (_root, base) = tree();
        let dotted = complete_in(&format!("{}/.h", base.display()), None, 6);
        assert_eq!(names(&dotted), [".hidden", ".hush"]);
        let dot = complete_in(&format!("{}/.", base.display()), None, 6);
        assert_eq!(dot.len(), 2);
    }

    #[test]
    fn completion_stops_at_the_limit_after_sorting() {
        let (_root, base) = tree();
        let found = complete_in(&format!("{}/", base.display()), None, 2);
        assert_eq!(names(&found), ["Alpha", "alpine"]);
    }

    #[test]
    fn completion_of_a_missing_parent_or_a_plain_word_is_empty() {
        let (_root, base) = tree();
        assert!(complete_in(&format!("{}/nope/x", base.display()), None, 6).is_empty());
        assert!(complete_in("alp", None, 6).is_empty());
    }

    #[test]
    fn completion_keeps_the_tilde_the_user_typed() {
        let (_root, base) = tree();
        let found = complete_in("~/be", Some(&base), 6);
        assert_eq!(names(&found), ["beta"]);
        assert_eq!(found[0].text, "~/beta/");
        assert_eq!(
            names(&complete_in("~", Some(&base), 6)),
            ["Alpha", "alpine", "beta", "other"]
        );
    }

    #[test]
    fn a_completed_folder_says_why_it_cannot_be_linked() {
        let (_root, repo, other) = folders();
        assert_eq!(blocker(&repo, &[], &repo), Some("This folder"));
        assert_eq!(blocker(&repo, &[], &repo.join("core")), Some("Inside the repo"));
        assert_eq!(
            blocker(&repo, std::slice::from_ref(&other), &other),
            Some("Already linked")
        );
        assert_eq!(blocker(&repo, &[], &other), None);
    }

    #[test]
    fn a_folder_that_is_gone_is_not_readable() {
        let (_root, repo, other) = folders();
        let gone = repo.parent().unwrap().join("gone");
        assert_eq!(readable(&[other.clone(), gone]), [other]);
    }
}
