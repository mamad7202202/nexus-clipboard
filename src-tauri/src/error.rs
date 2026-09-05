//! Unified error type for the whole backend.
//!
//! Everything that can fail funnels through [`Error`]. It serializes to a plain
//! string for the frontend so IPC never leaks internal structure, while the
//! Rust side keeps the full source chain for tracing.

use serde::{Serialize, Serializer};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("clipboard error: {0}")]
    Clipboard(String),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("archive error: {0}")]
    Archive(#[from] zip::result::ZipError),

    #[error("the vault is locked")]
    VaultLocked,

    #[error("wrong passphrase")]
    BadPassphrase,

    #[error("encryption error: {0}")]
    Crypto(String),

    #[error("not found")]
    NotFound,

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("platform error: {0}")]
    Platform(String),

    #[error("ai provider error: {0}")]
    Ai(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }

    pub fn platform(msg: impl Into<String>) -> Self {
        Self::Platform(msg.into())
    }

    /// A stable machine-readable code so the UI can branch without string matching.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Db(_) | Error::Pool(_) => "db",
            Error::Io(_) => "io",
            Error::Serde(_) => "serde",
            Error::Clipboard(_) => "clipboard",
            Error::Image(_) => "image",
            Error::Archive(_) => "archive",
            Error::VaultLocked => "vault_locked",
            Error::BadPassphrase => "bad_passphrase",
            Error::Crypto(_) => "crypto",
            Error::NotFound => "not_found",
            Error::Invalid(_) => "invalid",
            Error::Platform(_) => "platform",
            Error::Ai(_) => "ai",
            Error::Other(_) => "other",
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Other(e.to_string())
    }
}

impl From<tauri::Error> for Error {
    fn from(e: tauri::Error) -> Self {
        Error::Other(e.to_string())
    }
}

/// Serializes as `{ "code": "...", "message": "..." }` for structured handling
/// in the frontend.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Error", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}
