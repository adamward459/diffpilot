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
    fn a_tilde_means_the_home_folder() {
        let home = Path::new("/Users/me");
        assert_eq!(expand_home("~", Some(home)), home);
        assert_eq!(expand_home("~/lib", Some(home)), home.join("lib"));
        assert_eq!(expand_home("~lib", Some(home)), PathBuf::from("~lib"));
        assert_eq!(expand_home("~/lib", None), PathBuf::from("~/lib"));
    }

    #[test]
    fn a_folder_that_is_gone_is_not_readable() {
        let (_root, repo, other) = folders();
        let gone = repo.parent().unwrap().join("gone");
        assert_eq!(readable(&[other.clone(), gone]), [other]);
    }
}
