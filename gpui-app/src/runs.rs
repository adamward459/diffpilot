use diffpilot_core::mcp::{McpBridge, socket_path_for};
use diffpilot_core::runner::{EngineConfig, McpLaunch, RunEngine, missing_helpers};
use diffpilot_core::settings::{Settings, app_support_dir};
use diffpilot_core::store::Store;
use gpui::Global;
use std::sync::Arc;

pub const HELPERS_MISSING: &str = "Build the helpers: cargo build -p diffpilot-core --bins";

/// The Claude run engine of the whole app. It never takes a snapshot, so a run writes nothing into a repo's .git.
pub struct Runs {
    engine: RunEngine,
    unavailable: Option<String>,
}

impl Global for Runs {}

impl Runs {
    pub fn start(store: Arc<Store>, settings: &Settings) -> Self {
        let mut config = EngineConfig::new(store.clone());
        config.claude_binary = settings.claude_path();
        config.keep_snapshots = 0;
        let data_dir = app_support_dir().unwrap_or_else(|| std::env::temp_dir().join("diffpilot"));
        let mut unavailable = None;
        match McpBridge::start(socket_path_for(&data_dir), store) {
            Ok(bridge) => config.mcp = Some(McpLaunch::new(bridge)),
            Err(error) => unavailable = Some(format!("The diffpilot MCP server did not start: {error}")),
        }
        if !missing_helpers().is_empty() {
            unavailable = Some(HELPERS_MISSING.to_string());
        }
        Self {
            engine: RunEngine::new(config),
            unavailable,
        }
    }

    pub fn with_engine(engine: RunEngine) -> Self {
        let unavailable = (!engine.has_mcp()).then(|| "The diffpilot MCP server is not running.".to_string());
        Self { engine, unavailable }
    }

    pub fn engine(&self) -> &RunEngine {
        &self.engine
    }

    pub fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }

    pub fn use_settings(&self, settings: &Settings) {
        self.engine.set_claude_binary(settings.claude_path());
    }
}
