/**
 * The transform menu.
 *
 * Deterministic transforms run entirely in Rust with no network access; the
 * AI-backed ones are grouped separately and clearly labelled, so it is always
 * obvious which actions leave the machine.
 */

import { Sparkles } from "lucide-react";
import type { ReactNode } from "react";
import { useState } from "react";

import { Menu, MenuItem, MenuLabel, MenuSeparator } from "@/components/ui";
import * as api from "@/lib/api";
import type { Transform } from "@/lib/types";
import { errorMessage } from "@/lib/types";
import { useApp } from "@/store/app";

interface Entry {
  label: string;
  transform: Transform;
}

const TEXT: Entry[] = [
  { label: "UPPERCASE", transform: { kind: "upper" } },
  { label: "lowercase", transform: { kind: "lower" } },
  { label: "Title Case", transform: { kind: "title" } },
  { label: "Sentence case", transform: { kind: "sentence" } },
  { label: "Trim whitespace", transform: { kind: "trim" } },
  { label: "Collapse spaces", transform: { kind: "collapse" } },
  { label: "Remove indentation", transform: { kind: "dedent" } },
];

const IDENTIFIERS: Entry[] = [
  { label: "camelCase", transform: { kind: "camel" } },
  { label: "snake_case", transform: { kind: "snake" } },
  { label: "kebab-case", transform: { kind: "kebab" } },
  { label: "slug-ify", transform: { kind: "slugify" } },
];

const ENCODING: Entry[] = [
  { label: "Base64 encode", transform: { kind: "base64_encode" } },
  { label: "Base64 decode", transform: { kind: "base64_decode" } },
  { label: "URL encode", transform: { kind: "url_encode" } },
  { label: "URL decode", transform: { kind: "url_decode" } },
  { label: "Format JSON", transform: { kind: "json_pretty" } },
  { label: "Minify JSON", transform: { kind: "json_minify" } },
];

const LINES: Entry[] = [
  { label: "Sort lines", transform: { kind: "sort_lines" } },
  { label: "Reverse lines", transform: { kind: "reverse_lines" } },
  { label: "Remove duplicates", transform: { kind: "dedupe_lines" } },
  { label: "Count lines & words", transform: { kind: "count_lines" } },
];

const AI: Entry[] = [
  { label: "Summarise", transform: { kind: "summarize" } },
  { label: "Explain", transform: { kind: "explain" } },
  { label: "Translate to English", transform: { kind: "translate", language: "English" } },
];

export function TransformMenu({
  text,
  onResult,
  trigger,
}: {
  text: string;
  onResult: (result: string) => void;
  trigger: (props: { open: boolean; toggle: () => void }) => ReactNode;
}) {
  const [running, setRunning] = useState(false);
  const toast = useApp((s) => s.toast);
  const aiEnabled = useApp((s) => s.settings?.ai.provider !== "disabled");

  const apply = async (transform: Transform) => {
    if (running) return;
    setRunning(true);
    try {
      const result = await api.transformText(text, transform);
      onResult(result);
    } catch (e) {
      toast(errorMessage(e), "error");
    } finally {
      setRunning(false);
    }
  };

  const section = (label: string, entries: Entry[]) => (
    <>
      <MenuLabel>{label}</MenuLabel>
      {entries.map((entry) => (
        <MenuItem key={entry.label} onSelect={() => void apply(entry.transform)}>
          {entry.label}
        </MenuItem>
      ))}
    </>
  );

  return (
    <Menu trigger={trigger} width={230}>
      <div className="scroll-area max-h-[380px] overflow-y-auto">
        {section("Text", TEXT)}
        <MenuSeparator />
        {section("Identifiers", IDENTIFIERS)}
        <MenuSeparator />
        {section("Encoding", ENCODING)}
        <MenuSeparator />
        {section("Lines", LINES)}
        <MenuSeparator />
        <MenuLabel>Intelligence</MenuLabel>
        {AI.map((entry) => (
          <MenuItem
            key={entry.label}
            disabled={!aiEnabled}
            icon={<Sparkles />}
            onSelect={() => void apply(entry.transform)}
          >
            {entry.label}
          </MenuItem>
        ))}
        {!aiEnabled && (
          <p className="px-2 pb-1.5 pt-0.5 text-[11px] leading-snug text-text-faint">
            Turn on a provider in Settings → Intelligence to enable these.
          </p>
        )}
      </div>
    </Menu>
  );
}
