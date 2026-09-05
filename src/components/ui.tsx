/**
 * UI primitives.
 *
 * Small, unopinionated building blocks shared by every surface. They own
 * appearance and accessibility; callers own layout and behaviour.
 */

import clsx from "clsx";
import { AnimatePresence, motion } from "motion/react";
import {
  createContext,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "subtle";
type ButtonSize = "xs" | "sm" | "md";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: ReactNode;
  loading?: boolean;
}

const BUTTON_VARIANTS: Record<ButtonVariant, string> = {
  primary:
    "bg-[var(--accent)] text-white hover:brightness-110 active:brightness-95 " +
    "shadow-[0_1px_2px_hsl(var(--shadow-color)/0.2),inset_0_1px_0_hsl(0_0%_100%/0.16)]",
  secondary:
    "bg-surface-2 text-text border border-border hover:bg-surface-3 hover:border-border-strong",
  ghost: "text-text-muted hover:text-text hover:bg-surface-2",
  subtle: "bg-surface-2/60 text-text-muted hover:bg-surface-2 hover:text-text",
  danger:
    "bg-[var(--danger)] text-white hover:brightness-110 active:brightness-95 " +
    "shadow-[0_1px_2px_hsl(var(--shadow-color)/0.2)]",
};

const BUTTON_SIZES: Record<ButtonSize, string> = {
  xs: "h-6 px-2 text-[11.5px] gap-1 rounded-[var(--radius-xs)]",
  sm: "h-7.5 px-2.5 gap-1.5 rounded-[var(--radius-sm)]",
  md: "h-9 px-3.5 gap-2 rounded-[var(--radius-md)]",
};

export function Button({
  variant = "secondary",
  size = "sm",
  icon,
  loading,
  className,
  children,
  disabled,
  ...rest
}: ButtonProps) {
  return (
    <button
      {...rest}
      disabled={disabled || loading}
      className={clsx(
        "inline-flex items-center justify-center font-medium whitespace-nowrap",
        "transition-[background-color,color,border-color,filter,opacity] duration-150",
        "disabled:opacity-45 disabled:pointer-events-none no-drag",
        BUTTON_VARIANTS[variant],
        BUTTON_SIZES[size],
        className,
      )}
    >
      {loading ? <Spinner className="size-3.5" /> : icon}
      {children}
    </button>
  );
}

/** Square icon-only button. */
export function IconButton({
  size = "sm",
  className,
  ...rest
}: Omit<ButtonProps, "children"> & { children?: never; "aria-label": string }) {
  return (
    <Button
      {...rest}
      size={size}
      className={clsx(
        "!px-0 aspect-square",
        size === "xs" && "!h-6 w-6",
        size === "sm" && "!h-7.5 w-7.5",
        size === "md" && "!h-9 w-9",
        className,
      )}
    />
  );
}

export function Spinner({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      className={clsx("animate-spin", className)}
      aria-hidden
    >
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="2.5" opacity="0.22" />
      <path
        d="M21 12a9 9 0 0 0-9-9"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  icon?: ReactNode;
  suffix?: ReactNode;
}

export function Input({ icon, suffix, className, ...rest }: InputProps) {
  return (
    <div
      className={clsx(
        "group relative flex items-center gap-2 rounded-[var(--radius-md)]",
        "bg-surface-2 border border-border transition-colors duration-150",
        "focus-within:border-[color-mix(in_oklch,var(--accent)_55%,transparent)]",
        "focus-within:shadow-[0_0_0_3px_color-mix(in_oklch,var(--accent)_12%,transparent)]",
        className,
      )}
    >
      {icon && (
        <span className="pl-2.5 text-text-faint group-focus-within:text-text-muted transition-colors">
          {icon}
        </span>
      )}
      <input
        {...rest}
        className={clsx(
          "peer min-w-0 flex-1 bg-transparent py-1.5 text-[13px] text-text",
          "placeholder:text-text-faint outline-none selectable",
          icon ? "pl-0" : "pl-2.5",
          suffix ? "pr-0" : "pr-2.5",
        )}
      />
      {suffix && <span className="pr-2 text-text-faint">{suffix}</span>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Keyboard shortcut chip
// ---------------------------------------------------------------------------

export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={clsx(
        "inline-flex h-5 min-w-5 items-center justify-center rounded-[5px] px-1.5",
        "bg-surface-3 text-[10.5px] font-medium text-text-muted",
        "border border-border-strong/60 shadow-[0_1px_0_hsl(var(--shadow-color)/0.14)]",
        "font-sans tracking-wide",
        className,
      )}
    >
      {children}
    </kbd>
  );
}

export function KeyCombo({ keys }: { keys: string[] }) {
  return (
    <span className="inline-flex items-center gap-1">
      {keys.map((key, i) => (
        <Kbd key={`${key}-${i}`}>{key}</Kbd>
      ))}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Badge
// ---------------------------------------------------------------------------

export function Badge({
  children,
  color,
  className,
}: {
  children: ReactNode;
  color?: string;
  className?: string;
}) {
  return (
    <span
      className={clsx(
        "inline-flex items-center gap-1 rounded-full px-1.5 py-px",
        "text-[10.5px] font-medium leading-[1.5]",
        className,
      )}
      style={
        color
          ? {
              color,
              background: `color-mix(in oklch, ${color} 14%, transparent)`,
              boxShadow: `inset 0 0 0 1px color-mix(in oklch, ${color} 22%, transparent)`,
            }
          : undefined
      }
    >
      {children}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Tooltip
// ---------------------------------------------------------------------------

export function Tooltip({
  label,
  children,
  side = "top",
  delay = 380,
}: {
  label: ReactNode;
  children: ReactNode;
  side?: "top" | "bottom" | "left" | "right";
  delay?: number;
}) {
  const [open, setOpen] = useState(false);
  const [coords, setCoords] = useState({ x: 0, y: 0 });
  const anchorRef = useRef<HTMLSpanElement>(null);
  const timer = useRef<number>(0);

  const show = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      const rect = anchorRef.current?.getBoundingClientRect();
      if (!rect) return;
      const positions = {
        top: { x: rect.left + rect.width / 2, y: rect.top - 8 },
        bottom: { x: rect.left + rect.width / 2, y: rect.bottom + 8 },
        left: { x: rect.left - 8, y: rect.top + rect.height / 2 },
        right: { x: rect.right + 8, y: rect.top + rect.height / 2 },
      };
      setCoords(positions[side]);
      setOpen(true);
    }, delay);
  };

  const hide = () => {
    window.clearTimeout(timer.current);
    setOpen(false);
  };

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const transforms = {
    top: "translate(-50%, -100%)",
    bottom: "translate(-50%, 0)",
    left: "translate(-100%, -50%)",
    right: "translate(0, -50%)",
  };

  return (
    <>
      <span
        ref={anchorRef}
        onMouseEnter={show}
        onMouseLeave={hide}
        onPointerDown={hide}
        className="contents"
      >
        {children}
      </span>
      {open &&
        createPortal(
          <div
            role="tooltip"
            className={clsx(
              "pointer-events-none fixed z-[9999] animate-fade-in",
              "rounded-[var(--radius-sm)] bg-surface-3 px-2 py-1",
              "text-[11.5px] text-text elevation-2 border border-border-strong/50",
              "max-w-[280px] whitespace-nowrap",
            )}
            style={{ left: coords.x, top: coords.y, transform: transforms[side] }}
          >
            {label}
          </div>,
          document.body,
        )}
    </>
  );
}

// ---------------------------------------------------------------------------
// Dialog
// ---------------------------------------------------------------------------

export function Dialog({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 460,
}: {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  const titleId = useId();

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    // Capture phase so a dialog always wins over list-level key handling.
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, onClose]);

  return createPortal(
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-[9998] flex items-center justify-center p-6"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.14 }}
        >
          <div
            className="absolute inset-0 bg-[var(--overlay)] backdrop-blur-[2px]"
            onClick={onClose}
          />
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            className={clsx(
              "relative w-full rounded-[var(--radius-xl)] border border-border",
              "bg-surface elevation-float overflow-hidden",
            )}
            style={{ maxWidth: width }}
            initial={{ opacity: 0, scale: 0.96, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.97, y: 6 }}
            transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
          >
            <div className="px-5 pt-4 pb-3">
              <h2 id={titleId} className="text-[14.5px] font-semibold text-text">
                {title}
              </h2>
              {description && (
                <p className="mt-1 text-[12.5px] leading-relaxed text-text-muted">
                  {description}
                </p>
              )}
            </div>
            {children && <div className="px-5 pb-2">{children}</div>}
            {footer && (
              <div className="flex items-center justify-end gap-2 border-t border-border bg-surface-2/50 px-5 py-3">
                {footer}
              </div>
            )}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}

/** A confirm dialog that defaults to the safe choice. */
export function ConfirmDialog({
  open,
  onClose,
  onConfirm,
  title,
  description,
  confirmLabel = "Confirm",
  destructive,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: ReactNode;
  description?: ReactNode;
  confirmLabel?: string;
  destructive?: boolean;
}) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={title}
      description={description}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={destructive ? "danger" : "primary"}
            onClick={() => {
              onConfirm();
              onClose();
            }}
          >
            {confirmLabel}
          </Button>
        </>
      }
    />
  );
}

// ---------------------------------------------------------------------------
// Switch
// ---------------------------------------------------------------------------

export function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={clsx(
        "relative h-[19px] w-[33px] shrink-0 rounded-full transition-colors duration-200",
        "disabled:opacity-45 disabled:pointer-events-none no-drag",
        checked ? "bg-[var(--accent)]" : "bg-surface-3 border border-border-strong/70",
      )}
    >
      <motion.span
        className={clsx(
          "absolute top-[2px] size-[15px] rounded-full bg-white",
          "shadow-[0_1px_2px_hsl(var(--shadow-color)/0.3)]",
        )}
        animate={{ left: checked ? 16 : 2 }}
        transition={{ type: "spring", stiffness: 700, damping: 34 }}
      />
    </button>
  );
}

// ---------------------------------------------------------------------------
// Segmented control
// ---------------------------------------------------------------------------

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  size = "sm",
}: {
  value: T;
  options: Array<{ value: T; label: ReactNode; title?: string }>;
  onChange: (value: T) => void;
  size?: "xs" | "sm";
}) {
  const groupId = useId();

  return (
    <div
      role="radiogroup"
      className={clsx(
        "inline-flex items-center gap-0.5 rounded-[var(--radius-md)]",
        "bg-surface-2 p-0.5 border border-border",
      )}
    >
      {options.map((option) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            role="radio"
            aria-checked={active}
            title={option.title}
            onClick={() => onChange(option.value)}
            className={clsx(
              "relative rounded-[calc(var(--radius-md)-3px)] font-medium transition-colors no-drag",
              size === "xs" ? "h-5.5 px-2 text-[11px]" : "h-6.5 px-2.5 text-[12px]",
              active ? "text-text" : "text-text-faint hover:text-text-muted",
            )}
          >
            {active && (
              <motion.span
                layoutId={`segmented-${groupId}`}
                className="absolute inset-0 rounded-[calc(var(--radius-md)-3px)] bg-surface elevation-1"
                transition={{ type: "spring", stiffness: 520, damping: 38 }}
              />
            )}
            <span className="relative z-10">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Dropdown menu
// ---------------------------------------------------------------------------

interface MenuContext {
  close: () => void;
}
const MenuCtx = createContext<MenuContext>({ close: () => {} });

export function Menu({
  trigger,
  children,
  align = "start",
  width = 210,
}: {
  trigger: (props: { open: boolean; toggle: () => void }) => ReactNode;
  children: ReactNode;
  align?: "start" | "end";
  width?: number;
}) {
  const [open, setOpen] = useState(false);
  const [coords, setCoords] = useState({ x: 0, y: 0 });
  const anchorRef = useRef<HTMLDivElement>(null);

  const toggle = () => {
    const rect = anchorRef.current?.getBoundingClientRect();
    if (!rect) return;

    // Flip upwards when there is not enough room below.
    const below = window.innerHeight - rect.bottom;
    const y = below < 200 ? rect.top - 6 : rect.bottom + 6;
    const x = align === "end" ? rect.right : rect.left;

    setCoords({ x, y });
    setOpen((v) => !v);
  };

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!anchorRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const flipped = coords.y < (anchorRef.current?.getBoundingClientRect().top ?? 0);

  return (
    <div ref={anchorRef} className="contents">
      {trigger({ open, toggle })}
      {open &&
        createPortal(
          <MenuCtx.Provider value={{ close: () => setOpen(false) }}>
            <div
              role="menu"
              className={clsx(
                "fixed z-[9997] animate-scale-in origin-top",
                "rounded-[var(--radius-lg)] border border-border bg-surface",
                "elevation-3 p-1 overflow-hidden",
              )}
              style={{
                left: coords.x,
                top: coords.y,
                width,
                transform: `translate(${align === "end" ? "-100%" : "0"}, ${flipped ? "-100%" : "0"})`,
              }}
            >
              {children}
            </div>
          </MenuCtx.Provider>,
          document.body,
        )}
    </div>
  );
}

export function MenuItem({
  children,
  icon,
  shortcut,
  onSelect,
  danger,
  disabled,
  checked,
}: {
  children: ReactNode;
  icon?: ReactNode;
  shortcut?: string[];
  onSelect?: () => void;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
}) {
  const { close } = useContext(MenuCtx);

  return (
    <button
      role="menuitem"
      disabled={disabled}
      onClick={() => {
        onSelect?.();
        close();
      }}
      className={clsx(
        "flex w-full items-center gap-2 rounded-[var(--radius-sm)] px-2 py-[5px]",
        "text-left text-[12.5px] transition-colors",
        "disabled:opacity-40 disabled:pointer-events-none",
        danger
          ? "text-[var(--danger)] hover:bg-[color-mix(in_oklch,var(--danger)_12%,transparent)]"
          : "text-text hover:bg-surface-2",
      )}
    >
      {icon && <span className="shrink-0 text-text-muted [&>svg]:size-3.5">{icon}</span>}
      <span className="min-w-0 flex-1 truncate">{children}</span>
      {checked && <span className="text-[var(--accent)]">✓</span>}
      {shortcut && <KeyCombo keys={shortcut} />}
    </button>
  );
}

export function MenuSeparator() {
  return <div className="my-1 h-px bg-border" />;
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return (
    <div className="px-2 pt-2 pb-1 text-[10.5px] font-semibold uppercase tracking-wider text-text-faint">
      {children}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon?: ReactNode;
  title: string;
  description?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-8 text-center animate-fade-in">
      {icon && (
        <div
          className={clsx(
            "flex size-12 items-center justify-center rounded-[var(--radius-lg)]",
            "bg-surface-2 text-text-faint [&>svg]:size-5.5",
          )}
        >
          {icon}
        </div>
      )}
      <div className="space-y-1">
        <p className="text-[13.5px] font-medium text-text">{title}</p>
        {description && (
          <p className="max-w-[320px] text-[12.5px] leading-relaxed text-text-muted">
            {description}
          </p>
        )}
      </div>
      {action}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------------

export function ToastStack({
  toasts,
  onDismiss,
}: {
  toasts: Array<{ id: number; message: string; tone: "info" | "success" | "error" }>;
  onDismiss: (id: number) => void;
}) {
  const tones = {
    info: "border-border bg-surface-3 text-text",
    success:
      "border-[color-mix(in_oklch,var(--success)_35%,transparent)] bg-surface-3 text-text",
    error: "border-[color-mix(in_oklch,var(--danger)_40%,transparent)] bg-surface-3 text-text",
  };

  return createPortal(
    <div className="pointer-events-none fixed bottom-4 left-1/2 z-[9999] flex -translate-x-1/2 flex-col items-center gap-2">
      <AnimatePresence initial={false}>
        {toasts.map((toast) => (
          <motion.button
            key={toast.id}
            layout
            initial={{ opacity: 0, y: 14, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.97 }}
            transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
            onClick={() => onDismiss(toast.id)}
            className={clsx(
              "pointer-events-auto max-w-[380px] rounded-[var(--radius-lg)] border px-3 py-2",
              "text-[12.5px] elevation-3 text-left",
              tones[toast.tone],
            )}
          >
            <span className="flex items-center gap-2">
              {toast.tone !== "info" && (
                <span
                  className="size-1.5 shrink-0 rounded-full"
                  style={{
                    background:
                      toast.tone === "error" ? "var(--danger)" : "var(--success)",
                  }}
                />
              )}
              {toast.message}
            </span>
          </motion.button>
        ))}
      </AnimatePresence>
    </div>,
    document.body,
  );
}

// ---------------------------------------------------------------------------
// Misc
// ---------------------------------------------------------------------------

export function Separator({ className }: { className?: string }) {
  return <div className={clsx("h-px w-full bg-border", className)} />;
}

export function Skeleton({
  className,
  style,
}: {
  className?: string;
  style?: React.CSSProperties;
}) {
  return <div className={clsx("skeleton rounded-[var(--radius-sm)]", className)} style={style} />;
}

/** Section heading used inside the sidebar and settings. */
export function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <div className="px-2 pb-1 pt-3 text-[10.5px] font-semibold uppercase tracking-wider text-text-faint">
      {children}
    </div>
  );
}
