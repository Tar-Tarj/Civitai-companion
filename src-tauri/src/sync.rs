use std::{
    collections::{BTreeSet, HashSet},
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Local, Utc};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::{
    civitai::{CivitaiClient, FetchedNotification, follower_key, notification_category},
    credentials,
    error::{AppError, AppResult},
    model::{
        AppSnapshot, Buzz, BuzzChange, DailyTracker, MAX_KNOWN_NOTIFICATION_IDS,
        MAX_VISIBLE_NOTIFICATIONS, NotificationItem, PersistedData, StateEvent, SyncStatus,
        TodayState, local_date_key, now_iso,
    },
    storage::AppRuntime,
};

const RECENT_NOTIFICATION_LIMIT: usize = 30;
const MAX_NOTIFICATION_PAGES: usize = 10;
const NOTIFICATION_LOOKBACK_DAYS: i64 = 5;
const NATIVE_NOTIFICATION_LIFETIME: Duration = Duration::from_secs(4);

pub async fn snapshot(runtime: &AppRuntime) -> AppResult<AppSnapshot> {
    let data = runtime.data.read().await;
    Ok(AppSnapshot::new(&data, runtime.credential_configured()))
}

pub async fn emit_state(
    app: &AppHandle,
    runtime: &AppRuntime,
    sound_id: Option<String>,
) -> AppResult<AppSnapshot> {
    let snapshot = snapshot(runtime).await?;
    crate::badge_icon::update_notification_badges(app, snapshot.notifications.unread_count);
    app.emit(
        "app-state-changed",
        StateEvent {
            snapshot: snapshot.clone(),
            sound_id,
        },
    )
    .map_err(|_| AppError::Operation)?;
    Ok(snapshot)
}

pub async fn synchronize(
    app: &AppHandle,
    runtime: &AppRuntime,
    _reason: &'static str,
) -> AppResult<AppSnapshot> {
    let _guard = runtime.sync_guard.lock().await;
    let attempted_at = now_iso();

    let token = match credentials::read() {
        Ok(token) => {
            runtime.set_credential_configured(true);
            token
        }
        Err(AppError::MissingCredential) => {
            runtime.set_credential_configured(false);
            {
                let mut data = runtime.data.write().await;
                data.sync = SyncStatus {
                    last_attempt: Some(attempted_at),
                    error: Some("Configure a Civitai API key in Settings.".into()),
                    error_code: Some("NO_AUTH".into()),
                    ..data.sync.clone()
                };
                runtime.persist(&data)?;
            }
            return emit_state(app, runtime, None).await;
        }
        Err(error) => return Err(error),
    };

    {
        let mut data = runtime.data.write().await;
        if data
            .rate_limit_retry_at
            .is_some_and(|retry_at| retry_at > Utc::now().timestamp_millis())
        {
            data.sync.last_attempt = Some(attempted_at);
            data.sync.error = Some("Civitai rate limit reached. Retrying later.".into());
            data.sync.error_code = Some("RATE_LIMIT".into());
            data.sync.error_status = 429;
            data.sync.updating = false;
            runtime.persist(&data)?;
            drop(data);
            return emit_state(app, runtime, None).await;
        }
        data.sync.last_attempt = Some(attempted_at.clone());
        data.sync.updating = true;
        data.sync.error = None;
        data.sync.error_code = None;
        data.sync.error_status = 0;
        runtime.persist(&data)?;
    }
    emit_state(app, runtime, None).await?;

    let client = CivitaiClient::new(token)?;
    let result = perform_sync(runtime, &client).await;
    match result {
        Ok((new_items, baseline)) => {
            let (settings, sound_id) = {
                let data = runtime.data.read().await;
                (
                    data.settings.clone(),
                    select_sound(&new_items, &data.settings.notification_sounds, baseline),
                )
            };
            if !baseline && settings.windows_notifications_enabled {
                deliver_native_notifications(app, &new_items);
            }
            let played_in_background = if let Some(sound_id) = sound_id.as_ref() {
                crate::audio::play_notification_sound_async(sound_id.clone()).await
            } else {
                false
            };
            let webview_fallback = if played_in_background { None } else { sound_id };
            emit_state(app, runtime, webview_fallback).await
        }
        Err(error) => {
            {
                let mut data = runtime.data.write().await;
                let (code, status) = error_code_and_status(&error);
                data.sync.error = Some(error.public_message());
                data.sync.error_code = Some(code.into());
                data.sync.error_status = status;
                data.sync.updating = false;
                if matches!(error, AppError::RateLimited) {
                    data.rate_limit_retry_at =
                        Some(Utc::now().timestamp_millis().saturating_add(60_000));
                }
                runtime.persist(&data)?;
            }
            emit_state(app, runtime, None).await
        }
    }
}

async fn perform_sync(
    runtime: &AppRuntime,
    client: &CivitaiClient,
) -> AppResult<(Vec<NotificationItem>, bool)> {
    let (buzz, unread_count, first_page, identity) = tokio::try_join!(
        client.get_buzz(),
        client.get_unread_count(),
        client.get_notifications_page(None, RECENT_NOTIFICATION_LIMIT),
        client.get_identity(),
    )?;

    let (tracker, settings, previous_buzz, previous_items, prior_daily) = {
        let data = runtime.data.read().await;
        (
            data.notification_tracker.clone(),
            data.settings.clone(),
            data.buzz.clone(),
            data.notifications.items.clone(),
            data.daily_tracker.clone(),
        )
    };
    let mut all_items = first_page.items;
    let mut cursor = first_page.next_cursor;
    let known = tracker.known_ids.iter().cloned().collect::<HashSet<_>>();
    let cutoff = Utc::now() - chrono::Duration::days(NOTIFICATION_LOOKBACK_DAYS);
    for _ in 1..MAX_NOTIFICATION_PAGES {
        let found_known = all_items.iter().any(|item| known.contains(&item.public.id));
        let reached_cutoff = all_items
            .iter()
            .filter_map(notification_time)
            .min()
            .is_some_and(|time| time <= cutoff);
        if cursor.is_none() || (tracker.initialized && found_known && reached_cutoff) {
            break;
        }
        let page = client
            .get_notifications_page(cursor.as_deref(), RECENT_NOTIFICATION_LIMIT)
            .await?;
        all_items.extend(page.items);
        cursor = page.next_cursor;
    }
    apply_cached_thumbnails(&mut all_items, &previous_items);
    client.add_thumbnails(&mut all_items).await;

    let baseline = !tracker.initialized;
    let new_items = if baseline {
        Vec::new()
    } else {
        all_items
            .iter()
            .filter(|item| !known.contains(&item.public.id))
            .filter(|item| {
                settings.thread_responses_enabled || item.public.kind != "new-thread-response"
            })
            .map(|item| item.public.clone())
            .collect::<Vec<_>>()
    };
    let known_ids = merge_known_ids(&tracker.known_ids, &all_items);
    let filtered = all_items
        .iter()
        .filter(|item| {
            settings.thread_responses_enabled || item.public.kind != "new-thread-response"
        })
        .filter(|item| notification_time(item).is_some_and(|time| time >= cutoff))
        .map(|item| item.public.clone())
        .take(MAX_VISIBLE_NOTIFICATIONS)
        .collect::<Vec<_>>();
    let ignored_unread = all_items
        .iter()
        .filter(|item| {
            !settings.thread_responses_enabled
                && item.public.kind == "new-thread-response"
                && !item.public.read
        })
        .map(|item| item.public.id.as_str())
        .collect::<HashSet<_>>()
        .len();

    let today = local_date_key();
    let mut follower_keys = if prior_daily.date == today {
        prior_daily.follower_keys
    } else {
        BTreeSet::new()
    };
    for item in &all_items {
        if notification_time(item).is_some_and(is_today)
            && let Some(key) = follower_key(item)
        {
            follower_keys.insert(key);
        }
    }
    let daily_complete = cursor.is_none()
        || all_items
            .iter()
            .filter_map(notification_time)
            .min()
            .is_some_and(|time| time.date_naive() < Local::now().date_naive());
    let now = now_iso();
    let buzz_change = positive_buzz_change(&previous_buzz, &buzz, &now);
    let unread_count = unread_count.saturating_sub(ignored_unread);

    {
        let mut data = runtime.data.write().await;
        data.account.user_id = identity.user_id;
        data.account.username = identity.username;
        data.account.token_scope = identity.token_scope;
        data.account.profile_image_url = identity.profile_image_url;
        data.buzz = buzz;
        data.buzz_change = buzz_change;
        data.today = TodayState {
            date: today.clone(),
            followers: follower_keys.len(),
            followers_complete: daily_complete,
        };
        data.notifications.unread_count = unread_count;
        data.notifications.items = merge_cached_thumbnails(filtered, &previous_items);
        data.notification_tracker.initialized = true;
        data.notification_tracker.known_ids = known_ids;
        data.daily_tracker = DailyTracker {
            date: today,
            follower_keys,
            complete: daily_complete,
        };
        data.rate_limit_retry_at = None;
        data.sync = SyncStatus {
            last_success: Some(now),
            last_attempt: data.sync.last_attempt.clone(),
            error: None,
            error_code: None,
            error_status: 0,
            initialized: true,
            updating: false,
        };
        runtime.persist(&data)?;
    }
    Ok((new_items, baseline))
}

pub async fn test_connection() -> AppResult<(String, Option<i64>)> {
    let client = CivitaiClient::new(credentials::read()?)?;
    let (identity, _, _) = tokio::try_join!(
        client.get_identity(),
        client.get_buzz(),
        client.get_unread_count(),
    )?;
    Ok((identity.username, identity.token_scope))
}

fn merge_known_ids(previous: &[String], fetched: &[FetchedNotification]) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for id in fetched
        .iter()
        .map(|item| item.public.id.clone())
        .chain(previous.iter().cloned())
    {
        if seen.insert(id.clone()) {
            result.push(id);
        }
        if result.len() >= MAX_KNOWN_NOTIFICATION_IDS {
            break;
        }
    }
    result
}

fn merge_cached_thumbnails(
    mut current: Vec<NotificationItem>,
    previous: &[NotificationItem],
) -> Vec<NotificationItem> {
    for item in &mut current {
        if item.thumbnail_url.is_some() {
            continue;
        }
        if let Some(prior) = previous.iter().find(|prior| prior.id == item.id) {
            item.thumbnail_url.clone_from(&prior.thumbnail_url);
            item.thumbnail_kind.clone_from(&prior.thumbnail_kind);
        }
    }
    current
}

fn apply_cached_thumbnails(current: &mut [FetchedNotification], previous: &[NotificationItem]) {
    for item in current {
        if item.public.thumbnail_url.is_some() {
            continue;
        }
        if let Some(prior) = previous
            .iter()
            .find(|prior| prior.id == item.public.id && prior.thumbnail_kind.is_some())
        {
            item.public.thumbnail_url.clone_from(&prior.thumbnail_url);
            item.public.thumbnail_kind.clone_from(&prior.thumbnail_kind);
        }
    }
}

fn notification_time(item: &FetchedNotification) -> Option<DateTime<Utc>> {
    item.public
        .created_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn is_today(time: DateTime<Utc>) -> bool {
    time.with_timezone(&Local).date_naive() == Local::now().date_naive()
}

fn positive_buzz_change(before: &Buzz, after: &Buzz, at: &str) -> BuzzChange {
    let blue = positive_delta(before.blue, after.blue);
    let yellow = positive_delta(before.yellow, after.yellow);
    let green = positive_delta(before.green, after.green);
    BuzzChange {
        blue,
        yellow,
        green,
        at: (blue > 0.0 || yellow > 0.0 || green > 0.0).then(|| at.into()),
    }
}

fn positive_delta(before: Option<f64>, after: Option<f64>) -> f64 {
    match (before, after) {
        (Some(before), Some(after)) => {
            let delta = after - before;
            if delta.is_finite() {
                delta.max(0.0)
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

fn error_code_and_status(error: &AppError) -> (&'static str, u16) {
    match error {
        AppError::MissingCredential => ("NO_AUTH", 0),
        AppError::Timeout => ("TIMEOUT", 0),
        AppError::Network => ("NETWORK", 0),
        AppError::Unauthorized => ("UNAUTHORIZED", 401),
        AppError::Forbidden => ("FORBIDDEN", 403),
        AppError::RateLimited => ("RATE_LIMIT", 429),
        AppError::ServiceUnavailable => ("SERVICE_UNAVAILABLE", 503),
        _ => ("API_ERROR", 0),
    }
}

fn select_sound(
    items: &[NotificationItem],
    sounds: &crate::model::NotificationSounds,
    baseline: bool,
) -> Option<String> {
    if baseline {
        return None;
    }
    let mut selected: Option<(usize, String)> = None;
    for item in items {
        let category = notification_category(item);
        let (priority, sound) = match category {
            "tip" => (0, sounds.tips.as_ref()),
            "comment" => (1, sounds.site_activity.as_ref()),
            "submissionUpdate" => (2, sounds.site_activity.as_ref()),
            "follower" => (3, sounds.followers.as_ref()),
            "reactionMilestone" => (4, sounds.site_activity.as_ref()),
            _ => (5, sounds.site_activity.as_ref()),
        };
        if let Some(sound) = sound
            && selected
                .as_ref()
                .is_none_or(|(current_priority, _)| priority < *current_priority)
        {
            selected = Some((priority, sound.clone()));
        }
    }
    selected.map(|(_, sound)| sound)
}

fn deliver_native_notifications(app: &AppHandle, items: &[NotificationItem]) {
    let Some((title, body)) = native_notification_content(items) else {
        return;
    };
    let app_id = app.config().identifier.clone();
    clear_native_notification_history(&app_id);
    let _ = app.notification().builder().title(title).body(body).show();
    schedule_native_notification_clear(app);
}

fn native_notification_content(items: &[NotificationItem]) -> Option<(String, String)> {
    match items {
        [] => None,
        [item] => {
            let title = match notification_category(item) {
                "follower" => "New follower",
                "comment" => "New comment",
                "reactionMilestone" => "Reaction milestone",
                "tip" => "Buzz tip received",
                "submissionUpdate" => "Civitai update",
                _ => "Civitai notification",
            };
            let mut body = item.comment_preview.as_str();
            if body.is_empty() {
                body = &item.text;
            }
            Some((title.into(), body.chars().take(180).collect::<String>()))
        }
        _ => Some((
            "Civitai Companion".into(),
            format!("{} new notifications", items.len()),
        )),
    }
}

fn schedule_native_notification_clear(app: &AppHandle) {
    let app_id = app.config().identifier.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(NATIVE_NOTIFICATION_LIFETIME).await;
        clear_native_notification_history(&app_id);
    });
}

#[cfg(target_os = "windows")]
fn clear_native_notification_history(app_id: &str) {
    use windows::{UI::Notifications::ToastNotificationManager, core::HSTRING};

    if let Ok(history) = ToastNotificationManager::History() {
        let _ = history.ClearWithId(&HSTRING::from(app_id));
    }
}

#[cfg(not(target_os = "windows"))]
fn clear_native_notification_history(_app_id: &str) {}

pub fn start_scheduler(app: AppHandle, runtime: Arc<AppRuntime>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let minutes = runtime.data.read().await.settings.polling_minutes;
            let wait = Duration::from_secs_f64((minutes * 60.0).max(60.0));
            tokio::select! {
                () = tokio::time::sleep(wait) => {
                    let _ = synchronize(&app, &runtime, "scheduler").await;
                }
                () = runtime.scheduler_wakeup.notified() => {}
            }
        }
    });
}

pub fn reset_account_state(data: &mut PersistedData) {
    let settings = data.settings.clone();
    *data = PersistedData::default();
    data.settings = settings;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NotificationSounds, Settings};

    fn notification(id: &str, kind: &str) -> NotificationItem {
        NotificationItem {
            id: id.into(),
            kind: kind.into(),
            ..NotificationItem::default()
        }
    }

    #[test]
    fn dedupe_is_bounded_and_newest_first() {
        let fetched = vec![
            FetchedNotification {
                public: notification("3", "other"),
                details: serde_json::json!({}),
            },
            FetchedNotification {
                public: notification("2", "other"),
                details: serde_json::json!({}),
            },
        ];
        assert_eq!(
            merge_known_ids(&["2".into(), "1".into()], &fetched),
            ["3", "2", "1"]
        );
    }

    #[test]
    fn baseline_never_plays_sound() {
        let items = vec![notification("1", "tip-received")];
        assert_eq!(
            select_sound(&items, &NotificationSounds::default(), true),
            None
        );
        assert_eq!(
            select_sound(&items, &Settings::default().notification_sounds, false),
            Some("1".into())
        );
    }

    #[test]
    fn every_notification_group_selects_the_configured_sound() {
        let sounds = NotificationSounds {
            site_activity: Some("1".into()),
            tips: Some("2".into()),
            followers: Some("3".into()),
        };
        let cases = [
            (notification("1", "new-image-comment"), "1"),
            (notification("2", "image-reaction-milestone"), "1"),
            (notification("3", "collection-update"), "1"),
            (notification("4", "new-model-version"), "1"),
            (notification("5", "unknown-future-event"), "1"),
            (notification("6", "tip-received"), "2"),
            (notification("7", "followed-by"), "3"),
        ];
        for (mut item, expected) in cases {
            if item.kind == "new-model-version" || item.kind == "collection-update" {
                item.category = "Update".into();
            }
            assert_eq!(select_sound(&[item], &sounds, false), Some(expected.into()));
        }

        let mut thread_response = notification("8", "new-thread-response");
        thread_response.category = "Comment".into();
        assert_eq!(
            select_sound(&[thread_response], &sounds, false),
            Some("1".into())
        );
    }

    #[test]
    fn native_notifications_are_cleared_after_four_seconds() {
        assert_eq!(NATIVE_NOTIFICATION_LIFETIME, Duration::from_secs(4));
    }

    #[test]
    fn one_native_notification_keeps_its_exact_content() {
        let mut item = notification("1", "followed-by");
        item.text = "Ada has followed you!".into();
        assert_eq!(
            native_notification_content(&[item]),
            Some(("New follower".into(), "Ada has followed you!".into()))
        );
    }

    #[test]
    fn multiple_native_notifications_are_aggregated() {
        let items = vec![
            notification("1", "followed-by"),
            notification("2", "tip-received"),
        ];
        assert_eq!(
            native_notification_content(&items),
            Some(("Civitai Companion".into(), "2 new notifications".into()))
        );
    }

    #[test]
    fn buzz_change_tracks_green_balances() {
        let before = Buzz {
            green: Some(2.0),
            ..Buzz::default()
        };
        let after = Buzz {
            green: Some(5.0),
            ..Buzz::default()
        };
        let change = positive_buzz_change(&before, &after, "now");
        assert_eq!(change.green, 3.0);
        assert_eq!(change.at.as_deref(), Some("now"));
    }

    #[test]
    fn legacy_media_thumbnails_are_refetched_once() {
        let mut current = vec![FetchedNotification {
            public: notification("1", "image-reaction-milestone"),
            details: serde_json::json!({ "imageId": 1 }),
        }];
        let legacy = NotificationItem {
            id: "1".into(),
            thumbnail_url: Some("https://image.civitai.com/legacy.jpeg".into()),
            ..NotificationItem::default()
        };
        apply_cached_thumbnails(&mut current, &[legacy]);
        assert!(current[0].public.thumbnail_url.is_none());

        let tagged = NotificationItem {
            id: "1".into(),
            thumbnail_url: Some("https://image.civitai.com/current.jpeg".into()),
            thumbnail_kind: Some("image".into()),
            ..NotificationItem::default()
        };
        apply_cached_thumbnails(&mut current, &[tagged]);
        assert_eq!(
            current[0].public.thumbnail_url.as_deref(),
            Some("https://image.civitai.com/current.jpeg")
        );
    }
}
