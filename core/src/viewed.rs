use crate::comparison::Comparison;
use crate::paths::standardized;
use crate::refs::Refs;
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Stored in place of a fingerprint when the diff was not known at mark time.
pub const UNKNOWN: &str = "";

type Marks = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewedKey {
    pub repo: String,
    pub branch: String,
}

impl ViewedKey {
    pub fn new(repo: &str, branch: &str) -> Self {
        Self {
            repo: repo.to_string(),
            branch: branch.to_string(),
        }
    }

    pub fn for_comparison(repo: &Path, comparison: &Comparison, refs: Option<&Refs>) -> Self {
        let branch = comparison
            .branch_name()
            .or_else(|| refs.and_then(|refs| refs.checked_out.as_deref()))
            .unwrap_or("HEAD");
        Self::new(&standardized(repo).to_string_lossy(), branch)
    }
}

pub struct ViewedStore(Backing);

enum Backing {
    Memory {
        marks: Mutex<HashMap<ViewedKey, Marks>>,
        seed: BTreeSet<String>,
    },
    Database(Arc<Store>),
}

impl ViewedStore {
    pub fn in_memory() -> Self {
        Self::seeded(BTreeSet::new())
    }

    pub fn seeded(seed: BTreeSet<String>) -> Self {
        Self(Backing::Memory {
            marks: Mutex::default(),
            seed,
        })
    }

    pub fn in_database(store: Arc<Store>) -> Self {
        Self(Backing::Database(store))
    }

    /// `DIFFPILOT_VIEWED_SEED` names a JSON array of paths that start as viewed, kept in memory only.
    pub fn from_env(store: Arc<Store>) -> Self {
        Self::from_seed_file(std::env::var("DIFFPILOT_VIEWED_SEED").ok().as_deref(), store)
    }

    pub fn from_seed_file(seed_file: Option<&str>, store: Arc<Store>) -> Self {
        let Some(path) = seed_file.filter(|path| !path.is_empty()) else {
            return Self::in_database(store);
        };
        let paths: Vec<String> = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self::seeded(paths.into_iter().collect())
    }

    pub fn load(&self, key: &ViewedKey) -> Marks {
        match &self.0 {
            Backing::Memory { marks, seed } => marks
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .unwrap_or_else(|| seed.iter().map(|path| (path.clone(), UNKNOWN.to_string())).collect()),
            Backing::Database(store) => store.viewed(key).unwrap_or_default(),
        }
    }

    pub fn save(&self, key: &ViewedKey, marks: &Marks) {
        match &self.0 {
            Backing::Memory { marks: memory, .. } => {
                memory.lock().unwrap().insert(key.clone(), marks.clone());
            }
            Backing::Database(store) => {
                if let Err(error) = store.set_viewed(key, marks) {
                    eprintln!("diffpilot: {error}. Viewed files were not saved.");
                }
            }
        }
    }
}

pub struct ViewedFiles {
    fingerprints: Marks,
    key: ViewedKey,
    store: Arc<ViewedStore>,
}

impl ViewedFiles {
    pub fn new(key: ViewedKey, store: Arc<ViewedStore>) -> Self {
        Self {
            fingerprints: store.load(&key),
            key,
            store,
        }
    }

    pub fn paths(&self) -> HashSet<String> {
        self.fingerprints.keys().cloned().collect()
    }

    pub fn contains(&self, path: &str) -> bool {
        self.fingerprints.contains_key(path)
    }

    /// `fingerprint` is the file's current diff fingerprint, or `None` when it is not known yet.
    pub fn toggle(&mut self, path: &str, fingerprint: Option<&str>) {
        if self.fingerprints.remove(path).is_none() {
            self.fingerprints
                .insert(path.to_string(), fingerprint.unwrap_or(UNKNOWN).to_string());
        }
        self.store.save(&self.key, &self.fingerprints);
    }

    /// Unmarks files whose diff changed and returns them sorted.
    /// A mark saved without a fingerprint takes the current one instead of clearing.
    pub fn clear_changed(&mut self, current: &HashMap<String, String>) -> Vec<String> {
        let mut next = self.fingerprints.clone();
        let mut cleared = Vec::new();
        for (path, saved) in &self.fingerprints {
            let Some(now) = current.get(path).filter(|now| *now != saved) else {
                continue;
            };
            if saved == UNKNOWN {
                next.insert(path.clone(), now.clone());
            } else {
                next.remove(path);
                cleared.push(path.clone());
            }
        }
        if next == self.fingerprints {
            return Vec::new();
        }
        self.store.save(&self.key, &next);
        self.fingerprints = next;
        cleared.sort();
        cleared
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(pairs: &[(&str, &str)]) -> Marks {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn key(repo: &str, branch: &str) -> ViewedKey {
        ViewedKey::new(repo, branch)
    }

    fn database() -> Arc<Store> {
        Arc::new(Store::open_in_memory().unwrap())
    }

    #[test]
    fn marks_survive_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("review.sqlite3");
        let k = key("/r", "main");
        let open = || ViewedStore::in_database(Arc::new(Store::open(&path).unwrap()));
        open().save(&k, &marks(&[("b.ts", "2"), ("a.ts", "1")]));
        assert_eq!(open().load(&k), marks(&[("a.ts", "1"), ("b.ts", "2")]));
    }

    #[test]
    fn keeps_branches_of_one_repo_separate() {
        let store = ViewedStore::in_database(database());
        let (main, feature, other) = (key("/r", "main"), key("/r", "feature"), key("/other", "main"));
        store.save(&main, &marks(&[("a.ts", "1")]));
        store.save(&feature, &marks(&[("b.ts", "2")]));
        store.save(&other, &marks(&[("c.ts", "3")]));
        assert_eq!(store.load(&main), marks(&[("a.ts", "1")]));
        assert_eq!(store.load(&feature), marks(&[("b.ts", "2")]));
        assert_eq!(store.load(&other), marks(&[("c.ts", "3")]));
    }

    #[test]
    fn saving_replaces_the_set_for_that_key_only() {
        let store = ViewedStore::in_database(database());
        let (main, feature) = (key("/r", "main"), key("/r", "feature"));
        store.save(&main, &marks(&[("a.ts", "1"), ("b.ts", "1")]));
        store.save(&feature, &marks(&[("b.ts", "2")]));
        store.save(&main, &marks(&[("b.ts", "3")]));
        assert_eq!(store.load(&main), marks(&[("b.ts", "3")]));
        store.save(&main, &marks(&[]));
        assert!(store.load(&main).is_empty());
        assert_eq!(store.load(&feature), marks(&[("b.ts", "2")]));
    }

    #[test]
    fn a_store_without_a_file_keeps_everything_in_memory() {
        let store = ViewedStore::in_memory();
        let k = key("/r", "main");
        store.save(&k, &marks(&[("a.ts", "1")]));
        assert_eq!(store.load(&k), marks(&[("a.ts", "1")]));
        assert!(ViewedStore::in_memory().load(&k).is_empty());
    }

    #[test]
    fn a_seeded_store_answers_every_key_until_it_is_written() {
        let store = ViewedStore::seeded(["a.ts".to_string(), "b.ts".to_string()].into());
        let (one, two) = (key("/r", "main"), key("/s", "dev"));
        assert_eq!(store.load(&one), marks(&[("a.ts", ""), ("b.ts", "")]));
        assert_eq!(store.load(&two), marks(&[("a.ts", ""), ("b.ts", "")]));
        store.save(&one, &marks(&[]));
        assert!(store.load(&one).is_empty());
        assert_eq!(store.load(&two), marks(&[("a.ts", ""), ("b.ts", "")]));
    }

    #[test]
    fn a_seed_file_keeps_marks_out_of_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let seed = dir.path().join("seed.json");
        std::fs::write(&seed, r#"["a.ts"]"#).unwrap();
        let db = database();
        let store = ViewedStore::from_seed_file(seed.to_str(), db.clone());
        let k = key("/r", "main");
        assert_eq!(store.load(&k), marks(&[("a.ts", "")]));
        store.save(&k, &marks(&[("b.ts", "1")]));
        assert!(db.viewed(&k).unwrap().is_empty());
    }

    #[test]
    fn without_a_seed_the_store_uses_the_database() {
        for seed in [None, Some("")] {
            let db = database();
            let k = key("/r", "main");
            ViewedStore::from_seed_file(seed, db.clone()).save(&k, &marks(&[("a.ts", "1")]));
            assert_eq!(db.viewed(&k).unwrap(), marks(&[("a.ts", "1")]));
        }
    }

    #[test]
    fn builds_the_key_from_the_comparison_branch_then_the_checked_out_branch() {
        let repo = Path::new("/a/b/../b/repo");
        let refs = Refs {
            checked_out: Some("feature".into()),
            detected_base: "main".into(),
            ..Refs::default()
        };
        let checked_out = Comparison::checked_out("main", true);
        assert_eq!(
            ViewedKey::for_comparison(repo, &checked_out, Some(&refs)).branch,
            "feature"
        );
        assert_eq!(
            ViewedKey::for_comparison(repo, &Comparison::branch("topic", "main"), Some(&refs)).branch,
            "topic"
        );
        assert_eq!(ViewedKey::for_comparison(repo, &checked_out, None).branch, "HEAD");
        assert_eq!(
            ViewedKey::for_comparison(repo, &checked_out, Some(&refs)).repo,
            "/a/b/repo"
        );
    }

    fn viewed(store: &Arc<ViewedStore>) -> ViewedFiles {
        ViewedFiles::new(key("/r", "main"), store.clone())
    }

    fn current(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        marks(pairs).into_iter().collect()
    }

    #[test]
    fn toggles_and_writes_through_on_every_change() {
        let store = Arc::new(ViewedStore::in_memory());
        let mut files = viewed(&store);
        files.toggle("a.ts", None);
        assert!(files.contains("a.ts"));
        assert_eq!(store.load(&key("/r", "main")), marks(&[("a.ts", "")]));
        files.toggle("a.ts", None);
        assert!(!files.contains("a.ts"));
        assert!(store.load(&key("/r", "main")).is_empty());
    }

    #[test]
    fn loads_the_stored_set_for_its_key_only() {
        let store = Arc::new(ViewedStore::in_memory());
        store.save(&key("/r", "main"), &marks(&[("a.ts", "1")]));
        assert_eq!(viewed(&store).paths(), HashSet::from(["a.ts".to_string()]));
        assert!(ViewedFiles::new(key("/r", "other"), store).paths().is_empty());
    }

    #[test]
    fn keeps_a_mark_whose_diff_is_the_same() {
        let store = Arc::new(ViewedStore::in_memory());
        let mut files = viewed(&store);
        files.toggle("a.ts", Some("1"));
        assert!(files.clear_changed(&current(&[("a.ts", "1")])).is_empty());
        assert!(files.contains("a.ts"));
    }

    #[test]
    fn clears_a_mark_whose_diff_changed_and_saves_it() {
        let store = Arc::new(ViewedStore::in_memory());
        let mut files = viewed(&store);
        files.toggle("a.ts", Some("1"));
        files.toggle("b.ts", Some("2"));
        assert_eq!(
            files.clear_changed(&current(&[("a.ts", "changed"), ("b.ts", "2")])),
            ["a.ts"]
        );
        assert_eq!(files.paths(), HashSet::from(["b.ts".to_string()]));
        assert_eq!(store.load(&key("/r", "main")), marks(&[("b.ts", "2")]));
    }

    #[test]
    fn a_mark_without_a_fingerprint_takes_the_current_one() {
        let store = Arc::new(ViewedStore::seeded(["a.ts".to_string()].into()));
        let mut files = viewed(&store);
        assert!(files.clear_changed(&current(&[("a.ts", "1")])).is_empty());
        assert_eq!(store.load(&key("/r", "main")), marks(&[("a.ts", "1")]));
        assert_eq!(files.clear_changed(&current(&[("a.ts", "2")])), ["a.ts"]);
    }

    #[test]
    fn keeps_marks_for_files_that_left_the_diff() {
        let store = Arc::new(ViewedStore::in_memory());
        let mut files = viewed(&store);
        files.toggle("gone.ts", Some("1"));
        assert!(files.clear_changed(&HashMap::new()).is_empty());
        assert!(files.contains("gone.ts"));
    }
}
