use std::collections::{BTreeMap, BTreeSet};

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
pub const MAX_THANKS_MESSAGE_CHARS: usize = 300;
pub const MAX_THANKS_LEDGER: usize = 5000;

fn default_true() -> bool {
    true
}

fn default_polling_minutes() -> f64 {
    1.0
}

fn default_thanks_message() -> String {
    "thank you so much for the Buzz!".into()
}

/// Ledger key: one thanks per donor per image, whatever the number of tips.
pub fn thanks_key(image_id: i64, user_id: i64) -> String {
    format!("{image_id}:{user_id}")
}

/// Which notification groups are announced (sound, toast, pill). Disabled groups are marked read on sync.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NotificationTypes {
    pub comments: bool,
    pub followers: bool,
    pub milestones: bool,
    pub models: bool,
    pub collections: bool,
    pub articles: bool,
}

impl Default for NotificationTypes {
    fn default() -> Self {
        Self {
            comments: true,
            followers: true,
            milestones: true,
            models: true,
            collections: true,
            articles: true,
        }
    }
}

impl NotificationTypes {
    /// Mirrors the grouping of `notification-filters.ts`; kinds outside the six groups are always allowed.
    pub fn allows(&self, item: &NotificationItem) -> bool {
        let kind = item.kind.to_ascii_lowercase();
        let category = item.category.to_ascii_lowercase();
        if kind == "followed-by" {
            self.followers
        } else if kind.contains("comment") || category == "comment" {
            self.comments
        } else if kind.contains("reaction") || category == "milestone" {
            self.milestones
        } else if kind.starts_with("new-model-")
            || matches!(kind.as_str(), "early-access-complete" | "old-draft")
        {
            self.models
        } else if kind.starts_with("collection-") {
            self.collections
        } else if kind.contains("article") {
            self.articles
        } else {
            true
        }
    }
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
    pub notification_types: NotificationTypes,
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// Text after the leading @user in a thanks comment.
    #[serde(default = "default_thanks_message")]
    pub thanks_message: String,
    /// The THANKS button is hidden unless this is on: Civitai rate-limits comments.
    #[serde(default)]
    pub thanks_enabled: bool,
    /// One GitHub release lookup at startup; nothing is downloaded.
    #[serde(default = "default_true")]
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            thread_responses_enabled: true,
            polling_minutes: default_polling_minutes(),
            windows_notifications_enabled: true,
            notification_sounds: NotificationSounds::default(),
            notification_types: NotificationTypes::default(),
            start_with_windows: false,
            close_to_tray: true,
            thanks_message: default_thanks_message(),
            thanks_enabled: false,
            check_updates: true,
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
        self.thanks_message = self.thanks_message.trim().to_string();
        let thanks_chars = self.thanks_message.chars().count();
        if thanks_chars == 0
            || thanks_chars > MAX_THANKS_MESSAGE_CHARS
            || self.thanks_message.chars().any(char::is_control)
        {
            return None;
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
    pub notification_types: Option<NotificationTypes>,
    pub start_with_windows: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub thanks_message: Option<String>,
    pub thanks_enabled: Option<bool>,
    pub check_updates: Option<bool>,
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
            notification_types: self
                .notification_types
                .unwrap_or_else(|| current.notification_types.clone()),
            start_with_windows: self
                .start_with_windows
                .unwrap_or(current.start_with_windows),
            close_to_tray: self.close_to_tray.unwrap_or(current.close_to_tray),
            thanks_message: self
                .thanks_message
                .unwrap_or_else(|| current.thanks_message.clone()),
            thanks_enabled: self.thanks_enabled.unwrap_or(current.thanks_enabled),
            check_updates: self.check_updates.unwrap_or(current.check_updates),
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
    /// Tip on an image: target image and donor, needed to post a thanks comment.
    #[serde(default)]
    pub tip_image_id: Option<i64>,
    #[serde(default)]
    pub tip_user_id: Option<i64>,
    /// Filled from the thanks ledger when the snapshot is built; never persisted as truth.
    #[serde(default)]
    pub thanked: bool,
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
    /// `thanks_key` -> ISO time of the thanks comment already posted.
    #[serde(default)]
    pub thanks_ledger: BTreeMap<String, String>,
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
            thanks_ledger: BTreeMap::new(),
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
    pub account_generation: u64,
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
            account_generation: 0,
            credential_configured,
            account: data.account.clone(),
            buzz: data.buzz.clone(),
            buzz_change: data.buzz_change.clone(),
            today: data.today.clone(),
            notifications: {
                let mut notifications = data.notifications.clone();
                for item in &mut notifications.items {
                    item.thanked =
                        item.tip_image_id
                            .zip(item.tip_user_id)
                            .is_some_and(|(image, user)| {
                                data.thanks_ledger.contains_key(&thanks_key(image, user))
                            });
                }
                notifications
            },
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
    const REQUIREMENTS: [(&str, &str, &str, i64); 6] = [
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
        (
            "SocialWrite",
            "SocialWrite",
            "Post thank-you comments (optional)",
            524_288,
        ),
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
        assert_eq!(settings.notification_types, NotificationTypes::default());
    }

    #[test]
    fn disabled_notification_groups_are_matched_by_kind() {
        let item = |kind: &str, category: &str| NotificationItem {
            kind: kind.into(),
            category: category.into(),
            ..NotificationItem::default()
        };
        let off = NotificationTypes {
            comments: false,
            followers: false,
            milestones: false,
            models: false,
            collections: false,
            articles: false,
        };
        for (kind, category) in [
            ("new-comment-reply", "Comment"),
            ("followed-by", "Update"),
            ("image-reaction-milestone", "Milestone"),
            ("new-model-version", "Update"),
            ("collection-update", "Update"),
            ("new-article-from-following", "Update"),
        ] {
            assert!(!off.allows(&item(kind, category)), "{kind}");
            assert!(NotificationTypes::default().allows(&item(kind, category)));
        }
        assert!(off.allows(&item("tip-received", "Buzz")));
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
