//! Query model for the history list.
//!
//! One struct describes every way the UI can slice the history — free-text
//! search, kind filters, tags, collections, smart filters and pagination. The
//! repository compiles it into a single SQL statement; the UI never builds SQL.

use serde::{Deserialize, Serialize};

use super::item::Kind;

/// Built-in saved views. These are expressed here rather than as stored rows so
/// they stay consistent and cannot be corrupted by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartFilter {
    /// Everything that is not in the trash.
    All,
    /// Pinned items, newest first.
    Pinned,
    /// Favourites.
    Favorites,
    /// Copied in the last 24 hours.
    Today,
    /// Sensitive / encrypted entries.
    Vault,
    /// Most frequently re-used entries.
    Frequent,
    /// Never re-used since capture.
    Unused,
    /// Larger than 1 MB.
    Large,
    /// Soft-deleted entries awaiting purge.
    Trash,
}

impl Default for SmartFilter {
    fn default() -> Self {
        Self::All
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    /// Most recently copied or re-used first (the default timeline).
    Recent,
    /// Original capture time.
    Created,
    /// Re-use count.
    Frequency,
    /// Payload size.
    Size,
    /// Search relevance; falls back to `Recent` when there is no search text.
    Relevance,
}

impl Default for SortBy {
    fn default() -> Self {
        Self::Recent
    }
}

/// A page request. Pagination is keyset-based (`before`) rather than OFFSET so
/// deep scrolling stays O(1) even with millions of rows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Query {
    /// Free text. Compiled into an FTS5 MATCH when non-empty.
    pub text: String,
    /// Restrict to these kinds; empty means all kinds.
    pub kinds: Vec<Kind>,
    /// Require all of these tag ids.
    pub tags: Vec<i64>,
    /// Restrict to one collection.
    pub collection: Option<i64>,
    pub filter: SmartFilter,
    pub sort: SortBy,
    /// Restrict to captures from this application (executable name).
    pub source_app: Option<String>,
    /// Inclusive unix-ms range on the timeline.
    pub after: Option<i64>,
    pub before_time: Option<i64>,
    /// Keyset cursor: return rows ordered strictly after this one.
    pub cursor: Option<Cursor>,
    /// Page size. Clamped by the repository.
    pub limit: u32,
}

/// Opaque-to-the-UI cursor carrying both sort key and id, so ties never cause
/// skipped or repeated rows.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Cursor {
    /// Value of the active sort column on the last row of the previous page.
    pub key: i64,
    /// Id of that row, used as the tiebreaker.
    pub id: i64,
}

impl Query {
    pub const DEFAULT_LIMIT: u32 = 80;
    pub const MAX_LIMIT: u32 = 500;

    pub fn effective_limit(&self) -> u32 {
        match self.limit {
            0 => Self::DEFAULT_LIMIT,
            n => n.min(Self::MAX_LIMIT),
        }
    }

    /// Relevance ordering only makes sense with a search term.
    pub fn effective_sort(&self) -> SortBy {
        match self.sort {
            SortBy::Relevance if self.text.trim().is_empty() => SortBy::Recent,
            other => other,
        }
    }

    pub fn is_searching(&self) -> bool {
        !self.text.trim().is_empty()
    }
}

/// One page of results plus the cursor needed to fetch the next one.
#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<Cursor>,
    /// Total matching rows. Computed only when cheap (i.e. on the first page).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
}

/// Aggregate counters powering the sidebar badges.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    pub total: i64,
    pub pinned: i64,
    pub favorites: i64,
    pub today: i64,
    pub vault: i64,
    pub trash: i64,
    pub bytes: i64,
    pub by_kind: Vec<KindCount>,
    pub by_app: Vec<AppCount>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KindCount {
    pub kind: Kind,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppCount {
    pub app: String,
    pub count: i64,
}
