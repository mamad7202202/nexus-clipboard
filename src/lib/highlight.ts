/**
 * Syntax highlighting.
 *
 * `highlight.js` registers ~190 languages by default, which costs ~900 KB. We
 * register only the languages the classifier can produce, which keeps the
 * bundle small and startup fast.
 */

import hljs from "highlight.js/lib/core";

import bash from "highlight.js/lib/languages/bash";
import cpp from "highlight.js/lib/languages/cpp";
import css from "highlight.js/lib/languages/css";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import php from "highlight.js/lib/languages/php";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

const LANGUAGES = {
  bash,
  cpp,
  css,
  dockerfile,
  go,
  java,
  javascript,
  json,
  php,
  python,
  rust,
  sql,
  typescript,
  // `xml` covers HTML too, which is how highlight.js names it.
  xml,
  yaml,
} as const;

for (const [name, definition] of Object.entries(LANGUAGES)) {
  hljs.registerLanguage(name, definition);
}
hljs.registerAliases(["html"], { languageName: "xml" });

hljs.configure({ ignoreUnescapedHTML: true, throwUnescapedHTML: false });

/** Highlighting a megabyte of code would block the UI thread for seconds. */
const MAX_HIGHLIGHT_BYTES = 120_000;

/**
 * Return highlighted HTML, or `null` when the caller should render plain text.
 *
 * The output is trusted: highlight.js escapes the source before wrapping it in
 * spans, so this is safe for `dangerouslySetInnerHTML`. Plain-text fallbacks go
 * through React's normal escaping instead.
 */
export function highlight(code: string, language?: string): string | null {
  if (code.length > MAX_HIGHLIGHT_BYTES) return null;

  try {
    if (language && hljs.getLanguage(language)) {
      return hljs.highlight(code, { language, ignoreIllegals: true }).value;
    }
    // Auto-detection is limited to the registered set, so it stays fast.
    const result = hljs.highlightAuto(code, Object.keys(LANGUAGES));
    return result.relevance > 4 ? result.value : null;
  } catch {
    return null;
  }
}

export function isSupported(language?: string): boolean {
  return Boolean(language && hljs.getLanguage(language));
}
