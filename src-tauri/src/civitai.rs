use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Datelike, Months, SecondsFormat, TimeZone, Utc};
use reqwest::{
    Client, Method,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue},
    redirect::{Attempt, Policy},
};
use serde_json::{Value, json};
use url::Url;
use zeroize::Zeroizing;

use crate::{
    error::{AppError, AppResult},
    model::{Buzz, BuzzAccountType, BuzzTransactionItem, BuzzTransactionPage, NotificationItem},
    security::{civitai_red_url, notification_external_url, validate_api_url, validate_image_url},
};

const API_BASE: &str = "https://civitai.com";
const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Identity {
    pub user_id: Option<i64>,
    pub username: String,
    pub token_scope: Option<i64>,
    pub profile_image_url: String,
}

#[derive(Debug, Clone)]
pub struct FetchedNotification {
    pub public: NotificationItem,
    pub details: Value,
}

#[derive(Debug, Clone)]
pub struct NotificationPage {
    pub items: Vec<FetchedNotification>,
    pub next_cursor: Option<String>,
}

pub struct CivitaiClient {
    token: Zeroizing<String>,
    client: Client,
}

impl CivitaiClient {
    pub fn new(token: Zeroizing<String>) -> AppResult<Self> {
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::custom(restrict_redirects))
            .user_agent(concat!(
                "Civitai-Companion-Desktop/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|_| AppError::Operation)?;
        Ok(Self { token, client })
    }

    pub(crate) fn credential(&self) -> &str {
        self.token.as_str()
    }

    pub async fn get_identity(&self) -> AppResult<Identity> {
        let payload = self
            .fetch_json(
                Url::parse(&format!("{API_BASE}/api/v1/me")).map_err(|_| AppError::UnsafeUrl)?,
                Method::GET,
                None,
            )
            .await?;
        let user_id = integer(&payload, "id");
        let mut profile_image_url = String::new();
        if let Some(id) = user_id
            && let Ok(profile) = self
                .trpc_call("user.getById", Some(json!({ "id": id })), None)
                .await
        {
            profile_image_url = picture_url(&profile).unwrap_or_default();
        }
        Ok(Identity {
            user_id,
            username: string(&payload, "username").unwrap_or_default(),
            token_scope: integer(&payload, "tokenScope"),
            profile_image_url,
        })
    }

    pub async fn get_buzz(&self) -> AppResult<Buzz> {
        let payload = self.trpc_call("buzz.getBuzzAccount", None, None).await?;
        Ok(normalize_buzz(&payload))
    }

    pub async fn get_unread_count(&self) -> AppResult<usize> {
        let payload = self
            .trpc_call("user.checkNotifications", None, None)
            .await?;
        let value = payload
            .as_u64()
            .or_else(|| payload.get("all").and_then(Value::as_u64))
            .or_else(|| payload.get("count").and_then(Value::as_u64))
            .unwrap_or(0);
        Ok(usize::try_from(value).unwrap_or(usize::MAX))
    }

    pub async fn get_notifications_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> AppResult<NotificationPage> {
        let cursor = cursor.map(str::to_owned).unwrap_or_else(now_iso);
        let payload = self
            .trpc_call(
                "notification.getAllByUser",
                Some(json!({ "cursor": cursor, "limit": limit.min(200), "unread": false })),
                Some(json!({ "cursor": ["Date"] })),
            )
            .await?;
        let rows = payload
            .get("items")
            .or_else(|| payload.get("notifications"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let items = rows
            .into_iter()
            .filter_map(normalize_notification)
            .collect();
        let next_cursor = payload
            .get("nextCursor")
            .and_then(value_as_string)
            .filter(|value| value.len() <= 1024);
        Ok(NotificationPage { items, next_cursor })
    }

    pub async fn add_thumbnails(&self, items: &mut [FetchedNotification]) {
        let mut ids = items
            .iter()
            .filter(|item| item.public.thumbnail_url.is_none())
            .filter_map(notification_image_id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        if !ids.is_empty() {
            let entities = ids
                .iter()
                .map(|id| json!({ "entityType": "Image", "entityId": id }))
                .collect::<Vec<_>>();
            if let Ok(payload) = self
                .trpc_call(
                    "image.getEntitiesCoverImage",
                    Some(json!({ "entities": entities })),
                    None,
                )
                .await
            {
                let by_id = payload
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|image| integer(image, "id").map(|id| (id, image)))
                    .collect::<HashMap<_, _>>();
                for item in items.iter_mut() {
                    if let Some(image) = notification_image_id(item).and_then(|id| by_id.get(&id)) {
                        item.public.thumbnail_url = image_delivery_url(image);
                        item.public.thumbnail_kind = Some(
                            if is_video_media(image) {
                                "video"
                            } else {
                                "image"
                            }
                            .into(),
                        );
                    }
                }
            }
        }

        let follower_ids = items
            .iter()
            .filter(|item| item.public.kind == "followed-by" && item.public.thumbnail_url.is_none())
            .filter_map(|item| {
                integer(&item.details, "userId")
                    .or_else(|| nested_integer(&item.details, &["actor", "id"]))
            })
            .take(10)
            .collect::<Vec<_>>();
        for user_id in follower_ids {
            if let Ok(profile) = self
                .trpc_call("user.getById", Some(json!({ "id": user_id })), None)
                .await
                && let Some(url) = picture_url(&profile)
            {
                for item in items.iter_mut().filter(|item| {
                    integer(&item.details, "userId")
                        .or_else(|| nested_integer(&item.details, &["actor", "id"]))
                        == Some(user_id)
                }) {
                    item.public.thumbnail_url = Some(url.clone());
                    item.public.thumbnail_kind = Some("avatar".into());
                }
            }
        }

        let mut collection_ids = items
            .iter()
            .filter(|item| item.public.thumbnail_url.is_none())
            .filter_map(notification_collection_id)
            .collect::<Vec<_>>();
        collection_ids.sort_unstable();
        collection_ids.dedup();
        for collection_id in collection_ids.into_iter().take(10) {
            let Ok(url) = Url::parse(&format!("{API_BASE}/api/v1/collections/{collection_id}"))
            else {
                continue;
            };
            let Ok(payload) = self.fetch_json(url, Method::GET, None).await else {
                continue;
            };
            let Some(url) = collection_cover_image_url(&payload) else {
                continue;
            };
            for item in items
                .iter_mut()
                .filter(|item| notification_collection_id(item) == Some(collection_id))
            {
                item.public.thumbnail_url = Some(url.clone());
                item.public.thumbnail_kind = Some("collection".into());
            }
        }
    }

    pub async fn mark_all_notifications_read(&self) -> AppResult<()> {
        self.trpc_mutate("notification.markRead", json!({ "all": true }))
            .await?;
        Ok(())
    }

    pub async fn mark_notification_read(&self, id: &str) -> AppResult<()> {
        if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
            return Err(AppError::InvalidInput);
        }
        self.trpc_mutate("notification.markRead", json!({ "id": id }))
            .await?;
        Ok(())
    }

    pub async fn get_buzz_transaction_page(
        &self,
        account_type: BuzzAccountType,
        cursor: Option<&str>,
        user_id: Option<i64>,
    ) -> AppResult<BuzzTransactionPage> {
        if cursor.is_some_and(|value| !valid_buzz_cursor(value)) {
            return Err(AppError::InvalidInput);
        }
        let (start, end) = buzz_transaction_range();
        let mut input = json!({
            "start": start,
            "end": end,
            "accountTypes": [account_type.as_str()],
            "limit": 40,
        });
        if let Some(cursor) = cursor {
            input["cursor"] = Value::String(cursor.into());
        }
        let payload = self
            .trpc_call(
                "buzz.getUserTransactions",
                Some(input),
                Some(json!({ "start": ["Date"], "end": ["Date"] })),
            )
            .await?;
        let mut page = normalize_buzz_transaction_page(&payload, account_type, user_id);
        self.add_buzz_transaction_thumbnails(&mut page.transactions)
            .await;
        Ok(page)
    }

    async fn add_buzz_transaction_thumbnails(&self, items: &mut [BuzzTransactionItem]) {
        let mut ids = items
            .iter()
            .filter_map(|item| image_id_from_civitai_url(item.image_url.as_deref()?))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return;
        }
        let entities = ids
            .iter()
            .map(|id| json!({ "entityType": "Image", "entityId": id }))
            .collect::<Vec<_>>();
        let Ok(payload) = self
            .trpc_call(
                "image.getEntitiesCoverImage",
                Some(json!({ "entities": entities })),
                None,
            )
            .await
        else {
            return;
        };
        let by_id = payload
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|image| integer(image, "id").zip(image_delivery_url(image)))
            .collect::<HashMap<_, _>>();
        for item in items {
            if let Some(url) = item
                .image_url
                .as_deref()
                .and_then(image_id_from_civitai_url)
                .and_then(|id| by_id.get(&id))
            {
                item.thumbnail_url = Some(url.clone());
            }
        }
    }

    async fn trpc_call(
        &self,
        procedure: &str,
        input: Option<Value>,
        meta_values: Option<Value>,
    ) -> AppResult<Value> {
        if !safe_procedure(procedure) {
            return Err(AppError::InvalidInput);
        }
        let mut wrapped = json!({ "json": input.clone().unwrap_or(Value::Null) });
        if let Some(meta) = meta_values {
            wrapped["meta"] = json!({ "values": meta });
        }
        let mut url = Url::parse(&format!("{API_BASE}/api/trpc/{procedure}"))
            .map_err(|_| AppError::UnsafeUrl)?;
        if input.is_some() {
            url.query_pairs_mut()
                .append_pair("input", &wrapped.to_string());
        }
        let payload = self.fetch_json(url, Method::GET, None).await?;
        unwrap_trpc(payload)
    }

    async fn trpc_mutate(&self, procedure: &str, input: Value) -> AppResult<Value> {
        if !safe_procedure(procedure) {
            return Err(AppError::InvalidInput);
        }
        let url = Url::parse(&format!("{API_BASE}/api/trpc/{procedure}"))
            .map_err(|_| AppError::UnsafeUrl)?;
        let payload = self
            .fetch_json(url, Method::POST, Some(json!({ "json": input })))
            .await?;
        unwrap_trpc(payload)
    }

    async fn fetch_json(&self, url: Url, method: Method, body: Option<Value>) -> AppResult<Value> {
        validate_api_url(&url)?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let mut bearer = Zeroizing::new(String::with_capacity(self.token.len() + 7));
        bearer.push_str("Bearer ");
        bearer.push_str(self.token.as_str());
        let mut authorization =
            HeaderValue::from_str(&bearer).map_err(|_| AppError::InvalidCredential)?;
        authorization.set_sensitive(true);
        headers.insert(AUTHORIZATION, authorization);
        let mut request = self.client.request(method, url).headers(headers);
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").json(&body);
        }
        let mut response = request.send().await.map_err(map_network_error)?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AppError::Unauthorized);
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::Forbidden);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::RateLimited);
        }
        if status.is_server_error() {
            return Err(AppError::ServiceUnavailable);
        }
        if !status.is_success() {
            return Err(AppError::InvalidResponse);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(AppError::InvalidResponse);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_network_error)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(AppError::InvalidResponse);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| AppError::InvalidResponse)
    }
}

fn restrict_redirects(attempt: Attempt<'_>) -> reqwest::redirect::Action {
    if redirect_is_allowed(attempt.previous().len(), attempt.url()) {
        attempt.follow()
    } else {
        attempt.stop()
    }
}

fn redirect_is_allowed(previous_count: usize, next: &Url) -> bool {
    previous_count < 3
        && next.scheme() == "https"
        && next.host_str() == Some("civitai.com")
        && next.path().starts_with("/api/")
        && next.username().is_empty()
        && next.password().is_none()
}

fn map_network_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        AppError::Timeout
    } else {
        AppError::Network
    }
}

fn safe_procedure(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn unwrap_trpc(payload: Value) -> AppResult<Value> {
    if payload.get("error").is_some() {
        return Err(AppError::InvalidResponse);
    }
    let data = payload
        .get("result")
        .and_then(|result| result.get("data"))
        .cloned()
        .unwrap_or(payload);
    Ok(data.get("json").cloned().unwrap_or(data))
}

fn normalize_notification(value: Value) -> Option<FetchedNotification> {
    let id = value.get("id").and_then(value_as_string)?;
    if id.is_empty() || id.len() > 256 {
        return None;
    }
    let kind = string(&value, "type").unwrap_or_else(|| "notification".into());
    let category = string(&value, "category").unwrap_or_else(|| "Other".into());
    let details = value
        .get("details")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let (text, username, comment_preview, url) = present_notification(&kind, &category, &details);
    Some(FetchedNotification {
        public: NotificationItem {
            id,
            kind,
            category,
            read: value.get("read").and_then(Value::as_bool).unwrap_or(false),
            created_at: string(&value, "createdAt"),
            text,
            username,
            comment_preview,
            url,
            thumbnail_url: None,
            thumbnail_kind: None,
        },
        details,
    })
}

fn normalize_buzz(payload: &Value) -> Buzz {
    if let Some(rows) = payload.as_array() {
        let mut result = Buzz::default();
        for row in rows {
            let balance = number(row, "balance");
            match string(row, "accountType").as_deref() {
                Some("blue") => result.blue = balance,
                Some("yellow") => result.yellow = balance,
                Some("green") => result.green = balance,
                _ => {}
            }
        }
        return result;
    }
    Buzz {
        blue: number(payload, "blue"),
        yellow: number(payload, "yellow"),
        green: number(payload, "green"),
    }
}

fn buzz_transaction_range() -> (String, String) {
    let now = Utc::now();
    let current_month = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or(now);
    let start = current_month
        .checked_sub_months(Months::new(1))
        .unwrap_or(current_month);
    (
        start.to_rfc3339_opts(SecondsFormat::Millis, true),
        now.to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}

fn valid_buzz_cursor(value: &str) -> bool {
    if value.len() != 56 || value.chars().any(char::is_control) {
        return false;
    }
    let Some((date, id)) = value.split_once('|') else {
        return false;
    };
    date.len() == 19
        && date.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            10 => byte == b' ',
            13 | 16 => byte == b':',
            _ => byte.is_ascii_digit(),
        })
        && id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

fn normalize_buzz_transaction_page(
    payload: &Value,
    selected_type: BuzzAccountType,
    user_id: Option<i64>,
) -> BuzzTransactionPage {
    let transactions = payload
        .get("transactions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| normalize_buzz_transaction(row, selected_type, user_id))
        .take(40)
        .collect();
    let next_cursor = payload
        .get("cursor")
        .and_then(Value::as_str)
        .filter(|value| valid_buzz_cursor(value))
        .map(str::to_owned);
    BuzzTransactionPage {
        transactions,
        next_cursor,
    }
}

fn normalize_buzz_transaction(
    row: &Value,
    selected_type: BuzzAccountType,
    user_id: Option<i64>,
) -> Option<BuzzTransactionItem> {
    let date = string(row, "date")?;
    if date.len() > 64 || DateTime::parse_from_rfc3339(&date).is_err() {
        return None;
    }
    let raw_amount = number(row, "amount")?;
    let from_id = integer(row, "fromAccountId");
    let to_id = integer(row, "toAccountId");
    let is_debit =
        raw_amount < 0.0 || user_id.is_some_and(|id| from_id == Some(id) && to_id != Some(id));
    let amount = if is_debit {
        -raw_amount.abs()
    } else {
        raw_amount.abs()
    };
    let kind = buzz_transaction_kind(row.get("type"));
    let details = row.get("details").filter(|value| value.is_object());
    let counterparty = if is_debit {
        nested_string(row, &["toUser", "username"])
    } else {
        nested_string(row, &["fromUser", "username"])
    }
    .or_else(|| details.and_then(|value| string(value, "user")))
    .map(|value| truncate_preview(&value, 80));
    let raw_description = string(row, "description")
        .map(|value| truncate_preview(&value, 240))
        .filter(|value| !value.is_empty());
    let description = if kind == "Tip" {
        counterparty
            .map(|name| format!("{}: {name}", if is_debit { "To" } else { "From" }))
            .unwrap_or_else(|| "Buzz tip".into())
    } else {
        raw_description.unwrap_or_else(|| humanize_transaction_kind(&kind))
    };
    let image_id = details
        .filter(|value| {
            string(value, "entityType").is_some_and(|value| value.eq_ignore_ascii_case("image"))
        })
        .and_then(|value| integer(value, "entityId"))
        .or_else(|| {
            details
                .and_then(|value| string(value, "url"))
                .and_then(|value| civitai_red_url(&value))
                .as_deref()
                .and_then(image_id_from_civitai_url)
        })
        .filter(|id| *id > 0);
    Some(BuzzTransactionItem {
        date,
        kind,
        amount,
        account_type: selected_type.as_str().into(),
        description,
        image_url: image_id.and_then(|id| civitai_red_url(&format!("/images/{id}"))),
        thumbnail_url: None,
    })
}

fn buzz_transaction_kind(value: Option<&Value>) -> String {
    const NAMES: [&str; 28] = [
        "Tip",
        "Dues",
        "Generation",
        "Boost",
        "Incentive",
        "Reward",
        "Purchase",
        "Refund",
        "Bounty",
        "Bounty Entry",
        "Training",
        "Charge Back",
        "Donation",
        "Club Membership",
        "Club Membership Refund",
        "Club Withdrawal",
        "Club Deposit",
        "Withdrawal",
        "Redeemable",
        "Sell",
        "Authorized Purchase",
        "Compensation",
        "Appeal",
        "Bank",
        "Extract",
        "Fee",
        "Bid",
        "License Fee",
    ];
    let index = value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    });
    if let Some(name) = index.and_then(|index| NAMES.get(index as usize)) {
        return (*name).into();
    }
    value
        .and_then(Value::as_str)
        .filter(|value| value.len() <= 40)
        .map(humanize_transaction_kind)
        .unwrap_or_else(|| "Transaction".into())
}

fn humanize_transaction_kind(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 4);
    for (index, character) in value.chars().enumerate() {
        if index > 0 && character.is_ascii_uppercase() {
            output.push(' ');
        }
        output.push(character);
    }
    truncate_preview(&output, 48)
}

fn image_id_from_civitai_url(value: &str) -> Option<i64> {
    let url = Url::parse(value).ok()?;
    if url.scheme() != "https" || !matches!(url.host_str(), Some("civitai.com" | "civitai.red")) {
        return None;
    }
    let mut segments = url.path_segments()?;
    (segments.next()? == "images")
        .then(|| segments.next()?.parse::<i64>().ok())
        .flatten()
        .filter(|id| *id > 0)
}

fn present_notification(
    kind: &str,
    category: &str,
    details: &Value,
) -> (String, Option<String>, String, Option<String>) {
    let username = nested_string(details, &["actor", "username"])
        .or_else(|| string(details, "username"))
        .or_else(|| string(details, "user"));
    let mut text = string(details, "message").unwrap_or_default();
    let mut url = string(details, "url").and_then(|url| notification_external_url(&url));
    let kind_lower = kind.to_ascii_lowercase();
    let mut comment_preview = String::new();

    if kind_lower == "followed-by" {
        if text.is_empty() {
            text = format!(
                "{} has followed you!",
                username.as_deref().unwrap_or("A user")
            );
        }
        if url.is_none()
            && let Some(name) = username.as_deref()
        {
            url = civitai_red_url(&format!("/user/{}", encode_path_segment(name)));
        }
    } else if kind_lower.contains("comment") || category.eq_ignore_ascii_case("comment") {
        comment_preview = truncate_preview(string(details, "content").as_deref().unwrap_or(""), 48);
        url = comment_url(details, &kind_lower).or(url);
    } else if kind_lower == "tip-received" {
        if let Some(amount) = number(details, "amount") {
            let buzz_type = string(details, "toAccountType").unwrap_or_else(|| "Yellow".into());
            text = format!(
                "{} tipped you {} {buzz_type} Buzz",
                username.as_deref().unwrap_or("A user"),
                format_number(amount)
            );
        }
        url = image_url(details).or(url);
    } else if kind_lower == "image-reaction-milestone" {
        if text.is_empty() {
            let count = number(details, "reactionCount")
                .map(format_number)
                .unwrap_or_else(|| "more".into());
            text = format!("Your image has received {count} reactions");
        }
        url = image_url(details).or(url);
    } else if kind_lower == "collection-update" {
        if text.is_empty() {
            text = string(details, "collectionName")
                .map(|name| format!("Collection update: {name}"))
                .unwrap_or_else(|| "Collection update".into());
        }
        url = collection_url(details).or(url);
    } else if kind_lower.starts_with("sticker-placement-") && kind_lower.ends_with("-pending") {
        if text.is_empty() {
            let actor = string(details, "placerUsername").or_else(|| username.clone());
            text = format!(
                "{}: Sticker Placement Pending",
                actor.as_deref().unwrap_or("A user")
            );
        }
        url = image_url(details).or(url);
    }

    let model_name = string(details, "modelName").unwrap_or_default();
    let version_name = string(details, "versionName").unwrap_or_default();
    let article_title = string(details, "articleTitle").unwrap_or_default();
    let collection_name = string(details, "collectionName").unwrap_or_default();
    match kind_lower.as_str() {
        "new-model-version" if !model_name.is_empty() => {
            text = format!(
                "{}{model_name}{}",
                username
                    .as_deref()
                    .map(|name| format!("{name}: "))
                    .unwrap_or_default(),
                if version_name.is_empty() {
                    " · new version".into()
                } else {
                    format!(" · new version {version_name}")
                }
            );
        }
        "new-model-from-following" if !model_name.is_empty() => {
            text = format!(
                "{}New {} · {model_name}",
                username
                    .as_deref()
                    .map(|name| format!("{name}: "))
                    .unwrap_or_default(),
                string(details, "modelType").unwrap_or_else(|| "model".into())
            );
        }
        "early-access-complete" if !model_name.is_empty() => {
            text = format!(
                "Early access complete · {model_name}{}",
                if version_name.is_empty() {
                    String::new()
                } else {
                    format!(" · {version_name}")
                }
            );
        }
        "old-draft" if !model_name.is_empty() => text = format!("Old draft · {model_name}"),
        "new-article-from-following" if !article_title.is_empty() => {
            text = format!(
                "{}New article · {article_title}",
                username
                    .as_deref()
                    .map(|name| format!("{name}: "))
                    .unwrap_or_default()
            );
        }
        "collection-item-accepted" if !collection_name.is_empty() => {
            text = format!("Image accepted in {collection_name}");
        }
        "collection-item-rejected" if !collection_name.is_empty() => {
            text = format!("Image rejected from {collection_name}");
        }
        _ => {}
    }

    url = url
        .or_else(|| model_url(details))
        .or_else(|| article_url(details))
        .or_else(|| collection_url(details));
    if text.is_empty() {
        text = humanize(kind);
        if let Some(name) = username.as_deref() {
            text = format!("{name}: {text}");
        }
    }
    (
        truncate_preview(&text, 300),
        username.map(|value| truncate_preview(&value, 80)),
        comment_preview,
        url,
    )
}

fn notification_image_id(item: &FetchedNotification) -> Option<i64> {
    integer(&item.details, "imageId")
        .or_else(|| {
            string(&item.details, "entityType")
                .is_some_and(|value| value.eq_ignore_ascii_case("image"))
                .then(|| integer(&item.details, "entityId"))
                .flatten()
        })
        .or_else(|| {
            string(&item.details, "threadType")
                .is_some_and(|value| value.eq_ignore_ascii_case("image"))
                .then(|| integer(&item.details, "threadParentId"))
                .flatten()
        })
        .filter(|id| *id > 0)
}

fn notification_collection_id(item: &FetchedNotification) -> Option<i64> {
    item.public
        .kind
        .eq_ignore_ascii_case("collection-update")
        .then(|| integer(&item.details, "collectionId"))
        .flatten()
        .filter(|id| *id > 0)
}

fn collection_cover_image_url(payload: &Value) -> Option<String> {
    string(payload, "coverImageUrl").and_then(|value| validate_image_url(&value))
}

fn image_delivery_url(image: &Value) -> Option<String> {
    let raw = string(image, "url")?;
    let name = string(image, "name").unwrap_or_else(|| "image".into());
    let url = raw_image_delivery_url(&raw, &name)?;
    Some(if is_video_media(image) {
        url.replace(
            "anim=false,optimized=true",
            "anim=false,transcode=true,optimized=true",
        )
    } else {
        url
    })
}

fn is_video_media(image: &Value) -> bool {
    string(image, "type").is_some_and(|value| value.eq_ignore_ascii_case("video"))
        || string(image, "mimeType").is_some_and(|value| {
            value
                .split_once('/')
                .is_some_and(|(kind, _)| kind.eq_ignore_ascii_case("video"))
        })
}

fn raw_image_delivery_url(raw: &str, name: &str) -> Option<String> {
    if let Some(url) = validate_image_url(raw) {
        return Some(url.replace("/original=true/", "/width=96,anim=false,optimized=true/"));
    }
    if !raw
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    let stem = encode_path_segment(name.split('.').next().unwrap_or("image"));
    validate_image_url(&format!(
        "https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/{raw}/width=96,anim=false,optimized=true/{stem}.jpeg"
    ))
}

fn picture_url(profile: &Value) -> Option<String> {
    let picture = profile
        .get("profilePicture")
        .or_else(|| profile.get("image"))?;
    if let Some(raw) = picture.as_str() {
        return raw_image_delivery_url(raw, "avatar");
    }
    image_delivery_url(picture)
}

fn image_url(details: &Value) -> Option<String> {
    let id = integer(details, "imageId")
        .or_else(|| integer(details, "entityId"))
        .filter(|id| *id > 0)?;
    civitai_red_url(&format!("/images/{id}"))
}

fn collection_url(details: &Value) -> Option<String> {
    integer(details, "collectionId")
        .filter(|id| *id > 0)
        .and_then(|id| civitai_red_url(&format!("/collections/{id}")))
}

fn model_url(details: &Value) -> Option<String> {
    integer(details, "modelId")
        .filter(|id| *id > 0)
        .and_then(|id| civitai_red_url(&format!("/models/{id}")))
}

fn article_url(details: &Value) -> Option<String> {
    integer(details, "articleId")
        .filter(|id| *id > 0)
        .and_then(|id| civitai_red_url(&format!("/articles/{id}")))
}

fn comment_url(details: &Value, kind: &str) -> Option<String> {
    let comment_id = integer(details, "commentId").filter(|id| *id > 0);
    if let (Some(parent), Some(thread_type)) = (
        integer(details, "threadParentId").filter(|id| *id > 0),
        string(details, "threadType"),
    ) {
        let route = match thread_type.as_str() {
            "model" => "models",
            "image" => "images",
            "post" => "posts",
            "article" => "articles",
            "review" => "reviews",
            "bounty" => "bounties",
            "bountyEntry" => "bounties/entries",
            "challenge" => "challenges",
            "comicChapter" => "comics",
            "model3d" => "3d-models",
            _ => return None,
        };
        let mut url = Url::parse(&format!("https://civitai.red/{route}/{parent}")).ok()?;
        {
            let mut query = url.query_pairs_mut();
            if thread_type == "model" {
                query.append_pair("dialog", "commentThread");
            }
            if let Some(id) = comment_id {
                query.append_pair("highlight", &id.to_string());
            }
            for key in ["commentParentType", "commentParentId", "threadId"] {
                if let Some(value) = details.get(key).and_then(value_as_string) {
                    query.append_pair(key, &value);
                }
            }
        }
        return civitai_red_url(url.as_str());
    }
    if let (Some(image_id), Some(comment_id)) =
        (integer(details, "imageId").filter(|id| *id > 0), comment_id)
    {
        return civitai_red_url(&format!("/images/{image_id}?highlight={comment_id}"));
    }
    if kind == "new-comment-response"
        && let (Some(model_id), Some(comment_id)) =
            (integer(details, "modelId").filter(|id| *id > 0), comment_id)
    {
        return civitai_red_url(&format!(
            "/models/{model_id}?dialog=commentThread&highlight={comment_id}"
        ));
    }
    None
}

pub fn notification_category(item: &NotificationItem) -> &'static str {
    let kind = item.kind.to_ascii_lowercase();
    let category = item.category.to_ascii_lowercase();
    if kind == "followed-by" {
        "follower"
    } else if kind.contains("comment") || category == "comment" {
        "comment"
    } else if kind.contains("reaction") && kind.contains("milestone") {
        "reactionMilestone"
    } else if kind.contains("tip") {
        "tip"
    } else if category == "update"
        || category == "bounty"
        || matches!(
            kind.as_str(),
            "featured-image"
                | "image-featured"
                | "beggars-board-rejected"
                | "beggars-board-expired"
        )
    {
        "submissionUpdate"
    } else {
        "other"
    }
}

pub fn follower_key(item: &FetchedNotification) -> Option<String> {
    if item.public.kind != "followed-by" {
        return None;
    }
    integer(&item.details, "userId")
        .or_else(|| nested_integer(&item.details, &["actor", "id"]))
        .map(|id| format!("user:{id}"))
        .or_else(|| {
            item.public
                .username
                .as_deref()
                .map(|name| format!("username:{}", name.to_ascii_lowercase()))
        })
        .or_else(|| Some(format!("notification:{}", item.public.id)))
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(value_as_string)
}

fn nested_string(value: &Value, path: &[&str]) -> Option<String> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(value_as_string)
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn integer(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn nested_integer(value: &Value, path: &[&str]) -> Option<i64> {
    let (last, parents) = path.split_last()?;
    parents
        .iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(|value| integer(value, last))
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
            .filter(|value| value.is_finite())
    })
}

fn truncate_preview(value: &str, limit: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters = normalized.chars();
    let prefix = characters.by_ref().take(limit).collect::<String>();
    if characters.next().is_some() {
        format!("{}...", prefix.trim_end())
    } else {
        prefix
    }
}

fn humanize(value: &str) -> String {
    if value.is_empty() {
        return "Civitai notification".into();
    }
    value
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn encode_path_segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procedure_names_are_constrained() {
        assert!(safe_procedure("notification.getAllByUser"));
        assert!(!safe_procedure("https://evil.invalid"));
        assert!(!safe_procedure("notification/get"));
    }

    #[test]
    fn notification_text_is_bounded_and_urls_are_safe() {
        let item = normalize_notification(json!({
            "id": 7,
            "type": "followed-by",
            "category": "Other",
            "details": {
                "username": "<script>alert(1)</script>",
                "url": "file:///C:/Windows/System32/cmd.exe"
            }
        }))
        .unwrap();
        assert!(item.public.text.contains("<script>"));
        assert!(
            item.public
                .url
                .as_deref()
                .is_some_and(|url| url.starts_with("https://civitai.red/"))
        );
        assert!(item.public.text.len() <= 303);
    }

    #[test]
    fn category_mapping_matches_existing_rules() {
        let item = NotificationItem {
            kind: "tip-received".into(),
            ..NotificationItem::default()
        };
        assert_eq!(notification_category(&item), "tip");
    }

    #[test]
    fn buzz_normalization_supports_all_account_types() {
        let array = normalize_buzz(&json!([
            { "accountType": "blue", "balance": 10 },
            { "accountType": "yellow", "balance": 20 },
            { "accountType": "green", "balance": 30 }
        ]));
        assert_eq!(array.blue, Some(10.0));
        assert_eq!(array.yellow, Some(20.0));
        assert_eq!(array.green, Some(30.0));

        let object = normalize_buzz(&json!({ "blue": 1, "yellow": 2, "green": 3 }));
        assert_eq!(object.green, Some(3.0));
    }

    #[test]
    fn credentials_can_only_follow_same_origin_api_redirects() {
        assert!(redirect_is_allowed(
            0,
            &Url::parse("https://civitai.com/api/v1/me").unwrap()
        ));
        assert!(!redirect_is_allowed(
            0,
            &Url::parse("https://evil.invalid/api/v1/me").unwrap()
        ));
        assert!(!redirect_is_allowed(
            0,
            &Url::parse("http://civitai.com/api/v1/me").unwrap()
        ));
        assert!(!redirect_is_allowed(
            3,
            &Url::parse("https://civitai.com/api/v1/me").unwrap()
        ));
    }

    #[test]
    fn image_delivery_builds_small_allowlisted_previews() {
        let opaque = "12345678-1234-1234-1234-123456789abc";
        let profile = picture_url(&json!({ "image": opaque })).unwrap();
        assert!(profile.starts_with("https://image.civitai.com/"));
        assert!(profile.contains("/width=96,anim=false,optimized=true/"));

        let original = format!(
            "https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/{opaque}/original=true/sample.jpeg"
        );
        let preview = raw_image_delivery_url(&original, "sample.jpeg").unwrap();
        assert!(!preview.contains("/original=true/"));
        assert!(preview.contains("/width=96,anim=false,optimized=true/"));
        assert!(raw_image_delivery_url("https://evil.invalid/x.jpeg", "x.jpeg").is_none());
    }

    #[test]
    fn image_delivery_keeps_supported_mature_rating_previews() {
        let opaque = "12345678-1234-1234-1234-123456789abc";

        for nsfw_level in [4, 8, 16] {
            let preview = image_delivery_url(&json!({
                "url": format!(
                    "https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/{opaque}/original=true/mature.jpeg"
                ),
                "name": "mature.jpeg",
                "type": "image",
                "nsfwLevel": nsfw_level
            }))
            .unwrap();

            assert!(preview.starts_with("https://image.civitai.com/"));
            assert!(preview.contains("/width=96,anim=false,optimized=true/"));
        }
    }

    #[test]
    fn video_delivery_builds_a_small_transcoded_still() {
        let preview = image_delivery_url(&json!({
            "url": "12345678-1234-1234-1234-123456789abc",
            "name": "clip.mp4",
            "type": "video",
            "mimeType": "video/mp4"
        }))
        .unwrap();

        assert!(preview.starts_with("https://image.civitai.com/"));
        assert!(preview.contains("/width=96,anim=false,transcode=true,optimized=true/"));
        assert!(preview.ends_with("/clip.jpeg"));
    }

    #[test]
    fn collection_updates_accept_only_allowlisted_api_cover_images() {
        let item = normalize_notification(json!({
            "id": 10,
            "type": "collection-update",
            "category": "Update",
            "details": { "collectionId": 5639443 }
        }))
        .unwrap();
        assert_eq!(notification_collection_id(&item), Some(5639443));
        assert_eq!(
            collection_cover_image_url(&json!({
                "coverImageUrl": "https://image.civitai.com/path/width=450/cover.jpeg"
            })),
            Some("https://image.civitai.com/path/width=450/cover.jpeg".into())
        );
        assert!(
            collection_cover_image_url(&json!({
                "coverImageUrl": "https://evil.invalid/cover.jpeg"
            }))
            .is_none()
        );
    }

    #[test]
    fn buzz_transaction_page_keeps_only_sanitized_display_fields() {
        let cursor = "2026-09-13 01:02:03|12345678-1234-1234-1234-123456789abc";
        let page = normalize_buzz_transaction_page(
            &json!({
                "cursor": cursor,
                "transactions": [{
                    "date": "2026-09-13T01:02:03.000Z",
                    "type": 5,
                    "amount": 2,
                    "fromAccountId": 0,
                    "toAccountId": 42,
                    "toAccountType": "blue",
                    "description": "Buzz Reward: An image you posted was liked",
                    "details": { "entityId": 99, "entityType": "Image" },
                    "authorization": "must-not-cross-boundary"
                }]
            }),
            BuzzAccountType::Blue,
            Some(42),
        );
        assert_eq!(page.next_cursor.as_deref(), Some(cursor));
        assert_eq!(page.transactions.len(), 1);
        let row = &page.transactions[0];
        assert_eq!(row.kind, "Reward");
        assert_eq!(row.amount, 2.0);
        assert_eq!(
            row.image_url.as_deref(),
            Some("https://civitai.red/images/99")
        );
        let serialized = serde_json::to_string(&page).unwrap();
        assert!(!serialized.contains("authorization"));
        assert!(!serialized.contains("must-not-cross-boundary"));
    }

    #[test]
    fn buzz_cursor_and_account_type_are_strict() {
        assert!(valid_buzz_cursor(
            "2026-09-13 01:02:03|12345678-1234-1234-1234-123456789abc"
        ));
        assert!(!valid_buzz_cursor("2026-09-13|bad"));
        assert!(!valid_buzz_cursor(
            "2026-09-13 01:02:03|12345678-1234-1234-1234-123456789ABC"
        ));
        assert!(serde_json::from_str::<BuzzAccountType>(r#""blue""#).is_ok());
        assert!(serde_json::from_str::<BuzzAccountType>(r#""red""#).is_err());
    }
}
