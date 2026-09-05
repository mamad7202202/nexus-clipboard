/**
 * The main window: sidebar, search, list and preview.
 */

import clsx from "clsx";
import {
  ArrowDownUp,
  Command,
  Inbox,
  Pause,
  Play,
  Search,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { CommandPalette } from "@/components/CommandPalette";
import { ItemList } from "@/components/ItemList";
import { Preview } from "@/components/Preview";
import { SettingsPanel } from "@/components/Settings";
import { Sidebar } from "@/components/Sidebar";
import {
  Badge,
  Button,
  EmptyState,
  IconButton,
  Input,
  Kbd,
  Menu,
  MenuItem,
  MenuLabel,

  Tooltip,
} from "@/components/ui";
import * as api from "@/lib/api";
import { count, kindLabel } from "@/lib/format";
import type { SortBy } from "@/lib/types";
import { errorMessage } from "@/lib/types";
import { useApp } from "@/store/app";

const SORT_LABELS: Record<SortBy, string> = {
  recent: "Recent",
  created: "Captured",
  frequency: "Most used",
  size: "Largest",
  relevance: "Best match",
};

export function MainView() {
  const store = useApp();
  const [showSettings, setShowSettings] = useState(false);
  const [search, setSearch] = useState("");

  const {
    items,
    activeId,
    selection,
    query,
    settings,
    stats,
    loading,
    loadingMore,
    cursor,
    error,
  } = store;

  // Debounce search so a fast typist does not issue a query per keystroke.
  useEffect(() => {
    const timer = window.setTimeout(() => {
      if (search !== query.text) store.setSearch(search);
    }, 130);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search]);

  // Keep the box in sync when something else changes the query (palette, tray).
  useEffect(() => {
    if (query.text !== search) setSearch(query.text);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query.text]);

  const activate = useCallback(
    async (id: number) => {
      try {
        await api.useItem(id, settings?.paste_on_select ? "paste" : "copy_only");
        store.toast("Copied", "success");
      } catch (e) {
        store.toast(errorMessage(e), "error");
      }
    },
    [settings?.paste_on_select, store],
  );

  // Global keyboard handling for the window.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const typing =
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.isContentEditable;

      // Palette and settings work everywhere, including while typing.
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        store.setPaletteOpen(true);
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === ",") {
        e.preventDefault();
        setShowSettings(true);
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        document.querySelector<HTMLInputElement>("#nexus-search")?.focus();
        return;
      }

      if (typing) {
        // Arrow keys still drive the list while the search box has focus —
        // that is what makes search-then-pick a single gesture.
        if (e.key === "ArrowDown") {
          e.preventDefault();
          store.moveActive(1);
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          store.moveActive(-1);
        } else if (e.key === "Enter" && activeId != null) {
          e.preventDefault();
          void activate(activeId);
        } else if (e.key === "Escape" && search) {
          e.preventDefault();
          setSearch("");
        }
        return;
      }

      switch (e.key) {
        case "ArrowDown":
        case "j":
          e.preventDefault();
          store.moveActive(1);
          break;
        case "ArrowUp":
        case "k":
          e.preventDefault();
          store.moveActive(-1);
          break;
        case "Enter":
          if (activeId != null) {
            e.preventDefault();
            void activate(activeId);
          }
          break;
        case "Delete":
        case "Backspace":
          if (selection.length) {
            e.preventDefault();
            void api.deleteItems(selection).then(() => store.refresh());
          }
          break;
        case "Escape":
          store.clearSelection();
          break;
        case "/":
          e.preventDefault();
          document.querySelector<HTMLInputElement>("#nexus-search")?.focus();
          break;
        case "p":
          if (activeId != null) {
            const item = items.find((i) => i.id === activeId);
            if (item) void api.setPinned(item.id, !item.pinned).then(() => store.refresh());
          }
          break;
      }
    };

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [store, activeId, selection, items, search, activate]);

  if (showSettings) {
    return (
      <div className="h-full bg-bg">
        <SettingsPanel onClose={() => setShowSettings(false)} />
      </div>
    );
  }

  const filtersActive =
    query.kinds.length > 0 || query.tags.length > 0 || query.source_app != null;

  return (
    <div className="flex h-full bg-bg">
      {/* Sidebar ---------------------------------------------------------- */}
      <aside className="w-[212px] shrink-0 border-r border-border">
        <Sidebar onOpenSettings={() => setShowSettings(true)} />
      </aside>

      {/* List ------------------------------------------------------------- */}
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-2 border-b border-border px-3 py-2.5">
          <Input
            id="nexus-search"
            icon={<Search className="size-3.5" />}
            placeholder="Search everything you have copied…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="flex-1"
            suffix={
              search ? (
                <button
                  onClick={() => setSearch("")}
                  className="rounded p-0.5 transition-colors hover:text-text"
                  aria-label="Clear search"
                >
                  <X className="size-3" />
                </button>
              ) : (
                <Kbd>/</Kbd>
              )
            }
          />

          <Menu
            align="end"
            trigger={({ toggle }) => (
              <Tooltip label="Sort">
                <Button size="sm" onClick={toggle} icon={<ArrowDownUp className="size-3.5" />}>
                  {SORT_LABELS[query.sort]}
                </Button>
              </Tooltip>
            )}
          >
            <MenuLabel>Sort by</MenuLabel>
            {(Object.keys(SORT_LABELS) as SortBy[])
              .filter((s) => s !== "relevance" || query.text)
              .map((sort) => (
                <MenuItem
                  key={sort}
                  checked={query.sort === sort}
                  onSelect={() => store.setSort(sort)}
                >
                  {SORT_LABELS[sort]}
                </MenuItem>
              ))}
          </Menu>

          <Tooltip label="Command palette">
            <IconButton
              aria-label="Command palette"
              onClick={() => store.setPaletteOpen(true)}
              icon={<Command className="size-3.5" />}
            />
          </Tooltip>

          <Tooltip label={settings?.capture_enabled ? "Pause capture" : "Resume capture"}>
            <IconButton
              aria-label="Toggle capture"
              variant={settings?.capture_enabled ? "ghost" : "primary"}
              onClick={async () => {
                if (!settings) return;
                await api.setCapturePaused(settings.capture_enabled);
                store.applySettings({
                  ...settings,
                  capture_enabled: !settings.capture_enabled,
                });
              }}
              icon={
                settings?.capture_enabled ? (
                  <Pause className="size-3.5" />
                ) : (
                  <Play className="size-3.5" />
                )
              }
            />
          </Tooltip>
        </header>

        {/* Active filters ------------------------------------------------ */}
        {filtersActive && (
          <div className="flex flex-wrap items-center gap-1.5 border-b border-border px-3 py-1.5">
            <SlidersHorizontal className="size-3 text-text-faint" />
            {query.kinds.map((kind) => (
              <FilterChip key={kind} label={kindLabel(kind)} onRemove={() => store.toggleKind(kind)} />
            ))}
            {query.tags.map((id) => {
              const tag = store.tags.find((t) => t.id === id);
              return tag ? (
                <FilterChip
                  key={id}
                  label={tag.name}
                  color={tag.color}
                  onRemove={() => store.toggleTag(id)}
                />
              ) : null;
            })}
            {query.source_app && (
              <FilterChip
                label={query.source_app}
                onRemove={() => store.setQuery({ source_app: null })}
              />
            )}
            <button
              onClick={store.resetFilters}
              className="ml-1 text-[11px] text-text-faint transition-colors hover:text-text"
            >
              Clear all
            </button>
          </div>
        )}

        {/* Bulk actions -------------------------------------------------- */}
        {selection.length > 1 && (
          <div className="flex items-center gap-2 border-b border-border bg-surface-2/50 px-3 py-1.5 animate-slide-up">
            <span className="text-[12px] font-medium text-text">
              {selection.length} selected
            </span>
            <div className="flex-1" />
            <Button
              size="xs"
              onClick={() => void api.deleteItems(selection).then(() => store.refresh())}
              icon={<Trash2 className="size-3" />}
            >
              Delete
            </Button>
            <Button size="xs" variant="ghost" onClick={store.clearSelection}>
              Cancel
            </Button>
          </div>
        )}

        {error && (
          <div className="border-b border-border bg-[color-mix(in_oklch,var(--danger)_10%,transparent)] px-3 py-1.5 text-[12px] text-text">
            {error}
          </div>
        )}

        <div className="min-h-0 flex-1">
          <ItemList
            items={items}
            activeId={activeId}
            selection={selection}
            search={query.text}
            density={settings?.density ?? "comfortable"}
            loading={loading}
            loadingMore={loadingMore}
            hasMore={cursor != null}
            blurSecrets={settings?.blur_secrets ?? true}
            grouped={query.sort === "recent" || query.sort === "created"}
            onSelect={store.select}
            onActivate={(id) => void activate(id)}
            onLoadMore={store.loadMore}
            emptyState={<HistoryEmptyState searching={Boolean(query.text)} />}
          />
        </div>

        <footer className="flex items-center gap-3 border-t border-border px-3 py-1.5 text-[11px] text-text-faint">
          <span className="tabular-nums">
            {store.total != null ? `${count(store.total)} entries` : `${items.length} loaded`}
          </span>
          {stats && (
            <>
              <span>·</span>
              <span>{count(stats.total)} total</span>
            </>
          )}
          <div className="flex-1" />
          <span className="flex items-center gap-1">
            <Kbd>↵</Kbd> paste
          </span>
          <span className="flex items-center gap-1">
            <Kbd>Ctrl</Kbd>
            <Kbd>K</Kbd> commands
          </span>
        </footer>
      </main>

      {/* Preview ---------------------------------------------------------- */}
      <aside className="w-[420px] shrink-0 border-l border-border bg-bg-subtle">
        <Preview itemId={activeId} onChanged={store.refresh} />
      </aside>

      <CommandPalette
        open={store.paletteOpen}
        onClose={() => store.setPaletteOpen(false)}
        onOpenSettings={() => {
          store.setPaletteOpen(false);
          setShowSettings(true);
        }}
      />
    </div>
  );
}

function FilterChip({
  label,
  color,
  onRemove,
}: {
  label: string;
  color?: string;
  onRemove: () => void;
}) {
  return (
    <span
      className={clsx(
        "inline-flex items-center gap-1 rounded-full border border-border",
        "bg-surface-2 py-0.5 pl-2 pr-1 text-[11px]",
      )}
      style={color ? { color, borderColor: `color-mix(in oklch, ${color} 35%, transparent)` } : undefined}
    >
      {label}
      <button
        onClick={onRemove}
        aria-label={`Remove ${label} filter`}
        className="rounded-full p-0.5 opacity-60 transition-opacity hover:opacity-100"
      >
        <X className="size-2.5" />
      </button>
    </span>
  );
}

function HistoryEmptyState({ searching }: { searching: boolean }) {
  return searching ? (
    <EmptyState
      icon={<Search />}
      title="No matches"
      description="Try fewer words, or clear the filters in the sidebar."
    />
  ) : (
    <EmptyState
      icon={<Inbox />}
      title="Your history is empty"
      description="Copy something and it will appear here instantly. Nexus is already listening."
      action={
        <Badge>
          <Sparkles className="size-2.5" />
          Everything stays on this machine
        </Badge>
      }
    />
  );
}
