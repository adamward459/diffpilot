use crate::comparison::Comparison;
use crate::paths::standardized;
use crate::refs::Refs;
use crate::settings::{app_support_dir, write_atomically};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
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

    fn storage_key(&self) -> String {
        format!("{}\n{}", self.repo, self.branch)
    }
}

pub struct ViewedStore {
    file: Option<PathBuf>,
    memory: Mutex<HashMap<String, Marks>>,
    seed: BTreeSet<String>,
}

impl ViewedStore {
    pub fn in_memory() -> Self {
        Self {
            file: None,
            memory: Mutex::default(),
            seed: BTreeSet::new(),
        }
    }

    pub fn at(file: PathBuf) -> Self {
        Self {
            file: Some(file),
            ..Self::in_memory()
        }
    }

    pub fn seeded(seed: BTreeSet<String>) -> Self {
        Self {
            seed,
            ..Self::in_memory()
        }
    }

    pub fn default_path() -> Option<PathBuf> {
        app_support_dir().map(|dir| dir.join("viewed.json"))
    }

    /// `DIFFPILOT_VIEWED_SEED` names a JSON array of paths that start as viewed, kept in memory only.
    pub fn from_env() -> Self {
        Self::from_seed_file(std::env::var("DIFFPILOT_VIEWED_SEED").ok().as_deref())
    }

    pub fn from_seed_file(seed_file: Option<&str>) -> Self {
        let Some(path) = seed_file.filter(|path| !path.is_empty()) else {
            return Self::default_path().map_or_else(Self::in_memory, Self::at);
        };
        let paths: Vec<String> = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self::seeded(paths.into_iter().collect())
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    pub fn load(&self, key: &ViewedKey) -> Marks {
        let memory = self.memory.lock().unwrap();
        match &self.file {
            None => memory.get(&key.storage_key()).cloned().unwrap_or_else(|| {
                self.seed
                    .iter()
                    .map(|path| (path.clone(), UNKNOWN.to_string()))
                    .collect()
            }),
            Some(file) => read_file(file).remove(&key.storage_key()).unwrap_or_default(),
        }
    }

    pub fn save(&self, key: &ViewedKey, marks: &Marks) {
        let mut memory = self.memory.lock().unwrap();
        let Some(file) = &self.file else {
            memory.insert(key.storage_key(), marks.clone());
            return;
        };
        let mut all = read_file(file);
        if marks.is_empty() {
            all.remove(&key.storage_key());
        } else {
            all.insert(key.storage_key(), marks.clone());
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&all) {
            let _ = write_atomically(file, &bytes);
        }
    }
}

fn read_file(file: &Path) -> BTreeMap<String, Marks> {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
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

    fn raw(file: &Path) -> BTreeMap<String, Marks> {
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
    }

    fn temp_store() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("viewed").join("viewed.json");
        (dir, file)
    }

    #[test]
    fn round_trips_through_one_file_and_creates_the_folder() {
        let (_dir, file) = temp_store();
        let k = key("/r", "main");
        ViewedStore::at(file.clone()).save(&k, &marks(&[("b.ts", "2"), ("a.ts", "1")]));
        assert_eq!(
            ViewedStore::at(file.clone()).load(&k),
            marks(&[("a.ts", "1"), ("b.ts", "2")])
        );
        assert_eq!(
            raw(&file),
            BTreeMap::from([("/r\nmain".to_string(), marks(&[("a.ts", "1"), ("b.ts", "2")]))])
        );
    }

    #[test]
    fn keeps_branches_of_one_repo_separate() {
        let (_dir, file) = temp_store();
        let store = ViewedStore::at(file);
        let (main, feature, other) = (key("/r", "main"), key("/r", "feature"), key("/other", "main"));
        store.save(&main, &marks(&[("a.ts", "1")]));
        store.save(&feature, &marks(&[("b.ts", "2")]));
        store.save(&other, &marks(&[("c.ts", "3")]));
        assert_eq!(store.load(&main), marks(&[("a.ts", "1")]));
        assert_eq!(store.load(&feature), marks(&[("b.ts", "2")]));
        assert_eq!(store.load(&other), marks(&[("c.ts", "3")]));
    }

    #[test]
    fn saving_an_empty_set_removes_only_that_entry() {
        let (_dir, file) = temp_store();
        let store = ViewedStore::at(file.clone());
        let (main, feature) = (key("/r", "main"), key("/r", "feature"));
        store.save(&main, &marks(&[("a.ts", "1")]));
        store.save(&feature, &marks(&[("b.ts", "2")]));
        store.save(&main, &marks(&[]));
        assert_eq!(
            raw(&file),
            BTreeMap::from([("/r\nfeature".to_string(), marks(&[("b.ts", "2")]))])
        );
        assert!(store.load(&main).is_empty());
    }

    #[test]
    fn a_missing_or_broken_file_loads_as_empty() {
        let (_dir, file) = temp_store();
        let k = key("/r", "main");
        assert!(ViewedStore::at(file.clone()).load(&k).is_empty());
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "not json").unwrap();
        assert!(ViewedStore::at(file).load(&k).is_empty());
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
    fn reads_the_seed_from_the_environment_without_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let seed = dir.path().join("seed.json");
        std::fs::write(&seed, r#"["a.ts"]"#).unwrap();
        let store = ViewedStore::from_seed_file(seed.to_str());
        assert_eq!(store.file(), None);
        assert_eq!(store.load(&key("/r", "main")), marks(&[("a.ts", "")]));
    }

    #[test]
    fn without_a_seed_the_store_uses_the_default_file() {
        assert_eq!(
            ViewedStore::from_seed_file(None).file(),
            ViewedStore::default_path().as_deref()
        );
        assert_eq!(
            ViewedStore::from_seed_file(Some("")).file(),
            ViewedStore::default_path().as_deref()
        );
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
