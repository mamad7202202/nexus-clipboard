//! Domain layer — entities, value objects and pure business rules.
//!
//! Nothing in here depends on SQLite, Tauri or the operating system. That
//! boundary is what lets the capture pipeline, the repositories and the IPC
//! layer evolve independently.

pub mod classify;
pub mod item;
pub mod query;

pub use item::{Collection, Item, Kind, Meta, NewItem, Snapshot, Tag};
pub use query::{Cursor, Page, Query, SmartFilter, SortBy, Stats};
