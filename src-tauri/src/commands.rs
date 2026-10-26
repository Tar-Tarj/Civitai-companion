use std::{io::Write, path::Path, sync::Arc};

#[cfg(feature = "test-panel")]
use crate::security;
use atomic_write_file::AtomicWriteFile;
use tauri::{AppHandle, Manager, State};
#[cfg(feature = "test-panel")]
use tauri::{WebviewUrl, WebviewWindowBuilder, webview::NewWindowResponse};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;
use zeroize::Zeroizing;

use crate::{
    audio,
    civitai::{CivitaiClient, thanks_comment_html},
    credentials,
    error::{AppError, AppResult},
    media::{self, ImagePayload},
    model::{
        AppSnapshot, BuzzAccountType, BuzzTransactionPage, ConnectionResult, MAX_BACKUP_BYTES,
        MAX_THANKS_LEDGER, PreferencesBackup, Settings, SettingsPatch, now_iso,
        permission_statuses, thanks_key,
    },
    security::canonical_external_url,
    storage::AppRuntime,
    sync,
};

type CommandResult<T> = Result<T, String>;

fn public<T>(result: AppResult<T>) -> CommandResult<T> {
    result.map_err(|error| error.public_message())
}

#[tauri::command]
pub async fn get_snapshot(runtime: State<'_, Arc<AppRuntime>>) -> CommandResult<AppSnapshot> {
    public(sync::snapshot(&runtime).await)
}

#[tauri::command]
pub async fn configure_api_key(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
    api_key: String,
) -> CommandResult<AppSnapshot> {
    public(configure_api_key_inner(&app, &runtime, api_key).await)
}

async fn configure_api_key_inner(
    app: &AppHandle,
    runtime: &AppRuntime,
    api_key: String,
) -> AppResult<AppSnapshot> {
    let account_guard = runtime.sync_guard.lock().await;
    let api_key = Zeroizing::new(api_key);
    credentials::validate(&api_key)?;
    let client = CivitaiClient::new(Zeroizing::new(api_key.trim().to_owned()))?;
    let identity = client.get_identity().await?;
    if !identity.user_id.is_some_and(|id| id > 0) {
        return Err(AppError::InvalidResponse);
    }
    credentials::store(client.credential())?;
    runtime.set_credential_configured(true);
    {
        let mut data = runtime.data.write().await;
        reset_for_identity(&mut data, identity.user_id);
        data.account.user_id = identity.user_id;
        data.account.username = identity.username;
        data.account.token_scope = identity.token_scope;
        data.account.profile_image_url = identity.profile_image_url;
        runtime.advance_account_generation();
        runtime.persist(&data)?;
    }
    // Publish the transition before fetching new data, including when sync fails.
    sync::emit_state(app, runtime, None).await?;
    drop(account_guard);
    sync::synchronize(app, runtime, "credential-updated").await
}

fn reset_for_identity(data: &mut crate::model::PersistedData, user_id: Option<i64>) {
    let ledger = (user_id.is_some() && data.account.user_id == user_id)
        .then(|| std::mem::take(&mut data.thanks_ledger));
    sync::reset_account_state(data);
    if let Some(ledger) = ledger {
        data.thanks_ledger = ledger;
    }
}

#[tauri::command]
pub async fn remove_api_key(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
) -> CommandResult<AppSnapshot> {
    public(remove_api_key_inner(&app, &runtime).await)
}

async fn remove_api_key_inner(app: &AppHandle, runtime: &AppRuntime) -> AppResult<AppSnapshot> {
    let _account_guard = runtime.sync_guard.lock().await;
    credentials::remove()?;
    runtime.set_credential_configured(false);
    {
        let mut data = runtime.data.write().await;
        sync::reset_account_state(&mut data);
        runtime.advance_account_generation();
        runtime.persist(&data)?;
    }
    sync::emit_state(app, runtime, None).await
}

#[tauri::command]
pub async fn test_connection(
    runtime: State<'_, Arc<AppRuntime>>,
) -> CommandResult<ConnectionResult> {
    let _account_guard = runtime.sync_guard.lock().await;
    public(test_connection_inner().await)
}

async fn test_connection_inner() -> AppResult<ConnectionResult> {
    let (username, token_scope) = sync::test_connection().await?;
    Ok(ConnectionResult {
        username,
        token_scope,
        permissions: permission_statuses(token_scope),
    })
}

#[tauri::command]
pub async fn sync_now(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
) -> CommandResult<AppSnapshot> {
    public(sync::synchronize(&app, &runtime, "manual").await)
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
    patch: SettingsPatch,
) -> CommandResult<AppSnapshot> {
    public(update_settings_inner(&app, &runtime, patch).await)
}

async fn update_settings_inner(
    app: &AppHandle,
    runtime: &AppRuntime,
    patch: SettingsPatch,
) -> AppResult<AppSnapshot> {
    let _account_guard = runtime.sync_guard.lock().await;
    let current = runtime.data.read().await.settings.clone();
    let settings = patch.apply(&current).ok_or(AppError::InvalidInput)?;
    let close_to_tray = settings.close_to_tray;
    if settings.start_with_windows != current.start_with_windows {
        if settings.start_with_windows {
            app.autolaunch().enable().map_err(|_| AppError::Operation)?;
        } else {
            app.autolaunch()
                .disable()
                .map_err(|_| AppError::Operation)?;
        }
    }
    {
        let mut data = runtime.data.write().await;
        data.settings = settings;
        runtime.persist(&data)?;
    }
    runtime.set_close_to_tray(close_to_tray);
    runtime.scheduler_wakeup.notify_one();
    sync::emit_state(app, runtime, None).await
}

#[tauri::command]
pub async fn mark_notification_read(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
    notification_id: String,
) -> CommandResult<AppSnapshot> {
    public(mark_notification_read_inner(&app, &runtime, notification_id).await)
}

async fn mark_notification_read_inner(
    app: &AppHandle,
    runtime: &AppRuntime,
    notification_id: String,
) -> AppResult<AppSnapshot> {
    let _account_guard = runtime.sync_guard.lock().await;
    let client = CivitaiClient::new(credentials::read()?)?;
    client.mark_notification_read(&notification_id).await?;
    {
        let mut data = runtime.data.write().await;
        let changed = data
            .notifications
            .items
            .iter_mut()
            .find(|item| item.id == notification_id)
            .is_some_and(|item| {
                if item.read {
                    return false;
                }
                item.read = true;
                true
            });
        if changed {
            data.notifications.unread_count = data.notifications.unread_count.saturating_sub(1);
        }
        runtime.persist(&data)?;
    }
    sync::emit_state(app, runtime, None).await
}

#[tauri::command]
pub async fn send_thanks(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
    notification_id: String,
) -> CommandResult<AppSnapshot> {
    public(send_thanks_inner(&app, &runtime, notification_id).await)
}

/// Posts "@donor <custom message>" once per donor and image. Only the notification id comes from the UI.
async fn send_thanks_inner(
    app: &AppHandle,
    runtime: &AppRuntime,
    notification_id: String,
) -> AppResult<AppSnapshot> {
    let _account_guard = runtime.sync_guard.lock().await;
    let _guard = runtime.thanks_guard.lock().await;
    let (image_id, user_id, username, message, key, already_thanked) = {
        let data = runtime.data.read().await;
        if !data.settings.thanks_enabled {
            return Err(AppError::InvalidInput);
        }
        let item = data
            .notifications
            .items
            .iter()
            .find(|item| item.id == notification_id)
            .ok_or(AppError::InvalidInput)?;
        let (image_id, user_id) = item
            .tip_image_id
            .zip(item.tip_user_id)
            .ok_or(AppError::InvalidInput)?;
        let key = thanks_key(image_id, user_id);
        let already_thanked = data.thanks_ledger.contains_key(&key);
        (
            image_id,
            user_id,
            item.username.clone().ok_or(AppError::InvalidInput)?,
            data.settings.thanks_message.clone(),
            key,
            already_thanked,
        )
    };
    if already_thanked {
        return sync::emit_state(app, runtime, None).await;
    }
    let content =
        thanks_comment_html(user_id, &username, &message).ok_or(AppError::InvalidInput)?;
    let client = CivitaiClient::new(credentials::read()?)?;
    client.post_image_comment(image_id, &content).await?;
    {
        let mut data = runtime.data.write().await;
        data.thanks_ledger.insert(key, now_iso());
        trim_thanks_ledger(&mut data);
        runtime.persist(&data)?;
    }
    sync::emit_state(app, runtime, None).await
}

fn trim_thanks_ledger(data: &mut crate::model::PersistedData) {
    while data.thanks_ledger.len() > MAX_THANKS_LEDGER {
        let Some(oldest) = data
            .thanks_ledger
            .iter()
            .min_by(|a, b| a.1.cmp(b.1))
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        data.thanks_ledger.remove(&oldest);
    }
}

/// Records every tip on an image in the list as already thanked (for thanks sent by hand on the site).
#[tauri::command]
pub async fn mark_all_tips_thanked(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
) -> CommandResult<AppSnapshot> {
    public(mark_all_tips_thanked_inner(&app, &runtime).await)
}

async fn mark_all_tips_thanked_inner(
    app: &AppHandle,
    runtime: &AppRuntime,
) -> AppResult<AppSnapshot> {
    let _account_guard = runtime.sync_guard.lock().await;
    let _guard = runtime.thanks_guard.lock().await;
    {
        let mut data = runtime.data.write().await;
        let now = now_iso();
        let keys = data
            .notifications
            .items
            .iter()
            .filter_map(|item| item.tip_image_id.zip(item.tip_user_id))
            .map(|(image, user)| thanks_key(image, user))
            .collect::<Vec<_>>();
        for key in keys {
            data.thanks_ledger.entry(key).or_insert_with(|| now.clone());
        }
        trim_thanks_ledger(&mut data);
        runtime.persist(&data)?;
    }
    sync::emit_state(app, runtime, None).await
}

/// Opens the detached notification test window; the main window can stay minimized.
#[cfg(feature = "test-panel")]
#[tauri::command]
pub async fn open_test_panel(app: AppHandle) -> CommandResult<()> {
    public(open_test_panel_inner(&app))
}

#[cfg(feature = "test-panel")]
fn open_test_panel_inner(app: &AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("test-panel") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        "test-panel",
        WebviewUrl::App("index.html?panel=test".into()),
    )
    .title("Notification test panel")
    .inner_size(380.0, 520.0)
    .min_inner_size(340.0, 360.0)
    .maximizable(false)
    .on_navigation(security::is_allowed_webview_navigation)
    .on_new_window(|_, _| NewWindowResponse::Deny)
    .build()
    .map(|_| ())
    .map_err(|_| AppError::Operation)
}

#[cfg(feature = "test-panel")]
#[tauri::command]
pub async fn send_test_notification(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
    kind: String,
) -> CommandResult<String> {
    public(sync::send_test_notification(&app, &runtime, &kind).await)
}

#[tauri::command]
pub async fn mark_all_notifications_read(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
) -> CommandResult<AppSnapshot> {
    public(mark_all_notifications_read_inner(&app, &runtime).await)
}

async fn mark_all_notifications_read_inner(
    app: &AppHandle,
    runtime: &AppRuntime,
) -> AppResult<AppSnapshot> {
    let _account_guard = runtime.sync_guard.lock().await;
    let client = CivitaiClient::new(credentials::read()?)?;
    client.mark_all_notifications_read().await?;
    {
        let mut data = runtime.data.write().await;
        for item in &mut data.notifications.items {
            item.read = true;
        }
        data.notifications.unread_count = 0;
        runtime.persist(&data)?;
    }
    sync::emit_state(app, runtime, None).await
}

#[tauri::command]
pub async fn get_buzz_transactions(
    runtime: State<'_, Arc<AppRuntime>>,
    account_type: BuzzAccountType,
    cursor: Option<String>,
) -> CommandResult<BuzzTransactionPage> {
    public(get_buzz_transactions_inner(&runtime, account_type, cursor).await)
}

async fn get_buzz_transactions_inner(
    runtime: &AppRuntime,
    account_type: BuzzAccountType,
    cursor: Option<String>,
) -> AppResult<BuzzTransactionPage> {
    let _account_guard = runtime.sync_guard.lock().await;
    let user_id = runtime.data.read().await.account.user_id;
    let client = CivitaiClient::new(credentials::read()?)?;
    client
        .get_buzz_transaction_page(account_type, cursor.as_deref(), user_id)
        .await
}

#[tauri::command]
pub async fn preview_sound(sound_id: String) -> CommandResult<Option<String>> {
    if !matches!(sound_id.as_str(), "1" | "2" | "3" | "4") {
        return Err(AppError::InvalidInput.public_message());
    }
    let fallback = sound_id.clone();
    Ok((!audio::play_notification_sound_async(sound_id).await).then_some(fallback))
}

#[tauri::command]
pub async fn fetch_civitai_image(
    runtime: State<'_, Arc<AppRuntime>>,
    url: String,
) -> CommandResult<ImagePayload> {
    let permit = runtime
        .image_fetch_guard
        .acquire()
        .await
        .map_err(|_| AppError::Operation.public_message())?;
    let result = public(media::fetch(&url).await);
    drop(permit);
    result
}

/// Version of a newer GitHub release, or `None` when up to date, disabled or offline.
#[tauri::command]
pub async fn check_for_update(
    runtime: State<'_, Arc<AppRuntime>>,
) -> CommandResult<Option<String>> {
    if !runtime.data.read().await.settings.check_updates {
        return Ok(None);
    }
    Ok(crate::update::newer_release(env!("CARGO_PKG_VERSION"))
        .await
        .ok()
        .flatten())
}

/// Opens one of two fixed project pages; the frontend never supplies an address.
#[tauri::command]
pub fn open_github_page(app: AppHandle, target: String) -> CommandResult<()> {
    let url = match target.as_str() {
        "repo" => crate::update::REPO_URL,
        "release" => crate::update::RELEASES_URL,
        _ => return public(Err(AppError::InvalidInput)),
    };
    public(
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|_| AppError::Operation),
    )
}

#[tauri::command]
pub fn open_civitai_url(app: AppHandle, url: String) -> CommandResult<()> {
    public(open_civitai_url_inner(&app, &url))
}

fn open_civitai_url_inner(app: &AppHandle, value: &str) -> AppResult<()> {
    let url = canonical_external_url(value)?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|_| AppError::Operation)
}

#[tauri::command]
pub async fn export_preferences(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
) -> CommandResult<String> {
    let settings = runtime.data.read().await.settings.clone();
    let result = (|| {
        let json = serde_json::to_string_pretty(&PreferencesBackup::new(&settings))
            .map_err(|_| AppError::Operation)?;
        let directory = app.path().download_dir().map_err(|_| AppError::Operation)?;
        std::fs::create_dir_all(&directory).map_err(|_| AppError::Operation)?;
        let path = directory.join("civitai-companion-preferences.json");
        write_preferences_backup(&path, &json)?;
        Ok(path.to_string_lossy().into_owned())
    })();
    public(result)
}

fn write_preferences_backup(path: &Path, json: &str) -> AppResult<()> {
    let mut file = AtomicWriteFile::open(path).map_err(|_| AppError::Operation)?;
    file.write_all(json.as_bytes())
        .map_err(|_| AppError::Operation)?;
    file.commit().map_err(|_| AppError::Operation)
}

#[tauri::command]
pub async fn import_preferences(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
    json: String,
) -> CommandResult<AppSnapshot> {
    public(import_preferences_inner(&app, &runtime, json).await)
}

async fn import_preferences_inner(
    app: &AppHandle,
    runtime: &AppRuntime,
    json: String,
) -> AppResult<AppSnapshot> {
    let _account_guard = runtime.sync_guard.lock().await;
    let settings = validate_backup_json(&json)?;
    let current_autostart = runtime.data.read().await.settings.start_with_windows;
    let settings = preserve_local_autostart(settings, current_autostart);
    let close_to_tray = settings.close_to_tray;
    {
        let mut data = runtime.data.write().await;
        data.settings = settings;
        runtime.persist(&data)?;
    }
    runtime.set_close_to_tray(close_to_tray);
    runtime.scheduler_wakeup.notify_one();
    sync::emit_state(app, runtime, None).await
}

fn validate_backup_json(json: &str) -> AppResult<crate::model::Settings> {
    if json.len() > MAX_BACKUP_BYTES {
        return Err(AppError::BackupTooLarge);
    }
    if contains_secret_key(json) {
        return Err(AppError::InvalidBackup);
    }
    let backup: PreferencesBackup =
        serde_json::from_str(json).map_err(|_| AppError::InvalidBackup)?;
    backup.validate().ok_or(AppError::InvalidBackup)
}

fn preserve_local_autostart(mut imported: Settings, current_autostart: bool) -> Settings {
    imported.start_with_windows = current_autostart;
    imported
}

fn contains_secret_key(json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    fn visit(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
                matches!(
                    key.to_ascii_lowercase().as_str(),
                    "token" | "apikey" | "api_key" | "authorization" | "password" | "secret"
                ) || visit(value)
            }),
            serde_json::Value::Array(values) => values.iter().any(visit),
            _ => false,
        }
    }
    visit(&value)
}

#[tauri::command]
pub async fn reset_cached_data(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
) -> CommandResult<AppSnapshot> {
    public(reset_cached_data_inner(&app, &runtime).await)
}

async fn reset_cached_data_inner(app: &AppHandle, runtime: &AppRuntime) -> AppResult<AppSnapshot> {
    let account_guard = runtime.sync_guard.lock().await;
    {
        let mut data = runtime.data.write().await;
        // The ledger prevents duplicate THANKS comments, so it survives a cache reset.
        let ledger = std::mem::take(&mut data.thanks_ledger);
        let account_id = data.account.user_id;
        sync::reset_account_state(&mut data);
        data.thanks_ledger = ledger;
        // Keep the ledger's owner for a later key rotation even if reload fails.
        // Profile details and activity remain cleared with the rest of the cache.
        data.account.user_id = account_id;
        runtime.advance_account_generation();
        runtime.persist(&data)?;
    }
    sync::emit_state(app, runtime, None).await?;
    drop(account_guard);
    // The key is untouched: reload account, Buzz and notifications right away.
    sync::synchronize(app, runtime, "cache-reset").await
}

#[tauri::command]
pub async fn clear_account_data(
    app: AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
) -> CommandResult<AppSnapshot> {
    public(remove_api_key_inner(&app, &runtime).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotating_a_key_preserves_thanks_only_for_the_verified_same_account() {
        let mut data = crate::model::PersistedData::default();
        data.account.user_id = Some(42);
        data.thanks_ledger.insert(thanks_key(10, 20), now_iso());
        data.notifications.unread_count = 7;
        reset_for_identity(&mut data, Some(42));
        assert!(data.thanks_ledger.contains_key(&thanks_key(10, 20)));
        assert_eq!(data.notifications.unread_count, 0);

        data.account.user_id = Some(42);
        reset_for_identity(&mut data, Some(43));
        assert!(data.thanks_ledger.is_empty());
    }

    #[test]
    fn an_unknown_identity_never_inherits_thanks_history() {
        let mut data = crate::model::PersistedData::default();
        data.thanks_ledger.insert(thanks_key(10, 20), now_iso());
        reset_for_identity(&mut data, None);
        assert!(data.thanks_ledger.is_empty());
    }

    #[test]
    fn preference_export_writes_the_expected_safe_document() {
        let directory = std::env::temp_dir().join(format!(
            "civitai-companion-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("preferences.json");
        let json =
            serde_json::to_string_pretty(&PreferencesBackup::new(&Settings::default())).unwrap();

        write_preferences_backup(&path, &json).unwrap();

        let exported = std::fs::read_to_string(&path).unwrap();
        assert_eq!(exported, json);
        assert!(!contains_secret_key(&exported));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn backup_secret_detection_is_recursive_and_case_insensitive() {
        assert!(contains_secret_key(r#"{"settings":{"ApiKey":"secret"}}"#));
        assert!(contains_secret_key(r#"{"nested":[{"authorization":"x"}]}"#));
        assert!(!contains_secret_key(r#"{"settings":{"pollingMinutes":5}}"#));
    }

    #[test]
    fn external_opening_rejects_non_civitai_hosts_before_plugin_call() {
        assert!(crate::security::validate_external_url("https://evil.invalid/").is_err());
        assert!(crate::security::validate_external_url("file:///C:/Windows/System32").is_err());
        assert!(crate::security::validate_external_url("https://civitai.red/images/1").is_ok());
    }

    #[test]
    fn connection_shape_contains_permissions_but_no_secret() {
        let result = ConnectionResult {
            username: "alice".into(),
            token_scope: Some(1),
            permissions: vec![crate::model::PermissionStatus {
                id: "UserRead",
                label: "UserRead",
                description: "Account",
                granted: true,
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.to_ascii_lowercase().contains("secret"));
        assert!(!json.to_ascii_lowercase().contains("apikey"));
    }

    #[test]
    fn backup_validation_rejects_oversized_malformed_and_secret_bearing_files() {
        let oversized = " ".repeat(MAX_BACKUP_BYTES + 1);
        assert!(matches!(
            validate_backup_json(&oversized),
            Err(AppError::BackupTooLarge)
        ));
        assert!(matches!(
            validate_backup_json("not json"),
            Err(AppError::InvalidBackup)
        ));
        assert!(matches!(
            validate_backup_json(r#"{"apiKey":"do-not-accept"}"#),
            Err(AppError::InvalidBackup)
        ));
        let valid =
            serde_json::to_string(&PreferencesBackup::new(&crate::model::Settings::default()))
                .unwrap();
        assert!(validate_backup_json(&valid).is_ok());
    }

    #[test]
    fn backup_round_trips_every_setting_and_accepts_older_exports() {
        let mut settings = Settings::default();
        settings.thanks_enabled = true;
        settings.thanks_message = "cheers!".into();
        settings.check_updates = false;
        settings.thread_responses_enabled = false;
        settings.notification_types.articles = false;
        let json = serde_json::to_string(&PreferencesBackup::new(&settings)).unwrap();
        assert_eq!(validate_backup_json(&json).unwrap(), settings);

        // An export from 1.0.26 has none of the newer fields: they take their defaults.
        let old = r#"{"type":"civitai-companion-desktop-settings","version":1,"exportedAt":"2026-10-15T00:00:00.000Z","settings":{"pollingMinutes":5.0}}"#;
        let imported = validate_backup_json(old).unwrap();
        assert!(!imported.thanks_enabled && imported.check_updates);
    }

    #[test]
    fn preference_import_never_changes_autostart() {
        let mut imported = Settings::default();
        imported.start_with_windows = true;
        assert!(!preserve_local_autostart(imported, false).start_with_windows);

        let imported = Settings::default();
        assert!(preserve_local_autostart(imported, true).start_with_windows);
    }
}
