use std::path::{Component, Path, PathBuf};

pub(crate) fn standardized(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    result.push("..");
                }
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

pub(crate) fn find_executable(candidates: &[PathBuf], name: &str) -> Option<PathBuf> {
    candidates.iter().find(|path| is_executable(path)).cloned().or_else(|| {
        let search = std::env::var_os("PATH")?;
        std::env::split_paths(&search)
            .map(|dir| dir.join(name))
            .find(|path| is_executable(path))
    })
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standardizing_removes_dot_and_dotdot_components() {
        assert_eq!(standardized(Path::new("/a/b/../b/repo")), Path::new("/a/b/repo"));
        assert_eq!(standardized(Path::new("/a/./b")), Path::new("/a/b"));
    }
}
