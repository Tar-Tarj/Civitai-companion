use url::Url;

use crate::error::{AppError, AppResult};

const ALLOWED_APP_HOSTS: [&str; 2] = ["civitai.com", "civitai.red"];
const ALLOWED_API_HOST: &str = "civitai.com";
const ALLOWED_IMAGE_HOST: &str = "image.civitai.com";

pub fn is_allowed_webview_navigation(url: &Url) -> bool {
    let production_origin = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost"));
    let development_origin = cfg!(debug_assertions)
        && url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port() == Some(1420);
    production_origin || development_origin
}

pub fn validate_api_url(url: &Url) -> AppResult<()> {
    if url.scheme() != "https"
        || url.host_str() != Some(ALLOWED_API_HOST)
        || url.port_or_known_default() != Some(443)
        || !url.path().starts_with("/api/")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::UnsafeUrl);
    }
    Ok(())
}

pub fn validate_external_url(value: &str) -> AppResult<Url> {
    let mut url = Url::parse(value).map_err(|_| AppError::UnsafeUrl)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !url
            .host_str()
            .is_some_and(|host| ALLOWED_APP_HOSTS.contains(&host))
    {
        return Err(AppError::UnsafeUrl);
    }
    url.set_fragment(None);
    Ok(url)
}

pub fn canonical_external_url(value: &str) -> AppResult<Url> {
    let mut url = validate_external_url(value)?;
    url.set_host(Some("civitai.red"))
        .map_err(|_| AppError::UnsafeUrl)?;
    url.set_port(None).map_err(|_| AppError::UnsafeUrl)?;
    Ok(url)
}

pub fn civitai_red_url(value: &str) -> Option<String> {
    let base = Url::parse("https://civitai.com").ok()?;
    let url = base.join(value).ok()?;
    canonical_external_url(url.as_str()).ok().map(Into::into)
}

pub fn notification_external_url(value: &str) -> Option<String> {
    let normalized = civitai_red_url(value)?;
    let mut url = Url::parse(&normalized).ok()?;
    let segments = url.path_segments()?.collect::<Vec<_>>();
    let valid_id = |value: Option<&&str>| {
        value
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|value| value > 0)
    };
    let allowed = match segments.as_slice() {
        ["user", name, ..] => !name.is_empty(),
        ["bounties", "entries", id, ..] => valid_id(Some(id)),
        [route, id, ..]
            if matches!(
                *route,
                "images"
                    | "models"
                    | "posts"
                    | "articles"
                    | "collections"
                    | "reviews"
                    | "bounties"
                    | "challenges"
                    | "comics"
                    | "3d-models"
            ) =>
        {
            valid_id(Some(id))
        }
        _ => false,
    };
    if !allowed {
        return None;
    }
    url.set_query(None);
    Some(url.into())
}

pub fn validate_image_url(value: &str) -> Option<String> {
    if value.len() > 2048 {
        return None;
    }
    let mut url = Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some(ALLOWED_IMAGE_HOST)
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);
    Some(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_urls_are_strictly_allowlisted() {
        assert!(validate_external_url("https://civitai.red/images/1").is_ok());
        assert!(validate_external_url("http://civitai.com/images/1").is_err());
        assert!(validate_external_url("https://civitai.com.evil.invalid/").is_err());
        assert!(validate_external_url("https://user:pass@civitai.com/").is_err());
        assert!(validate_external_url("https://civitai.com:444/images/1").is_err());
        assert!(validate_external_url("file:///C:/Windows/System32/cmd.exe").is_err());
    }

    #[test]
    fn notification_fallback_urls_allow_only_known_content_routes() {
        assert_eq!(
            notification_external_url("https://civitai.com/models/42/example?highlight=7"),
            Some("https://civitai.red/models/42/example".into())
        );
        assert_eq!(
            notification_external_url("/user/alice?returnUrl=https://evil.invalid"),
            Some("https://civitai.red/user/alice".into())
        );
        assert!(
            notification_external_url("https://civitai.com/redirect?to=https://evil.invalid")
                .is_none()
        );
        assert!(notification_external_url("https://civitai.com/models/not-an-id").is_none());
    }

    #[test]
    fn every_external_civitai_url_is_canonicalized_to_red() {
        assert_eq!(
            canonical_external_url("https://civitai.com/user/alice?tab=models#ignored")
                .unwrap()
                .as_str(),
            "https://civitai.red/user/alice?tab=models"
        );
        assert_eq!(
            canonical_external_url("https://civitai.red/images/1")
                .unwrap()
                .as_str(),
            "https://civitai.red/images/1"
        );
    }

    #[test]
    fn api_and_image_hosts_are_not_interchangeable() {
        let image = Url::parse("https://image.civitai.com/a.jpeg").unwrap();
        assert!(validate_api_url(&image).is_err());
        assert!(
            validate_api_url(&Url::parse("https://civitai.com:444/api/v1/me").unwrap()).is_err()
        );
        assert!(
            validate_api_url(&Url::parse("https://civitai.com:443/api/v1/me").unwrap()).is_ok()
        );
        assert!(validate_image_url(image.as_str()).is_some());
        assert_eq!(
            validate_image_url("https://image.civitai.com/a.jpeg#ignored"),
            Some("https://image.civitai.com/a.jpeg".into())
        );
        assert!(validate_image_url("https://evil.invalid/x.jpeg").is_none());
        assert!(validate_image_url("https://image.civitai.com:444/x.jpeg").is_none());
        assert!(validate_image_url("https://image.civitai.com:443/x.jpeg").is_some());
        assert!(
            validate_image_url(&format!("https://image.civitai.com/{}", "x".repeat(2049)))
                .is_none()
        );
    }

    #[test]
    fn webview_navigation_is_local_only() {
        assert!(is_allowed_webview_navigation(
            &Url::parse("tauri://localhost/index.html").unwrap()
        ));
        assert!(is_allowed_webview_navigation(
            &Url::parse("http://tauri.localhost/index.html").unwrap()
        ));
        assert!(!is_allowed_webview_navigation(
            &Url::parse("https://civitai.com/").unwrap()
        ));
        assert!(!is_allowed_webview_navigation(
            &Url::parse("data:text/html,<script>alert(1)</script>").unwrap()
        ));
    }
}
