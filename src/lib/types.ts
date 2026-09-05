/**
 * TypeScript mirrors of the Rust domain types.
 *
 * These must stay in sync with `src-tauri/src/domain` and
 * `src-tauri/src/config.rs`. Serde uses snake_case for enum variants and field
 * names, so the shapes below match the wire format exactly — no transformation
 * layer, and therefore nothing to drift.
 */

export type Kind =
  | "text"
  | "link"
  | "email"
  | "phone"
  | "color"
  | "code"
  | "json"
  | "image"
  | "files"
  | "rich"
  | "secret";

export const ALL_KINDS: Kind[] = [
  "text",
  "link",
  "code",
  "json",
  "image",
  "files",
  "color",
  "email",
  "phone",
  "rich",
  "secret",
];

export interface Meta {
  width?: number;
  height?: number;
  thumb?: string;
  files?: string[];
  host?: string;
  language?: string;
  color?: string;
  html?: string;
  secret_reason?: string;
  lines?: number;
  words?: number;
  chars?: number;
  summary?: string;
}

export interface Tag {
  id: number;
  name: string;
  color: string;
  count: number;
}

export interface Collection {
  id: number;
  name: string;
  icon: string;
  color: string;
  sort: number;
  count: number;
}

export interface Item {
  id: number;
  uuid: string;
  kind: Kind;
  preview: string;
  body?: string;
  blob?: string;
  bytes: number;
  meta: Meta;
  source_app?: string;
  source_title?: string;
  pinned: boolean;
  favorite: boolean;
  encrypted: boolean;
  sensitive: boolean;
  use_count: number;
  created_at: number;
  updated_at: number;
  last_used_at: number;
  tags: Tag[];
}

export type SmartFilter =
  | "all"
  | "pinned"
  | "favorites"
  | "today"
  | "vault"
  | "frequent"
  | "unused"
  | "large"
  | "trash";

export type SortBy = "recent" | "created" | "frequency" | "size" | "relevance";

export interface Cursor {
  key: number;
  id: number;
}

export interface Query {
  text: string;
  kinds: Kind[];
  tags: number[];
  collection: number | null;
  filter: SmartFilter;
  sort: SortBy;
  source_app: string | null;
  after: number | null;
  before_time: number | null;
  cursor: Cursor | null;
  limit: number;
}

export const emptyQuery = (): Query => ({
  text: "",
  kinds: [],
  tags: [],
  collection: null,
  filter: "all",
  sort: "recent",
  source_app: null,
  after: null,
  before_time: null,
  cursor: null,
  limit: 80,
});

export interface Page<T> {
  items: T[];
  next?: Cursor;
  total?: number;
}

export interface KindCount {
  kind: Kind;
  count: number;
}

export interface AppCount {
  app: string;
  count: number;
}

export interface Stats {
  total: number;
  pinned: number;
  favorites: number;
  today: number;
  vault: number;
  trash: number;
  bytes: number;
  by_kind: KindCount[];
  by_app: AppCount[];
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

export type Retention = "forever" | { days: number };
export type Theme = "system" | "light" | "dark";
export type Density = "compact" | "comfortable" | "spacious";
export type AiProvider = "disabled" | "anthropic" | "openai" | "custom";

export interface AiSettings {
  provider: AiProvider;
  api_key?: string;
  model: string;
  base_url?: string;
  auto_summarize: boolean;
  summarize_threshold: number;
}

export interface Settings {
  capture_enabled: boolean;
  capture_images: boolean;
  capture_files: boolean;
  max_capture_bytes: number;
  encrypt_secrets: boolean;
  respect_ignore_list: boolean;

  retention: Retention;
  max_items: number;
  trash_days: number;

  theme: Theme;
  density: Density;
  accent: string;
  launcher_follows_cursor: boolean;
  paste_on_select: boolean;
  hide_after_paste: boolean;
  blur_secrets: boolean;

  hotkey_launcher: string;
  hotkey_palette: string;
  hotkey_quick_paste: string;

  start_with_system: boolean;
  start_minimized: boolean;
  show_tray_icon: boolean;

  ai: AiSettings;

  sync_folder: string;
  sync_enabled: boolean;
}

export interface VaultStatus {
  configured: boolean;
  unlocked: boolean;
  requires_passphrase: boolean;
}

export interface Bootstrap {
  settings: Settings;
  stats: Stats;
  tags: Tag[];
  collections: Collection[];
  vault: VaultStatus;
  version: string;
  platform: string;
}

export interface Diagnostics {
  version: string;
  data_dir: string;
  db_bytes: number;
  blob_bytes: number;
  schema_version: number;
  watcher_running: boolean;
  vault_unlocked: boolean;
}

export interface MaintenanceReport {
  expired: number;
  overflowed: number;
  purged: number;
  blobs_removed: number;
  bytes_freed: number;
}

export interface ImportReport {
  read: number;
  imported: number;
  duplicates: number;
  skipped: number;
}

export interface ExportOptions {
  include_secrets: boolean;
  include_blobs: boolean;
  only_starred: boolean;
}

export type PasteMode = "copy_only" | "paste" | "copy_plain";

/** Mirrors `features::ai::Transform`, which serde tags with `kind`. */
export type Transform =
  | { kind: "upper" }
  | { kind: "lower" }
  | { kind: "title" }
  | { kind: "sentence" }
  | { kind: "trim" }
  | { kind: "collapse" }
  | { kind: "dedent" }
  | { kind: "slugify" }
  | { kind: "camel" }
  | { kind: "snake" }
  | { kind: "kebab" }
  | { kind: "base64_encode" }
  | { kind: "base64_decode" }
  | { kind: "url_encode" }
  | { kind: "url_decode" }
  | { kind: "json_pretty" }
  | { kind: "json_minify" }
  | { kind: "reverse_lines" }
  | { kind: "sort_lines" }
  | { kind: "dedupe_lines" }
  | { kind: "count_lines" }
  | { kind: "summarize" }
  | { kind: "explain" }
  | { kind: "translate"; language: string }
  | { kind: "custom"; instruction: string };

/** The `{code, message}` shape every failed command returns. */
export interface BackendError {
  code: string;
  message: string;
}

export function isBackendError(e: unknown): e is BackendError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isBackendError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}
