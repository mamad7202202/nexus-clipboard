/**
 * Settings.
 *
 * A single scrolling surface split into sections. Every control writes straight
 * through to the backend — there is no Save button, because a settings screen
 * that can be "lost" by closing it is a bug generator.
 */

import clsx from "clsx";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  Ban,
  Database,
  Download,
  Eye,
  KeyRound,
  Keyboard,
  Palette,
  Plus,
  RefreshCw,
  Shield,
  Sparkles,
  Trash2,
  Upload,
  X,
  Zap,
} from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useState } from "react";

import {
  Badge,
  Button,
  ConfirmDialog,
  Dialog,
  IconButton,
  Input,
  Segmented,
  Separator,
  Switch,
  Tooltip,
} from "@/components/ui";
import * as api from "@/lib/api";
import { bytes, prettyAccelerator } from "@/lib/format";
import type { Diagnostics, Settings as SettingsType } from "@/lib/types";
import { errorMessage } from "@/lib/types";
import { useApp } from "@/store/app";

const ACCENTS = [
  { name: "Indigo", value: "#6366f1" },
  { name: "Violet", value: "#8b5cf6" },
  { name: "Blue", value: "#3b82f6" },
  { name: "Cyan", value: "#06b6d4" },
  { name: "Emerald", value: "#10b981" },
  { name: "Amber", value: "#f59e0b" },
  { name: "Rose", value: "#f43f5e" },
  { name: "Pink", value: "#ec4899" },
];

export function SettingsPanel({ onClose }: { onClose: () => void }) {
  const settings = useApp((s) => s.settings);
  const applySettings = useApp((s) => s.applySettings);
  const vault = useApp((s) => s.vault);
  const setVault = useApp((s) => s.setVault);
  const toast = useApp((s) => s.toast);
  const refresh = useApp((s) => s.refresh);
  const version = useApp((s) => s.version);

  const [ignored, setIgnored] = useState<string[]>([]);
  const [newApp, setNewApp] = useState("");
  const [diag, setDiag] = useState<Diagnostics | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const [passphraseOpen, setPassphraseOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    api.listIgnoredApps().then(setIgnored).catch(() => {});
    api.diagnostics().then(setDiag).catch(() => {});
  }, []);

  if (!settings) return null;

  const patch = (changes: Partial<SettingsType>) =>
    applySettings({ ...settings, ...changes });

  const withBusy = async (key: string, fn: () => Promise<void>) => {
    setBusy(key);
    try {
      await fn();
    } catch (e) {
      toast(errorMessage(e), "error");
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="scroll-area h-full overflow-y-auto">
      <div className="mx-auto max-w-[720px] px-6 py-6">
        <header className="mb-6 flex items-start justify-between">
          <div>
            <h1 className="text-[19px] font-semibold tracking-tight text-text">Settings</h1>
            <p className="mt-0.5 text-[12.5px] text-text-muted">
              Nexus Clipboard {version} · everything stays on this machine
            </p>
          </div>
          <IconButton
            aria-label="Close settings"
            variant="ghost"
            onClick={onClose}
            icon={<X className="size-4" />}
          />
        </header>

        {/* --- Appearance ------------------------------------------------ */}
        <Section icon={<Palette />} title="Appearance">
          <Row label="Theme">
            <Segmented
              value={settings.theme}
              onChange={(theme) => patch({ theme })}
              options={[
                { value: "system", label: "System" },
                { value: "light", label: "Light" },
                { value: "dark", label: "Dark" },
              ]}
            />
          </Row>

          <Row label="Accent colour">
            <div className="flex items-center gap-1.5">
              {ACCENTS.map((accent) => (
                <Tooltip key={accent.value} label={accent.name}>
                  <button
                    onClick={() => patch({ accent: accent.value })}
                    aria-label={accent.name}
                    className={clsx(
                      "size-5 rounded-full transition-transform",
                      settings.accent === accent.value ? "scale-110" : "hover:scale-105",
                    )}
                    style={{
                      background: accent.value,
                      boxShadow:
                        settings.accent === accent.value
                          ? `0 0 0 2px var(--surface), 0 0 0 4px ${accent.value}`
                          : undefined,
                    }}
                  />
                </Tooltip>
              ))}
            </div>
          </Row>

          <Row label="List density">
            <Segmented
              value={settings.density}
              onChange={(density) => patch({ density })}
              options={[
                { value: "compact", label: "Compact" },
                { value: "comfortable", label: "Cosy" },
                { value: "spacious", label: "Roomy" },
              ]}
            />
          </Row>
        </Section>

        {/* --- Capture --------------------------------------------------- */}
        <Section icon={<Zap />} title="Capture">
          <Toggle
            label="Monitor the clipboard"
            hint="Turn this off to stop recording new entries."
            checked={settings.capture_enabled}
            onChange={async (capture_enabled) => {
              await api.setCapturePaused(!capture_enabled);
              patch({ capture_enabled });
            }}
          />
          <Toggle
            label="Capture images"
            hint="Screenshots and copied pictures."
            checked={settings.capture_images}
            onChange={(capture_images) => patch({ capture_images })}
          />
          <Toggle
            label="Capture files"
            hint="Paths copied from Explorer."
            checked={settings.capture_files}
            onChange={(capture_files) => patch({ capture_files })}
          />
          <Row
            label="Maximum entry size"
            hint="Anything larger is skipped so a huge copy never stalls capture."
          >
            <Segmented
              value={String(settings.max_capture_bytes)}
              onChange={(value) => patch({ max_capture_bytes: Number(value) })}
              options={[
                { value: String(4 * 1024 * 1024), label: "4 MB" },
                { value: String(32 * 1024 * 1024), label: "32 MB" },
                { value: String(128 * 1024 * 1024), label: "128 MB" },
              ]}
            />
          </Row>
        </Section>

        {/* --- Privacy --------------------------------------------------- */}
        <Section icon={<Shield />} title="Privacy & security">
          <Toggle
            label="Encrypt detected credentials"
            hint="API keys, tokens, private keys and card numbers are stored encrypted and kept out of the search index."
            checked={settings.encrypt_secrets}
            onChange={(encrypt_secrets) => patch({ encrypt_secrets })}
          />
          <Toggle
            label="Blur sensitive previews"
            hint="Encrypted entries stay obscured until you hover or click."
            checked={settings.blur_secrets}
            onChange={(blur_secrets) => patch({ blur_secrets })}
          />

          <Row
            label="Vault"
            hint={
              vault?.requires_passphrase
                ? vault.unlocked
                  ? "Unlocked for this session."
                  : "Locked — encrypted entries cannot be read."
                : "Protected by a machine-local key. Add a passphrase for stronger protection."
            }
          >
            <div className="flex items-center gap-1.5">
              {vault?.requires_passphrase &&
                (vault.unlocked ? (
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={async () => {
                      await api.vaultLock();
                      setVault({ ...vault, unlocked: false });
                    }}
                  >
                    Lock
                  </Button>
                ) : (
                  <Button size="sm" variant="secondary" onClick={() => setPassphraseOpen(true)}>
                    Unlock
                  </Button>
                ))}
              <Button
                size="sm"
                variant="secondary"
                icon={<KeyRound className="size-3.5" />}
                onClick={() => setPassphraseOpen(true)}
              >
                {vault?.requires_passphrase ? "Change passphrase" : "Set passphrase"}
              </Button>
            </div>
          </Row>

          <Row
            label="Ignored applications"
            hint="Nexus never records anything copied from these. Password managers are added by default."
          >
            <div />
          </Row>

          <div className="-mt-1 space-y-2">
            <div className="flex flex-wrap gap-1.5">
              {ignored.map((app) => (
                <span
                  key={app}
                  className={clsx(
                    "group inline-flex items-center gap-1 rounded-full border border-border",
                    "bg-surface-2 py-0.5 pl-2 pr-1 text-[11.5px] text-text-muted",
                  )}
                >
                  {app}
                  <button
                    aria-label={`Stop ignoring ${app}`}
                    onClick={() =>
                      api.removeIgnoredApp(app).then(setIgnored).catch(() => {})
                    }
                    className="rounded-full p-0.5 text-text-faint transition-colors hover:bg-surface-3 hover:text-text"
                  >
                    <X className="size-2.5" />
                  </button>
                </span>
              ))}
              {ignored.length === 0 && (
                <span className="text-[12px] text-text-faint">None yet.</span>
              )}
            </div>

            <div className="flex gap-1.5">
              <Input
                icon={<Ban className="size-3.5" />}
                placeholder="Application.exe"
                value={newApp}
                onChange={(e) => setNewApp(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && newApp.trim()) {
                    api
                      .addIgnoredApp(newApp.trim())
                      .then((list) => {
                        setIgnored(list);
                        setNewApp("");
                      })
                      .catch((err) => toast(errorMessage(err), "error"));
                  }
                }}
                className="flex-1"
              />
              <Button
                icon={<Plus className="size-3.5" />}
                disabled={!newApp.trim()}
                onClick={() =>
                  api
                    .addIgnoredApp(newApp.trim())
                    .then((list) => {
                      setIgnored(list);
                      setNewApp("");
                    })
                    .catch((err) => toast(errorMessage(err), "error"))
                }
              >
                Add
              </Button>
            </div>
          </div>
        </Section>

        {/* --- Shortcuts ------------------------------------------------- */}
        <Section icon={<Keyboard />} title="Shortcuts">
          <HotkeyRow
            label="Open the quick picker"
            value={settings.hotkey_launcher}
            onChange={(hotkey_launcher) => patch({ hotkey_launcher })}
          />
          <HotkeyRow
            label="Open the command palette"
            value={settings.hotkey_palette}
            onChange={(hotkey_palette) => patch({ hotkey_palette })}
          />
          <HotkeyRow
            label="Paste the previous entry"
            value={settings.hotkey_quick_paste}
            onChange={(hotkey_quick_paste) => patch({ hotkey_quick_paste })}
          />
          <Toggle
            label="Paste immediately when picking an entry"
            hint="Otherwise the entry is only copied and you paste yourself."
            checked={settings.paste_on_select}
            onChange={(paste_on_select) => patch({ paste_on_select })}
          />
          <Toggle
            label="Show the picker at the cursor"
            hint="Off centres it on the active monitor."
            checked={settings.launcher_follows_cursor}
            onChange={(launcher_follows_cursor) => patch({ launcher_follows_cursor })}
          />
        </Section>

        {/* --- History --------------------------------------------------- */}
        <Section icon={<Database />} title="History & storage">
          <Row label="Keep entries for">
            <Segmented
              value={settings.retention === "forever" ? "forever" : String(settings.retention.days)}
              onChange={(value) =>
                patch({ retention: value === "forever" ? "forever" : { days: Number(value) } })
              }
              options={[
                { value: "7", label: "7 days" },
                { value: "30", label: "30 days" },
                { value: "365", label: "1 year" },
                { value: "forever", label: "Forever" },
              ]}
            />
          </Row>
          <Row label="Keep trash for" hint="Deleted entries are recoverable until then.">
            <Segmented
              value={String(settings.trash_days)}
              onChange={(value) => patch({ trash_days: Number(value) })}
              options={[
                { value: "7", label: "7 days" },
                { value: "30", label: "30 days" },
                { value: "90", label: "90 days" },
              ]}
            />
          </Row>

          {diag && (
            <div className="grid grid-cols-3 gap-2 pt-1">
              <Stat label="Database" value={bytes(diag.db_bytes)} />
              <Stat label="Attachments" value={bytes(diag.blob_bytes)} />
              <Stat label="Schema" value={`v${diag.schema_version}`} />
            </div>
          )}

          <div className="flex flex-wrap gap-1.5 pt-1">
            <Button
              size="sm"
              icon={<RefreshCw className="size-3.5" />}
              loading={busy === "compact"}
              onClick={() =>
                withBusy("compact", async () => {
                  const size = await api.compactDatabase();
                  setDiag(await api.diagnostics());
                  toast(`Compacted to ${bytes(size)}`, "success");
                })
              }
            >
              Compact
            </Button>

            <Button
              size="sm"
              icon={<Upload className="size-3.5" />}
              loading={busy === "export"}
              onClick={() =>
                withBusy("export", async () => {
                  const path = await saveDialog({
                    defaultPath: "nexus-history.json",
                    filters: [{ name: "JSON", extensions: ["json"] }],
                  });
                  if (!path) return;
                  const n = await api.exportHistory(path, {
                    include_secrets: false,
                    include_blobs: false,
                    only_starred: false,
                  });
                  toast(`Exported ${n} entries`, "success");
                })
              }
            >
              Export
            </Button>

            <Button
              size="sm"
              icon={<Download className="size-3.5" />}
              loading={busy === "import"}
              onClick={() =>
                withBusy("import", async () => {
                  const path = await openDialog({
                    multiple: false,
                    filters: [{ name: "JSON", extensions: ["json"] }],
                  });
                  if (typeof path !== "string") return;
                  const report = await api.importHistory(path);
                  await refresh();
                  toast(
                    `Imported ${report.imported}, skipped ${report.duplicates} duplicates`,
                    "success",
                  );
                })
              }
            >
              Import
            </Button>

            <Button
              size="sm"
              icon={<Database className="size-3.5" />}
              loading={busy === "backup"}
              onClick={() =>
                withBusy("backup", async () => {
                  const path = await saveDialog({
                    defaultPath: "nexus-backup.nxbak",
                    filters: [{ name: "Nexus backup", extensions: ["nxbak"] }],
                  });
                  if (!path) return;
                  const size = await api.createBackup(path);
                  toast(`Backup written (${bytes(size)})`, "success");
                })
              }
            >
              Backup
            </Button>

            <div className="flex-1" />

            <Button
              size="sm"
              variant="danger"
              icon={<Trash2 className="size-3.5" />}
              onClick={() => setConfirmClear(true)}
            >
              Clear history
            </Button>
          </div>
        </Section>

        {/* --- Intelligence ---------------------------------------------- */}
        <Section icon={<Sparkles />} title="Intelligence">
          <Row
            label="Provider"
            hint="Off by default. Nothing is sent anywhere until you enable a provider and add a key."
          >
            <Segmented
              value={settings.ai.provider}
              onChange={(provider) => {
                const defaultModel =
                  provider === "anthropic"
                    ? "claude-sonnet-5"
                    : provider === "openai"
                      ? "gpt-4o-mini"
                      : settings.ai.model || "gpt-4o-mini";
                const defaultBaseUrl =
                  provider === "openai"
                    ? "https://api.openai.com/v1"
                    : provider === "anthropic"
                      ? "https://api.anthropic.com/v1"
                      : settings.ai.base_url ?? "";
                patch({
                  ai: {
                    ...settings.ai,
                    provider,
                    model: defaultModel,
                    base_url: defaultBaseUrl,
                  },
                });
              }}
              options={[
                { value: "disabled", label: "Off" },
                { value: "anthropic", label: "Claude" },
                { value: "openai", label: "OpenAI" },
                { value: "custom", label: "Custom / Local" },
              ]}
            />
          </Row>

          {settings.ai.provider !== "disabled" && (
            <>
              {(settings.ai.provider === "custom" || settings.ai.provider === "openai") && (
                <Row
                  label="Base URL"
                  hint={
                    settings.ai.provider === "custom"
                      ? "e.g. http://localhost:11434/v1 (Ollama), https://api.deepseek.com/v1, or OpenRouter"
                      : "OpenAI-compatible endpoint"
                  }
                >
                  <Input
                    placeholder="https://api.openai.com/v1"
                    value={settings.ai.base_url ?? ""}
                    onChange={(e) => patch({ ai: { ...settings.ai, base_url: e.target.value } })}
                    className="w-[280px]"
                  />
                </Row>
              )}

              <Row
                label="Model name / ID"
                hint={
                  settings.ai.provider === "anthropic"
                    ? "e.g. claude-sonnet-5, claude-3-5-haiku-20241022"
                    : settings.ai.provider === "openai"
                      ? "e.g. gpt-4o, gpt-4o-mini"
                      : "e.g. deepseek-chat, llama3:8b, mistral"
                }
              >
                <Input
                  value={settings.ai.model}
                  onChange={(e) => patch({ ai: { ...settings.ai, model: e.target.value } })}
                  className="w-[280px]"
                />
              </Row>

              <Row
                label="API key"
                hint={
                  settings.ai.provider === "custom"
                    ? "Leave blank if using local server (like Ollama / LM Studio)"
                    : undefined
                }
              >
                <Input
                  type="password"
                  placeholder={
                    settings.ai.provider === "anthropic"
                      ? "sk-ant-…"
                      : settings.ai.provider === "openai"
                        ? "sk-proj-…"
                        : "API key (optional for local)"
                  }
                  value={settings.ai.api_key ?? ""}
                  onChange={(e) => patch({ ai: { ...settings.ai, api_key: e.target.value } })}
                  className="w-[280px]"
                />
              </Row>

              <p
                className={clsx(
                  "rounded-[var(--radius-md)] border border-border bg-surface-2/50 px-3 py-2",
                  "text-[11.5px] leading-relaxed text-text-muted",
                )}
              >
                <Eye className="mr-1 inline size-3 align-[-2px]" />
                Only the entry you explicitly act on is sent, and only when you trigger a
                summary, explanation or translation. Your history is never uploaded.
              </p>
            </>
          )}
        </Section>

        {/* --- System ---------------------------------------------------- */}
        <Section icon={<Zap />} title="System">
          <Toggle
            label="Start with Windows"
            checked={settings.start_with_system}
            onChange={(start_with_system) => patch({ start_with_system })}
          />
          <Toggle
            label="Start hidden in the tray"
            hint="Applies when Windows launches Nexus at login."
            checked={settings.start_minimized}
            onChange={(start_minimized) => patch({ start_minimized })}
          />
          <Toggle
            label="Show the tray icon"
            hint="With this off, closing the window quits the app and stops capture."
            checked={settings.show_tray_icon}
            onChange={(show_tray_icon) => patch({ show_tray_icon })}
          />
          <Row label="Data folder">
            <Button size="sm" onClick={() => api.openDataDir()}>
              Open
            </Button>
          </Row>
        </Section>
      </div>

      <ConfirmDialog
        open={confirmClear}
        onClose={() => setConfirmClear(false)}
        onConfirm={async () => {
          const n = await api.clearHistory(true);
          await refresh();
          toast(`Moved ${n} entries to the trash`, "success");
        }}
        destructive
        confirmLabel="Clear history"
        title="Clear the history?"
        description="Everything except pinned and favourited entries moves to the trash, where it stays recoverable until the retention period expires."
      />

      <PassphraseDialog
        open={passphraseOpen}
        onClose={() => setPassphraseOpen(false)}
        hasPassphrase={vault?.requires_passphrase ?? false}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------

function Section({
  icon,
  title,
  children,
}: {
  icon: ReactNode;
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="mb-7">
      <div className="mb-2.5 flex items-center gap-2">
        <span className="text-text-faint [&>svg]:size-3.5">{icon}</span>
        <h2 className="text-[13px] font-semibold tracking-tight text-text">{title}</h2>
        <Separator className="ml-1 flex-1" />
      </div>
      <div className="space-y-3">{children}</div>
    </section>
  );
}

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-6">
      <div className="min-w-0 flex-1 pt-0.5">
        <p className="text-[12.5px] font-medium text-text">{label}</p>
        {hint && <p className="mt-0.5 text-[11.5px] leading-relaxed text-text-muted">{hint}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Toggle({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (value: boolean) => void | Promise<void>;
}) {
  return (
    <Row label={label} hint={hint}>
      <Switch checked={checked} onChange={(v) => void onChange(v)} label={label} />
    </Row>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[var(--radius-md)] border border-border bg-surface-2/50 px-3 py-2">
      <p className="text-[10.5px] uppercase tracking-wide text-text-faint">{label}</p>
      <p className="mt-0.5 text-[13px] font-medium tabular-nums text-text">{value}</p>
    </div>
  );
}

/**
 * Records a key combination by listening for the next chord the user presses.
 * Accelerators are stored in Tauri's own format so the backend can register
 * them verbatim.
 */
function HotkeyRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [recording, setRecording] = useState(false);

  useEffect(() => {
    if (!recording) return;

    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape") {
        setRecording(false);
        return;
      }
      // Ignore bare modifiers; wait for a real key.
      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;

      const parts: string[] = [];
      if (e.ctrlKey || e.metaKey) parts.push("CommandOrControl");
      if (e.shiftKey) parts.push("Shift");
      if (e.altKey) parts.push("Alt");

      // A shortcut with no modifier would swallow ordinary typing globally.
      if (parts.length === 0) return;

      parts.push(e.key.length === 1 ? e.key.toUpperCase() : e.key);
      onChange(parts.join("+"));
      setRecording(false);
    };

    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording, onChange]);

  return (
    <Row label={label}>
      <button
        onClick={() => setRecording((v) => !v)}
        className={clsx(
          "flex h-7.5 min-w-[130px] items-center justify-center gap-1 rounded-[var(--radius-md)]",
          "border px-2 text-[12px] transition-colors",
          recording
            ? "border-[var(--accent)] bg-[color-mix(in_oklch,var(--accent)_12%,transparent)] text-text animate-pulse-ring"
            : "border-border bg-surface-2 text-text-muted hover:border-border-strong",
        )}
      >
        {recording ? (
          "Press a combination…"
        ) : (
          <span className="flex items-center gap-1">
            {prettyAccelerator(value).map((key, i) => (
              <Badge key={`${key}-${i}`}>{key}</Badge>
            ))}
          </span>
        )}
      </button>
    </Row>
  );
}

function PassphraseDialog({
  open,
  onClose,
  hasPassphrase,
}: {
  open: boolean;
  onClose: () => void;
  hasPassphrase: boolean;
}) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);

  const toast = useApp((s) => s.toast);
  const setVault = useApp((s) => s.setVault);

  const mismatch = next !== confirm && confirm.length > 0;

  const submit = async () => {
    if (mismatch || busy) return;
    setBusy(true);
    try {
      await api.vaultSetPassphrase(next || null, hasPassphrase ? current : null);
      setVault(await api.vaultStatus());
      toast(next ? "Passphrase updated" : "Passphrase removed", "success");
      setCurrent("");
      setNext("");
      setConfirm("");
      onClose();
    } catch (e) {
      toast(errorMessage(e), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={hasPassphrase ? "Change vault passphrase" : "Set a vault passphrase"}
      description="Encrypted entries are re-encrypted under the new key. Leave the new passphrase empty to fall back to the machine-local key."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={submit} loading={busy} disabled={mismatch}>
            Save
          </Button>
        </>
      }
    >
      <div className="space-y-2 pb-2">
        {hasPassphrase && (
          <Input
            type="password"
            placeholder="Current passphrase"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
          />
        )}
        <Input
          type="password"
          placeholder="New passphrase"
          value={next}
          onChange={(e) => setNext(e.target.value)}
        />
        <Input
          type="password"
          placeholder="Confirm new passphrase"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit()}
        />
        {mismatch && (
          <p className="text-[11.5px] text-[var(--danger)]">The passphrases do not match.</p>
        )}
      </div>
    </Dialog>
  );
}
