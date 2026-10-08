export type SoundId = "1" | "2" | "3" | "4";

export interface NotificationSounds {
  siteActivity: SoundId | null;
  tips: SoundId | null;
  followers: SoundId | null;
}

export interface Settings {
  threadResponsesEnabled: boolean;
  pollingMinutes: number;
  windowsNotificationsEnabled: boolean;
  notificationSounds: NotificationSounds;
  startWithWindows: boolean;
  closeToTray: boolean;
}

export interface SettingsPatch {
  threadResponsesEnabled?: boolean;
  pollingMinutes?: number;
  windowsNotificationsEnabled?: boolean;
  notificationSounds?: NotificationSounds;
  startWithWindows?: boolean;
  closeToTray?: boolean;
}

export interface PermissionStatus {
  id: string;
  label: string;
  description: string;
  granted: boolean;
}

export interface NotificationItem {
  id: string;
  type: string;
  category: string;
  read: boolean;
  createdAt: string | null;
  text: string;
  username: string | null;
  commentPreview: string;
  url: string | null;
  thumbnailUrl: string | null;
  thumbnailKind: string | null;
}

export type BuzzAccountType = "blue" | "yellow" | "green";

export interface BuzzTransactionItem {
  date: string;
  kind: string;
  amount: number;
  accountType: BuzzAccountType;
  description: string;
  imageUrl: string | null;
  thumbnailUrl: string | null;
}

export interface BuzzTransactionPage {
  transactions: BuzzTransactionItem[];
  nextCursor: string | null;
}

export interface AppSnapshot {
  appVersion: string;
  credentialConfigured: boolean;
  account: {
    userId: number | null;
    username: string;
    tokenScope: number | null;
    profileImageUrl: string;
  };
  buzz: { blue: number | null; yellow: number | null; green: number | null };
  buzzChange: { blue: number; yellow: number; green: number; at: string | null };
  today: { date: string; followers: number; followersComplete: boolean };
  notifications: { unreadCount: number; items: NotificationItem[] };
  sync: {
    lastSuccess: string | null;
    lastAttempt: string | null;
    error: string | null;
    errorCode: string | null;
    errorStatus: number;
    initialized: boolean;
    updating: boolean;
  };
  settings: Settings;
  permissions: PermissionStatus[];
}

export interface StateEvent {
  snapshot: AppSnapshot;
  soundId: SoundId | null;
}

export interface ConnectionResult {
  username: string;
  tokenScope: number | null;
  permissions: PermissionStatus[];
}

export type View = "notifications" | "buzz" | "settings";
export type SettingsPage =
  | "root"
  | "account"
  | "notifications"
  | "behavior"
  | "privacy"
  | "about";

export type Filter = "all" | "comments" | "followers" | "milestones" | "tips" | "models" | "updates";
