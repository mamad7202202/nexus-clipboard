//! Small shared helpers that belong to no particular layer.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current wall-clock time in milliseconds since the Unix epoch.
///
/// A single definition keeps every timestamp in the database on the same scale;
/// mixing seconds and millis is a classic source of "why is this item from 1970"
/// bugs.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        // A clock before the epoch is pathological; 0 keeps ordering sane.
        .unwrap_or(0)
}

/// BLAKE3 content hash, hex-encoded. Fast enough (>1 GB/s) to run on every
/// clipboard event without a perceptible cost.
pub fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Human-readable byte size for the UI, e.g. `1.4 MB`.
pub fn human_bytes(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stable_and_distinct() {
        assert_eq!(hash_bytes(b"abc"), hash_bytes(b"abc"));
        assert_ne!(hash_bytes(b"abc"), hash_bytes(b"abd"));
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(1_572_864), "1.5 MB");
    }

    #[test]
    fn now_is_plausible() {
        // Sometime after 2020 and before 2100.
        let n = now_ms();
        assert!(n > 1_577_836_800_000 && n < 4_102_444_800_000);
    }
}
