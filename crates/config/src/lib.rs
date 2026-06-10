use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info};

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub worker: WorkerConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub timeout_ms: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8317/v1".to_string(),
            api_key: "sk-placeholder".to_string(),
            model: "qwen3.5-35b-a3b".to_string(),
            max_tokens: 50_000,
            timeout_ms: 3_600_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct WorkerConfig {
    pub count: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self { count: 8 }
    }
}

pub fn get_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("article-scraper")
        .join("config.toml")
}

pub fn load_config() -> Result<AppConfig> {
    let path = get_config_path();
    if !path.exists() {
        // Return defaults if missing
        return Ok(AppConfig::default());
    }
    let content = std::fs::read_to_string(&path)?;
    let cfg: AppConfig = toml::from_str(&content)?;
    Ok(cfg)
}

pub struct ConfigWatcher {
    pub rx: watch::Receiver<Arc<AppConfig>>,
    _watcher: RecommendedWatcher,
}

impl ConfigWatcher {
    pub fn new() -> Result<Self> {
        let initial_config = load_config().unwrap_or_default();
        let (tx, rx) = watch::channel(Arc::new(initial_config));

        let tx = Arc::new(tx);
        let tx_clone = tx.clone();

        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<Event>| match res {
                Ok(event) => {
                    if event.kind.is_modify() || event.kind.is_create() {
                        match load_config() {
                            Ok(new_config) => {
                                let new_arc = Arc::new(new_config);
                                if tx_clone.send(new_arc).is_ok() {
                                    info!("Config updated successfully");
                                }
                            }
                            Err(e) => {
                                error!("Failed to load config after change: {}", e);
                            }
                        }
                    }
                }
                Err(e) => error!("Watch error: {}", e),
            })?;

        let path = get_config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if path.exists() {
            watcher.watch(&path, RecursiveMode::NonRecursive)?;
        } else {
            // If file doesn't exist, watch the parent directory for its creation
            let parent = path.parent().unwrap();
            watcher.watch(parent, RecursiveMode::NonRecursive)?;
        }

        Ok(Self {
            rx,
            _watcher: watcher,
        })
    }
}
