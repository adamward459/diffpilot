use crate::comparison::Comparison;
use crate::diff::DiffMode;
use crate::editor::Editor;
use crate::policy::EditPermission;
use crate::review::ReviewModel;
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
    folders: Vec<PathBuf>,
    comparisons: BTreeMap<String, Comparison>,
    reviews_model: Option<ReviewModel>,
    fixes_model: Option<ReviewModel>,
    review_focuses: BTreeMap<String, Vec<String>>,
    linked_folders: BTreeMap<String, Vec<PathBuf>>,
    diff_mode: Option<DiffMode>,
    edit_permission: Option<EditPermission>,
    claude_path: Option<PathBuf>,
    allow_terminal_agents: Option<bool>,
    open_last_repository: Option<bool>,
    appearance: Option<AppearanceChoice>,
    wrap_lines: Option<bool>,
    editor: Option<Editor>,
    tour_seen: Option<bool>,
    ask_before_stopping_runs: Option<bool>,
}

type Listener = Box<dyn Fn() + Send + Sync>;

pub struct Settings {
    file: Option<PathBuf>,
    data: Mutex<Data>,
    unreadable: AtomicBool,
    listeners: Mutex<Vec<Listener>>,
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
            listeners: Mutex::default(),
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
            listeners: Mutex::default(),
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

    /// The folder to open at launch. `DIFFPILOT_REPO` always counts; the saved folder only when
    /// "open the last repository" is on.
    pub fn launch_folder(&self) -> Option<PathBuf> {
        let env = std::env::var("DIFFPILOT_REPO").ok();
        self.launch_folder_with(env.as_deref())
    }

    pub fn launch_folder_with(&self, env_override: Option<&str>) -> Option<PathBuf> {
        match env_override.filter(|path| !path.is_empty()) {
            Some(path) => Some(PathBuf::from(path)),
            None if self.open_last_repository() => self.read(|data| data.recent_folder.clone()),
            None => None,
        }
    }

    pub fn remember_folder(&self, folder: &Path) {
        self.write(|data| {
            data.recent_folder = Some(folder.to_path_buf());
            if !data.folders.iter().any(|known| known == folder) {
                data.folders.push(folder.to_path_buf());
            }
        });
    }

    pub fn folders(&self) -> Vec<PathBuf> {
        self.read(|data| data.folders.clone())
    }

    pub fn forget_folder(&self, folder: &Path) {
        self.write(|data| {
            data.folders.retain(|known| known != folder);
            if data.recent_folder.as_deref() == Some(folder) {
                data.recent_folder = data.folders.first().cloned();
            }
        });
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

    pub fn review_focuses(&self, repo: &Path) -> Vec<String> {
        self.read(|data| data.review_focuses.get(&repo_key(repo)).cloned())
            .unwrap_or_default()
    }

    pub fn set_review_focuses(&self, repo: &Path, focuses: Vec<String>) {
        self.write(|data| {
            data.review_focuses.insert(repo_key(repo), focuses);
        });
    }

    pub fn linked_folders(&self, repo: &Path) -> Vec<PathBuf> {
        self.read(|data| data.linked_folders.get(&repo_key(repo)).cloned())
            .unwrap_or_default()
    }

    pub fn set_linked_folders(&self, repo: &Path, folders: Vec<PathBuf>) {
        self.write(|data| {
            if folders.is_empty() {
                data.linked_folders.remove(&repo_key(repo));
            } else {
                data.linked_folders.insert(repo_key(repo), folders);
            }
        });
    }

    pub fn diff_mode(&self) -> DiffMode {
        self.read(|data| data.diff_mode).unwrap_or_default()
    }

    pub fn set_diff_mode(&self, mode: DiffMode) {
        self.write(|data| data.diff_mode = Some(mode));
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

    pub fn tour_seen(&self) -> bool {
        self.read(|data| data.tour_seen).unwrap_or(false)
    }

    pub fn set_tour_seen(&self, seen: bool) {
        self.write(|data| data.tour_seen = Some(seen));
    }

    pub fn ask_before_stopping_runs(&self) -> bool {
        self.read(|data| data.ask_before_stopping_runs).unwrap_or(true)
    }

    pub fn set_ask_before_stopping_runs(&self, ask: bool) {
        self.write(|data| data.ask_before_stopping_runs = Some(ask));
    }

    pub fn wrap_lines(&self) -> bool {
        self.read(|data| data.wrap_lines).unwrap_or(false)
    }

    pub fn set_wrap_lines(&self, wrap: bool) {
        self.write(|data| data.wrap_lines = Some(wrap));
    }

    /// `None` means detect an editor when a file opens.
    pub fn editor(&self) -> Option<Editor> {
        self.read(|data| data.editor.clone())
    }

    pub fn set_editor(&self, editor: Option<Editor>) {
        self.write(|data| data.editor = editor);
    }

    fn read<T>(&self, get: impl FnOnce(&Data) -> T) -> T {
        get(&self.data.lock().unwrap())
    }

    /// Runs after every write that changes a value, on the thread that wrote it, with no lock held.
    pub fn on_change(&self, listener: impl Fn() + Send + Sync + 'static) {
        self.listeners.lock().unwrap().push(Box::new(listener));
    }

    fn write(&self, change: impl FnOnce(&mut Data)) {
        let changed = {
            let mut data = self.data.lock().unwrap();
            let before = data.clone();
            change(&mut data);
            self.save(&data);
            *data != before
        };
        if changed {
            for listener in self.listeners.lock().unwrap().iter() {
                listener();
            }
        }
    }

    fn save(&self, data: &Data) {
        let Some(file) = &self.file else { return };
        if self.unreadable.swap(false, Ordering::SeqCst) {
            let mut aside = file.as_os_str().to_owned();
            aside.push(".bad");
            let _ = std::fs::rename(file, aside);
        }
        let Ok(bytes) = serde_json::to_vec_pretty(data) else {
            return;
        };
        let _ = write_atomically(file, &bytes);
    }
}

impl Data {
    fn decode(mut fields: Map<String, Value>) -> Self {
        let mut take = |key: &str| fields.remove(key).unwrap_or(Value::Null);
        let recent_folder: Option<PathBuf> = field(take("recentFolder"));
        let mut folders: Vec<PathBuf> = Vec::new();
        for folder in field::<Vec<PathBuf>>(take("folders")).unwrap_or_default() {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        if let Some(recent) = recent_folder.as_ref().filter(|recent| !folders.contains(recent)) {
            folders.push(recent.clone());
        }
        Self {
            recent_folder,
            folders,
            comparisons: entries(take("comparisons")),
            reviews_model: field(take("reviewsModel")).or_else(|| field(take("reviewModel"))),
            fixes_model: field(take("fixesModel")),
            review_focuses: entries(take("reviewFocuses")),
            linked_folders: entries(take("linkedFolders")),
            diff_mode: serde_json::from_value(take("diffMode")).ok().flatten(),
            edit_permission: field(take("editPermission")),
            claude_path: field(take("claudePath")),
            allow_terminal_agents: field(take("allowTerminalAgents")),
            open_last_repository: field(take("openLastRepository")),
            appearance: field(take("appearance")),
            wrap_lines: field(take("wrapLines")),
            editor: field(take("editor")),
            tour_seen: field(take("tourSeen")),
            ask_before_stopping_runs: field(take("askBeforeStoppingRuns")),
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

/// `DIFFPILOT_DIFF_MODE`; `None` means use the saved mode.
pub fn diff_mode_override() -> Option<DiffMode> {
    std::env::var("DIFFPILOT_DIFF_MODE")
        .ok()
        .and_then(|value| DiffMode::parse(&value))
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
    fn listeners_hear_changes_but_not_writes_of_the_same_value() {
        let settings = Settings::in_memory();
        let heard = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = heard.clone();
        settings.on_change(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        settings.set_wrap_lines(true);
        settings.set_wrap_lines(true);
        settings.set_diff_mode(DiffMode::Split);
        assert_eq!(heard.load(Ordering::SeqCst), 2);
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
    fn remembered_folders_keep_their_order_without_duplicates() {
        let settings = Settings::in_memory();
        for folder in ["/a", "/b", "/a", "/c"] {
            settings.remember_folder(Path::new(folder));
        }
        let expected: Vec<PathBuf> = ["/a", "/b", "/c"].iter().map(PathBuf::from).collect();
        assert_eq!(settings.folders(), expected);
        assert_eq!(settings.recent_folder_with(None), Some(PathBuf::from("/c")));
    }

    #[test]
    fn forgetting_the_active_folder_moves_to_the_first_left() {
        let settings = Settings::in_memory();
        settings.remember_folder(Path::new("/a"));
        settings.remember_folder(Path::new("/b"));
        settings.forget_folder(Path::new("/b"));
        assert_eq!(settings.folders(), vec![PathBuf::from("/a")]);
        assert_eq!(settings.recent_folder_with(None), Some(PathBuf::from("/a")));
        settings.forget_folder(Path::new("/a"));
        assert_eq!(settings.recent_folder_with(None), None);
    }

    #[test]
    fn an_old_file_with_only_a_recent_folder_lists_it() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{"recentFolder":"/work/repo"}"#).unwrap();
        let settings = Settings::at(file);
        assert_eq!(settings.folders(), vec![PathBuf::from("/work/repo")]);
    }

    #[test]
    fn a_saved_list_is_deduped_on_decode_and_keeps_the_active_folder_in_it() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{"folders":["/a","/b","/a"],"recentFolder":"/c"}"#).unwrap();
        let settings = Settings::at(file);
        let expected: Vec<PathBuf> = ["/a", "/b", "/c"].iter().map(PathBuf::from).collect();
        assert_eq!(settings.folders(), expected);
    }

    #[test]
    fn a_folder_list_of_the_wrong_type_is_dropped() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{"folders":"nope","recentFolder":"/a"}"#).unwrap();
        assert_eq!(Settings::at(file).folders(), vec![PathBuf::from("/a")]);
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
        settings.set_linked_folders(Path::new("/r"), vec![PathBuf::from("/shared")]);
        settings.set_diff_mode(DiffMode::Split);

        let again = Settings::at(file);
        assert_eq!(again.recent_folder_with(None), Some(PathBuf::from("/work/repo")));
        assert_eq!(again.comparison("k"), Some(Comparison::Staged));
        assert_eq!(again.reviews_model().name, crate::review::ModelName::Haiku);
        assert_eq!(again.review_focuses(Path::new("/r")), ["Tests"]);
        assert!(again.review_focuses(Path::new("/other")).is_empty());
        assert_eq!(again.linked_folders(Path::new("/r")), [PathBuf::from("/shared")]);
        assert!(again.linked_folders(Path::new("/other")).is_empty());
        assert_eq!(again.diff_mode(), DiffMode::Split);
    }

    #[test]
    fn the_diff_mode_starts_unified() {
        assert_eq!(Settings::in_memory().diff_mode(), DiffMode::Unified);
    }

    #[test]
    fn a_diff_mode_this_build_cannot_read_falls_back_and_keeps_the_rest() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{ "recentFolder": "/work/repo", "diffMode": "sideways" }"#).unwrap();
        let settings = Settings::at(file);
        assert_eq!(settings.diff_mode(), DiffMode::Unified);
        assert_eq!(settings.recent_folder_with(None), Some(PathBuf::from("/work/repo")));
    }

    #[test]
    fn parses_the_diff_mode_override() {
        assert_eq!(DiffMode::parse("split"), Some(DiffMode::Split));
        assert_eq!(DiffMode::parse("unified"), Some(DiffMode::Unified));
        assert_eq!(DiffMode::parse("Split"), None);
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
    fn the_launch_folder_follows_the_open_last_repository_setting() {
        let settings = Settings::in_memory();
        settings.remember_folder(Path::new("/work/repo"));
        assert_eq!(settings.launch_folder_with(None), Some(PathBuf::from("/work/repo")));
        settings.set_open_last_repository(false);
        assert_eq!(settings.launch_folder_with(None), None);
        assert_eq!(settings.launch_folder_with(Some("")), None);
        assert_eq!(
            settings.launch_folder_with(Some("/from/env")),
            Some(PathBuf::from("/from/env"))
        );
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
    fn the_editor_defaults_to_automatic_and_survives_a_reopen() {
        let (_dir, file) = temp_file();
        let settings = Settings::at(file.clone());
        assert_eq!(settings.editor(), None);
        settings.set_editor(Some(Editor::Custom("ed {file}".into())));
        assert_eq!(
            Settings::at(file.clone()).editor(),
            Some(Editor::Custom("ed {file}".into()))
        );
        settings.set_editor(None);
        assert_eq!(Settings::at(file).editor(), None);
    }

    #[test]
    fn an_editor_this_build_cannot_read_falls_back_and_keeps_the_rest() {
        let (_dir, file) = temp_file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{"editor": "emacs", "wrapLines": true}"#).unwrap();
        let settings = Settings::at(file);
        assert_eq!(settings.editor(), None);
        assert!(settings.wrap_lines());
    }

    #[test]
    fn the_tour_is_unseen_until_it_is_marked_and_the_mark_is_saved() {
        let (_dir, file) = temp_file();
        let settings = Settings::at(file.clone());
        assert!(!settings.tour_seen());
        settings.set_tour_seen(true);
        assert!(Settings::at(file).tour_seen());
    }

    #[test]
    fn stopping_runs_asks_first_until_it_is_turned_off_and_the_choice_is_saved() {
        let (_dir, file) = temp_file();
        let settings = Settings::at(file.clone());
        assert!(settings.ask_before_stopping_runs());
        settings.set_ask_before_stopping_runs(false);
        assert!(!Settings::at(file).ask_before_stopping_runs());
    }

    #[test]
    fn parses_the_appearance_override() {
        assert_eq!(Appearance::parse("light"), Some(Appearance::Light));
        assert_eq!(Appearance::parse("dark"), Some(Appearance::Dark));
        assert_eq!(Appearance::parse("auto"), None);
    }
}
