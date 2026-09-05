/**
 * Application store.
 *
 * One store holds the query, the loaded page, the selection and the reference
 * data (tags, collections, settings). Fetching is centralised here so the
 * components stay declarative and there is exactly one place where a race
 * between an in-flight query and a newer one is resolved.
 */

import { create } from "zustand";

import * as api from "@/lib/api";
import type {
  Collection,
  Cursor,
  Item,
  Kind,
  Query,
  Settings,
  SmartFilter,
  SortBy,
  Stats,
  Tag,
  VaultStatus,
} from "@/lib/types";
import { emptyQuery, errorMessage } from "@/lib/types";

export interface Toast {
  id: number;
  message: string;
  tone: "info" | "success" | "error";
}

interface AppStore {
  // --- data ---
  ready: boolean;
  items: Item[];
  total: number | null;
  cursor: Cursor | null;
  loading: boolean;
  loadingMore: boolean;
  error: string | null;

  query: Query;
  stats: Stats | null;
  tags: Tag[];
  collections: Collection[];
  settings: Settings | null;
  vault: VaultStatus | null;
  version: string;

  // --- ui ---
  /** Ids of selected rows, in selection order. */
  selection: number[];
  /** Row the keyboard cursor is on. */
  activeId: number | null;
  paletteOpen: boolean;
  toasts: Toast[];

  // --- actions ---
  init: () => Promise<void>;
  refresh: () => Promise<void>;
  loadMore: () => Promise<void>;
  setQuery: (patch: Partial<Query>) => void;
  setSearch: (text: string) => void;
  setFilter: (filter: SmartFilter) => void;
  setSort: (sort: SortBy) => void;
  toggleKind: (kind: Kind) => void;
  toggleTag: (id: number) => void;
  setCollection: (id: number | null) => void;
  resetFilters: () => void;

  select: (id: number, mode?: "replace" | "toggle" | "range") => void;
  clearSelection: () => void;
  setActive: (id: number | null) => void;
  moveActive: (delta: number) => void;

  reloadReference: () => Promise<void>;
  applySettings: (settings: Settings) => Promise<void>;
  setVault: (vault: VaultStatus) => void;

  setPaletteOpen: (open: boolean) => void;
  toast: (message: string, tone?: Toast["tone"]) => void;
  dismissToast: (id: number) => void;
}

/**
 * Monotonic token identifying the newest request. An older response that
 * arrives late is discarded rather than overwriting fresher results — the
 * classic search-as-you-type bug.
 */
let requestToken = 0;
let toastSeq = 0;

export const useApp = create<AppStore>((set, get) => ({
  ready: false,
  items: [],
  total: null,
  cursor: null,
  loading: false,
  loadingMore: false,
  error: null,

  query: emptyQuery(),
  stats: null,
  tags: [],
  collections: [],
  settings: null,
  vault: null,
  version: "",

  selection: [],
  activeId: null,
  paletteOpen: false,
  toasts: [],

  // -------------------------------------------------------------------------

  async init() {
    try {
      const boot = await api.bootstrap();
      set({
        settings: boot.settings,
        stats: boot.stats,
        tags: boot.tags,
        collections: boot.collections,
        vault: boot.vault,
        version: boot.version,
        ready: true,
      });
      applyTheme(boot.settings);
      await get().refresh();
    } catch (e) {
      set({ error: errorMessage(e), ready: true });
    }
  },

  async refresh() {
    const token = ++requestToken;
    set({ loading: true, error: null });

    try {
      const query = { ...get().query, cursor: null };
      const [page, stats] = await Promise.all([api.listItems(query), api.getStats()]);
      if (token !== requestToken) return;

      const activeId = page.items.some((i) => i.id === get().activeId)
        ? get().activeId
        : (page.items[0]?.id ?? null);

      set({
        items: page.items,
        total: page.total ?? null,
        cursor: page.next ?? null,
        stats,
        loading: false,
        activeId,
        // Drop selected ids that fell out of the current result set.
        selection: get().selection.filter((id) => page.items.some((i) => i.id === id)),
      });
    } catch (e) {
      if (token !== requestToken) return;
      set({ loading: false, error: errorMessage(e) });
    }
  },

  async loadMore() {
    const { cursor, loadingMore, loading, items, query } = get();
    if (!cursor || loadingMore || loading) return;

    const token = requestToken;
    set({ loadingMore: true });

    try {
      const page = await api.listItems({ ...query, cursor });
      if (token !== requestToken) return;

      // Guard against a duplicate page arriving from a double-triggered scroll.
      const known = new Set(items.map((i) => i.id));
      const fresh = page.items.filter((i) => !known.has(i.id));

      set({
        items: [...items, ...fresh],
        cursor: page.next ?? null,
        loadingMore: false,
      });
    } catch (e) {
      if (token !== requestToken) return;
      set({ loadingMore: false, error: errorMessage(e) });
    }
  },

  setQuery(patch) {
    set({ query: { ...get().query, ...patch, cursor: null } });
    void get().refresh();
  },

  setSearch(text) {
    // Relevance ordering only makes sense while searching; restore recency
    // automatically when the box is cleared.
    const sort: SortBy = text.trim() ? "relevance" : "recent";
    set({ query: { ...get().query, text, sort, cursor: null } });
    void get().refresh();
  },

  setFilter(filter) {
    get().setQuery({ filter });
  },

  setSort(sort) {
    get().setQuery({ sort });
  },

  toggleKind(kind) {
    const kinds = get().query.kinds;
    get().setQuery({
      kinds: kinds.includes(kind) ? kinds.filter((k) => k !== kind) : [...kinds, kind],
    });
  },

  toggleTag(id) {
    const tags = get().query.tags;
    get().setQuery({
      tags: tags.includes(id) ? tags.filter((t) => t !== id) : [...tags, id],
    });
  },

  setCollection(id) {
    get().setQuery({ collection: id, filter: "all" });
  },

  resetFilters() {
    set({ query: { ...emptyQuery(), text: get().query.text } });
    void get().refresh();
  },

  // -------------------------------------------------------------------------

  select(id, mode = "replace") {
    const { selection, items } = get();

    if (mode === "toggle") {
      set({
        selection: selection.includes(id)
          ? selection.filter((s) => s !== id)
          : [...selection, id],
        activeId: id,
      });
      return;
    }

    if (mode === "range" && selection.length) {
      const anchor = selection[selection.length - 1]!;
      const from = items.findIndex((i) => i.id === anchor);
      const to = items.findIndex((i) => i.id === id);
      if (from !== -1 && to !== -1) {
        const [lo, hi] = from < to ? [from, to] : [to, from];
        const range = items.slice(lo, hi + 1).map((i) => i.id);
        const merged = [...new Set([...selection, ...range])];
        set({ selection: merged, activeId: id });
        return;
      }
    }

    set({ selection: [id], activeId: id });
  },

  clearSelection() {
    set({ selection: [] });
  },

  setActive(id) {
    set({ activeId: id });
  },

  moveActive(delta) {
    const { items, activeId } = get();
    if (!items.length) return;

    const current = items.findIndex((i) => i.id === activeId);
    const next = Math.max(0, Math.min(items.length - 1, (current === -1 ? 0 : current) + delta));
    const target = items[next];
    if (!target) return;

    set({ activeId: target.id, selection: [target.id] });

    // Fetch ahead when the cursor approaches the end of the loaded window.
    if (next > items.length - 12) void get().loadMore();
  },

  // -------------------------------------------------------------------------

  async reloadReference() {
    try {
      const [tags, collections, stats] = await Promise.all([
        api.listTags(),
        api.listCollections(),
        api.getStats(),
      ]);
      set({ tags, collections, stats });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  async applySettings(settings) {
    try {
      const saved = await api.saveSettings(settings);
      set({ settings: saved });
      applyTheme(saved);
    } catch (e) {
      get().toast(errorMessage(e), "error");
    }
  },

  setVault(vault) {
    set({ vault });
  },

  setPaletteOpen(open) {
    set({ paletteOpen: open });
  },

  toast(message, tone = "info") {
    const id = ++toastSeq;
    set({ toasts: [...get().toasts, { id, message, tone }] });
    // Errors stay long enough to read; confirmations get out of the way.
    window.setTimeout(() => get().dismissToast(id), tone === "error" ? 6000 : 2600);
  },

  dismissToast(id) {
    set({ toasts: get().toasts.filter((t) => t.id !== id) });
  },
}));

/**
 * Push theme and accent into the document. Mirrored into `localStorage` so the
 * inline script in `index.html` can apply them before the next first paint.
 */
export function applyTheme(settings: Settings) {
  const dark =
    settings.theme === "dark" ||
    (settings.theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);

  document.documentElement.classList.toggle("dark", dark);
  document.documentElement.style.setProperty("--accent", settings.accent);

  try {
    localStorage.setItem("nexus.theme", settings.theme);
    localStorage.setItem("nexus.accent", settings.accent);
  } catch {
    // Private mode or a locked-down webview; the in-memory value still applies.
  }
}

/** Keep "system" theme reactive to OS changes while the app is open. */
export function watchSystemTheme() {
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const handler = () => {
    const settings = useApp.getState().settings;
    if (settings?.theme === "system") applyTheme(settings);
  };
  media.addEventListener("change", handler);
  return () => media.removeEventListener("change", handler);
}
