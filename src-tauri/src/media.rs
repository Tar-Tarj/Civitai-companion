use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, redirect::Policy};
use serde::Serialize;
use url::Url;

use crate::{
    error::{AppError, AppResult},
    security::validate_image_url,
};

const MAX_IMAGE_BYTES: usize = 1024 * 1024;
const IMAGE_HOST: &str = "image.civitai.com";
const IMAGE_BLOB_HOST: &str = "blobs-b2.civitai.com";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePayload {
    pub mime_type: String,
    pub data: String,
}

pub async fn fetch(value: &str) -> AppResult<ImagePayload> {
    let normalized = validate_image_url(value).ok_or(AppError::UnsafeUrl)?;
    let url = Url::parse(&normalized).map_err(|_| AppError::UnsafeUrl)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .redirect(Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() < 2 && is_allowed_redirect(url) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .user_agent(concat!(
            "Civitai-Companion-Desktop/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|_| AppError::Operation)?;
    let mut response = client.get(url).send().await.map_err(map_network_error)?;
    if !response.status().is_success() {
        return Err(AppError::InvalidResponse);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_BYTES as u64)
    {
        return Err(AppError::InvalidResponse);
    }
    let mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(allowed_mime_type)
        .ok_or(AppError::InvalidResponse)?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_network_error)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_IMAGE_BYTES {
            return Err(AppError::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(AppError::InvalidResponse);
    }
    Ok(ImagePayload {
        mime_type: mime_type.into(),
        data: STANDARD.encode(bytes),
    })
}

fn is_allowed_redirect(url: &Url) -> bool {
    url.scheme() == "https"
        && matches!(url.host_str(), Some(IMAGE_HOST | IMAGE_BLOB_HOST))
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
}

fn allowed_mime_type(value: &str) -> Option<&'static str> {
    match value
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/jpeg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        "image/webp" => Some("image/webp"),
        "image/gif" => Some("image/gif"),
        _ => None,
    }
}

fn map_network_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        AppError::Timeout
    } else {
        AppError::Network
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_raster_mime_types_are_allowed() {
        assert_eq!(allowed_mime_type("image/jpeg"), Some("image/jpeg"));
        assert_eq!(
            allowed_mime_type("image/webp; charset=binary"),
            Some("image/webp")
        );
        assert_eq!(allowed_mime_type("image/svg+xml"), None);
        assert_eq!(allowed_mime_type("text/html"), None);
    }

    #[test]
    fn redirects_cannot_leave_the_image_origin() {
        assert!(is_allowed_redirect(
            &Url::parse("https://image.civitai.com/x.webp").unwrap()
        ));
        assert!(is_allowed_redirect(
            &Url::parse("https://blobs-b2.civitai.com/x.webp").unwrap()
        ));
        assert!(!is_allowed_redirect(
            &Url::parse("http://image.civitai.com/x.webp").unwrap()
        ));
        assert!(!is_allowed_redirect(
            &Url::parse("https://image.civitai.com.evil.invalid/x.webp").unwrap()
        ));
        assert!(!is_allowed_redirect(
            &Url::parse("https://blobs-b2.civitai.com.evil.invalid/x.webp").unwrap()
        ));
        assert!(!is_allowed_redirect(
            &Url::parse("https://user:secret@image.civitai.com/x.webp").unwrap()
        ));
        assert!(!is_allowed_redirect(
            &Url::parse("https://image.civitai.com:444/x.webp").unwrap()
        ));
    }
}
