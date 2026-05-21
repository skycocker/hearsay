//! TOML config loaded from `~/.config/hearsay/config.toml` (or the
//! `$HEARSAY_CONFIG` env var, which takes precedence — handy for tests and
//! for shipping a default file inside the .app bundle).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub paths: PathsConfig,
    pub transcription: TranscriptionConfig,
    pub summarization: SummarizationConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { host: "127.0.0.1".into(), port: 7717 }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    /// Where the SQLite DB and per-session audio files live. Defaults to
    /// platform's data dir + "hearsay".
    pub data_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    /// Whisper.cpp model name — used to build the default path
    /// `<data_dir>/models/ggml-<model>.bin` when `model_path` is unset.
    pub model: String,
    /// Explicit override for the model file. Takes precedence over `model`.
    pub model_path: Option<PathBuf>,
    pub n_threads: i32,
    /// ISO-639-1 language, or `"auto"` for Whisper's detector.
    pub default_language: String,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            model: "large-v3-turbo".into(),
            model_path: None,
            n_threads: 4,
            default_language: "auto".into(),
        }
    }
}

impl TranscriptionConfig {
    /// Resolve the model file path. Explicit `model_path` wins; otherwise
    /// build it from `model` under `<data_dir>/models/`.
    pub fn resolved_model_path(&self, data_dir: &Path) -> PathBuf {
        if let Some(p) = &self.model_path {
            return p.clone();
        }
        data_dir.join("models").join(format!("ggml-{}.bin", self.model))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SummarizationConfig {
    /// Logical model name — used to build the default path
    /// `<data_dir>/models/<model>.gguf` when `model_path` is unset.
    pub model: String,
    pub model_path: Option<PathBuf>,
    pub n_ctx: u32,
    /// `999` = all layers on GPU (default), `0` = pure CPU.
    pub n_gpu_layers: u32,
    /// Hard cap on generated tokens per summary.
    pub max_tokens: u32,
    /// When `false`, the model is loaded per summarization job and freed
    /// afterwards. When `true` (default), it stays resident — fast at the
    /// cost of ~7 GB RAM held for Gemma 12B Q4.
    pub keep_loaded: bool,
    /// Path to the `hearsay-summarize-child` worker binary. We must spawn
    /// summarization in a child process to avoid an llama-cpp-2 +
    /// whisper-rs collision (see crates/hearsayd/src/bin/...child.rs).
    /// Default: alongside the daemon binary.
    pub child_binary: Option<PathBuf>,
}

impl Default for SummarizationConfig {
    fn default() -> Self {
        Self {
            model: "gemma-3-12b".into(),
            model_path: None,
            n_ctx: 32_768,
            n_gpu_layers: 999,
            max_tokens: 1_500,
            keep_loaded: true,
            child_binary: None,
        }
    }
}

impl SummarizationConfig {
    pub fn resolved_model_path(&self, data_dir: &Path) -> PathBuf {
        if let Some(p) = &self.model_path {
            return p.clone();
        }
        data_dir.join("models").join(format!("{}.gguf", self.model))
    }
}

impl Config {
    /// Load from `$HEARSAY_CONFIG` if set, otherwise the platform config
    /// dir. Missing file is fine — caller gets defaults.
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::resolved_config_path();
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            tracing::info!(?path, "no config file found, using defaults");
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&text)?;
        Ok(cfg)
    }

    pub fn resolved_config_path() -> PathBuf {
        if let Ok(p) = std::env::var("HEARSAY_CONFIG") {
            return PathBuf::from(p);
        }
        dirs::config_dir()
            .map(|d| d.join("hearsay").join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("hearsay.toml"))
    }

    pub fn resolved_data_dir(&self) -> PathBuf {
        if let Some(d) = &self.paths.data_dir {
            return d.clone();
        }
        dirs::data_dir()
            .map(|d| d.join("hearsay"))
            .unwrap_or_else(|| PathBuf::from("./hearsay-data"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("nonexistent.toml")).unwrap();
        assert_eq!(cfg.server.port, 7717);
        assert_eq!(cfg.summarization.model, "gemma-3-12b");
    }

    #[test]
    fn partial_toml_keeps_defaults_for_unspecified_sections() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("c.toml");
        std::fs::write(&path, "[server]\nport = 9000\n").unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.server.port, 9000);
        assert_eq!(cfg.server.host, "127.0.0.1"); // default still applied
        assert_eq!(cfg.transcription.model, "large-v3-turbo");
    }

    #[test]
    fn empty_toml_yields_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("c.toml");
        std::fs::write(&path, "").unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.server.port, 7717);
        assert!(cfg.summarization.keep_loaded);
    }
}
