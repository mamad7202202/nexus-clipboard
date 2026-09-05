/**
 * Backend → UI events. Names mirror `src-tauri/src/ipc/events.rs`.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const EVENTS = {
  historyChanged: "history:changed",
  itemCaptured: "history:captured",
  captureState: "capture:state",
  vaultState: "vault:state",
  settingsChanged: "settings:changed",
  hotkey: "hotkey:fired",
  maintenanceDone: "maintenance:done",
  navigate: "navigate",
} as const;

/**
 * Subscribe to an event.
 *
 * `listen` resolves asynchronously, so an effect that unmounts before it
 * settles would otherwise leak a listener. The returned function handles both
 * orders correctly.
 */
export function on<T>(event: string, handler: (payload: T) => void): () => void {
  let unlisten: UnlistenFn | undefined;
  let cancelled = false;

  listen<T>(event, (e) => handler(e.payload)).then((fn) => {
    if (cancelled) {
      fn();
      return;
    }
    unlisten = fn;
  });

  return () => {
    cancelled = true;
    unlisten?.();
  };
}
