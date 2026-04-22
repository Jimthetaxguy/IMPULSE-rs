//! Persistence layer — in-memory state with dirty-flag disk sync.
//!
//! Provides [`SharedState`] (an `Arc<RwLock<State>>` wrapper) that tracks sessions,
//! configuration, and history. Writes are deferred via a dirty flag and flushed
//! atomically on sync or drop. Sub-modules handle config key validation,
//! session lifecycle, and storage serialization.

pub mod config;
mod config_keys;
pub mod persistence;
pub mod session;

pub use config::*;
pub use persistence::*;
pub use session::*;
