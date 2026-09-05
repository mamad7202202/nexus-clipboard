/**
 * Typed wrappers over every Tauri command.
 *
 * The rest of the UI imports from here and never calls `invoke` directly, so
 * a renamed command breaks in exactly one file.
 */

import { invoke } from "@tauri-apps/api/core";

import type {
  Bootstrap,
  Collection,
  Diagnostics,
  ExportOptions,
  ImportReport,
  Item,
  MaintenanceReport,
  Page,
  PasteMode,
  Query,
  Settings,
  Stats,
  Tag,
  Transform,
  VaultStatus,
} from "./types";

// --- history ---------------------------------------------------------------

export const bootstrap = () => invoke<Bootstrap>("bootstrap");

export const listItems = (query: Query) => invoke<Page<Item>>("list_items", { query });

export const getItem = (id: number, reveal = false) =>
  invoke<Item>("get_item", { id, reveal });

export const getImage = (id: number) => invoke<string>("get_image", { id });

export const getStats = () => invoke<Stats>("get_stats");

export const setPinned = (id: number, pinned: boolean) =>
  invoke<void>("set_pinned", { id, pinned });

export const setFavorite = (id: number, favorite: boolean) =>
  invoke<void>("set_favorite", { id, favorite });

export const updateItem = (id: number, body: string) =>
  invoke<Item>("update_item", { id, body });

export const deleteItems = (ids: number[]) => invoke<number>("delete_items", { ids });

export const restoreItems = (ids: number[]) => invoke<number>("restore_items", { ids });

export const purgeItems = (ids: number[]) => invoke<number>("purge_items", { ids });

export const emptyTrash = () => invoke<number>("empty_trash");

export const clearHistory = (keepStarred: boolean) =>
  invoke<number>("clear_history", { keepStarred });

// --- clipboard -------------------------------------------------------------

export const useItem = (id: number, mode: PasteMode) =>
  invoke<void>("use_item", { id, mode });

export const copyText = (text: string) => invoke<void>("copy_text", { text });

export const captureNow = () => invoke<string>("capture_now");

export const setCapturePaused = (paused: boolean) =>
  invoke<void>("set_capture_paused", { paused });

export const isCapturePaused = () => invoke<boolean>("is_capture_paused");

// --- tags & collections ----------------------------------------------------

export const listTags = () => invoke<Tag[]>("list_tags");

export const createTag = (name: string, color: string) =>
  invoke<Tag>("create_tag", { name, color });

export const updateTag = (id: number, name: string, color: string) =>
  invoke<void>("update_tag", { id, name, color });

export const deleteTag = (id: number) => invoke<void>("delete_tag", { id });

export const tagItems = (ids: number[], tagId: number) =>
  invoke<void>("tag_items", { ids, tagId });

export const untagItems = (ids: number[], tagId: number) =>
  invoke<void>("untag_items", { ids, tagId });

export const listCollections = () => invoke<Collection[]>("list_collections");

export const createCollection = (name: string, icon: string, color: string) =>
  invoke<Collection>("create_collection", { name, icon, color });

export const updateCollection = (id: number, name: string, icon: string, color: string) =>
  invoke<void>("update_collection", { id, name, icon, color });

export const deleteCollection = (id: number) => invoke<void>("delete_collection", { id });

export const addToCollection = (ids: number[], collectionId: number) =>
  invoke<void>("add_to_collection", { ids, collectionId });

export const removeFromCollection = (ids: number[], collectionId: number) =>
  invoke<void>("remove_from_collection", { ids, collectionId });

export const reorderCollection = (collectionId: number, ordered: number[]) =>
  invoke<void>("reorder_collection", { collectionId, ordered });

// --- settings --------------------------------------------------------------

export const getSettings = () => invoke<Settings>("get_settings");

export const saveSettings = (settings: Settings) =>
  invoke<Settings>("save_settings", { settings });

export const listIgnoredApps = () => invoke<string[]>("list_ignored_apps");

export const addIgnoredApp = (name: string) => invoke<string[]>("add_ignored_app", { name });

export const removeIgnoredApp = (name: string) =>
  invoke<string[]>("remove_ignored_app", { name });

// --- vault -----------------------------------------------------------------

export const vaultStatus = () => invoke<VaultStatus>("vault_status");

export const vaultUnlock = (passphrase?: string) =>
  invoke<void>("vault_unlock", { passphrase: passphrase ?? null });

export const vaultLock = () => invoke<void>("vault_lock");

export const vaultSetPassphrase = (passphrase: string | null, current: string | null) =>
  invoke<void>("vault_set_passphrase", { passphrase, current });

// --- transforms & AI -------------------------------------------------------

export const transformText = (text: string, transform: Transform) =>
  invoke<string>("transform_text", { text, transform });

export const summarizeItem = (id: number) => invoke<string>("summarize_item", { id });

// --- backup ----------------------------------------------------------------

export const exportHistory = (path: string, options: ExportOptions) =>
  invoke<number>("export_history", { path, options });

export const importHistory = (path: string) => invoke<ImportReport>("import_history", { path });

export const createBackup = (path: string) => invoke<number>("create_backup", { path });

export const restoreBackup = (path: string) => invoke<string>("restore_backup", { path });

// --- maintenance -----------------------------------------------------------

export const runMaintenance = () => invoke<MaintenanceReport>("run_maintenance");

export const compactDatabase = () => invoke<number>("compact_database");

export const diagnostics = () => invoke<Diagnostics>("diagnostics");

// --- windows & system ------------------------------------------------------

export const hideLauncher = () => invoke<void>("hide_launcher");

export const showMain = () => invoke<void>("show_main");

export const rememberFocus = () => invoke<void>("remember_focus");

export const quitApp = () => invoke<void>("quit_app");

export const openDataDir = () => invoke<void>("open_data_dir");

export const revealPath = (path: string) => invoke<void>("reveal_path", { path });

export const openUrl = (url: string) => invoke<void>("open_url", { url });
