/**
 * The history list.
 *
 * Virtualised with `@tanstack/react-virtual`, so a million rows cost the same
 * as fifty: only what fits on screen is mounted. Pagination is triggered from
 * the virtualiser's own range rather than a scroll listener, which keeps
 * loading in step with what the user can actually see.
 */

import clsx from "clsx";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Pin, Star } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef } from "react";

import { ItemIcon } from "@/components/ItemIcon";
import { Badge, EmptyState, Skeleton } from "@/components/ui";
import { dayBucket, highlightParts, itemSubtitle, relativeTime } from "@/lib/format";
import type { Density, Item } from "@/lib/types";

const ROW_HEIGHT: Record<Density, number> = {
  compact: 44,
  comfortable: 58,
  spacious: 72,
};

const HEADER_HEIGHT = 30;

/** A flattened list of date headers and item rows, so both can be virtualised. */
type Row =
  | { type: "header"; key: string; label: string }
  | { type: "item"; key: string; item: Item };

function buildRows(items: Item[], grouped: boolean): Row[] {
  if (!grouped) {
    return items.map((item) => ({ type: "item", key: `i${item.id}`, item }));
  }

  const rows: Row[] = [];
  let bucket = "";

  for (const item of items) {
    const label = dayBucket(item.updated_at);
    if (label !== bucket) {
      bucket = label;
      rows.push({ type: "header", key: `h${label}-${item.id}`, label });
    }
    rows.push({ type: "item", key: `i${item.id}`, item });
  }

  return rows;
}

export function ItemList({
  items,
  activeId,
  selection,
  search,
  density,
  loading,
  loadingMore,
  hasMore,
  blurSecrets,
  grouped = true,
  onSelect,
  onActivate,
  onContextMenu,
  onLoadMore,
  emptyState,
}: {
  items: Item[];
  activeId: number | null;
  selection: number[];
  search: string;
  density: Density;
  loading: boolean;
  loadingMore: boolean;
  hasMore: boolean;
  blurSecrets: boolean;
  grouped?: boolean;
  onSelect: (id: number, mode: "replace" | "toggle" | "range") => void;
  onActivate: (id: number) => void;
  onContextMenu?: (id: number, event: React.MouseEvent) => void;
  onLoadMore: () => void;
  emptyState?: React.ReactNode;
}) {
  const parentRef = useRef<HTMLDivElement>(null);
  const rows = useMemo(() => buildRows(items, grouped), [items, grouped]);
  const rowHeight = ROW_HEIGHT[density];

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: useCallback(
      (index: number) => (rows[index]?.type === "header" ? HEADER_HEIGHT : rowHeight),
      [rows, rowHeight],
    ),
    // Enough overscan that fast scrolling never shows blank space, small enough
    // that we are not rendering hundreds of hidden rows.
    overscan: 12,
    getItemKey: useCallback((index: number) => rows[index]?.key ?? index, [rows]),
  });

  const virtualRows = virtualizer.getVirtualItems();

  // Fetch the next page as the viewport approaches the end of what is loaded.
  useEffect(() => {
    const last = virtualRows[virtualRows.length - 1];
    if (!last) return;
    if (hasMore && !loadingMore && last.index >= rows.length - 10) {
      onLoadMore();
    }
  }, [virtualRows, rows.length, hasMore, loadingMore, onLoadMore]);

  // Keep the keyboard cursor in view when it moves off screen.
  useEffect(() => {
    if (activeId == null) return;
    const index = rows.findIndex((r) => r.type === "item" && r.item.id === activeId);
    if (index === -1) return;

    const visible = virtualizer.getVirtualItems();
    const onScreen = visible.some((v) => v.index === index);
    if (!onScreen) virtualizer.scrollToIndex(index, { align: "auto" });
    // `virtualizer` is stable; re-running on every virtual-item change would
    // fight the user's own scrolling.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId, rows]);

  if (loading && !items.length) {
    return (
      <div className="space-y-1 p-2">
        {Array.from({ length: 9 }, (_, i) => (
          <div key={i} className="flex items-center gap-3 px-2" style={{ height: rowHeight }}>
            <Skeleton className="size-[34px] shrink-0" />
            <div className="flex-1 space-y-1.5">
              <Skeleton className="h-3" style={{ width: `${45 + ((i * 13) % 40)}%` }} />
              <Skeleton className="h-2.5 w-24" />
            </div>
          </div>
        ))}
      </div>
    );
  }

  if (!items.length) {
    return <>{emptyState ?? <EmptyState title="Nothing here yet" />}</>;
  }

  return (
    <div ref={parentRef} className="scroll-area h-full overflow-y-auto overflow-x-hidden">
      <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualRows.map((virtualRow) => {
          const row = rows[virtualRow.index];
          if (!row) return null;

          return (
            <div
              key={virtualRow.key}
              data-index={virtualRow.index}
              ref={virtualizer.measureElement}
              className="absolute left-0 top-0 w-full"
              style={{ transform: `translateY(${virtualRow.start}px)` }}
            >
              {row.type === "header" ? (
                <DateHeader label={row.label} />
              ) : (
                <ItemRow
                  item={row.item}
                  height={rowHeight}
                  density={density}
                  active={row.item.id === activeId}
                  selected={selection.includes(row.item.id)}
                  search={search}
                  blurSecrets={blurSecrets}
                  onSelect={onSelect}
                  onActivate={onActivate}
                  onContextMenu={onContextMenu}
                />
              )}
            </div>
          );
        })}
      </div>

      {loadingMore && (
        <div className="flex items-center justify-center gap-2 py-3 text-[11.5px] text-text-faint">
          <span className="size-1 animate-pulse rounded-full bg-current" />
          Loading more…
        </div>
      )}
    </div>
  );
}

function DateHeader({ label }: { label: string }) {
  return (
    <div
      className={clsx(
        "flex items-end px-3.5 pb-1",
        "text-[10.5px] font-semibold uppercase tracking-wider text-text-faint",
      )}
      style={{ height: HEADER_HEIGHT }}
    >
      {label}
    </div>
  );
}

const ItemRow = memo(function ItemRow({
  item,
  height,
  density,
  active,
  selected,
  search,
  blurSecrets,
  onSelect,
  onActivate,
  onContextMenu,
}: {
  item: Item;
  height: number;
  density: Density;
  active: boolean;
  selected: boolean;
  search: string;
  blurSecrets: boolean;
  onSelect: (id: number, mode: "replace" | "toggle" | "range") => void;
  onActivate: (id: number) => void;
  onContextMenu?: (id: number, event: React.MouseEvent) => void;
}) {
  const parts = useMemo(() => highlightParts(item.preview, search), [item.preview, search]);
  const masked = item.sensitive && blurSecrets;
  const showSubtitle = density !== "compact";

  return (
    <div
      role="option"
      aria-selected={selected}
      tabIndex={-1}
      onMouseDown={(e) => {
        // Modifier-aware selection, matching every file manager.
        const mode = e.ctrlKey || e.metaKey ? "toggle" : e.shiftKey ? "range" : "replace";
        onSelect(item.id, mode);
      }}
      onDoubleClick={() => onActivate(item.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        if (!selected) onSelect(item.id, "replace");
        onContextMenu?.(item.id, e);
      }}
      className={clsx(
        "group mx-1.5 flex cursor-default items-center gap-3 rounded-[var(--radius-md)] px-2",
        "transition-colors duration-100",
        selected
          ? "bg-[color-mix(in_oklch,var(--accent)_14%,transparent)]"
          : "hover:bg-surface-2",
        active &&
          !selected &&
          "bg-surface-2 ring-1 ring-inset ring-[color-mix(in_oklch,var(--accent)_28%,transparent)]",
        active &&
          selected &&
          "ring-1 ring-inset ring-[color-mix(in_oklch,var(--accent)_45%,transparent)]",
      )}
      style={{ height: height - 2, marginTop: 1 }}
    >
      <ItemIcon item={item} size={density === "compact" ? 26 : 34} />

      <div className="min-w-0 flex-1">
        <div
          className={clsx(
            "truncate text-[13px] leading-tight text-text",
            item.kind === "code" || item.kind === "json" ? "font-mono text-[12px]" : "",
            masked && "blur-[3px] select-none transition-[filter] group-hover:blur-0",
          )}
        >
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

        {showSubtitle && (
          <div className="mt-0.5 flex items-center gap-1.5 truncate text-[11px] text-text-faint">
            <span className="truncate">{itemSubtitle(item)}</span>
            {item.tags.slice(0, 2).map((tag) => (
              <Badge key={tag.id} color={tag.color}>
                {tag.name}
              </Badge>
            ))}
            {item.tags.length > 2 && <span>+{item.tags.length - 2}</span>}
          </div>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-1.5 pr-0.5">
        {item.pinned && <Pin className="size-3 text-[var(--accent)]" fill="currentColor" />}
        {item.favorite && (
          <Star className="size-3 text-[var(--warning)]" fill="currentColor" />
        )}
        {item.use_count > 0 && density === "spacious" && (
          <span className="text-[10.5px] tabular-nums text-text-faint">
            ×{item.use_count}
          </span>
        )}
        <span className="w-14 text-right text-[11px] tabular-nums text-text-faint">
          {relativeTime(item.updated_at)}
        </span>
      </div>
    </div>
  );
});
