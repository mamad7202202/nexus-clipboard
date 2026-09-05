//! Optional AI assistance.
//!
//! Every function here is a no-op unless the user has explicitly configured a
//! provider and key. That is the whole privacy contract: local-first by
//! default, network only on demand, and only the specific item the user asked
//! about — never the history as a whole.

use serde::{Deserialize, Serialize};

use crate::app::state::AppState;
use crate::config::AiProvider;
use crate::error::{Error, Result};

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const OPENAI_DEFAULT_URL: &str = "https://api.openai.com/v1/chat/completions";

/// Transformations the UI can request on a text item. Deterministic ones run
/// locally with no network call at all; only [`Transform::Custom`],
/// [`Transform::Summarize`], [`Transform::Explain`] and [`Transform::Translate`]
/// need a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Transform {
    Upper,
    Lower,
    Title,
    Sentence,
    Trim,
    /// Collapse all whitespace runs into single spaces.
    Collapse,
    /// Strip every line's leading indentation.
    Dedent,
    Slugify,
    /// camelCase / snake_case / kebab-case conversions.
    Camel,
    Snake,
    Kebab,
    Base64Encode,
    Base64Decode,
    UrlEncode,
    UrlDecode,
    JsonPretty,
    JsonMinify,
    /// Reverse the order of lines.
    ReverseLines,
    SortLines,
    /// Remove duplicate lines, preserving first occurrence.
    DedupeLines,
    CountLines,
    Summarize,
    Explain,
    Translate { language: String },
    Custom { instruction: String },
}

impl Transform {
    /// Whether applying this transform contacts a provider.
    pub fn needs_ai(&self) -> bool {
        matches!(
            self,
            Transform::Summarize
                | Transform::Explain
                | Transform::Translate { .. }
                | Transform::Custom { .. }
        )
    }
}

/// Apply a transform. Local ones are pure functions; AI ones go over the wire.
pub async fn apply(state: &AppState, input: &str, transform: Transform) -> Result<String> {
    if !transform.needs_ai() {
        return local(input, &transform);
    }

    let settings = state.settings();
    if settings.ai.provider == AiProvider::Disabled {
        return Err(Error::Ai(
            "AI features are turned off. Enable a provider in Settings → Intelligence.".into(),
        ));
    }

    let prompt = match &transform {
        Transform::Summarize => {
            "Summarize the following clipboard content in one clear sentence. \
             Reply with the summary only.".to_string()
        }
        Transform::Explain => {
            "Explain what the following clipboard content is and what it does. \
             Be concise — at most three sentences.".to_string()
        }
        Transform::Translate { language } => {
            format!("Translate the following text into {language}. Reply with the translation only.")
        }
        Transform::Custom { instruction } => instruction.clone(),
        // Unreachable: every other variant returned above.
        _ => return local(input, &transform),
    };

    complete(state, &prompt, input).await
}

/// Deterministic, offline transforms.
fn local(input: &str, transform: &Transform) -> Result<String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;

    Ok(match transform {
        Transform::Upper => input.to_uppercase(),
        Transform::Lower => input.to_lowercase(),
        Transform::Title => title_case(input),
        Transform::Sentence => sentence_case(input),
        Transform::Trim => input.trim().to_string(),
        Transform::Collapse => input.split_whitespace().collect::<Vec<_>>().join(" "),
        Transform::Dedent => dedent(input),
        Transform::Slugify => slugify(input),
        Transform::Camel => to_camel(input),
        Transform::Snake => to_delimited(input, '_'),
        Transform::Kebab => to_delimited(input, '-'),
        Transform::Base64Encode => b64.encode(input.as_bytes()),
        Transform::Base64Decode => {
            let bytes = b64
                .decode(input.trim())
                .map_err(|e| Error::invalid(format!("not valid base64: {e}")))?;
            String::from_utf8(bytes)
                .map_err(|_| Error::invalid("decoded bytes are not valid UTF-8"))?
        }
        Transform::UrlEncode => urlencoding::encode(input).into_owned(),
        Transform::UrlDecode => urlencoding::decode(input)
            .map_err(|e| Error::invalid(format!("not valid percent-encoding: {e}")))?
            .into_owned(),
        Transform::JsonPretty => {
            let value: serde_json::Value = serde_json::from_str(input)
                .map_err(|e| Error::invalid(format!("not valid JSON: {e}")))?;
            serde_json::to_string_pretty(&value)?
        }
        Transform::JsonMinify => {
            let value: serde_json::Value = serde_json::from_str(input)
                .map_err(|e| Error::invalid(format!("not valid JSON: {e}")))?;
            serde_json::to_string(&value)?
        }
        Transform::ReverseLines => input.lines().rev().collect::<Vec<_>>().join("\n"),
        Transform::SortLines => {
            let mut lines: Vec<&str> = input.lines().collect();
            lines.sort_unstable();
            lines.join("\n")
        }
        Transform::DedupeLines => {
            let mut seen = std::collections::HashSet::new();
            input
                .lines()
                .filter(|l| seen.insert(*l))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Transform::CountLines => format!(
            "{} lines · {} words · {} characters",
            input.lines().count(),
            input.split_whitespace().count(),
            input.chars().count()
        ),
        // AI variants never reach here.
        _ => input.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicBlock>,
}

#[derive(Deserialize)]
struct AnthropicBlock {
    #[serde(default)]
    text: String,
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}

async fn complete(state: &AppState, system: &str, input: &str) -> Result<String> {
    let settings = state.settings();

    // Guard rail: never ship an unbounded payload to a third party.
    const MAX_INPUT: usize = 24_000;
    let input = if input.len() > MAX_INPUT {
        &input[..input
            .char_indices()
            .take_while(|(i, _)| *i < MAX_INPUT)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0)]
    } else {
        input
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| Error::Ai(e.to_string()))?;

    match settings.ai.provider {
        AiProvider::Disabled => {
            Err(Error::Ai("AI features are turned off.".into()))
        }
        AiProvider::Anthropic => {
            let api_key = settings
                .ai
                .api_key
                .clone()
                .filter(|k| !k.trim().is_empty())
                .ok_or_else(|| Error::Ai("no API key is configured for Anthropic".into()))?;

            let raw_endpoint = settings
                .ai
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .unwrap_or(ANTHROPIC_URL);

            let mut endpoint = raw_endpoint.trim_end_matches('/').to_string();
            if !endpoint.ends_with("/messages") {
                if endpoint.ends_with("/v1") {
                    endpoint.push_str("/messages");
                } else {
                    endpoint.push_str("/v1/messages");
                }
            }

            let body = AnthropicRequest {
                model: &settings.ai.model,
                max_tokens: 1024,
                system,
                messages: vec![AnthropicMessage { role: "user", content: input }],
            };

            let response = client
                .post(&endpoint)
                .header("x-api-key", api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| Error::Ai(format!("request failed: {e}")))?;

            let status = response.status();
            if !status.is_success() {
                let detail = response.text().await.unwrap_or_default();
                return Err(Error::Ai(format!("provider returned {status}: {}", truncate(&detail, 300))));
            }

            let raw_text = response
                .text()
                .await
                .map_err(|e| Error::Ai(format!("failed to read response: {e}")))?;

            let parsed: AnthropicResponse = serde_json::from_str(&raw_text)
                .map_err(|e| Error::Ai(format!("unexpected response shape: {e} (body: {})", truncate(&raw_text, 300))))?;

            let text = parsed
                .content
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("")
                .trim()
                .to_string();

            if text.is_empty() {
                return Err(Error::Ai("the provider returned an empty response".into()));
            }
            Ok(text)
        }
        AiProvider::Openai | AiProvider::Custom => {
            let default_url = OPENAI_DEFAULT_URL;
            let raw_url = settings
                .ai
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .unwrap_or(default_url);

            // Normalize endpoint: allow passing either the root URL, /v1, or /chat/completions
            let mut url = raw_url.trim_end_matches('/').to_string();
            if !url.ends_with("/chat/completions") {
                if url.ends_with("/v1") {
                    url.push_str("/chat/completions");
                } else {
                    url.push_str("/v1/chat/completions");
                }
            }

            let is_o_series = settings.ai.model.starts_with("o1") || settings.ai.model.starts_with("o3");
            let (max_tokens, max_completion_tokens) = if is_o_series {
                (None, Some(1024))
            } else {
                (Some(1024), None)
            };

            let body = OpenAiRequest {
                model: &settings.ai.model,
                messages: vec![
                    OpenAiMessage { role: "system", content: system },
                    OpenAiMessage { role: "user", content: input },
                ],
                max_tokens,
                max_completion_tokens,
            };

            let mut req = client
                .post(&url)
                .header("content-type", "application/json")
                .json(&body);

            if let Some(key) = settings.ai.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
                req = req.header("authorization", format!("Bearer {key}"));
            }

            let response = req
                .send()
                .await
                .map_err(|e| Error::Ai(format!("request failed: {e}")))?;

            let status = response.status();
            if !status.is_success() {
                let detail = response.text().await.unwrap_or_default();
                return Err(Error::Ai(format!("provider returned {status}: {}", truncate(&detail, 300))));
            }

            let raw_text = response
                .text()
                .await
                .map_err(|e| Error::Ai(format!("failed to read response: {e}")))?;

            let parsed: OpenAiResponse = serde_json::from_str(&raw_text)
                .map_err(|e| Error::Ai(format!("unexpected response shape: {e} (body: {})", truncate(&raw_text, 300))))?;

            let text = parsed
                .choices
                .into_iter()
                .next()
                .and_then(|c| c.message.content.or(c.message.reasoning_content))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();

            if text.is_empty() {
                return Err(Error::Ai("the provider returned an empty response".into()));
            }
            Ok(text)
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

// ---------------------------------------------------------------------------
// Local string helpers
// ---------------------------------------------------------------------------

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sentence_case(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut capitalize = true;
    for ch in lower.chars() {
        if capitalize && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            capitalize = false;
        } else {
            out.push(ch);
            if matches!(ch, '.' | '!' | '?' | '\n') {
                capitalize = true;
            }
        }
    }
    out
}

fn dedent(s: &str) -> String {
    let indent = s
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    s.lines()
        .map(|l| if l.len() >= indent { &l[indent..] } else { l.trim_start() })
        .collect::<Vec<_>>()
        .join("\n")
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = true; // suppresses a leading dash
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Split an identifier or phrase into its constituent words, handling
/// camelCase, snake_case, kebab-case and spaces alike.
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    for ch in s.chars() {
        if ch.is_alphanumeric() {
            // A lower→upper transition marks a camelCase boundary.
            if ch.is_uppercase() && current.chars().last().is_some_and(|p| p.is_lowercase()) {
                out.push(std::mem::take(&mut current));
            }
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }

    out.into_iter().map(|w| w.to_lowercase()).collect()
}

fn to_camel(s: &str) -> String {
    let parts = words(s);
    let mut out = String::new();
    for (i, word) in parts.iter().enumerate() {
        if i == 0 {
            out.push_str(word);
        } else {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    out
}

fn to_delimited(s: &str, sep: char) -> String {
    words(s).join(&sep.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str, t: Transform) -> String {
        local(input, &t).unwrap()
    }

    #[test]
    fn case_transforms() {
        assert_eq!(run("hello world", Transform::Upper), "HELLO WORLD");
        assert_eq!(run("HELLO WORLD", Transform::Title), "Hello World");
        assert_eq!(run("hi. there", Transform::Sentence), "Hi. There");
    }

    #[test]
    fn identifier_transforms() {
        assert_eq!(run("hello world", Transform::Camel), "helloWorld");
        assert_eq!(run("helloWorld", Transform::Snake), "hello_world");
        assert_eq!(run("hello_world", Transform::Kebab), "hello-world");
        assert_eq!(run("Hello, World!", Transform::Slugify), "hello-world");
    }

    #[test]
    fn base64_round_trips() {
        let encoded = run("سلام", Transform::Base64Encode);
        assert_eq!(run(&encoded, Transform::Base64Decode), "سلام");
    }

    #[test]
    fn invalid_base64_is_reported() {
        assert!(local("!!!not base64!!!", &Transform::Base64Decode).is_err());
    }

    #[test]
    fn json_transforms() {
        let pretty = run(r#"{"a":1}"#, Transform::JsonPretty);
        assert!(pretty.contains('\n'));
        assert_eq!(run(&pretty, Transform::JsonMinify), r#"{"a":1}"#);
    }

    #[test]
    fn invalid_json_is_reported() {
        assert!(local("{nope}", &Transform::JsonPretty).is_err());
    }

    #[test]
    fn line_transforms() {
        assert_eq!(run("b\na\nb", Transform::DedupeLines), "b\na");
        assert_eq!(run("b\na", Transform::SortLines), "a\nb");
        assert_eq!(run("a\nb", Transform::ReverseLines), "b\na");
    }

    #[test]
    fn dedent_strips_common_indentation() {
        assert_eq!(dedent("    a\n    b"), "a\nb");
        assert_eq!(dedent("  a\n    b"), "a\n  b");
    }

    #[test]
    fn ai_transforms_are_flagged() {
        assert!(Transform::Summarize.needs_ai());
        assert!(!Transform::Upper.needs_ai());
    }

    #[test]
    fn url_encoding_round_trips() {
        let encoded = run("a b&c", Transform::UrlEncode);
        assert_eq!(run(&encoded, Transform::UrlDecode), "a b&c");
    }
}
