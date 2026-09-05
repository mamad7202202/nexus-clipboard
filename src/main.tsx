/**
 * Entry point.
 *
 * One bundle serves both windows; `?view=launcher` selects the quick picker.
 * Sharing the bundle means the launcher is already warm in the webview cache
 * when the hotkey fires.
 */

import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";

import { ToastStack } from "@/components/ui";
import { EVENTS, on } from "@/lib/events";
import { LauncherView } from "@/views/Launcher";
import { MainView } from "@/views/Main";
import { applyTheme, useApp, watchSystemTheme } from "@/store/app";
import type { Settings } from "@/lib/types";

import "@/styles/index.css";

const isLauncher = new URLSearchParams(window.location.search).get("view") === "launcher";

function App() {
  const ready = useApp((s) => s.ready);
  const toasts = useApp((s) => s.toasts);
  const dismissToast = useApp((s) => s.dismissToast);

  useEffect(() => {
    void useApp.getState().init();
    return watchSystemTheme();
  }, []);

  // Backend → UI events.
  useEffect(() => {
    const store = useApp.getState();

    // The backend emits `history:changed` once per capture. Copying a dozen
    // things in a few seconds would otherwise mean a dozen full list + stats
    // round trips; coalescing them costs at most 120 ms of staleness.
    let refreshTimer = 0;
    const scheduleRefresh = () => {
      window.clearTimeout(refreshTimer);
      refreshTimer = window.setTimeout(() => {
        void useApp.getState().refresh();
      }, 120);
    };

    const unsubscribers = [
      on(EVENTS.historyChanged, scheduleRefresh),
      on<Settings>(EVENTS.settingsChanged, (settings) => {
        useApp.setState({ settings });
        applyTheme(settings);
      }),
      on<boolean>(EVENTS.vaultState, (unlocked) => {
        const vault = useApp.getState().vault;
        if (vault) useApp.setState({ vault: { ...vault, unlocked } });
      }),
      on<boolean>(EVENTS.captureState, (enabled) => {
        const settings = useApp.getState().settings;
        if (settings) useApp.setState({ settings: { ...settings, capture_enabled: enabled } });
      }),
      on<string>(EVENTS.hotkey, (which) => {
        if (which === "palette") store.setPaletteOpen(true);
      }),
    ];

    return () => {
      window.clearTimeout(refreshTimer);
      unsubscribers.forEach((fn) => fn());
    };
  }, []);

  if (!ready) return <Splash />;

  return (
    <>
      <MainView />
      <ToastStack toasts={toasts} onDismiss={dismissToast} />
    </>
  );
}

/**
 * Shown for the few hundred milliseconds before the first page of history
 * arrives. Deliberately minimal — a spinner-heavy splash makes a fast app feel
 * slow.
 */
function Splash() {
  return (
    <div className="flex h-full items-center justify-center bg-bg">
      <div className="flex items-center gap-2.5 opacity-60">
        <div className="size-4 rounded-[5px] bg-[var(--accent)]" />
        <span className="text-[13px] font-medium tracking-tight text-text-muted">
          Nexus Clipboard
        </span>
      </div>
    </div>
  );
}

/** Suppress the webview's native context menu and refresh shortcuts. */
function hardenWebview() {
  if (import.meta.env.DEV) return;

  document.addEventListener("contextmenu", (e) => e.preventDefault());
  document.addEventListener("keydown", (e) => {
    // F5 / Ctrl+R would reload the app and drop in-flight state.
    if (e.key === "F5" || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "r")) {
      e.preventDefault();
    }
  });
}

hardenWebview();

const container = document.getElementById("root");
if (!container) throw new Error("#root is missing from index.html");

createRoot(container).render(
  <StrictMode>{isLauncher ? <LauncherView /> : <App />}</StrictMode>,
);
