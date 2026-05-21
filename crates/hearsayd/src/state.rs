//! AppState — the handle the router shares across requests.

use std::sync::Arc;

use hearsay_storage::Storage;
use hearsay_summarize::{Summarizer, SummarizerConfig};

use crate::config::Config;
use crate::session_manager::SessionManager;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub storage: Storage,
    pub sessions: Arc<SessionManager>,
    /// `None` when the summarization model is missing on disk — the daemon
    /// still works, summaries just don't auto-generate. `try_load_summarizer`
    /// is what made the decision.
    pub summarizer: Option<Arc<Summarizer>>,
}

impl AppState {
    pub fn new(config: Config, storage: Storage) -> Self {
        let config = Arc::new(config);
        let summarizer = try_load_summarizer(&config);
        let sessions = Arc::new(SessionManager::new(
            storage.clone(),
            Arc::clone(&config),
            summarizer.clone(),
        ));
        Self {
            config,
            storage,
            sessions,
            summarizer,
        }
    }
}

fn try_load_summarizer(config: &Config) -> Option<Arc<Summarizer>> {
    let data_dir = config.resolved_data_dir();
    let model_path = config.summarization.resolved_model_path(&data_dir);
    if !model_path.exists() {
        tracing::warn!(
            path = %model_path.display(),
            "summarization model not found; summaries will not auto-generate. \
             Run scripts/setup-models.sh gemma-3-12b."
        );
        return None;
    }
    let cfg = SummarizerConfig {
        model_path: model_path.clone(),
        n_ctx: config.summarization.n_ctx,
        n_threads: 0,
        n_gpu_layers: config.summarization.n_gpu_layers,
        max_tokens: config.summarization.max_tokens,
        seed: 42,
    };
    match Summarizer::new(cfg) {
        Ok(s) => {
            tracing::info!(path = %model_path.display(), "summarizer loaded");
            Some(Arc::new(s))
        }
        Err(e) => {
            tracing::error!(?e, "failed to load summarizer; summaries disabled");
            None
        }
    }
}
