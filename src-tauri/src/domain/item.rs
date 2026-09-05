//! Core clipboard entity and its value objects.
//!
//! The domain layer knows nothing about SQLite, Tauri or Win32 — it only
//! describes *what* a clipboard entry is. Persistence and capture map onto
//! these types from the outside.

use serde::{Deserialize, Serialize};
use std::fmt;

/// What kind of thing the user copied. Detected by [`crate::domain::classify`],
/// never trusted from the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Plain prose / anything that didn't match a sharper category.
    Text,
    /// A single URL, or text that is overwhelmingly one link.
    Link,
    /// An email address.
    Email,
    /// A phone number.
    Phone,
    /// A hex / rgb / hsl colour literal.
    Color,
    /// Source code, with `subkind` carrying the detected language.
    Code,
    /// Well-formed JSON.
    Json,
    /// A raster image (screenshots, copied pictures).
    Image,
    /// One or more file-system paths copied from a file manager.
    Files,
    /// Rich text captured with HTML markup alongside the plain fallback.
    Rich,
    /// Something that looks like a credential and is encrypted at rest.
    Secret,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Link => "link",
            Kind::Email => "email",
            Kind::Phone => "phone",
            Kind::Color => "color",
            Kind::Code => "code",
            Kind::Json => "json",
            Kind::Image => "image",
            Kind::Files => "files",
            Kind::Rich => "rich",
            Kind::Secret => "secret",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "text" => Kind::Text,
            "link" => Kind::Link,
            "email" => Kind::Email,
            "phone" => Kind::Phone,
            "color" => Kind::Color,
            "code" => Kind::Code,
            "json" => Kind::Json,
            "image" => Kind::Image,
            "files" => Kind::Files,
            "rich" => Kind::Rich,
            "secret" => Kind::Secret,
            _ => return None,
        })
    }

    /// Binary kinds keep their payload in the blob store rather than the DB row.
    pub fn is_binary(self) -> bool {
        matches!(self, Kind::Image)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Extra per-kind facts, kept as a single JSON column so new kinds never need a
/// migration. Every field is optional by design.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Meta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Data-URI thumbnail (PNG) used by the list view — small enough to inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    /// Absolute paths for [`Kind::Files`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Host part of a link, precomputed for grouping and favicons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Detected programming language for [`Kind::Code`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Normalised `#rrggbb` for [`Kind::Color`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// HTML flavour captured alongside plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// Why the capture was flagged sensitive (e.g. "aws_key", "jwt").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_reason: Option<String>,
    /// Line / word / char counts, computed once at capture time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chars: Option<u32>,
    /// AI-generated one-line summary, filled in lazily and cached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// A tag attached to items. Colours are stored as hex so the UI owns rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub count: i64,
}

/// A user-curated grouping. Unlike tags, an item's position inside a collection
/// is meaningful and reorderable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub color: String,
    pub sort: i64,
    #[serde(default)]
    pub count: i64,
}

/// A clipboard entry as the rest of the system sees it.
///
/// `body` is `None` for encrypted or blob-backed items; the caller asks for the
/// full payload separately so list rendering never pays for megabyte-sized
/// content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub uuid: String,
    pub kind: Kind,
    /// Short, always-safe-to-render excerpt used by the list and by FTS.
    pub preview: String,
    /// Full text payload. Absent for images, encrypted items and oversized text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Blob-store key when the payload lives on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    pub bytes: i64,
    pub meta: Meta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_title: Option<String>,
    pub pinned: bool,
    pub favorite: bool,
    pub encrypted: bool,
    pub sensitive: bool,
    pub use_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_used_at: i64,
    #[serde(default)]
    pub tags: Vec<Tag>,
}

/// Everything the capture pipeline produces before the repository assigns an id.
#[derive(Debug, Clone)]
pub struct NewItem {
    pub kind: Kind,
    pub preview: String,
    pub body: Option<String>,
    pub blob: Option<String>,
    pub bytes: i64,
    pub meta: Meta,
    pub hash: String,
    pub source_app: Option<String>,
    pub source_title: Option<String>,
    pub sensitive: bool,
}

/// The raw, unclassified snapshot handed over by the platform clipboard reader.
#[derive(Debug, Clone)]
pub enum Snapshot {
    Text { text: String, html: Option<String> },
    Image { png: Vec<u8>, width: u32, height: u32 },
    Files(Vec<String>),
}

impl Snapshot {
    /// Content used for dedupe hashing. Two copies of the same thing must
    /// produce identical bytes here regardless of which app they came from.
    pub fn hash_material(&self) -> Vec<u8> {
        match self {
            // Trailing-whitespace-only differences are treated as the same copy.
            Snapshot::Text { text, .. } => text.trim_end().as_bytes().to_vec(),
            Snapshot::Image { png, .. } => png.clone(),
            Snapshot::Files(paths) => {
                let mut sorted = paths.clone();
                sorted.sort();
                sorted.join("\u{1}").into_bytes()
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Snapshot::Text { text, .. } => text.trim().is_empty(),
            Snapshot::Image { png, .. } => png.is_empty(),
            Snapshot::Files(p) => p.is_empty(),
        }
    }
}
