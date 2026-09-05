/**
 * Presentation helpers. Pure functions only — everything here is safe to call
 * during render.
 */

import type { Item, Kind } from "./types";

/** `1.4 MB`, `812 B`. */
export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = n / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** `just now`, `4m`, `3h`, `Tue`, `12 Mar`. Tuned to stay short in a list. */
export function relativeTime(ms: number): string {
  const delta = Date.now() - ms;

  if (delta < 45_000) return "just now";
  if (delta < HOUR) return `${Math.round(delta / MINUTE)}m ago`;
  if (delta < DAY) return `${Math.round(delta / HOUR)}h ago`;
  if (delta < 7 * DAY) {
    return new Date(ms).toLocaleDateString(undefined, { weekday: "short" });
  }
  const date = new Date(ms);
  const sameYear = date.getFullYear() === new Date().getFullYear();
  return date.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    ...(sameYear ? {} : { year: "numeric" }),
  });
}

export function fullTime(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

/** Day bucket used to group the timeline. */
export function dayBucket(ms: number): string {
  const date = new Date(ms);
  const today = new Date();
  const yesterday = new Date(today.getTime() - DAY);

  const sameDay = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate();

  if (sameDay(date, today)) return "Today";
  if (sameDay(date, yesterday)) return "Yesterday";
  if (Date.now() - ms < 7 * DAY) {
    return date.toLocaleDateString(undefined, { weekday: "long" });
  }
  return date.toLocaleDateString(undefined, {
    day: "numeric",
    month: "long",
    year: date.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}

const KIND_LABELS: Record<Kind, string> = {
  text: "Text",
  link: "Link",
  email: "Email",
  phone: "Phone",
  color: "Colour",
  code: "Code",
  json: "JSON",
  image: "Image",
  files: "Files",
  rich: "Rich text",
  secret: "Secret",
};

export const kindLabel = (kind: Kind): string => KIND_LABELS[kind] ?? kind;

/**
 * Accent colour per kind, expressed as an OKLCH hue so it harmonises with the
 * palette regardless of which accent the user picked.
 */
const KIND_HUES: Record<Kind, number> = {
  text: 265,
  link: 240,
  email: 200,
  phone: 170,
  color: 320,
  code: 285,
  json: 55,
  image: 150,
  files: 30,
  rich: 300,
  secret: 25,
};

export function kindColor(kind: Kind): string {
  return `oklch(68% 0.15 ${KIND_HUES[kind] ?? 265})`;
}

/** Short descriptor shown under an item's preview. */
export function itemSubtitle(item: Item): string {
  const parts: string[] = [kindLabel(item.kind)];

  switch (item.kind) {
    case "image":
      if (item.meta.width && item.meta.height) {
        parts[0] = `${item.meta.width} × ${item.meta.height}`;
      }
      break;
    case "code":
      if (item.meta.language) parts[0] = item.meta.language;
      break;
    case "link":
      if (item.meta.host) parts[0] = item.meta.host;
      break;
    case "files": {
      const n = item.meta.files?.length ?? 0;
      parts[0] = n === 1 ? "1 file" : `${n} files`;
      break;
    }
    case "text":
    case "rich":
      if (item.meta.words) {
        parts[0] = `${item.meta.words} ${item.meta.words === 1 ? "word" : "words"}`;
      }
      break;
  }

  if (item.bytes > 0) parts.push(bytes(item.bytes));
  if (item.source_app) parts.push(appName(item.source_app));

  return parts.join(" · ");
}

/** `Code.exe` → `Code`. */
export function appName(exe: string): string {
  return exe.replace(/\.exe$/i, "");
}

/** Case-insensitively split `text` around every occurrence of `needle`. */
export function highlightParts(
  text: string,
  needle: string,
): Array<{ text: string; hit: boolean }> {
  const term = needle.trim();
  if (!term) return [{ text, hit: false }];

  // Search terms are user input, so escape before building the pattern.
  const escaped = term
    .split(/\s+/)
    .filter(Boolean)
    .map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join("|");
  if (!escaped) return [{ text, hit: false }];

  const parts: Array<{ text: string; hit: boolean }> = [];
  const regex = new RegExp(`(${escaped})`, "gi");
  let last = 0;

  for (const match of text.matchAll(regex)) {
    const index = match.index ?? 0;
    if (index > last) parts.push({ text: text.slice(last, index), hit: false });
    parts.push({ text: match[0], hit: true });
    last = index + match[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last), hit: false });

  return parts.length ? parts : [{ text, hit: false }];
}

/** Render a keyboard accelerator for display: `CommandOrControl+Shift+V` → `Ctrl ⇧ V`. */
export function prettyAccelerator(accelerator: string): string[] {
  return accelerator
    .split("+")
    .map((part) => {
      switch (part.toLowerCase()) {
        case "commandorcontrol":
        case "cmdorctrl":
        case "control":
        case "ctrl":
          return "Ctrl";
        case "shift":
          return "⇧";
        case "alt":
          return "Alt";
        case "super":
        case "meta":
          return "Win";
        default:
          return part.toUpperCase();
      }
    })
    .filter(Boolean);
}

/** Number with thousands separators, for counts in the sidebar. */
export function count(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}
