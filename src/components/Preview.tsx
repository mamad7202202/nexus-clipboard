/**
 * The detail pane.
 *
 * Renders an item the way it actually is — code with syntax highlighting, a
 * colour as a swatch, an image at full size, files as a browsable list — and
 * exposes the actions that make sense for that kind.
 */

import clsx from "clsx";
import {
  Check,
  Clipboard,
  Copy,
  ExternalLink,
  Eye,
  EyeOff,
  FolderOpen,
  Pencil,
  Pin,
  Sparkles,
  Star,
  Trash2,
  Wand2,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { TransformMenu } from "@/components/TransformMenu";
import {
  Badge,
  Button,
  EmptyState,
  IconButton,
  Spinner,
  Tooltip,
} from "@/components/ui";
import * as api from "@/lib/api";
import { appName, bytes, fullTime, kindColor, kindLabel } from "@/lib/format";
import { highlight } from "@/lib/highlight";
import type { Item } from "@/lib/types";
import { errorMessage } from "@/lib/types";
import { useApp } from "@/store/app";

export function Preview({
  itemId,
  onClose,
  onChanged,
}: {
  itemId: number | null;
  onClose?: () => void;
  onChanged?: () => void;
}) {
  const [item, setItem] = useState<Item | null>(null);
  const [image, setImage] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  const toast = useApp((s) => s.toast);
  const blurSecrets = useApp((s) => s.settings?.blur_secrets ?? true);
  const vaultUnlocked = useApp((s) => s.vault?.unlocked ?? false);

  // Load whenever the selection changes. A stale response for a previously
  // selected item must not overwrite the current one.
  useEffect(() => {
    if (itemId == null) {
      setItem(null);
      setImage(null);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setRevealed(false);
    setEditing(false);
    setImage(null);

    api
      .getItem(itemId, false)
      .then(async (loaded) => {
        if (cancelled) return;
        setItem(loaded);
        setDraft(loaded.body ?? "");
        if (loaded.kind === "image") {
          const uri = await api.getImage(loaded.id).catch(() => null);
          if (!cancelled) setImage(uri);
        }
      })
      .catch((e) => {
        if (!cancelled) toast(errorMessage(e), "error");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [itemId, toast]);

  const reveal = async () => {
    if (!item) return;
    try {
      const full = await api.getItem(item.id, true);
      setItem(full);
      setDraft(full.body ?? "");
      setRevealed(true);
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };

  const copy = async () => {
    if (!item) return;
    try {
      await api.useItem(item.id, "copy_only");
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };

  const togglePin = async () => {
    if (!item) return;
    await api.setPinned(item.id, !item.pinned);
    setItem({ ...item, pinned: !item.pinned });
    onChanged?.();
  };

  const toggleFavorite = async () => {
    if (!item) return;
    await api.setFavorite(item.id, !item.favorite);
    setItem({ ...item, favorite: !item.favorite });
    onChanged?.();
  };

  const remove = async () => {
    if (!item) return;
    await api.deleteItems([item.id]);
    onChanged?.();
    onClose?.();
  };

  const saveEdit = async () => {
    if (!item) return;
    setBusy(true);
    try {
      const updated = await api.updateItem(item.id, draft);
      setItem(updated);
      setEditing(false);
      onChanged?.();
      toast("Saved", "success");
    } catch (e) {
      toast(errorMessage(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const summarize = async () => {
    if (!item) return;
    setBusy(true);
    try {
      const summary = await api.summarizeItem(item.id);
      setItem({ ...item, meta: { ...item.meta, summary } });
    } catch (e) {
      toast(errorMessage(e), "error");
    } finally {
      setBusy(false);
    }
  };

  if (itemId == null) {
    return (
      <EmptyState
        icon={<Clipboard />}
        title="Nothing selected"
        description="Pick an entry from the list to see it here."
      />
    );
  }

  if (loading && !item) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner className="size-5 text-text-faint" />
      </div>
    );
  }

  if (!item) return null;

  const hidden = item.encrypted && !revealed;
  const canEdit = !item.encrypted && item.kind !== "image" && item.kind !== "files";

  return (
    <div className="flex h-full flex-col">
      {/* Header ---------------------------------------------------------- */}
      <div className="flex items-start gap-2 border-b border-border px-4 py-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <Badge color={kindColor(item.kind)}>
              {item.kind === "code" && item.meta.language
                ? item.meta.language
                : kindLabel(item.kind)}
            </Badge>
            {item.sensitive && <Badge color="var(--danger)">encrypted</Badge>}
            {item.tags.map((tag) => (
              <Badge key={tag.id} color={tag.color}>
                {tag.name}
              </Badge>
            ))}
          </div>

          <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11.5px] text-text-faint">
            <span title={fullTime(item.created_at)}>{fullTime(item.updated_at)}</span>
            <span>·</span>
            <span>{bytes(item.bytes)}</span>
            {item.source_app && (
              <>
                <span>·</span>
                <span className="truncate" title={item.source_title ?? undefined}>
                  {appName(item.source_app)}
                </span>
              </>
            )}
            {item.use_count > 0 && (
              <>
                <span>·</span>
                <span>used {item.use_count}×</span>
              </>
            )}
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-0.5">
          <Tooltip label={item.pinned ? "Unpin" : "Pin"}>
            <IconButton
              aria-label="Pin"
              variant="ghost"
              onClick={togglePin}
              icon={
                <Pin
                  className={clsx("size-3.5", item.pinned && "text-[var(--accent)]")}
                  fill={item.pinned ? "currentColor" : "none"}
                />
              }
            />
          </Tooltip>
          <Tooltip label={item.favorite ? "Remove from favourites" : "Add to favourites"}>
            <IconButton
              aria-label="Favourite"
              variant="ghost"
              onClick={toggleFavorite}
              icon={
                <Star
                  className={clsx("size-3.5", item.favorite && "text-[var(--warning)]")}
                  fill={item.favorite ? "currentColor" : "none"}
                />
              }
            />
          </Tooltip>
          {onClose && (
            <IconButton
              aria-label="Close preview"
              variant="ghost"
              onClick={onClose}
              icon={<X className="size-3.5" />}
            />
          )}
        </div>
      </div>

      {/* AI summary ------------------------------------------------------ */}
      {item.meta.summary && (
        <div className="flex gap-2 border-b border-border bg-surface-2/40 px-4 py-2">
          <Sparkles className="mt-px size-3.5 shrink-0 text-[var(--accent)]" />
          <p className="text-[12px] leading-relaxed text-text-muted selectable">
            {item.meta.summary}
          </p>
        </div>
      )}

      {/* Body ------------------------------------------------------------ */}
      <div className="scroll-area min-h-0 flex-1 overflow-auto">
        {hidden ? (
          <LockedBody unlocked={vaultUnlocked} onReveal={reveal} reason={item.meta.secret_reason} />
        ) : editing ? (
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            spellCheck={false}
            autoFocus
            className={clsx(
              "h-full w-full resize-none bg-transparent p-4 outline-none selectable",
              "font-mono text-[12.5px] leading-relaxed text-text",
            )}
          />
        ) : (
          <ItemBody item={item} image={image} blur={item.sensitive && blurSecrets && !revealed} />
        )}
      </div>

      {/* Footer ---------------------------------------------------------- */}
      <div className="flex items-center gap-1.5 border-t border-border px-3 py-2">
        {editing ? (
          <>
            <Button variant="primary" size="sm" onClick={saveEdit} loading={busy}>
              Save
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setDraft(item.body ?? "");
                setEditing(false);
              }}
            >
              Cancel
            </Button>
          </>
        ) : (
          <>
            <Button
              variant="primary"
              size="sm"
              onClick={copy}
              icon={copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            >
              {copied ? "Copied" : "Copy"}
            </Button>

            {item.kind === "link" && (
              <Button
                variant="secondary"
                size="sm"
                icon={<ExternalLink className="size-3.5" />}
                onClick={() =>
                  api.openUrl(item.body ?? item.preview).catch((e) => toast(errorMessage(e), "error"))
                }
              >
                Open
              </Button>
            )}

            {item.kind === "files" && item.meta.files?.[0] && (
              <Button
                variant="secondary"
                size="sm"
                icon={<FolderOpen className="size-3.5" />}
                onClick={() =>
                  api
                    .revealPath(item.meta.files![0]!)
                    .catch(() => toast("That file no longer exists", "error"))
                }
              >
                Reveal
              </Button>
            )}

            {canEdit && !hidden && (
              <Tooltip label="Edit contents">
                <IconButton
                  aria-label="Edit"
                  variant="ghost"
                  onClick={() => setEditing(true)}
                  icon={<Pencil className="size-3.5" />}
                />
              </Tooltip>
            )}

            {!hidden && item.body != null && (
              <TransformMenu
                text={item.body}
                onResult={(result) => {
                  setDraft(result);
                  setEditing(true);
                }}
                trigger={({ toggle }) => (
                  <Tooltip label="Transform">
                    <IconButton
                      aria-label="Transform"
                      variant="ghost"
                      onClick={toggle}
                      icon={<Wand2 className="size-3.5" />}
                    />
                  </Tooltip>
                )}
              />
            )}

            {!hidden && !item.meta.summary && item.bytes > 400 && (
              <Tooltip label="Summarise with AI">
                <IconButton
                  aria-label="Summarise"
                  variant="ghost"
                  onClick={summarize}
                  loading={busy}
                  icon={<Sparkles className="size-3.5" />}
                />
              </Tooltip>
            )}

            <div className="flex-1" />

            <Tooltip label="Move to trash">
              <IconButton
                aria-label="Delete"
                variant="ghost"
                onClick={remove}
                icon={<Trash2 className="size-3.5" />}
                className="hover:!text-[var(--danger)]"
              />
            </Tooltip>
          </>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------

function LockedBody({
  unlocked,
  onReveal,
  reason,
}: {
  unlocked: boolean;
  onReveal: () => void;
  reason?: string;
}) {
  return (
    <EmptyState
      icon={<EyeOff />}
      title="This entry is encrypted"
      description={
        unlocked
          ? `Nexus detected ${reason ? reason.replace(/_/g, " ") : "a credential"} and stored it encrypted. Reveal it to see the contents.`
          : "Unlock the vault in Settings to reveal encrypted entries."
      }
      action={
        unlocked ? (
          <Button variant="secondary" icon={<Eye className="size-3.5" />} onClick={onReveal}>
            Reveal
          </Button>
        ) : undefined
      }
    />
  );
}

function ItemBody({
  item,
  image,
  blur,
}: {
  item: Item;
  image: string | null;
  blur: boolean;
}) {
  const [showBlurred, setShowBlurred] = useState(false);
  const bodyRef = useRef<HTMLPreElement>(null);

  const highlighted = useMemo(() => {
    if (item.kind !== "code" && item.kind !== "json") return null;
    if (!item.body) return null;
    return highlight(item.body, item.kind === "json" ? "json" : item.meta.language);
  }, [item.body, item.kind, item.meta.language]);

  if (item.kind === "image") {
    return (
      <div className="flex h-full items-center justify-center p-4">
        {image ? (
          <img
            src={image}
            alt=""
            className="max-h-full max-w-full rounded-[var(--radius-md)] border border-border object-contain"
            style={{
              // A chequerboard reveals transparency instead of hiding it.
              backgroundImage:
                "repeating-conic-gradient(var(--surface-2) 0% 25%, var(--surface-3) 0% 50%)",
              backgroundSize: "16px 16px",
            }}
            draggable={false}
          />
        ) : (
          <Spinner className="size-5 text-text-faint" />
        )}
      </div>
    );
  }

  if (item.kind === "files") {
    const files = item.meta.files ?? [];
    return (
      <ul className="divide-y divide-border">
        {files.map((path) => (
          <li key={path} className="group flex items-center gap-2 px-4 py-2">
            <FolderOpen className="size-3.5 shrink-0 text-text-faint" />
            <span className="min-w-0 flex-1 truncate text-[12.5px] selectable" title={path}>
              {path}
            </span>
            <IconButton
              aria-label="Reveal in Explorer"
              size="xs"
              variant="ghost"
              className="opacity-0 transition-opacity group-hover:opacity-100"
              onClick={() => api.revealPath(path).catch(() => {})}
              icon={<ExternalLink className="size-3" />}
            />
          </li>
        ))}
      </ul>
    );
  }

  if (item.kind === "color" && item.meta.color) {
    return (
      <div className="space-y-3 p-4">
        <div
          className="h-32 w-full rounded-[var(--radius-lg)] border border-border"
          style={{ background: item.meta.color }}
        />
        <p className="text-center font-mono text-[15px] tracking-wide selectable">
          {item.body ?? item.meta.color}
        </p>
      </div>
    );
  }

  const body = item.body ?? item.preview;
  const isMono = item.kind === "code" || item.kind === "json";

  return (
    <div className="relative">
      <pre
        ref={bodyRef}
        onClick={() => blur && setShowBlurred(true)}
        className={clsx(
          "whitespace-pre-wrap break-words p-4 selectable",
          isMono ? "font-mono text-[12.5px] leading-[1.65]" : "text-[13px] leading-relaxed",
          blur && !showBlurred && "cursor-pointer blur-[5px] select-none",
        )}
      >
        {highlighted ? (
          // Safe: highlight.js escapes the source before adding markup.
          <code className="hljs" dangerouslySetInnerHTML={{ __html: highlighted }} />
        ) : (
          <code className={isMono ? "hljs" : undefined}>{body}</code>
        )}
      </pre>

      {blur && !showBlurred && (
        <button
          onClick={() => setShowBlurred(true)}
          className={clsx(
            "absolute inset-0 flex items-center justify-center gap-2",
            "text-[12.5px] font-medium text-text-muted",
          )}
        >
          <Eye className="size-3.5" />
          Click to reveal
        </button>
      )}
    </div>
  );
}
