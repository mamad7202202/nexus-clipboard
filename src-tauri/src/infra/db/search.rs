//! Translating user search text into a safe FTS5 MATCH expression.
//!
//! FTS5 has its own query syntax, and raw user input routinely contains
//! characters that make it throw (`"`, `*`, `:`, `-`, `(`). Rather than reject
//! those, we tokenise the input ourselves and rebuild a well-formed expression,
//! so search can never fail on punctuation.

/// Words shorter than this are still indexed but do not get a prefix wildcard,
/// otherwise every one-letter query would scan the whole index.
const MIN_PREFIX_LEN: usize = 2;

/// Build an FTS5 MATCH expression from arbitrary user text.
///
/// Returns `None` when the input contains nothing searchable, in which case the
/// caller should fall back to a plain (non-FTS) listing.
///
/// Supported user syntax:
/// - bare words are ANDed, with the trailing word treated as a prefix so
///   results narrow as you type
/// - `"quoted phrases"` match verbatim
/// - `-word` excludes
pub fn build(input: &str) -> Option<String> {
    let tokens = tokenize(input);
    if tokens.is_empty() {
        return None;
    }

    let last = tokens.len() - 1;
    let mut parts: Vec<String> = Vec::with_capacity(tokens.len());

    for (i, token) in tokens.iter().enumerate() {
        let quoted = quote(&token.text);
        let part = match token.kind {
            TokenKind::Phrase => quoted,
            TokenKind::Exclude => format!("NOT {quoted}"),
            TokenKind::Word => {
                // Only the final word gets prefix matching: it is the one the
                // user is still typing.
                if i == last && token.text.chars().count() >= MIN_PREFIX_LEN {
                    format!("{quoted}*")
                } else {
                    quoted
                }
            }
        };
        parts.push(part);
    }

    // FTS5 treats juxtaposition as AND, but being explicit avoids surprises
    // when a NOT clause lands in the middle.
    let mut expr = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            expr.push_str(if part.starts_with("NOT ") { " " } else { " AND " });
        }
        expr.push_str(part);
    }

    Some(expr)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TokenKind {
    Word,
    Phrase,
    Exclude,
}

#[derive(Debug)]
struct Token {
    text: String,
    kind: TokenKind,
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }

        if c == '"' {
            chars.next();
            let mut phrase = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                phrase.push(c);
            }
            let cleaned = sanitize(&phrase);
            if !cleaned.is_empty() {
                tokens.push(Token { text: cleaned, kind: TokenKind::Phrase });
            }
            continue;
        }

        let exclude = c == '-' || c == '!';
        if exclude {
            chars.next();
        }

        let mut word = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            word.push(c);
            chars.next();
        }

        let cleaned = sanitize(&word);
        if !cleaned.is_empty() {
            tokens.push(Token {
                text: cleaned,
                kind: if exclude { TokenKind::Exclude } else { TokenKind::Word },
            });
        }
    }

    tokens
}

/// Strip everything the unicode61 tokenizer would not index anyway, so the
/// rebuilt expression can never contain FTS5 operators.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// FTS5 string literal: wrap in double quotes, doubling any inner quote.
/// Sanitisation already removed quotes, but this stays correct if that changes.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_has_no_expression() {
        assert!(build("").is_none());
        assert!(build("   ").is_none());
        assert!(build("!!!").is_none());
    }

    #[test]
    fn last_word_is_a_prefix() {
        let expr = build("hello wor").unwrap();
        assert_eq!(expr, "\"hello\" AND \"wor\"*");
    }

    #[test]
    fn single_char_gets_no_wildcard() {
        assert_eq!(build("a").unwrap(), "\"a\"");
    }

    #[test]
    fn phrases_are_preserved() {
        let expr = build("\"hello world\" x").unwrap();
        assert!(expr.contains("\"hello world\""));
    }

    #[test]
    fn exclusions_become_not() {
        let expr = build("rust -python").unwrap();
        assert!(expr.contains("NOT \"python\""));
    }

    #[test]
    fn fts_operators_cannot_leak_through() {
        // Any of these would be a syntax error if passed to FTS5 verbatim.
        for input in ["a AND (b OR c*", "foo: bar", "\"unclosed", "* * *", "NEAR(a b)"] {
            if let Some(expr) = build(input) {
                assert!(!expr.contains('('), "parens leaked from {input:?}: {expr}");
                assert!(!expr.contains(':'), "colon leaked from {input:?}: {expr}");
            }
        }
    }

    #[test]
    fn unicode_is_searchable() {
        let expr = build("سلام دنیا").unwrap();
        assert!(expr.contains("سلام"));
    }
}
