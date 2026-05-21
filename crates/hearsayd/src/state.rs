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
        let config = Arc::new(config);
        let sessions = Arc::new(SessionManager::new(
            storage.clone(),
            Arc::clone(&config),
        ));
        Self {
            config,
            storage,
            sessions,
        }
    }
}
