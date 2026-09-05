//! Infrastructure layer — the adapters that connect the domain to the outside
//! world: SQLite, the file system, the operating system clipboard and crypto.

pub mod blob;
pub mod clipboard;
pub mod crypto;
pub mod db;
pub mod platform;
