use crate::comparison::Comparison;
use crate::policy::EditPermission;
use crate::review::ReviewModel;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    recent_folder: Option<PathBuf>,
    comparisons: BTreeMap<String, Comparison>,
    review_model: Option<ReviewModel>,
    review_focuses: BTreeMap<String, Vec<String>>,
    edit_permission: Option<EditPermission>,
}

pub struct Settings {
    file: Option<PathBuf>,
    data: Mutex<Data>,
    unreadable: AtomicBool,
}

pub fn app_support_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("diffpilot"))
}

impl Settings {
    pub fn in_memory() -> Self {
        Self {
            file: None,
            data: Mutex::default(),
            unreadable: AtomicBool::new(false),
        }
    }

    /// A missing file starts empty. A value this build cannot read is dropped and the
    /// others are kept. A file that is not a JSON object is moved aside before the first write.
    pub fn at(file: PathBuf) -> Self {
        let (data, unreadable) = match std::fs::read(&file) {
            Err(_) => (Data::default(), false),
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(fields) => (Data::decode(fields), false),
                Err(_) => (Data::default(), true),
            },
        };
        Self {
            file: Some(file),
            data: Mutex::new(data),
            unreadable: AtomicBool::new(unreadable),
        }
    }

    pub fn open_default() -> Self {
        match app_support_dir() {
            Some(dir) => Self::at(dir.join("settings.json")),
            None => Self::in_memory(),
        }
    }

    /// `DIFFPILOT_REPO` wins over the saved folder.
    pub fn recent_folder(&self) -> Option<PathBuf> {
        let env = std::env::var("DIFFPILOT_REPO").ok();
        self.recent_folder_with(env.as_deref())
    }

    pub fn recent_folder_with(&self, env_override: Option<&str>) -> Option<PathBuf> {
        match env_override.filter(|path| !path.is_empty()) {
            Some(path) => Some(PathBuf::from(path)),
            None => self.read(|data| data.recent_folder.clone()),
        }
    }

    pub fn remember_folder(&self, folder: &Path) {
        self.write(|data| data.recent_folder = Some(folder.to_path_buf()));
    }

    pub fn comparison(&self, key: &str) -> Option<Comparison> {
        self.read(|data| data.comparisons.get(key).cloned())
    }

    pub fn set_comparison(&self, key: &str, comparison: &Comparison) {
        self.write(|data| {
            data.comparisons.insert(key.to_string(), comparison.clone());
        });
    }

    pub fn review_model(&self) -> ReviewModel {
        self.read(|data| data.review_model).unwrap_or(ReviewModel::STANDARD)
    }

    pub fn set_review_model(&self, model: ReviewModel) {
        self.write(|data| data.review_model = Some(model));
    }

    pub fn review_focuses(&self, repo: &Path) -> Vec<String> {
        self.read(|data| data.review_focuses.get(&repo_key(repo)).cloned())
            .unwrap_or_default()
    }

    pub fn set_review_focuses(&self, repo: &Path, focuses: Vec<String>) {
        self.write(|data| {
            data.review_focuses.insert(repo_key(repo), focuses);
        });
    }

    pub fn edit_permission(&self) -> EditPermission {
        self.read(|data| data.edit_permission).unwrap_or_default()
    }

    pub fn set_edit_permission(&self, permission: EditPermission) {
        self.write(|data| data.edit_permission = Some(permission));
    }

    fn read<T>(&self, get: impl FnOnce(&Data) -> T) -> T {
        get(&self.data.lock().unwrap())
    }

    fn write(&self, change: impl FnOnce(&mut Data)) {
        let mut data = self.data.lock().unwrap();
        change(&mut data);
        let Some(file) = &self.file else { return };
        if self.unreadable.swap(false, Ordering::SeqCst) {
            let mut aside = file.as_os_str().to_owned();
            aside.push(".bad");
            let _ = std::fs::rename(file, aside);
        }
        let Ok(bytes) = serde_json::to_vec_pretty(&*data) else {
            return;
        };
        let _ = write_atomically(file, &bytes);
    }
}

impl Data {
    fn decode(mut fields: Map<String, Value>) -> Self {
        let mut take = |key: &str| fields.remove(key).unwrap_or(Value::Null);
        Self {
            recent_folder: serde_json::from_value(take("recentFolder")).ok().flatten(),
            comparisons: entries(take("comparisons")),
            review_model: serde_json::from_value(take("reviewModel")).ok().flatten(),
            review_focuses: entries(take("reviewFocuses")),
            edit_permission: serde_json::from_value(take("editPermission")).ok().flatten(),
        }
    }
}

fn entries<T: DeserializeOwned>(value: Value) -> BTreeMap<String, T> {
    let Value::Object(map) = value else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter_map(|(key, value)| Some((key, serde_json::from_value(value).ok()?)))
        .collect()
}

fn repo_key(repo: &Path) -> String {
    repo.to_string_lossy().into_owned()
}

pub(crate) fn write_atomically(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp = file.as_os_str().to_owned();
    temp.push(format!(".{}.tmp", std::process::id()));
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// `DIFFPILOT_APPEARANCE`; `None` means follow the system.
pub fn appearance_override() -> Option<Appearance> {
    std::env::var("DIFFPILOT_APPEARANCE")
        .ok()
        .and_then(|value| Appearance::parse(&value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nested").join("settings.json");
        (dir, file)
    }

    #[test]
    fn recent_folder_is_none_with_nothing_stored() {
        assert_eq!(Settings::in_memory().recent_folder_with(None), None);
    }

    #[test]
    fn remember_then_restore_returns_the_same_folder() {
        let settings = Settings::in_memory();
        settings.remember_folder(Path::new("/work/repo"));
        assert_eq!(settings.recent_folder_with(None), Some(PathBuf::from("/work/repo")));
    }

    #[test]
    fn environment_wins_over_the_saved_folder() {
        let settings = Settings::in_memory();
        settings.remember_folder(Path::new("/work/repo"));
        assert_eq!(
            settings.recent_folder_with(Some("/tmp/from-env")),
            Some(PathBuf::from("/tmp/from-env"))
        );
    }

    #[test]
    fn empty_environment_value_falls_back_to_the_saved_folder() {
        let settings = Settings::in_memory();
        settings.remember_folder(Path::new("/work/repo"));
        assert_eq!(settings.recent_folder_with(Some("")), Some(PathBuf::from("/work/repo")));
    }

    #[test]
    fn values_survive_a_reopen_and_the_folder_is_created() {
        let (_dir, file) = temp_file();
        let settings = Settings::at(file.clone());
        settings.remember_folder(Path::new("/work/repo"));
        settings.set_comparison("k", &Comparison::Staged);
        settings.set_review_model(ReviewModel::new(
            crate::review::ModelName::Haiku,
            crate::review::Effort::Low,
        ));
        settings.set_review_focuses(Path::new("/r"), vec!["Tests".into()]);

        let again = Settings::at(file);
        assert_eq!(again.recent_folder_with(None), Some(PathBuf::from("/work/repo")));
        assert_eq!(again.comparison("k"), Some(Comparison::Staged));
        assert_eq!(again.review_model().name, crate::review::ModelName::Haiku);
        assert_eq!(again.review_focuses(Path::new("/r")), ["Tests"]);
        assert!(again.review_focuses(Path::new("/other")).is_empty());
    }

    #[test]
    fn the_edit_permission_defaults_to_dont_ask_and_is_saved() {
        let (_dir, file) = temp_file();
        let settings = Settings::at(file.clone());
        assert_eq!(settings.edit_permission(), EditPermission::DontAsk);
        settings.set_edit_permission(EditPermission::AskEach);
        assert_eq!(Settings::at(file.clone()).edit_permission(), EditPermission::AskEach);
        std::fs::write(&file, r#"{"editPermission": "somethingNew", "recentFolder": "/x"}"#).unwrap();
        let unknown = Settings::at(file);
        assert_eq!(unknown.edit_permission(), EditPermission::DontAsk);
        assert_eq!(unknown.recent_folder_with(None), Some(PathBuf::from("/x")));
    }

    #[test]
    fn a_broken_file_starts_empty() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "not json").unwrap();
        let settings = Settings::at(file);
        assert_eq!(settings.recent_folder_with(None), None);
        assert_eq!(settings.review_model(), ReviewModel::STANDARD);
    }

    #[test]
    fn a_broken_file_is_moved_aside_before_the_first_write() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "not json").unwrap();
        let settings = Settings::at(file.clone());
        settings.remember_folder(Path::new("/work/repo"));
        settings.remember_folder(Path::new("/work/other"));
        let aside = file.with_file_name("settings.json.bad");
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "not json");
        assert_eq!(
            Settings::at(file).recent_folder_with(None),
            Some(PathBuf::from("/work/other"))
        );
    }

    #[test]
    fn one_unknown_value_keeps_every_other_setting() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            r#"{
  "recentFolder": "/work/repo",
  "comparisons": {
    "/work/repo\nfeature": { "branch": { "branch": "topic", "base": "main" } },
    "/work/repo\nnewer": { "fromTheFuture": {} }
  },
  "reviewModel": { "name": "fable", "effort": "high" },
  "reviewFocuses": { "/work/repo": ["Tests"], "/work/bad": 3 }
}"#,
        )
        .unwrap();
        let settings = Settings::at(file.clone());
        settings.set_review_focuses(Path::new("/other"), vec!["Security".into()]);

        let again = Settings::at(file.clone());
        assert_eq!(again.recent_folder_with(None), Some(PathBuf::from("/work/repo")));
        assert_eq!(
            again.comparison("/work/repo\nfeature"),
            Some(Comparison::branch("topic", "main"))
        );
        assert_eq!(again.comparison("/work/repo\nnewer"), None);
        assert_eq!(again.review_model(), ReviewModel::STANDARD);
        assert_eq!(again.review_focuses(Path::new("/work/repo")), ["Tests"]);
        assert_eq!(again.review_focuses(Path::new("/other")), ["Security"]);
        assert!(!file.with_file_name("settings.json.bad").exists());
    }

    #[test]
    fn parses_the_appearance_override() {
        assert_eq!(Appearance::parse("light"), Some(Appearance::Light));
        assert_eq!(Appearance::parse("dark"), Some(Appearance::Dark));
        assert_eq!(Appearance::parse("auto"), None);
    }
}
