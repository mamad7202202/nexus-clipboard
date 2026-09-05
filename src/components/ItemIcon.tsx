/**
 * The leading glyph on every history row.
 *
 * Image items show their own thumbnail, colours show the colour itself, and
 * everything else gets a tinted kind icon. That makes the list scannable at a
 * glance without reading a single word.
 */

import clsx from "clsx";
import {
  Braces,
  Code2,
  FileText,
  Files,
  Hash,
  Image as ImageIcon,
  KeyRound,
  Link2,
  Mail,
  Phone,
  Type,
} from "lucide-react";
import type { ComponentType } from "react";

import { kindColor } from "@/lib/format";
import type { Item, Kind } from "@/lib/types";

const ICONS: Record<Kind, ComponentType<{ className?: string }>> = {
  text: Type,
  link: Link2,
  email: Mail,
  phone: Phone,
  color: Hash,
  code: Code2,
  json: Braces,
  image: ImageIcon,
  files: Files,
  rich: FileText,
  secret: KeyRound,
};

export function ItemIcon({
  item,
  size = 34,
  className,
}: {
  item: Item;
  size?: number;
  className?: string;
}) {
  const color = kindColor(item.kind);

  // A thumbnail is far more informative than a generic image glyph.
  if (item.kind === "image" && item.meta.thumb) {
    return (
      <div
        className={clsx(
          "shrink-0 overflow-hidden rounded-[var(--radius-sm)]",
          "border border-border bg-surface-2",
          className,
        )}
        style={{ width: size, height: size }}
      >
        <img
          src={item.meta.thumb}
          alt=""
          loading="lazy"
          decoding="async"
          className="size-full object-cover"
          draggable={false}
        />
      </div>
    );
  }

  if (item.kind === "color" && item.meta.color) {
    return (
      <div
        className={clsx(
          "shrink-0 rounded-[var(--radius-sm)] border border-border",
          // A chequerboard behind the swatch so translucent colours read
          // correctly rather than blending into the row.
          "bg-[repeating-conic-gradient(var(--surface-3)_0%_25%,var(--surface)_0%_50%)] bg-[length:8px_8px]",
          className,
        )}
        style={{ width: size, height: size }}
      >
        <div
          className="size-full rounded-[calc(var(--radius-sm)-1px)]"
          style={{ background: item.meta.color }}
        />
      </div>
    );
  }

  const Icon = ICONS[item.kind] ?? Type;

  return (
    <div
      className={clsx(
        "flex shrink-0 items-center justify-center rounded-[var(--radius-sm)]",
        className,
      )}
      style={{
        width: size,
        height: size,
        color,
        background: `color-mix(in oklch, ${color} 12%, transparent)`,
        boxShadow: `inset 0 0 0 1px color-mix(in oklch, ${color} 18%, transparent)`,
      }}
    >
      <Icon className="size-[46%]" />
    </div>
  );
}
