//! The review database: threads, comments, runs, guides and viewed files, in one sqlite file under Application Support.
//!
//! The schema follows diffity's `review_sessions`, `threads` and `comments` tables (MIT, see
//! THIRD_PARTY_NOTICES). A session is one review: repo + branch + base.

mod guides;
mod rows;
mod runs;
mod schema;
mod viewed;

pub use guides::{GuideSummary, StoredGuide};
pub use rows::{NewComment, NewThread, Written};

use crate::comparison::Comparison;
use crate::paths::standardized;
use crate::refs::Refs;
use crate::settings::app_support_dir;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("The review database failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Could not prepare {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{0} is inside a .git folder. diffpilot keeps its data outside the repo.")]
    InsideGit(PathBuf),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RunId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct GuideId(pub i64);

/// Marks stand in for the base of views that have none, so they never share a session with a branch.
pub const WORKING_TREE_BASE: &str = "@working-tree";
pub const STAGED_BASE: &str = "@staged";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub repo: String,
    pub branch: String,
    pub base: String,
}

impl SessionKey {
    pub fn new(repo: &str, branch: &str, base: &str) -> Self {
        Self {
            repo: repo.to_string(),
            branch: branch.to_string(),
            base: base.to_string(),
        }
    }

    pub fn for_comparison(repo: &Path, comparison: &Comparison, refs: Option<&Refs>) -> Self {
        let checked_out = refs.and_then(|refs| refs.checked_out.as_deref());
        let branch = comparison.branch_name().or(checked_out).unwrap_or("HEAD");
        let base = match comparison {
            Comparison::WorkingTree => WORKING_TREE_BASE,
            Comparison::Staged => STAGED_BASE,
            Comparison::CheckedOut { base, .. } | Comparison::Branch { base, .. } => base,
        };
        Self::new(&standardized(repo).to_string_lossy(), branch, base)
    }
}

pub struct Store {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("path", &self.path).finish()
    }
}

pub const FILE_NAME: &str = "review.sqlite3";

impl Store {
    pub fn default_path() -> Option<PathBuf> {
        app_support_dir().map(|dir| dir.join(FILE_NAME))
    }

    /// Refuses a path inside a `.git` folder: review state never goes into the repo.
    pub fn open(path: &Path) -> Result<Self> {
        if path.components().any(|part| part.as_os_str() == ".git") {
            return Err(StoreError::InsideGit(path.to_path_buf()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::prepare(conn, Some(path.to_path_buf()))
    }

    pub fn open_default() -> Result<Self> {
        let path = Self::default_path().ok_or_else(|| StoreError::Invalid("There is no data folder.".into()))?;
        Self::open(&path)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?, None)
    }

    fn prepare(conn: Connection, path: Option<PathBuf>) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::create(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn session(&self, key: &SessionKey) -> Result<SessionId> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO sessions (repo, branch, base, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (repo, branch, base) DO NOTHING",
            (&key.repo, &key.branch, &key.base, now_ms()),
        )?;
        let id = conn.query_row(
            "SELECT id FROM sessions WHERE repo = ?1 AND branch = ?2 AND base = ?3",
            (&key.repo, &key.branch, &key.base),
            |row| row.get(0),
        )?;
        Ok(SessionId(id))
    }

    pub fn session_key(&self, id: SessionId) -> Result<Option<SessionKey>> {
        let conn = self.lock();
        let mut statement = conn.prepare("SELECT repo, branch, base FROM sessions WHERE id = ?1")?;
        let mut rows = statement.query_map([id.0], |row| {
            Ok(SessionKey::new(
                &row.get::<_, String>(0)?,
                &row.get::<_, String>(1)?,
                &row.get::<_, String>(2)?,
            ))
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn revision(&self, id: SessionId) -> Result<i64> {
        Ok(self
            .lock()
            .query_row("SELECT revision FROM sessions WHERE id = ?1", [id.0], |row| row.get(0))?)
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_session_per_repo_branch_and_base() {
        let store = Store::open_in_memory().unwrap();
        let a = store.session(&SessionKey::new("/r", "feature", "main")).unwrap();
        let again = store.session(&SessionKey::new("/r", "feature", "main")).unwrap();
        let other_base = store.session(&SessionKey::new("/r", "feature", "develop")).unwrap();
        let other_branch = store.session(&SessionKey::new("/r", "topic", "main")).unwrap();
        assert_eq!(a, again);
        assert_ne!(a, other_base);
        assert_ne!(a, other_branch);
        assert_eq!(
            store.session_key(a).unwrap(),
            Some(SessionKey::new("/r", "feature", "main"))
        );
    }

    #[test]
    fn the_key_comes_from_the_comparison() {
        let repo = Path::new("/work/./repo");
        let key = |comparison: Comparison| SessionKey::for_comparison(repo, &comparison, None);
        assert_eq!(
            key(Comparison::branch("topic", "main")),
            SessionKey::new("/work/repo", "topic", "main")
        );
        assert_eq!(
            key(Comparison::checked_out("main", true)),
            SessionKey::new("/work/repo", "HEAD", "main")
        );
        assert_eq!(key(Comparison::WorkingTree).base, WORKING_TREE_BASE);
        assert_eq!(key(Comparison::Staged).base, STAGED_BASE);
        assert_ne!(key(Comparison::Staged), key(Comparison::WorkingTree));
    }

    #[test]
    fn refuses_a_path_inside_dot_git() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join(".git").join("diffpilot.sqlite3");
        assert!(matches!(Store::open(&inside), Err(StoreError::InsideGit(_))));
        assert!(!inside.exists());
    }

    #[test]
    fn the_default_path_is_under_application_support() {
        let path = Store::default_path().unwrap();
        assert!(path.starts_with(app_support_dir().unwrap()));
        assert!(path.components().all(|part| part.as_os_str() != ".git"));
    }

    #[test]
    fn a_file_store_keeps_its_rows_after_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(FILE_NAME);
        let key = SessionKey::new("/r", "feature", "main");
        let id = Store::open(&path).unwrap().session(&key).unwrap();
        assert_eq!(Store::open(&path).unwrap().session(&key).unwrap(), id);
    }
}
