use std::collections::BTreeSet;

use chrono::{Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

pub const STORAGE_VERSION: u32 = 1;
pub const BACKUP_TYPE: &str = "civitai-companion-desktop-settings";
pub const BACKUP_VERSION: u32 = 1;
pub const MAX_BACKUP_BYTES: usize = 1024 * 1024;
pub const MIN_POLLING_MINUTES: f64 = 1.0;
pub const MAX_POLLING_MINUTES: f64 = 60.0;
pub const MAX_VISIBLE_NOTIFICATIONS: usize = 300;
pub const MAX_KNOWN_NOTIFICATION_IDS: usize = 300;

fn default_true() -> bool {
    true
}

fn default_polling_minutes() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationSounds {
    pub site_activity: Option<String>,
    pub tips: Option<String>,
    pub followers: Option<String>,
}

impl Default for NotificationSounds {
    fn default() -> Self {
        Self {
            site_activity: Some("1".into()),
            tips: Some("1".into()),
            followers: Some("1".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    #[serde(default = "default_true")]
    pub thread_responses_enabled: bool,
    #[serde(default = "default_polling_minutes")]
    pub polling_minutes: f64,
    #[serde(default = "default_true")]
    pub windows_notifications_enabled: bool,
    #[serde(default)]
    pub notification_sounds: NotificationSounds,
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            thread_responses_enabled: true,
            polling_minutes: default_polling_minutes(),
            windows_notifications_enabled: true,
            notification_sounds: NotificationSounds::default(),
            start_with_windows: false,
            close_to_tray: true,
        }
    }
}

impl Settings {
    pub fn validate(mut self) -> Option<Self> {
        if !self.polling_minutes.is_finite()
            || !(MIN_POLLING_MINUTES..=MAX_POLLING_MINUTES).contains(&self.polling_minutes)
        {
            return None;
        }
        for value in [
            &mut self.notification_sounds.site_activity,
            &mut self.notification_sounds.tips,
            &mut self.notification_sounds.followers,
        ] {
            if value
                .as_deref()
                .is_some_and(|id| !matches!(id, "1" | "2" | "3" | "4"))
            {
                return None;
            }
        }
        Some(self)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub thread_responses_enabled: Option<bool>,
    pub polling_minutes: Option<f64>,
    pub windows_notifications_enabled: Option<bool>,
    pub notification_sounds: Option<NotificationSounds>,
    pub start_with_windows: Option<bool>,
    pub close_to_tray: Option<bool>,
}

impl SettingsPatch {
    pub fn apply(self, current: &Settings) -> Option<Settings> {
        Settings {
            thread_responses_enabled: self
                .thread_responses_enabled
                .unwrap_or(current.thread_responses_enabled),
            polling_minutes: self.polling_minutes.unwrap_or(current.polling_minutes),
            windows_notifications_enabled: self
                .windows_notifications_enabled
                .unwrap_or(current.windows_notifications_enabled),
            notification_sounds: self
                .notification_sounds
                .unwrap_or_else(|| current.notification_sounds.clone()),
            start_with_windows: self
                .start_with_windows
                .unwrap_or(current.start_with_windows),
            close_to_tray: self.close_to_tray.unwrap_or(current.close_to_tray),
        }
        .validate()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Account {
    pub user_id: Option<i64>,
    pub username: String,
    pub token_scope: Option<i64>,
    pub profile_image_url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Buzz {
    pub blue: Option<f64>,
    pub yellow: Option<f64>,
    #[serde(default)]
    pub green: Option<f64>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BuzzAccountType {
    Blue,
    Yellow,
    Green,
}

impl BuzzAccountType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Yellow => "yellow",
            Self::Green => "green",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuzzTransactionItem {
    pub date: String,
    pub kind: String,
    pub amount: f64,
    pub account_type: String,
    pub description: String,
    pub image_url: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuzzTransactionPage {
    pub transactions: Vec<BuzzTransactionItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuzzChange {
    pub blue: f64,
    pub yellow: f64,
    #[serde(default)]
    pub green: f64,
    pub at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TodayState {
    pub date: String,
    pub followers: usize,
    pub followers_complete: bool,
}

impl Default for TodayState {
    fn default() -> Self {
        Self {
            date: local_date_key(),
            followers: 0,
            followers_complete: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncStatus {
    pub last_success: Option<String>,
    pub last_attempt: Option<String>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub error_status: u16,
    pub initialized: bool,
    pub updating: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationItem {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub category: String,
    pub read: bool,
    pub created_at: Option<String>,
    pub text: String,
    pub username: Option<String>,
    pub comment_preview: String,
    pub url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_kind: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationState {
    pub unread_count: usize,
    pub items: Vec<NotificationItem>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationTracker {
    pub initialized: bool,
    pub known_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DailyTracker {
    pub date: String,
    pub follower_keys: BTreeSet<String>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedData {
    pub version: u32,
    pub settings: Settings,
    pub account: Account,
    pub buzz: Buzz,
    pub buzz_change: BuzzChange,
    pub today: TodayState,
    pub notifications: NotificationState,
    pub sync: SyncStatus,
    pub notification_tracker: NotificationTracker,
    pub daily_tracker: DailyTracker,
    pub rate_limit_retry_at: Option<i64>,
}

impl Default for PersistedData {
    fn default() -> Self {
        Self {
            version: STORAGE_VERSION,
            settings: Settings::default(),
            account: Account::default(),
            buzz: Buzz::default(),
            buzz_change: BuzzChange::default(),
            today: TodayState::default(),
            notifications: NotificationState::default(),
            sync: SyncStatus::default(),
            notification_tracker: NotificationTracker::default(),
            daily_tracker: DailyTracker::default(),
            rate_limit_retry_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub granted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub app_version: String,
    pub credential_configured: bool,
    pub account: Account,
    pub buzz: Buzz,
    pub buzz_change: BuzzChange,
    pub today: TodayState,
    pub notifications: NotificationState,
    pub sync: SyncStatus,
    pub settings: Settings,
    pub permissions: Vec<PermissionStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateEvent {
    pub snapshot: AppSnapshot,
    pub sound_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResult {
    pub username: String,
    pub token_scope: Option<i64>,
    pub permissions: Vec<PermissionStatus>,
}

impl AppSnapshot {
    pub fn new(data: &PersistedData, credential_configured: bool) -> Self {
        Self {
            app_version: env!("CARGO_PKG_VERSION").into(),
            credential_configured,
            account: data.account.clone(),
            buzz: data.buzz.clone(),
            buzz_change: data.buzz_change.clone(),
            today: data.today.clone(),
            notifications: data.notifications.clone(),
            sync: data.sync.clone(),
            settings: data.settings.clone(),
            permissions: permission_statuses(data.account.token_scope),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreferencesBackup {
    #[serde(rename = "type")]
    pub backup_type: String,
    pub version: u32,
    pub exported_at: String,
    pub settings: Settings,
}

impl PreferencesBackup {
    pub fn new(settings: &Settings) -> Self {
        Self {
            backup_type: BACKUP_TYPE.into(),
            version: BACKUP_VERSION,
            exported_at: now_iso(),
            settings: settings.clone(),
        }
    }

    pub fn validate(self) -> Option<Settings> {
        (self.backup_type == BACKUP_TYPE && self.version == BACKUP_VERSION)
            .then_some(self.settings)
            .and_then(Settings::validate)
    }
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn local_date_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn permission_statuses(scope: Option<i64>) -> Vec<PermissionStatus> {
    const REQUIREMENTS: [(&str, &str, &str, i64); 5] = [
        ("UserRead", "UserRead", "Account identity and profile", 1),
        (
            "BuzzRead",
            "BuzzRead",
            "Buzz balances and supported history",
            65_536,
        ),
        (
            "NotificationsRead",
            "NotificationsRead",
            "Notification history and unread count",
            2_097_152,
        ),
        (
            "NotificationsWrite",
            "NotificationsWrite",
            "Mark notifications as read",
            4_194_304,
        ),
        ("MediaRead", "MediaRead", "Notification thumbnails", 32),
    ];
    REQUIREMENTS
        .into_iter()
        .map(|(id, label, description, bit)| PermissionStatus {
            id,
            label,
            description,
            granted: scope.is_some_and(|mask| mask & bit == bit),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_enforce_bounds_and_sound_ids() {
        assert!(Settings::default().validate().is_some());
        assert!(
            Settings {
                polling_minutes: 0.5,
                ..Settings::default()
            }
            .validate()
            .is_none()
        );
        assert!(
            Settings {
                polling_minutes: 61.0,
                ..Settings::default()
            }
            .validate()
            .is_none()
        );
        assert!(
            Settings {
                notification_sounds: NotificationSounds {
                    site_activity: Some("9".into()),
                    ..NotificationSounds::default()
                },
                ..Settings::default()
            }
            .validate()
            .is_none()
        );
    }

    #[test]
    fn older_settings_default_to_close_to_tray() {
        let settings: Settings = serde_json::from_str(
            r#"{"threadResponsesEnabled":true,"pollingMinutes":1,"windowsNotificationsEnabled":true,"notificationSounds":{"siteActivity":"1","tips":"2","followers":"3"},"startWithWindows":false}"#,
        )
        .unwrap();
        assert!(settings.close_to_tray);
    }

    #[test]
    fn snapshot_never_contains_a_credential_field() {
        let json =
            serde_json::to_string(&AppSnapshot::new(&PersistedData::default(), true)).unwrap();
        assert!(json.contains("credentialConfigured"));
        assert!(!json.to_ascii_lowercase().contains("api_key"));
        assert!(!json.to_ascii_lowercase().contains("apikey"));
        assert!(!json.to_ascii_lowercase().contains("token\""));
    }

    #[test]
    fn backup_is_preferences_only_and_strict() {
        let json = serde_json::to_string(&PreferencesBackup::new(&Settings::default())).unwrap();
        assert!(!json.to_ascii_lowercase().contains("token"));
        let malicious = json.replace("\"settings\":{", "\"secret\":\"value\",\"settings\":{");
        assert!(serde_json::from_str::<PreferencesBackup>(&malicious).is_err());
    }

    #[test]
    fn older_buzz_state_defaults_green_balance_safely() {
        let buzz: Buzz = serde_json::from_str(r#"{"blue":12,"yellow":7}"#).unwrap();
        assert_eq!(buzz.blue, Some(12.0));
        assert_eq!(buzz.yellow, Some(7.0));
        assert_eq!(buzz.green, None);

        let change: BuzzChange =
            serde_json::from_str(r#"{"blue":1,"yellow":2,"at":null}"#).unwrap();
        assert_eq!(change.green, 0.0);
    }
}
