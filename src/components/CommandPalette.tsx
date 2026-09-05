/**
 * The command palette.
 *
 * One keystroke away from every action in the app. Commands are scored with a
 * subsequence matcher (so "cph" finds "Copy as plain text") rather than a plain
 * substring test, which is what makes typing three letters enough.
 */

import clsx from "clsx";
import {
  ArrowDownUp,
  Clock,
  Copy,
  Database,
  Download,
  Eraser,
  FileUp,
  KeyRound,
  Layers,
  Moon,
  Pause,
  Pin,
  Play,
  Search,
  Settings2,
  Star,
  Sun,
  Trash2,
  Upload,
} from "lucide-react";
import type { ComponentType } from "react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Dialog, Kbd } from "@/components/ui";
import * as api from "@/lib/api";
import { errorMessage } from "@/lib/types";
import { useApp } from "@/store/app";

export interface Command {
  id: string;
  label: string;
  hint?: string;
  group: string;
  icon: ComponentType<{ className?: string }>;
  shortcut?: string[];
  run: () => void | Promise<void>;
}

/**
 * Subsequence match with a simple relevance score.
 *
 * Returns `null` when the query does not match at all. Higher scores are
 * better: consecutive characters and word-start matches are rewarded, which
 * puts "Clear history" above "Collections" for the query "cl".
 */
function score(text: string, query: string): number | null {
  if (!query) return 0;

  const haystack = text.toLowerCase();
  const needle = query.toLowerCase();

  let hi = 0;
  let total = 0;
  let streak = 0;

  for (const char of needle) {
    const found = haystack.indexOf(char, hi);
    if (found === -1) return null;

    if (found === hi && hi > 0) {
      streak += 1;
      total += 8 + streak * 2;
    } else {
      streak = 0;
      total += 1;
    }
    // Matching the first letter of a word is a strong signal.
    if (found === 0 || haystack[found - 1] === " ") total += 6;

    hi = found + 1;
  }

  // Prefer shorter labels when scores are otherwise close.
  return total - haystack.length * 0.05;
}

export function CommandPalette({
  open,
  onClose,
  onOpenSettings,
}: {
  open: boolean;
  onClose: () => void;
  onOpenSettings: () => void;
}) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const commands = useCommands(onClose, onOpenSettings);

  const results = useMemo(() => {
    if (!query.trim()) return commands;
    return commands
      .map((command) => ({
        command,
        score: Math.max(
          score(command.label, query) ?? -Infinity,
          (score(`${command.group} ${command.label}`, query) ?? -Infinity) - 4,
        ),
      }))
      .filter((entry) => entry.score > -Infinity)
      .sort((a, b) => b.score - a.score)
      .map((entry) => entry.command);
  }, [commands, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setIndex(0);
    }
  }, [open]);

  useEffect(() => {
    setIndex(0);
  }, [query]);

  // Keep the highlighted command scrolled into view.
  useEffect(() => {
    const node = listRef.current?.querySelector<HTMLElement>(`[data-index="${index}"]`);
    node?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setIndex((i) => Math.min(results.length - 1, i + 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setIndex((i) => Math.max(0, i - 1));
        break;
      case "Enter": {
        e.preventDefault();
        const command = results[index];
        if (command) {
          void command.run();
          onClose();
        }
        break;
      }
      case "Escape":
        e.preventDefault();
        onClose();
        break;
    }
  };

  // Group headings, preserving the ranked order within each group.
  const grouped = useMemo(() => {
    const map = new Map<string, Command[]>();
    for (const command of results) {
      const list = map.get(command.group) ?? [];
      list.push(command);
      map.set(command.group, list);
    }
    return [...map.entries()];
  }, [results]);

  let flatIndex = -1;

  return (
    <Dialog open={open} onClose={onClose} title="" width={560}>
      <div className="-mx-5 -mt-10" onKeyDown={onKeyDown}>
        <div className="flex items-center gap-2.5 border-b border-border px-4 py-3">
          <Search className="size-4 shrink-0 text-text-faint" />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Type a command…"
            className="flex-1 bg-transparent text-[14px] text-text outline-none placeholder:text-text-faint selectable"
          />
          <Kbd>Esc</Kbd>
        </div>

        <div ref={listRef} className="scroll-area max-h-[380px] overflow-y-auto p-1.5">
          {results.length === 0 ? (
            <p className="px-3 py-8 text-center text-[12.5px] text-text-faint">
              No commands match “{query}”.
            </p>
          ) : (
            grouped.map(([group, items]) => (
              <div key={group}>
                <div className="px-2 pb-1 pt-2 text-[10.5px] font-semibold uppercase tracking-wider text-text-faint">
                  {group}
                </div>
                {items.map((command) => {
                  flatIndex += 1;
                  const active = flatIndex === index;
                  const current = flatIndex;
                  return (
                    <button
                      key={command.id}
                      data-index={current}
                      onMouseMove={() => setIndex(current)}
                      onClick={() => {
                        void command.run();
                        onClose();
                      }}
                      className={clsx(
                        "flex w-full items-center gap-2.5 rounded-[var(--radius-sm)] px-2 py-1.5",
                        "text-left text-[12.5px] transition-colors",
                        active ? "bg-surface-2 text-text" : "text-text-muted",
                      )}
                    >
                      <command.icon
                        className={clsx(
                          "size-3.5 shrink-0",
                          active ? "text-[var(--accent)]" : "text-text-faint",
                        )}
                      />
                      <span className="min-w-0 flex-1 truncate">{command.label}</span>
                      {command.hint && (
                        <span className="shrink-0 text-[11px] text-text-faint">
                          {command.hint}
                        </span>
                      )}
                      {command.shortcut?.map((key) => <Kbd key={key}>{key}</Kbd>)}
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </div>

        <div className="flex items-center gap-3 border-t border-border bg-surface-2/40 px-4 py-2 text-[11px] text-text-faint">
          <span className="flex items-center gap-1">
            <Kbd>↑</Kbd>
            <Kbd>↓</Kbd> navigate
          </span>
          <span className="flex items-center gap-1">
            <Kbd>↵</Kbd> run
          </span>
          <div className="flex-1" />
          <span>{results.length} commands</span>
        </div>
      </div>
    </Dialog>
  );
}

/** Builds the command list from current state. */
function useCommands(onClose: () => void, onOpenSettings: () => void): Command[] {
  const store = useApp();

  return useMemo(() => {
    const { settings, selection, items, activeId, stats } = store;
    const targetIds = selection.length ? selection : activeId != null ? [activeId] : [];
    const target = items.find((i) => i.id === (activeId ?? targetIds[0]));

    const guard = async (label: string, fn: () => Promise<unknown>) => {
      try {
        await fn();
        await store.refresh();
      } catch (e) {
        store.toast(`${label}: ${errorMessage(e)}`, "error");
      }
    };

    const commands: Command[] = [];

    // --- selection actions ---
    if (target) {
      commands.push(
        {
          id: "copy",
          label: "Copy to clipboard",
          group: "Entry",
          icon: Copy,
          shortcut: ["↵"],
          run: () => guard("Copy", () => api.useItem(target.id, "copy_only")),
        },
        {
          id: "pin",
          label: target.pinned ? "Unpin entry" : "Pin entry",
          group: "Entry",
          icon: Pin,
          run: () => guard("Pin", () => api.setPinned(target.id, !target.pinned)),
        },
        {
          id: "favorite",
          label: target.favorite ? "Remove from favourites" : "Add to favourites",
          group: "Entry",
          icon: Star,
          run: () => guard("Favourite", () => api.setFavorite(target.id, !target.favorite)),
        },
        {
          id: "delete",
          label:
            targetIds.length > 1 ? `Delete ${targetIds.length} entries` : "Delete entry",
          group: "Entry",
          icon: Trash2,
          run: () => guard("Delete", () => api.deleteItems(targetIds)),
        },
      );
    }

    // --- navigation ---
    const filters: Array<[string, string, ComponentType<{ className?: string }>]> = [
      ["all", "Show all entries", Layers],
      ["today", "Show today", Clock],
      ["pinned", "Show pinned", Pin],
      ["favorites", "Show favourites", Star],
      ["vault", "Show vault", KeyRound],
      ["trash", "Show trash", Trash2],
    ];
    for (const [value, label, icon] of filters) {
      commands.push({
        id: `filter-${value}`,
        label,
        group: "Go to",
        icon,
        run: () => store.setFilter(value as never),
      });
    }

    // --- sorting ---
    const sorts: Array<[string, string]> = [
      ["recent", "Sort by most recent"],
      ["created", "Sort by capture time"],
      ["frequency", "Sort by most used"],
      ["size", "Sort by size"],
    ];
    for (const [value, label] of sorts) {
      commands.push({
        id: `sort-${value}`,
        label,
        group: "View",
        icon: ArrowDownUp,
        run: () => store.setSort(value as never),
      });
    }

    // --- appearance ---
    if (settings) {
      commands.push(
        {
          id: "theme-dark",
          label: "Switch to dark theme",
          group: "View",
          icon: Moon,
          run: () => store.applySettings({ ...settings, theme: "dark" }),
        },
        {
          id: "theme-light",
          label: "Switch to light theme",
          group: "View",
          icon: Sun,
          run: () => store.applySettings({ ...settings, theme: "light" }),
        },
        {
          id: "capture-toggle",
          label: settings.capture_enabled ? "Pause clipboard capture" : "Resume clipboard capture",
          group: "Capture",
          icon: settings.capture_enabled ? Pause : Play,
          run: async () => {
            await api.setCapturePaused(settings.capture_enabled);
            store.applySettings({ ...settings, capture_enabled: !settings.capture_enabled });
            store.toast(
              settings.capture_enabled ? "Capture paused" : "Capture resumed",
              "success",
            );
          },
        },
        {
          id: "capture-now",
          label: "Capture clipboard now",
          group: "Capture",
          icon: Download,
          run: () => guard("Capture", () => api.captureNow()),
        },
      );
    }

    // --- maintenance ---
    commands.push(
      {
        id: "settings",
        label: "Open settings",
        group: "App",
        icon: Settings2,
        shortcut: ["Ctrl", ","],
        run: onOpenSettings,
      },
      {
        id: "maintenance",
        label: "Run cleanup now",
        group: "App",
        icon: Eraser,
        run: () =>
          guard("Cleanup", async () => {
            const report = await api.runMaintenance();
            store.toast(
              `Cleaned up ${report.expired + report.overflowed} entries and ${report.blobs_removed} files`,
              "success",
            );
          }),
      },
      {
        id: "compact",
        label: "Compact database",
        group: "App",
        icon: Database,
        run: () =>
          guard("Compact", async () => {
            const size = await api.compactDatabase();
            store.toast(`Database is now ${(size / 1_048_576).toFixed(1)} MB`, "success");
          }),
      },
      {
        id: "data-dir",
        label: "Open data folder",
        group: "App",
        icon: FileUp,
        run: () => api.openDataDir(),
      },
    );

    if (stats?.trash) {
      commands.push({
        id: "empty-trash",
        label: `Empty trash (${stats.trash})`,
        group: "App",
        icon: Trash2,
        run: () => guard("Empty trash", () => api.emptyTrash()),
      });
    }

    commands.push({
      id: "export",
      label: "Export history…",
      group: "App",
      icon: Upload,
      run: onOpenSettings,
    });

    return commands;
    // The store object identity changes on every update, which is exactly when
    // the command list needs rebuilding.
  }, [store, onClose, onOpenSettings]);
}
