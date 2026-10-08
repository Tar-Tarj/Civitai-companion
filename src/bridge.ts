import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppSnapshot,
  BuzzAccountType,
  BuzzTransactionPage,
  ConnectionResult,
  SettingsPatch,
  SoundId,
  StateEvent,
} from "./types";

export const backend = {
  snapshot: (): Promise<AppSnapshot> => invoke("get_snapshot"),
  configureKey: (apiKey: string): Promise<AppSnapshot> =>
    invoke("configure_api_key", { apiKey }),
  removeKey: (): Promise<AppSnapshot> => invoke("remove_api_key"),
  testConnection: (): Promise<ConnectionResult> => invoke("test_connection"),
  sync: (): Promise<AppSnapshot> => invoke("sync_now"),
  updateSettings: (patch: SettingsPatch): Promise<AppSnapshot> =>
    invoke("update_settings", { patch }),
  markRead: (notificationId: string): Promise<AppSnapshot> =>
    invoke("mark_notification_read", { notificationId }),
  markAllRead: (): Promise<AppSnapshot> => invoke("mark_all_notifications_read"),
  buzzTransactions: (
    accountType: BuzzAccountType,
    cursor: string | null = null,
  ): Promise<BuzzTransactionPage> => invoke("get_buzz_transactions", { accountType, cursor }),
  previewSound: (soundId: SoundId): Promise<SoundId | null> => invoke("preview_sound", { soundId }),
  fetchImage: (url: string): Promise<{ mimeType: string; data: string }> =>
    invoke("fetch_civitai_image", { url }),
  openCivitaiUrl: (url: string): Promise<void> => invoke("open_civitai_url", { url }),
  exportPreferences: (): Promise<string> => invoke("export_preferences"),
  importPreferences: (json: string): Promise<AppSnapshot> =>
    invoke("import_preferences", { json }),
  resetCache: (): Promise<AppSnapshot> => invoke("reset_cached_data"),
  clearAccount: (): Promise<AppSnapshot> => invoke("clear_account_data"),
  onStateChanged: (handler: (event: StateEvent) => void): Promise<UnlistenFn> =>
    listen<StateEvent>("app-state-changed", ({ payload }) => handler(payload)),
};
