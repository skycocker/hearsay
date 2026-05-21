//! AppState — the handle the router shares across requests.

use std::sync::Arc;

use hearsay_storage::Storage;

use crate::config::Config;
use crate::session_manager::SessionManager;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub storage: Storage,
    pub sessions: Arc<SessionManager>,
}

impl AppState {
    pub fn new(config: Config, storage: Storage) -> Self {
        let sessions = Arc::new(SessionManager::new(storage.clone(), config.resolved_data_dir()));
        Self {
            config: Arc::new(config),
            storage,
            sessions,
        }
    }
}
