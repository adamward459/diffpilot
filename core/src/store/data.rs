use super::{Result, Store};
use crate::paths::standardized;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataScope {
    All,
    Repo(String),
    Branch { repo: String, branch: String },
}

impl DataScope {
    pub fn covers(&self, repo: &str, branch: &str) -> bool {
        match self {
            Self::All => true,
            Self::Repo(scope) => scope == repo,
            Self::Branch { repo: r, branch: b } => r == repo && b == branch,
        }
    }

    /// True when deleting this scope removes something of the repo in `folder`, whatever branch it shows.
    pub fn touches_folder(&self, folder: &Path) -> bool {
        match self {
            Self::All => true,
            Self::Repo(repo) | Self::Branch { repo, .. } => Path::new(repo) == standardized(folder),
        }
    }
}

/// What is stored for one branch of a repo, across every base it was compared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchData {
    pub branch: String,
    pub threads: u32,
    pub comments: u32,
    pub runs: u32,
    pub guides: u32,
    pub viewed_files: u32,
    pub last_activity_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoData {
    pub repo: String,
    pub branches: Vec<BranchData>,
}

impl RepoData {
    pub fn total(&self, count: impl Fn(&BranchData) -> u32) -> u32 {
        self.branches.iter().map(count).sum()
    }

    pub fn last_activity_ms(&self) -> i64 {
        self.branches
            .iter()
            .map(|branch| branch.last_activity_ms)
            .max()
            .unwrap_or(0)
    }
}

const BRANCHES: &str = "
    WITH keys (repo, branch) AS (SELECT repo, branch FROM sessions UNION SELECT repo, branch FROM viewed)
    SELECT k.repo, k.branch,
      (SELECT COUNT(*) FROM threads t JOIN sessions s ON s.id = t.session_id
        WHERE s.repo = k.repo AND s.branch = k.branch),
      (SELECT COUNT(*) FROM comments c JOIN threads t ON t.id = c.thread_id JOIN sessions s ON s.id = t.session_id
        WHERE s.repo = k.repo AND s.branch = k.branch),
      (SELECT COUNT(*) FROM runs r JOIN sessions s ON s.id = r.session_id
        WHERE s.repo = k.repo AND s.branch = k.branch),
      (SELECT COUNT(*) FROM guides g JOIN sessions s ON s.id = g.session_id
        WHERE s.repo = k.repo AND s.branch = k.branch),
      (SELECT COUNT(*) FROM viewed v WHERE v.repo = k.repo AND v.branch = k.branch),
      MAX(
        COALESCE((SELECT MAX(created_at) FROM sessions s WHERE s.repo = k.repo AND s.branch = k.branch), 0),
        COALESCE((SELECT MAX(t.updated_at) FROM threads t JOIN sessions s ON s.id = t.session_id
          WHERE s.repo = k.repo AND s.branch = k.branch), 0),
        COALESCE((SELECT MAX(r.queued_at) FROM runs r JOIN sessions s ON s.id = r.session_id
          WHERE s.repo = k.repo AND s.branch = k.branch), 0),
        COALESCE((SELECT MAX(g.created_at) FROM guides g JOIN sessions s ON s.id = g.session_id
          WHERE s.repo = k.repo AND s.branch = k.branch), 0)
      )
    FROM keys k";

impl Store {
    /// Repos that have stored data, newest activity first. A branch with only an empty review session is left out.
    pub fn projects(&self) -> Result<Vec<RepoData>> {
        let conn = self.lock();
        let mut statement = conn.prepare(BRANCHES)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                BranchData {
                    branch: row.get(1)?,
                    threads: row.get(2)?,
                    comments: row.get(3)?,
                    runs: row.get(4)?,
                    guides: row.get(5)?,
                    viewed_files: row.get(6)?,
                    last_activity_ms: row.get(7)?,
                },
            ))
        })?;
        let mut repos: Vec<RepoData> = Vec::new();
        for row in rows {
            let (repo, branch) = row?;
            if branch.threads + branch.runs + branch.guides + branch.viewed_files == 0 {
                continue;
            }
            match repos.iter_mut().find(|known| known.repo == repo) {
                Some(known) => known.branches.push(branch),
                None => repos.push(RepoData {
                    repo,
                    branches: vec![branch],
                }),
            }
        }
        for repo in &mut repos {
            repo.branches.sort_by(|a, b| {
                b.last_activity_ms
                    .cmp(&a.last_activity_ms)
                    .then_with(|| a.branch.cmp(&b.branch))
            });
        }
        repos.sort_by(|a, b| {
            b.last_activity_ms()
                .cmp(&a.last_activity_ms())
                .then_with(|| a.repo.cmp(&b.repo))
        });
        Ok(repos)
    }

    pub fn delete_branch(&self, repo: &str, branch: &str) -> Result<()> {
        self.delete(&DataScope::Branch {
            repo: repo.to_string(),
            branch: branch.to_string(),
        })
    }

    pub fn delete_repo(&self, repo: &str) -> Result<()> {
        self.delete(&DataScope::Repo(repo.to_string()))
    }

    pub fn delete_all(&self) -> Result<()> {
        self.delete(&DataScope::All)
    }

    /// Threads, comments, runs and guides go with their sessions; viewed marks have no session, so they go by key.
    pub fn delete(&self, scope: &DataScope) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        match scope {
            DataScope::All => {
                tx.execute("DELETE FROM sessions", [])?;
                tx.execute("DELETE FROM viewed", [])?;
            }
            DataScope::Repo(repo) => {
                tx.execute("DELETE FROM sessions WHERE repo = ?1", [repo])?;
                tx.execute("DELETE FROM viewed WHERE repo = ?1", [repo])?;
            }
            DataScope::Branch { repo, branch } => {
                tx.execute("DELETE FROM sessions WHERE repo = ?1 AND branch = ?2", (repo, branch))?;
                tx.execute("DELETE FROM viewed WHERE repo = ?1 AND branch = ?2", (repo, branch))?;
            }
        }
        Ok(tx.commit()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::RunKind;
    use crate::store::{NewComment, NewThread, SessionKey};
    use crate::threads::{CommentSource, ThreadAnchor};
    use crate::viewed::ViewedKey;
    use std::collections::BTreeMap;

    fn comment(body: &str) -> NewComment {
        NewComment {
            author: "me".into(),
            source: CommentSource::User,
            body: body.into(),
            streaming: false,
            pending: false,
        }
    }

    fn add_thread(store: &Store, key: &SessionKey, comments: &[&str]) {
        let session = store.session(key).unwrap();
        let mut thread = NewThread::at(ThreadAnchor::file("a.rs"));
        thread.first_comment = Some(comment(comments[0]));
        let thread = store.insert_thread(session, &thread).unwrap().value;
        for body in &comments[1..] {
            store.insert_comment(thread.id, &comment(body)).unwrap().unwrap();
        }
    }

    fn mark_viewed(store: &Store, repo: &str, branch: &str, paths: &[&str]) {
        let marks: BTreeMap<String, String> = paths.iter().map(|path| ((*path).into(), "f".into())).collect();
        store.set_viewed(&ViewedKey::new(repo, branch), &marks).unwrap();
    }

    fn filled() -> Store {
        let store = Store::open_in_memory().unwrap();
        let topic = SessionKey::new("/a", "topic", "main");
        add_thread(&store, &topic, &["one", "two"]);
        add_thread(&store, &SessionKey::new("/a", "topic", "develop"), &["three"]);
        add_thread(&store, &SessionKey::new("/a", "other", "main"), &["four"]);
        add_thread(&store, &SessionKey::new("/b", "topic", "main"), &["five"]);
        let session = store.session(&topic).unwrap();
        store.insert_run(session, RunKind::Review, "Review").unwrap();
        store
            .lock()
            .execute(
                "INSERT INTO guides (session_id, state, fingerprint, created_at, data)
                 VALUES (?1, 'done', 'f', 1, '{}')",
                [session.0],
            )
            .unwrap();
        mark_viewed(&store, "/a", "topic", &["x", "y", "z"]);
        mark_viewed(&store, "/c", "only-viewed", &["x"]);
        store
    }

    fn row<'a>(projects: &'a [RepoData], repo: &str, branch: &str) -> Option<&'a BranchData> {
        projects
            .iter()
            .find(|known| known.repo == repo)?
            .branches
            .iter()
            .find(|known| known.branch == branch)
    }

    fn count(store: &Store, table: &str) -> i64 {
        store
            .lock()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn projects_count_each_branch_across_its_bases() {
        let projects = filled().projects().unwrap();
        let topic = row(&projects, "/a", "topic").unwrap();
        assert_eq!(
            (
                topic.threads,
                topic.comments,
                topic.runs,
                topic.guides,
                topic.viewed_files
            ),
            (2, 3, 1, 1, 3)
        );
        let other = row(&projects, "/a", "other").unwrap();
        assert_eq!((other.threads, other.comments, other.viewed_files), (1, 1, 0));
        assert!(
            row(&projects, "/c", "only-viewed").is_some(),
            "viewed marks alone count as data"
        );
        let repo = projects.iter().find(|repo| repo.repo == "/a").unwrap();
        assert_eq!(repo.total(|branch| branch.threads), 3);
    }

    #[test]
    fn a_branch_with_an_empty_session_is_not_listed() {
        let store = Store::open_in_memory().unwrap();
        store.session(&SessionKey::new("/a", "empty", "main")).unwrap();
        assert!(store.projects().unwrap().is_empty());
    }

    #[test]
    fn the_newest_repo_and_branch_come_first() {
        let store = Store::open_in_memory().unwrap();
        let activity = [
            ("/old", "x", 10),
            ("/new", "x", 30),
            ("/new", "y", 40),
            ("/new", "z", 20),
        ];
        for (repo, branch, _) in activity {
            add_thread(&store, &SessionKey::new(repo, branch, "main"), &["c"]);
        }
        store.lock().execute("UPDATE sessions SET created_at = 1", []).unwrap();
        for (repo, branch, at) in activity {
            store
                .lock()
                .execute(
                    "UPDATE threads SET updated_at = ?1
                     WHERE session_id = (SELECT id FROM sessions WHERE repo = ?2 AND branch = ?3)",
                    (at, repo, branch),
                )
                .unwrap();
        }
        let projects = store.projects().unwrap();
        assert_eq!(projects[0].repo, "/new");
        let order: Vec<_> = projects[0].branches.iter().map(|b| b.branch.as_str()).collect();
        assert_eq!(order, ["y", "x", "z"]);
        assert_eq!(projects[0].last_activity_ms(), 40);
    }

    #[test]
    fn deleting_a_branch_removes_everything_of_it_and_nothing_else() {
        let store = filled();
        store.delete_branch("/a", "topic").unwrap();
        let projects = store.projects().unwrap();
        assert!(row(&projects, "/a", "topic").is_none());
        assert!(row(&projects, "/a", "other").is_some());
        assert!(row(&projects, "/b", "topic").is_some());
        assert_eq!((count(&store, "runs"), count(&store, "guides")), (0, 0));
        assert_eq!(count(&store, "comments"), 2);
        assert_eq!(count(&store, "viewed"), 1);
    }

    #[test]
    fn deleting_a_repo_removes_all_its_branches() {
        let store = filled();
        store.delete_repo("/a").unwrap();
        let repos: Vec<_> = store.projects().unwrap().into_iter().map(|repo| repo.repo).collect();
        assert_eq!(repos.len(), 2);
        assert!(repos.contains(&"/b".to_string()) && repos.contains(&"/c".to_string()));
    }

    #[test]
    fn deleting_all_empties_every_table() {
        let store = filled();
        store.delete_all().unwrap();
        for table in ["sessions", "threads", "comments", "runs", "guides", "viewed"] {
            assert_eq!(count(&store, table), 0, "{table}");
        }
        assert!(store.projects().unwrap().is_empty());
    }

    #[test]
    fn a_new_comment_works_after_its_branch_was_deleted() {
        let store = filled();
        let key = SessionKey::new("/a", "topic", "main");
        store.delete_branch("/a", "topic").unwrap();
        add_thread(&store, &key, &["again"]);
        assert_eq!(row(&store.projects().unwrap(), "/a", "topic").unwrap().threads, 1);
    }

    #[test]
    fn the_scope_covers_what_it_names() {
        assert!(DataScope::All.covers("/a", "x"));
        assert!(DataScope::Repo("/a".into()).covers("/a", "x"));
        assert!(!DataScope::Repo("/a".into()).covers("/b", "x"));
        let branch = DataScope::Branch {
            repo: "/a".into(),
            branch: "x".into(),
        };
        assert!(branch.covers("/a", "x"));
        assert!(!branch.covers("/a", "y"));
        assert!(branch.touches_folder(Path::new("/a/./")));
        assert!(!branch.touches_folder(Path::new("/b")));
        assert!(DataScope::All.touches_folder(Path::new("/b")));
    }
}
