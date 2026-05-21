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
    pub model: String,
    pub workers: u8,
    /// ISO-639-1 language, or `"auto"` for Whisper's detector.
    pub default_language: String,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            model: "whisper-large-v3-turbo".into(),
            workers: 2,
            default_language: "auto".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SummarizationConfig {
    pub model: String,
    /// When `false`, the model is loaded per summarization job and freed
    /// afterwards. When `true`, it's kept resident for fast follow-ups.
    pub keep_loaded: bool,
}

impl Default for SummarizationConfig {
    fn default() -> Self {
        Self {
            model: "gemma3-12b-q4".into(),
            keep_loaded: true,
        }
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
        assert_eq!(cfg.summarization.model, "gemma3-12b-q4");
    }

    #[test]
    fn partial_toml_keeps_defaults_for_unspecified_sections() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("c.toml");
        std::fs::write(&path, "[server]\nport = 9000\n").unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.server.port, 9000);
        assert_eq!(cfg.server.host, "127.0.0.1"); // default still applied
        assert_eq!(cfg.transcription.model, "whisper-large-v3-turbo");
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
