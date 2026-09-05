//! Content classification: turns a raw [`Snapshot`] into a typed [`NewItem`].
//!
//! The rules are ordered from most specific to most general, and every regex is
//! compiled exactly once. Classification runs on the capture thread for every
//! copy, so it must stay allocation-light and finish in microseconds.

use once_cell::sync::Lazy;
use regex::Regex;

use super::item::{Kind, Meta, NewItem, Snapshot};

/// Text longer than this is never scanned character-by-character by the more
/// expensive heuristics; we sample the head instead.
const DEEP_SCAN_LIMIT: usize = 16 * 1024;

/// Preview text stored on the row. Long enough to be useful in the list and for
/// full-text search, short enough that list queries stay cheap.
pub const PREVIEW_LIMIT: usize = 512;

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

static RE_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(https?|ftp|file|ssh|git|mailto|data):\S+$").unwrap()
});

static RE_URL_LOOSE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(www\.)?([a-z0-9-]+\.)+[a-z]{2,}(/\S*)?$").unwrap()
});

static RE_EMAIL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}$").unwrap()
});

static RE_PHONE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\+?[\d][\d\s\-().]{6,20}\d$").unwrap()
});

static RE_HEX_COLOR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^#([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$").unwrap()
});

static RE_FUNC_COLOR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(rgb|rgba|hsl|hsla|oklch|lab)\(\s*[\d.%\s,/-]+\)$").unwrap()
});

/// Signals that a blob of text is code rather than prose. Each hit adds weight;
/// see [`code_score`].
static RE_CODE_SIGNALS: Lazy<Vec<(Regex, &'static str, u32)>> = Lazy::new(|| {
    vec![
        (Regex::new(r"(?m)^\s*(fn|impl|pub fn|let mut|use crate::|#\[derive)").unwrap(), "rust", 4),
        (Regex::new(r"(?m)^\s*(def |class |import |from \w+ import|if __name__)").unwrap(), "python", 4),
        (Regex::new(r"(?m)^\s*(function |const |let |var |=>|export default|import .* from)").unwrap(), "javascript", 3),
        (Regex::new(r"(?m)^\s*(interface |type \w+ =|enum |declare |: string|: number)").unwrap(), "typescript", 3),
        (Regex::new(r"(?m)^\s*(public|private|protected)\s+(static\s+)?(class|void|int|String)").unwrap(), "java", 4),
        (Regex::new(r"(?m)^\s*(#include|std::|template<|int main\()").unwrap(), "cpp", 4),
        (Regex::new(r"(?m)^\s*(func |package |go func|:= )").unwrap(), "go", 4),
        (Regex::new(r"(?i)(?m)^\s*(SELECT |INSERT INTO|UPDATE .* SET|CREATE TABLE|ALTER TABLE)").unwrap(), "sql", 4),
        (Regex::new(r"(?m)^\s*(<\?php|\$\w+\s*=)").unwrap(), "php", 4),
        (Regex::new(r"(?m)^\s*(<[a-zA-Z][\w-]*[\s>/]|</[a-zA-Z])").unwrap(), "html", 3),
        (Regex::new(r"(?m)^\s*[.#]?[\w-]+\s*\{[^}]*:[^}]*;").unwrap(), "css", 3),
        (Regex::new(r"(?m)^\s*(\$ |sudo |npm |pnpm |yarn |cargo |git |docker |curl |echo )").unwrap(), "bash", 3),
        (Regex::new(r"(?m)^\s*(FROM |RUN |CMD |ENTRYPOINT |WORKDIR )").unwrap(), "dockerfile", 4),
        (Regex::new(r"(?m)^\s*<\?xml|^\s*<\w+:\w+").unwrap(), "xml", 3),
        (Regex::new(r"(?m)^\s*(- |[\w-]+:\s)").unwrap(), "yaml", 1),
    ]
});

/// Credential shapes. Matching any of these marks the capture sensitive, which
/// causes the pipeline to encrypt the payload at rest.
static RE_SECRETS: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    vec![
        (Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(), "aws_access_key"),
        (Regex::new(r"(?i)aws_secret_access_key\s*[=:]\s*\S{30,}").unwrap(), "aws_secret"),
        (Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{30,}\b").unwrap(), "github_token"),
        (Regex::new(r"\bsk-(ant-)?[A-Za-z0-9_\-]{20,}\b").unwrap(), "api_key"),
        (Regex::new(r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b").unwrap(), "slack_token"),
        (Regex::new(r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b").unwrap(), "jwt"),
        (Regex::new(r"-----BEGIN (RSA |EC |OPENSSH |PGP )?PRIVATE KEY-----").unwrap(), "private_key"),
        (Regex::new(r"(?i)\b(password|passwd|pwd|secret|token|api[_-]?key)\s*[=:]\s*\S{6,}").unwrap(), "credential_pair"),
        (Regex::new(r"\b(?:\d[ -]*?){13,19}\b").unwrap(), "card_number"),
        (Regex::new(r"(?i)\b(postgres|mysql|mongodb(\+srv)?|redis|amqp)://[^\s:]+:[^\s@]+@").unwrap(), "connection_string"),
    ]
});

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Classify a snapshot and build the persistable item.
///
/// `hash` is computed by the caller (it also drives dedupe, so it is needed
/// before we decide whether classification is worth doing at all).
pub fn classify(snapshot: &Snapshot, hash: String) -> NewItem {
    match snapshot {
        Snapshot::Image { png, width, height } => classify_image(png, *width, *height, hash),
        Snapshot::Files(paths) => classify_files(paths, hash),
        Snapshot::Text { text, html } => classify_text(text, html.as_deref(), hash),
    }
}

fn classify_image(png: &[u8], width: u32, height: u32, hash: String) -> NewItem {
    let meta = Meta {
        width: Some(width),
        height: Some(height),
        ..Default::default()
    };
    NewItem {
        kind: Kind::Image,
        preview: format!("Image {width}×{height}"),
        body: None,
        blob: None, // filled in by the capture pipeline once the blob is written
        bytes: png.len() as i64,
        meta,
        hash,
        source_app: None,
        source_title: None,
        sensitive: false,
    }
}

fn classify_files(paths: &[String], hash: String) -> NewItem {
    let preview = if paths.len() == 1 {
        file_name(&paths[0])
    } else {
        let first = paths.first().map(|p| file_name(p)).unwrap_or_default();
        format!("{first} + {} more", paths.len() - 1)
    };
    let joined = paths.join("\n");
    NewItem {
        kind: Kind::Files,
        bytes: joined.len() as i64,
        preview: truncate(&preview, PREVIEW_LIMIT),
        body: Some(joined),
        blob: None,
        meta: Meta {
            files: paths.to_vec(),
            ..Default::default()
        },
        hash,
        source_app: None,
        source_title: None,
        sensitive: false,
    }
}

fn classify_text(text: &str, html: Option<&str>, hash: String) -> NewItem {
    let trimmed = text.trim();
    let mut meta = Meta {
        chars: Some(text.chars().count() as u32),
        words: Some(text.split_whitespace().count() as u32),
        lines: Some(text.lines().count().max(1) as u32),
        html: html.map(|h| h.to_string()),
        ..Default::default()
    };

    // Secrets win over every other classification: we would rather encrypt a
    // false positive than store a real credential in the clear.
    let secret_reason = detect_secret(trimmed);
    let sensitive = secret_reason.is_some();
    if let Some(reason) = secret_reason {
        meta.secret_reason = Some(reason.to_string());
        return NewItem {
            kind: Kind::Secret,
            preview: mask_secret(trimmed),
            body: Some(text.to_string()),
            blob: None,
            bytes: text.len() as i64,
            meta,
            hash,
            source_app: None,
            source_title: None,
            sensitive,
        };
    }

    let kind = detect_text_kind(trimmed, html, &mut meta);

    NewItem {
        kind,
        preview: truncate(&normalize_preview(trimmed), PREVIEW_LIMIT),
        body: Some(text.to_string()),
        blob: None,
        bytes: text.len() as i64,
        meta,
        hash,
        source_app: None,
        source_title: None,
        sensitive: false,
    }
}

fn detect_text_kind(trimmed: &str, html: Option<&str>, meta: &mut Meta) -> Kind {
    let single_line = !trimmed.contains('\n');

    if single_line && trimmed.len() < 2048 {
        if RE_EMAIL.is_match(trimmed) {
            return Kind::Email;
        }
        if RE_URL.is_match(trimmed) || RE_URL_LOOSE.is_match(trimmed) {
            meta.host = extract_host(trimmed);
            return Kind::Link;
        }
        if RE_HEX_COLOR.is_match(trimmed) || RE_FUNC_COLOR.is_match(trimmed) {
            meta.color = normalize_color(trimmed);
            return Kind::Color;
        }
        // Digit-heavy strings only: avoids classifying "2024 was a good year".
        if RE_PHONE.is_match(trimmed)
            && trimmed.chars().filter(|c| c.is_ascii_digit()).count() >= 7
        {
            return Kind::Phone;
        }
    }

    if looks_like_json(trimmed) {
        return Kind::Json;
    }

    let scan = &trimmed[..trimmed.len().min(DEEP_SCAN_LIMIT)];
    if let Some((language, score)) = code_score(scan) {
        // Require either a strong single signal or multi-line structure so a
        // stray "const" in prose does not flip the whole item to code.
        if score >= 4 || (score >= 3 && scan.lines().count() > 1) {
            meta.language = Some(language.to_string());
            return Kind::Code;
        }
    }

    if html.is_some_and(|h| h.len() > trimmed.len() / 2 && h.contains('<')) {
        return Kind::Rich;
    }

    Kind::Text
}

// ---------------------------------------------------------------------------
// Heuristics
// ---------------------------------------------------------------------------

fn looks_like_json(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 2 {
        return false;
    }
    let bounded = (s.starts_with('{') && s.ends_with('}'))
        || (s.starts_with('[') && s.ends_with(']'));
    bounded && serde_json::from_str::<serde_json::Value>(s).is_ok()
}

/// Returns the best-scoring language and its accumulated weight.
fn code_score(s: &str) -> Option<(&'static str, u32)> {
    let mut best: Option<(&'static str, u32)> = None;
    let mut total = 0u32;

    for (re, lang, weight) in RE_CODE_SIGNALS.iter() {
        if re.is_match(s) {
            total += weight;
            let entry = best.get_or_insert((lang, 0));
            if *weight > entry.1 {
                *entry = (lang, *weight);
            }
        }
    }

    // Structural bonuses: braces, semicolon line endings and consistent indent
    // are language-agnostic evidence of code.
    let lines: Vec<&str> = s.lines().take(80).collect();
    if lines.len() > 2 {
        let indented = lines.iter().filter(|l| l.starts_with("  ") || l.starts_with('\t')).count();
        if indented * 3 >= lines.len() {
            total += 2;
        }
        let terminated = lines
            .iter()
            .filter(|l| {
                let t = l.trim_end();
                t.ends_with(';') || t.ends_with('{') || t.ends_with('}')
            })
            .count();
        if terminated * 3 >= lines.len() {
            total += 2;
        }
    }

    best.map(|(lang, _)| (lang, total))
}

fn detect_secret(s: &str) -> Option<&'static str> {
    if s.len() > DEEP_SCAN_LIMIT {
        return None;
    }
    for (re, reason) in RE_SECRETS.iter() {
        if re.is_match(s) {
            // Card-number detection is noisy; require a Luhn check before we
            // treat a long digit run as a real card.
            if *reason == "card_number" && !luhn_valid(s) {
                continue;
            }
            return Some(reason);
        }
    }
    None
}

fn luhn_valid(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 { doubled - 9 } else { doubled }
            } else {
                d
            }
        })
        .sum();
    sum % 10 == 0
}

/// Secrets get a masked preview so the list view never renders a live credential.
fn mask_secret(s: &str) -> String {
    let visible: String = s.chars().take(4).collect();
    let len = s.chars().count();
    format!("{visible}{} ({len} chars)", "•".repeat(8))
}

fn normalize_color(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let expanded = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            4 => hex.chars().take(3).flat_map(|c| [c, c]).collect::<String>(),
            6 => hex.to_string(),
            8 => hex[..6].to_string(),
            _ => return None,
        };
        return Some(format!("#{}", expanded.to_lowercase()));
    }
    // Functional notations are handed to the UI untouched; CSS can render them.
    Some(s.to_lowercase())
}

fn extract_host(url: &str) -> Option<String> {
    let without_scheme = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url);
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()?
        .split('@')
        .next_back()?
        .split(':')
        .next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.trim_start_matches("www.").to_lowercase())
    }
}

fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Collapses runs of whitespace so multi-line copies still read well in a
/// single-line list row, while preserving the first line break as a separator.
fn normalize_preview(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(PREVIEW_LIMIT * 2));
    let mut last_ws = false;
    for ch in s.chars().take(PREVIEW_LIMIT * 2) {
        if ch.is_whitespace() {
            if !last_ws {
                out.push(' ');
                last_ws = true;
            }
        } else {
            out.push(ch);
            last_ws = false;
        }
    }
    out.trim().to_string()
}

/// Truncate on a char boundary, never mid-grapheme.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of(s: &str) -> Kind {
        let snap = Snapshot::Text { text: s.into(), html: None };
        classify(&snap, "h".into()).kind
    }

    #[test]
    fn detects_links() {
        assert_eq!(kind_of("https://example.com/a?b=1"), Kind::Link);
        assert_eq!(kind_of("www.example.com"), Kind::Link);
    }

    #[test]
    fn detects_email_and_color() {
        assert_eq!(kind_of("someone@example.com"), Kind::Email);
        assert_eq!(kind_of("#1a2b3c"), Kind::Color);
        assert_eq!(kind_of("rgb(10, 20, 30)"), Kind::Color);
    }

    #[test]
    fn detects_json_over_code() {
        assert_eq!(kind_of(r#"{"a": 1, "b": [2,3]}"#), Kind::Json);
    }

    #[test]
    fn detects_code() {
        let rust = "pub fn main() {\n    let mut x = 1;\n    println!(\"{}\", x);\n}";
        assert_eq!(kind_of(rust), Kind::Code);
    }

    #[test]
    fn prose_stays_text() {
        assert_eq!(kind_of("The quick brown fox jumps over the lazy dog."), Kind::Text);
    }

    #[test]
    fn detects_secrets_and_masks_preview() {
        let snap = Snapshot::Text {
            text: "ghp_abcdefghijklmnopqrstuvwxyz012345".into(),
            html: None,
        };
        let item = classify(&snap, "h".into());
        assert_eq!(item.kind, Kind::Secret);
        assert!(item.sensitive);
        assert!(!item.preview.contains("abcdefghij"));
    }

    #[test]
    fn luhn_rejects_random_digits() {
        assert!(!luhn_valid("1234567890123"));
        assert!(luhn_valid("4242424242424242"));
    }

    #[test]
    fn normalizes_short_hex_colors() {
        assert_eq!(normalize_color("#ABC").as_deref(), Some("#aabbcc"));
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        let s = "سلام دنیا";
        let t = truncate(s, 5);
        assert!(t.chars().count() <= 6);
    }
}
