/**
 * The navigation rail: smart filters, collections, tags and source apps.
 *
 * Everything here writes into the same `Query` the list reads from, so the
 * sidebar is pure navigation with no state of its own beyond disclosure.
 */

import clsx from "clsx";
import {
  Braces,
  ChevronRight,
  Clock,
  Code2,
  Files,
  FolderPlus,
  Image as ImageIcon,
  Inbox,
  KeyRound,
  Link2,
  Pin,
  Plus,
  Repeat,
  Settings2,
  Star,
  Tag as TagIcon,
  Trash2,
  Type,
} from "lucide-react";
import type { ComponentType, ReactNode } from "react";
import { useState } from "react";

import {
  Badge,
  Button,
  Dialog,
  IconButton,
  Input,
  SectionLabel,
  Tooltip,
} from "@/components/ui";
import * as api from "@/lib/api";
import { appName, count } from "@/lib/format";
import type { Kind, SmartFilter } from "@/lib/types";
import { errorMessage } from "@/lib/types";
import { useApp } from "@/store/app";

const FILTERS: Array<{
  value: SmartFilter;
  label: string;
  icon: ComponentType<{ className?: string }>;
  countKey?: keyof import("@/lib/types").Stats;
}> = [
  { value: "all", label: "All", icon: Inbox, countKey: "total" },
  { value: "today", label: "Today", icon: Clock, countKey: "today" },
  { value: "pinned", label: "Pinned", icon: Pin, countKey: "pinned" },
  { value: "favorites", label: "Favourites", icon: Star, countKey: "favorites" },
  { value: "frequent", label: "Frequent", icon: Repeat },
  { value: "vault", label: "Vault", icon: KeyRound, countKey: "vault" },
  { value: "trash", label: "Trash", icon: Trash2, countKey: "trash" },
];

const KIND_ICONS: Partial<Record<Kind, ComponentType<{ className?: string }>>> = {
  text: Type,
  link: Link2,
  code: Code2,
  json: Braces,
  image: ImageIcon,
  files: Files,
};

export function Sidebar({ onOpenSettings }: { onOpenSettings: () => void }) {
  const query = useApp((s) => s.query);
  const stats = useApp((s) => s.stats);
  const tags = useApp((s) => s.tags);
  const collections = useApp((s) => s.collections);
  const setFilter = useApp((s) => s.setFilter);
  const toggleKind = useApp((s) => s.toggleKind);
  const toggleTag = useApp((s) => s.toggleTag);
  const setCollection = useApp((s) => s.setCollection);
  const setQuery = useApp((s) => s.setQuery);
  const reloadReference = useApp((s) => s.reloadReference);
  const toast = useApp((s) => s.toast);

  const [showKinds, setShowKinds] = useState(true);
  const [showApps, setShowApps] = useState(false);
  const [newCollection, setNewCollection] = useState(false);

  return (
    <nav className="flex h-full w-full flex-col bg-bg-subtle">
      <div className="scroll-area min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {/* Smart filters ------------------------------------------------- */}
        <div className="pt-2">
          {FILTERS.map((filter) => {
            const active = query.filter === filter.value && query.collection == null;
            const n = filter.countKey ? (stats?.[filter.countKey] as number | undefined) : undefined;
            return (
              <NavItem
                key={filter.value}
                icon={<filter.icon className="size-3.5" />}
                label={filter.label}
                active={active}
                badge={n ? count(n) : undefined}
                onClick={() => setFilter(filter.value)}
              />
            );
          })}
        </div>

        {/* Collections --------------------------------------------------- */}
        <div className="flex items-center justify-between pr-1">
          <SectionLabel>Collections</SectionLabel>
          <Tooltip label="New collection">
            <IconButton
              aria-label="New collection"
              size="xs"
              variant="ghost"
              onClick={() => setNewCollection(true)}
              icon={<Plus className="size-3" />}
            />
          </Tooltip>
        </div>

        {collections.length === 0 ? (
          <button
            onClick={() => setNewCollection(true)}
            className={clsx(
              "mx-1 flex w-[calc(100%-8px)] items-center gap-2 rounded-[var(--radius-sm)]",
              "px-2 py-1.5 text-[12px] text-text-faint transition-colors hover:bg-surface-2 hover:text-text-muted",
            )}
          >
            <FolderPlus className="size-3.5" />
            Create your first
          </button>
        ) : (
          collections.map((collection) => (
            <NavItem
              key={collection.id}
              icon={
                <span
                  className="size-2 rounded-[3px]"
                  style={{ background: collection.color }}
                />
              }
              label={collection.name}
              active={query.collection === collection.id}
              badge={collection.count ? count(collection.count) : undefined}
              onClick={() =>
                setCollection(query.collection === collection.id ? null : collection.id)
              }
            />
          ))
        )}

        {/* Types --------------------------------------------------------- */}
        <Disclosure
          label="Types"
          open={showKinds}
          onToggle={() => setShowKinds((v) => !v)}
        >
          {(stats?.by_kind ?? []).map(({ kind, count: n }) => {
            const Icon = KIND_ICONS[kind] ?? Type;
            return (
              <NavItem
                key={kind}
                icon={<Icon className="size-3.5" />}
                label={kind === "json" ? "JSON" : kind[0]!.toUpperCase() + kind.slice(1)}
                active={query.kinds.includes(kind)}
                badge={count(n)}
                onClick={() => toggleKind(kind)}
              />
            );
          })}
        </Disclosure>

        {/* Tags ---------------------------------------------------------- */}
        {tags.length > 0 && (
          <>
            <SectionLabel>Tags</SectionLabel>
            <div className="flex flex-wrap gap-1 px-2 pb-1">
              {tags.map((tag) => {
                const active = query.tags.includes(tag.id);
                return (
                  <button
                    key={tag.id}
                    onClick={() => toggleTag(tag.id)}
                    className={clsx(
                      "rounded-full px-1.5 py-px text-[10.5px] font-medium transition-all",
                      active ? "ring-1" : "opacity-70 hover:opacity-100",
                    )}
                    style={{
                      color: tag.color,
                      background: `color-mix(in oklch, ${tag.color} ${active ? 22 : 12}%, transparent)`,
                      boxShadow: active
                        ? `inset 0 0 0 1px color-mix(in oklch, ${tag.color} 45%, transparent)`
                        : undefined,
                    }}
                  >
                    {tag.name}
                    {tag.count > 0 && (
                      <span className="ml-1 opacity-60">{count(tag.count)}</span>
                    )}
                  </button>
                );
              })}
            </div>
          </>
        )}

        {/* Sources ------------------------------------------------------- */}
        {(stats?.by_app.length ?? 0) > 0 && (
          <Disclosure label="Sources" open={showApps} onToggle={() => setShowApps((v) => !v)}>
            {stats!.by_app.map(({ app, count: n }) => (
              <NavItem
                key={app}
                icon={<span className="size-1.5 rounded-full bg-current opacity-40" />}
                label={appName(app)}
                active={query.source_app === app}
                badge={count(n)}
                onClick={() =>
                  setQuery({ source_app: query.source_app === app ? null : app })
                }
              />
            ))}
          </Disclosure>
        )}
      </div>

      {/* Footer ----------------------------------------------------------- */}
      <div className="border-t border-border px-2 py-2">
        <NavItem
          icon={<Settings2 className="size-3.5" />}
          label="Settings"
          onClick={onOpenSettings}
        />
      </div>

      <NewCollectionDialog
        open={newCollection}
        onClose={() => setNewCollection(false)}
        onCreated={async () => {
          await reloadReference();
          toast("Collection created", "success");
        }}
        onError={(e) => toast(errorMessage(e), "error")}
      />
    </nav>
  );
}

// ---------------------------------------------------------------------------

function NavItem({
  icon,
  label,
  active,
  badge,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  active?: boolean;
  badge?: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={clsx(
        "group mx-1 flex w-[calc(100%-8px)] items-center gap-2 rounded-[var(--radius-sm)]",
        "px-2 py-[5px] text-left text-[12.5px] transition-colors duration-100",
        active
          ? "bg-[color-mix(in_oklch,var(--accent)_15%,transparent)] font-medium text-text"
          : "text-text-muted hover:bg-surface-2 hover:text-text",
      )}
    >
      <span
        className={clsx(
          "flex size-3.5 shrink-0 items-center justify-center",
          active ? "text-[var(--accent)]" : "text-text-faint group-hover:text-text-muted",
        )}
      >
        {icon}
      </span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {badge && (
        <span className="shrink-0 text-[10.5px] tabular-nums text-text-faint">{badge}</span>
      )}
    </button>
  );
}

function Disclosure({
  label,
  open,
  onToggle,
  children,
}: {
  label: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <>
      <button
        onClick={onToggle}
        className={clsx(
          "flex w-full items-center gap-1 px-2 pb-1 pt-3",
          "text-[10.5px] font-semibold uppercase tracking-wider text-text-faint",
          "transition-colors hover:text-text-muted",
        )}
      >
        <ChevronRight
          className={clsx("size-3 transition-transform duration-150", open && "rotate-90")}
        />
        {label}
      </button>
      {open && children}
    </>
  );
}

const COLLECTION_COLORS = [
  "#6366f1",
  "#8b5cf6",
  "#ec4899",
  "#f43f5e",
  "#f59e0b",
  "#10b981",
  "#06b6d4",
  "#3b82f6",
];

function NewCollectionDialog({
  open,
  onClose,
  onCreated,
  onError,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
  onError: (e: unknown) => void;
}) {
  const [name, setName] = useState("");
  const [color, setColor] = useState(COLLECTION_COLORS[0]!);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    try {
      await api.createCollection(name.trim(), "folder", color);
      setName("");
      onCreated();
      onClose();
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="New collection"
      description="Group related entries so they stay together and survive automatic cleanup."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={submit} loading={busy} disabled={!name.trim()}>
            Create
          </Button>
        </>
      }
    >
      <div className="space-y-3 pb-2">
        <Input
          autoFocus
          placeholder="Collection name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit()}
        />
        <div className="flex items-center gap-1.5">
          {COLLECTION_COLORS.map((c) => (
            <button
              key={c}
              onClick={() => setColor(c)}
              aria-label={`Colour ${c}`}
              className={clsx(
                "size-5 rounded-full transition-transform",
                color === c ? "scale-110 ring-2 ring-offset-2 ring-offset-surface" : "hover:scale-105",
              )}
              style={{ background: c, ...(color === c ? { boxShadow: `0 0 0 2px ${c}` } : {}) }}
            />
          ))}
        </div>
      </div>
    </Dialog>
  );
}

/** Compact tag chip reused by the bulk-action bar. */
export function TagChip({ name, color }: { name: string; color: string }) {
  return (
    <Badge color={color}>
      <TagIcon className="size-2.5" />
      {name}
    </Badge>
  );
}
