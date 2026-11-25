use crate::comparison::Comparison;
use crate::policy::EditPermission;
use crate::review::ReviewModel;
use crate::run::RunKind;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
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
    reviews_model: Option<ReviewModel>,
    fixes_model: Option<ReviewModel>,
    review_focuses: BTreeMap<String, Vec<String>>,
    edit_permission: Option<EditPermission>,
    claude_path: Option<PathBuf>,
    allow_terminal_agents: Option<bool>,
    open_last_repository: Option<bool>,
    appearance: Option<AppearanceChoice>,
    wrap_lines: Option<bool>,
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

    /// The model for reviews and guides. A build before the two defaults saved it as `reviewModel`.
    pub fn reviews_model(&self) -> ReviewModel {
        self.read(|data| data.reviews_model).unwrap_or(ReviewModel::STANDARD)
    }

    pub fn set_reviews_model(&self, model: ReviewModel) {
        self.write(|data| data.reviews_model = Some(model));
    }

    pub fn fixes_model(&self) -> ReviewModel {
        self.read(|data| data.fixes_model).unwrap_or(ReviewModel::FIXES)
    }

    pub fn set_fixes_model(&self, model: ReviewModel) {
        self.write(|data| data.fixes_model = Some(model));
    }

    pub fn model_for(&self, kind: RunKind) -> ReviewModel {
        match kind {
            RunKind::Review => self.reviews_model(),
            RunKind::FixComments | RunKind::MakeChanges | RunKind::Reply => self.fixes_model(),
        }
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

    pub fn claude_path(&self) -> Option<PathBuf> {
        self.read(|data| data.claude_path.clone())
    }

    pub fn set_claude_path(&self, path: Option<&Path>) {
        self.write(|data| data.claude_path = path.map(Path::to_path_buf));
    }

    pub fn allow_terminal_agents(&self) -> bool {
        self.read(|data| data.allow_terminal_agents).unwrap_or(true)
    }

    pub fn set_allow_terminal_agents(&self, allow: bool) {
        self.write(|data| data.allow_terminal_agents = Some(allow));
    }

    pub fn open_last_repository(&self) -> bool {
        self.read(|data| data.open_last_repository).unwrap_or(true)
    }

    pub fn set_open_last_repository(&self, open: bool) {
        self.write(|data| data.open_last_repository = Some(open));
    }

    pub fn appearance(&self) -> AppearanceChoice {
        self.read(|data| data.appearance).unwrap_or_default()
    }

    pub fn set_appearance(&self, choice: AppearanceChoice) {
        self.write(|data| data.appearance = Some(choice));
    }

    pub fn wrap_lines(&self) -> bool {
        self.read(|data| data.wrap_lines).unwrap_or(false)
    }

    pub fn set_wrap_lines(&self, wrap: bool) {
        self.write(|data| data.wrap_lines = Some(wrap));
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
            recent_folder: field(take("recentFolder")),
            comparisons: entries(take("comparisons")),
            reviews_model: field(take("reviewsModel")).or_else(|| field(take("reviewModel"))),
            fixes_model: field(take("fixesModel")),
            review_focuses: entries(take("reviewFocuses")),
            edit_permission: field(take("editPermission")),
            claude_path: field(take("claudePath")),
            allow_terminal_agents: field(take("allowTerminalAgents")),
            open_last_repository: field(take("openLastRepository")),
            appearance: field(take("appearance")),
            wrap_lines: field(take("wrapLines")),
        }
    }
}

fn field<T: DeserializeOwned>(value: Value) -> Option<T> {
    serde_json::from_value(value).ok().flatten()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppearanceChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl AppearanceChoice {
    pub const ALL: [AppearanceChoice; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn title(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
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
    use crate::review::{Effort, ModelName};

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
        settings.set_reviews_model(ReviewModel::new(
            crate::review::ModelName::Haiku,
            crate::review::Effort::Low,
        ));
        settings.set_review_focuses(Path::new("/r"), vec!["Tests".into()]);

        let again = Settings::at(file);
        assert_eq!(again.recent_folder_with(None), Some(PathBuf::from("/work/repo")));
        assert_eq!(again.comparison("k"), Some(Comparison::Staged));
        assert_eq!(again.reviews_model().name, crate::review::ModelName::Haiku);
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
    fn the_two_models_default_to_opus_high_and_sonnet_medium() {
        let settings = Settings::in_memory();
        assert_eq!(settings.reviews_model().title(), "Opus · High");
        assert_eq!(settings.fixes_model().title(), "Sonnet · Medium");
    }

    #[test]
    fn the_old_review_model_becomes_the_reviews_model_and_leaves_the_fixes_model_alone() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{"reviewModel": {"name": "haiku", "effort": "low"}}"#).unwrap();
        let settings = Settings::at(file.clone());
        assert_eq!(
            settings.reviews_model(),
            ReviewModel::new(ModelName::Haiku, Effort::Low)
        );
        assert_eq!(settings.fixes_model(), ReviewModel::FIXES);

        settings.set_fixes_model(ReviewModel::new(ModelName::Opus, Effort::Low));
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(
            saved.contains("reviewsModel") && !saved.contains("\"reviewModel\""),
            "{saved}"
        );
        let again = Settings::at(file);
        assert_eq!(again.reviews_model(), ReviewModel::new(ModelName::Haiku, Effort::Low));
        assert_eq!(again.fixes_model(), ReviewModel::new(ModelName::Opus, Effort::Low));
    }

    #[test]
    fn the_new_models_win_over_the_old_review_model() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            r#"{"reviewModel": {"name": "haiku", "effort": "low"}, "reviewsModel": {"name": "sonnet", "effort": "high"}}"#,
        )
        .unwrap();
        assert_eq!(
            Settings::at(file).reviews_model(),
            ReviewModel::new(ModelName::Sonnet, Effort::High)
        );
    }

    #[test]
    fn each_kind_of_run_reads_its_own_model() {
        let settings = Settings::in_memory();
        settings.set_reviews_model(ReviewModel::new(ModelName::Haiku, Effort::Low));
        settings.set_fixes_model(ReviewModel::new(ModelName::Opus, Effort::Medium));
        assert_eq!(settings.model_for(RunKind::Review).name, ModelName::Haiku);
        for kind in [RunKind::FixComments, RunKind::MakeChanges, RunKind::Reply] {
            assert_eq!(settings.model_for(kind).name, ModelName::Opus);
        }
    }

    #[test]
    fn the_app_settings_default_and_survive_a_reopen() {
        let (_dir, file) = temp_file();
        let settings = Settings::at(file.clone());
        assert_eq!(settings.claude_path(), None);
        assert!(settings.allow_terminal_agents());
        assert!(settings.open_last_repository());
        assert_eq!(settings.appearance(), AppearanceChoice::System);
        assert!(!settings.wrap_lines());

        settings.set_claude_path(Some(Path::new("/opt/claude")));
        settings.set_allow_terminal_agents(false);
        settings.set_open_last_repository(false);
        settings.set_appearance(AppearanceChoice::Dark);
        settings.set_wrap_lines(true);

        let again = Settings::at(file.clone());
        assert_eq!(again.claude_path(), Some(PathBuf::from("/opt/claude")));
        assert!(!again.allow_terminal_agents());
        assert!(!again.open_last_repository());
        assert_eq!(again.appearance(), AppearanceChoice::Dark);
        assert!(again.wrap_lines());

        again.set_claude_path(None);
        assert_eq!(Settings::at(file).claude_path(), None);
    }

    #[test]
    fn a_value_of_the_wrong_type_keeps_its_default_and_the_others() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            r#"{"appearance": "sepia", "wrapLines": "yes", "claudePath": 4, "openLastRepository": false}"#,
        )
        .unwrap();
        let settings = Settings::at(file);
        assert_eq!(settings.appearance(), AppearanceChoice::System);
        assert!(!settings.wrap_lines());
        assert_eq!(settings.claude_path(), None);
        assert!(!settings.open_last_repository());
    }

    #[test]
    fn a_broken_file_starts_empty() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "not json").unwrap();
        let settings = Settings::at(file);
        assert_eq!(settings.recent_folder_with(None), None);
        assert_eq!(settings.reviews_model(), ReviewModel::STANDARD);
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
        assert_eq!(again.reviews_model(), ReviewModel::STANDARD);
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
