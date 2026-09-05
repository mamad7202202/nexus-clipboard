/**
 * The quick picker.
 *
 * Summoned by the global hotkey over whatever the user is working in. It has
 * one job — find an entry and paste it — so it is search-first, keyboard-only,
 * and dismisses itself the moment the job is done.
 */

import clsx from "clsx";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ArrowUpDown, CornerDownLeft, Inbox, Search, Settings2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ItemIcon } from "@/components/ItemIcon";
import { EmptyState, Kbd, Spinner } from "@/components/ui";
import * as api from "@/lib/api";
import { EVENTS, on } from "@/lib/events";
import { highlightParts, itemSubtitle, kindColor, kindLabel, relativeTime } from "@/lib/format";
import { highlight } from "@/lib/highlight";
import type { Item, Kind, Settings } from "@/lib/types";
import { emptyQuery, errorMessage } from "@/lib/types";

/** Kinds offered as quick filters, in the order they appear. */
const QUICK_KINDS: Kind[] = ["text", "link", "code", "image", "files"];

export function LauncherView() {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<Kind | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [index, setIndex] = useState(0);
  const [loading, setLoading] = useState(true);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);

  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const requestToken = useRef(0);

  const selected = items[index];

  // --- data ---------------------------------------------------------------

  const load = useCallback(async (text: string, kindFilter: Kind | null) => {
    const token = ++requestToken.current;
    setLoading(true);
    try {
      const page = await api.listItems({
        ...emptyQuery(),
        text,
        kinds: kindFilter ? [kindFilter] : [],
        sort: text.trim() ? "relevance" : "recent",
        limit: 60,
      });
      if (token !== requestToken.current) return;
      setItems(page.items);
      setIndex(0);
      setError(null);
    } catch (e) {
      if (token !== requestToken.current) return;
      setError(errorMessage(e));
    } finally {
      if (token === requestToken.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    api.getSettings().then(setSettings).catch(() => {});
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => void load(query, kind), query ? 110 : 0);
    return () => window.clearTimeout(timer);
  }, [query, kind, load]);

  // Reset and refocus every time the window is summoned.
  useEffect(() => {
    const unlistenHotkey = on<string>(EVENTS.hotkey, () => {
      setQuery("");
      setKind(null);
      setIndex(0);
      void load("", null);
      // The window is shown by the backend just before this fires; the focus
      // call has to happen after the webview is actually visible.
      window.requestAnimationFrame(() => inputRef.current?.focus());
    });

    // Coalesce bursts: a rapid series of copies emits one event each, and the
    // launcher only needs the final state.
    let refreshTimer = 0;
    const unlistenHistory = on(EVENTS.historyChanged, () => {
      window.clearTimeout(refreshTimer);
      refreshTimer = window.setTimeout(() => void load(query, kind), 150);
    });

    return () => {
      window.clearTimeout(refreshTimer);
      unlistenHotkey();
      unlistenHistory();
    };
  }, [load, query, kind]);

  // Dismiss when focus leaves — clicking away should behave like Escape.
  useEffect(() => {
    const window_ = getCurrentWindow();
    const unlisten = window_.onFocusChanged(({ payload: focused }) => {
      if (!focused) void api.hideLauncher();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Keep the highlighted row visible.
  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>(`[data-index="${index}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [index]);

  // --- actions ------------------------------------------------------------

  const paste = useCallback(
    async (item: Item, plain = false) => {
      try {
        await api.useItem(
          item.id,
          plain ? "copy_plain" : settings?.paste_on_select ? "paste" : "copy_only",
        );
      } catch (e) {
        setError(errorMessage(e));
        return;
      }
      await api.hideLauncher();
    },
    [settings?.paste_on_select],
  );

  const onKeyDown = (e: React.KeyboardEvent) => {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setIndex((i) => Math.min(items.length - 1, i + 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setIndex((i) => Math.max(0, i - 1));
        break;
      case "Home":
        e.preventDefault();
        setIndex(0);
        break;
      case "End":
        e.preventDefault();
        setIndex(items.length - 1);
        break;
      case "Enter":
        e.preventDefault();
        if (selected) void paste(selected, e.shiftKey);
        break;
      case "Escape":
        e.preventDefault();
        if (query || kind) {
          setQuery("");
          setKind(null);
        } else {
          void api.hideLauncher();
        }
        break;
      case "Tab": {
        // Tab cycles the kind filter, which is faster than reaching for a chip.
        e.preventDefault();
        const order: Array<Kind | null> = [null, ...QUICK_KINDS];
        const current = order.indexOf(kind);
        const next = order[(current + (e.shiftKey ? -1 : 1) + order.length) % order.length];
        setKind(next ?? null);
        break;
      }
      default:
        // Ctrl+1…5 jump straight to a kind filter.
        if ((e.ctrlKey || e.metaKey) && /^[1-5]$/.test(e.key)) {
          e.preventDefault();
          const target = QUICK_KINDS[Number(e.key) - 1];
          if (target) setKind((k) => (k === target ? null : target));
        }
    }
  };

  // --- render -------------------------------------------------------------

  return (
    <div
      onKeyDown={onKeyDown}
      className={clsx(
        "flex h-screen w-screen flex-col overflow-hidden",
        "rounded-[14px] border border-border-strong/70 bg-surface/95 backdrop-blur-xl",
        "elevation-float",
      )}
    >
      {/* Search ---------------------------------------------------------- */}
      <div className="drag-region flex items-center gap-2.5 border-b border-border px-3.5 py-3">
        <Search className="size-4 shrink-0 text-text-faint" />
        <input
          ref={inputRef}
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search your clipboard history…"
          spellCheck={false}
          className={clsx(
            "no-drag min-w-0 flex-1 bg-transparent text-[14.5px] text-text",
            "outline-none placeholder:text-text-faint selectable",
          )}
        />
        {loading && <Spinner className="size-3.5 text-text-faint" />}
        <button
          onClick={() => void api.showMain()}
          className="no-drag rounded p-1 text-text-faint transition-colors hover:text-text"
          aria-label="Open the full window"
        >
          <Settings2 className="size-3.5" />
        </button>
      </div>

      {/* Kind filters ---------------------------------------------------- */}
      <div className="no-drag flex items-center gap-1 border-b border-border px-3 py-1.5">
        <FilterPill active={kind === null} onClick={() => setKind(null)}>
          All
        </FilterPill>
        {QUICK_KINDS.map((k, i) => (
          <FilterPill
            key={k}
            active={kind === k}
            color={kindColor(k)}
            hint={`Ctrl ${i + 1}`}
            onClick={() => setKind(kind === k ? null : k)}
          >
            {kindLabel(k)}
          </FilterPill>
        ))}
      </div>

      {/* Results + preview ----------------------------------------------- */}
      <div className="flex min-h-0 flex-1">
        <div ref={listRef} className="scroll-area w-[46%] shrink-0 overflow-y-auto border-r border-border py-1">
          {error ? (
            <p className="px-4 py-6 text-center text-[12px] text-[var(--danger)]">{error}</p>
          ) : items.length === 0 && !loading ? (
            <EmptyState
              icon={<Inbox />}
              title={query ? "No matches" : "Nothing yet"}
              description={query ? undefined : "Copy something to get started."}
            />
          ) : (
            items.map((item, i) => (
              <Row
                key={item.id}
                item={item}
                index={i}
                active={i === index}
                search={query}
                blur={item.sensitive && (settings?.blur_secrets ?? true)}
                onHover={() => setIndex(i)}
                onClick={() => void paste(item)}
              />
            ))
          )}
        </div>

        <div className="min-w-0 flex-1">
          {selected ? (
            <QuickPreview item={selected} blur={selected.sensitive && (settings?.blur_secrets ?? true)} />
          ) : (
            <div className="h-full" />
          )}
        </div>
      </div>

      {/* Footer ---------------------------------------------------------- */}
      <div className="flex items-center gap-3 border-t border-border bg-surface-2/40 px-3.5 py-2 text-[11px] text-text-faint">
        <span className="flex items-center gap-1">
          <Kbd>
            <CornerDownLeft className="size-2.5" />
          </Kbd>
          {settings?.paste_on_select ? "paste" : "copy"}
        </span>
        <span className="flex items-center gap-1">
          <Kbd>⇧</Kbd>
          <Kbd>
            <CornerDownLeft className="size-2.5" />
          </Kbd>
          plain text
        </span>
        <span className="flex items-center gap-1">
          <Kbd>
            <ArrowUpDown className="size-2.5" />
          </Kbd>
          navigate
        </span>
        <span className="flex items-center gap-1">
          <Kbd>Tab</Kbd> filter
        </span>
        <div className="flex-1" />
        <span className="tabular-nums">{items.length ? `${index + 1} / ${items.length}` : "—"}</span>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------

function FilterPill({
  children,
  active,
  color,
  hint,
  onClick,
}: {
  children: React.ReactNode;
  active: boolean;
  color?: string;
  hint?: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      title={hint}
      className={clsx(
        "rounded-full px-2 py-0.5 text-[11.5px] font-medium transition-all duration-100",
        active ? "text-text" : "text-text-faint hover:text-text-muted",
      )}
      style={
        active
          ? {
              background: color
                ? `color-mix(in oklch, ${color} 18%, transparent)`
                : "var(--surface-3)",
              boxShadow: color
                ? `inset 0 0 0 1px color-mix(in oklch, ${color} 35%, transparent)`
                : "inset 0 0 0 1px var(--border-strong)",
              color: color ?? undefined,
            }
          : undefined
      }
    >
      {children}
    </button>
  );
}

function Row({
  item,
  index,
  active,
  search,
  blur,
  onHover,
  onClick,
}: {
  item: Item;
  index: number;
  active: boolean;
  search: string;
  blur: boolean;
  onHover: () => void;
  onClick: () => void;
}) {
  const parts = useMemo(() => highlightParts(item.preview, search), [item.preview, search]);

  return (
    <div
      data-index={index}
      onMouseMove={onHover}
      onClick={onClick}
      className={clsx(
        "mx-1.5 flex cursor-default items-center gap-2.5 rounded-[var(--radius-md)] px-2 py-1.5",
        "transition-colors duration-75",
        active ? "bg-[color-mix(in_oklch,var(--accent)_16%,transparent)]" : "hover:bg-surface-2",
      )}
    >
      <ItemIcon item={item} size={26} />
      <div className="min-w-0 flex-1">
        <div className={clsx("truncate text-[12.5px] leading-tight", blur && "blur-[3px]")}>
          {parts.map((part, i) =>
            part.hit ? (
              <mark key={i} className="nexus-hit">
                {part.text}
              </mark>
            ) : (
              <span key={i}>{part.text}</span>
            ),
          )}
        </div>
        <div className="truncate text-[10.5px] text-text-faint">{itemSubtitle(item)}</div>
      </div>
      <span className="shrink-0 text-[10.5px] tabular-nums text-text-faint">
        {relativeTime(item.updated_at)}
      </span>
    </div>
  );
}

function QuickPreview({ item, blur }: { item: Item; blur: boolean }) {
  const [image, setImage] = useState<string | null>(null);
  const [body, setBody] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setImage(null);
    setBody(null);

    if (item.kind === "image") {
      api.getImage(item.id).then((uri) => !cancelled && setImage(uri)).catch(() => {});
    } else if (!item.encrypted) {
      api
        .getItem(item.id, false)
        .then((full) => !cancelled && setBody(full.body ?? full.preview))
        .catch(() => {});
    }

    return () => {
      cancelled = true;
    };
  }, [item.id, item.kind, item.encrypted]);

  const highlighted = useMemo(() => {
    if (!body) return null;
    if (item.kind !== "code" && item.kind !== "json") return null;
    return highlight(body, item.kind === "json" ? "json" : item.meta.language);
  }, [body, item.kind, item.meta.language]);

  if (item.kind === "image") {
    return (
      <div className="flex h-full items-center justify-center p-3">
        {image ? (
          <img
            src={image}
            alt=""
            className="max-h-full max-w-full rounded-[var(--radius-sm)] object-contain"
            draggable={false}
          />
        ) : (
          <Spinner className="size-4 text-text-faint" />
        )}
      </div>
    );
  }

  if (item.kind === "color" && item.meta.color) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-4">
        <div
          className="size-24 rounded-[var(--radius-lg)] border border-border"
          style={{ background: item.meta.color }}
        />
        <span className="font-mono text-[13px]">{item.preview}</span>
      </div>
    );
  }

  const isMono = item.kind === "code" || item.kind === "json";
  const text = item.encrypted ? item.preview : (body ?? item.preview);

  return (
    <div className="scroll-area h-full overflow-auto p-3">
      <pre
        className={clsx(
          "whitespace-pre-wrap break-words",
          isMono ? "font-mono text-[11.5px] leading-[1.6]" : "text-[12.5px] leading-relaxed",
          blur && "blur-[4px]",
        )}
      >
        {highlighted ? (
          <code className="hljs" dangerouslySetInnerHTML={{ __html: highlighted }} />
        ) : (
          <code>{text}</code>
        )}
      </pre>
    </div>
  );
}
