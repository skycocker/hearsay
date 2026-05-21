//! hearsayd internals exposed as a library so integration tests can spin
//! up the router without going through `main`.

pub mod config;
pub mod error;
pub mod routes;
pub mod session_manager;
pub mod state;
pub mod summarize_child;
#[cfg(feature = "tray")]
pub mod tray;

pub use routes::build_router;
pub use state::AppState;
