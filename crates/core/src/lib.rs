//! Core types shared across the hearsay workspace.

mod audio;
mod ids;
mod segment;
mod session;
mod speaker;
mod summary;

pub use audio::AudioFrame;
pub use ids::SessionId;
pub use segment::Segment;
pub use session::{SessionMeta, SessionStatus, SourceKind};
pub use speaker::Speaker;
pub use summary::Summary;
