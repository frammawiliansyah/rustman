//! Local persistence layer.

pub mod sqlite;

pub use sqlite::{HistoryEntry, SqliteStorage};
