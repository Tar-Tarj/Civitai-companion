use serde_json::Value;

#[test]
fn capability_exposes_only_required_window_and_state_controls() {
    let capability: Value =
        serde_json::from_str(include_str!("../capabilities/main.json")).unwrap();
    let permissions = capability["permissions"].as_array().unwrap();
    for required in [
        "core:event:allow-listen",
        "core:event:allow-unlisten",
        "core:window:allow-start-dragging",
        "core:window:allow-minimize",
        "core:window:allow-close",
        "allow-get-snapshot",
        "allow-configure-api-key",
        "allow-remove-api-key",
        "allow-test-connection",
        "allow-sync-now",
        "allow-update-settings",
        "allow-mark-notification-read",
        "allow-mark-all-notifications-read",
        "allow-get-buzz-transactions",
        "allow-preview-sound",
        "allow-fetch-civitai-image",
        "allow-open-civitai-url",
        "allow-export-preferences",
        "allow-import-preferences",
        "allow-reset-cached-data",
        "allow-clear-account-data",
    ] {
        assert!(permissions.iter().any(|permission| permission == required));
    }
    assert_eq!(permissions.len(), 21);
    let serialized = capability.to_string().to_ascii_lowercase();
    for forbidden in ["shell", "filesystem", "fs:", "http:", "process", "opener"] {
        assert!(
            !serialized.contains(forbidden),
            "forbidden capability: {forbidden}"
        );
    }
}

#[test]
fn every_registered_command_has_an_acl_permission() {
    let build = include_str!("../build.rs");
    let capability: Value =
        serde_json::from_str(include_str!("../capabilities/main.json")).unwrap();
    for permission in capability["permissions"].as_array().unwrap() {
        let Some(permission) = permission.as_str() else {
            continue;
        };
        if let Some(command) = permission.strip_prefix("allow-") {
            let command = command.replace('-', "_");
            assert!(build.contains(&format!("\"{command}\"")));
        }
    }
}

#[test]
fn tauri_security_contract_is_strict() {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let window = &config["app"]["windows"][0];
    assert_eq!(window["create"], false);
    assert_eq!(window["devtools"], false);
    assert_eq!(window["width"], 430);
    assert_eq!(window["height"], 540);
    assert_eq!(window["minWidth"], 430);
    assert_eq!(window["minHeight"], 540);
    assert!(window.get("maxWidth").is_none());
    assert!(window.get("maxHeight").is_none());
    assert_eq!(window["resizable"], true);
    assert_eq!(window["maximizable"], false);
    assert_eq!(window["minimizable"], true);
    assert_eq!(window["closable"], true);
    assert_eq!(window["decorations"], false);
    assert_eq!(window["transparent"], true);
    assert_eq!(window["shadow"], true);
    assert_eq!(config["bundle"]["createUpdaterArtifacts"], false);
    let csp = config["app"]["security"]["csp"].to_string();
    assert!(csp.contains("'none'"));
    assert!(csp.contains("blob:"));
    assert!(!csp.contains("data:"));
    assert!(!csp.contains("https://image.civitai.com"));
    assert!(!csp.contains("unsafe-inline"));
    assert!(!csp.contains("unsafe-eval"));
    assert!(!csp.contains("https://*"));
}

#[test]
fn bundled_icons_do_not_require_a_data_url_csp_exception() {
    let vite = include_str!("../../vite.config.ts");
    assert!(vite.contains("assetsInlineLimit: 0"));
}

#[test]
fn follower_previews_keep_uniform_notification_height() {
    let styles = include_str!("../../src/styles.css");
    assert!(
        styles.contains(".thumbnail-frame.avatar-thumbnail-frame { width: 42px; height: 42px;")
    );
    assert!(styles.contains(".follower-avatar-fallback .icon-user { width: 26px; height: 26px;"));
    assert!(!styles.contains("width: 63px; height: 63px;"));
}

#[test]
fn frontend_has_no_html_execution_or_navigation_escape_hatches() {
    let sources = [
        include_str!("../../src/app.ts"),
        include_str!("../../src/main.ts"),
        include_str!("../../src/dom.ts"),
        include_str!("../../src/images.ts"),
    ]
    .join("\n");
    for forbidden in [
        ".innerHTML",
        "insertAdjacentHTML",
        "document.write",
        "window.open",
        "eval(",
        "new Function",
        "target=\"_blank\"",
    ] {
        assert!(
            !sources.contains(forbidden),
            "forbidden frontend primitive: {forbidden}"
        );
    }
    assert!(sources.contains("textContent"));
}

#[test]
fn image_delivery_has_no_credential_or_arbitrary_network_path() {
    let source = include_str!("media.rs");
    assert!(source.contains("image.civitai.com"));
    assert!(source.contains("blobs-b2.civitai.com"));
    assert!(source.contains("MAX_IMAGE_BYTES"));
    assert!(source.contains("allowed_mime_type"));
    assert!(!source.contains("AUTHORIZATION"));
    assert!(!source.contains("credentials::"));
}

#[test]
fn no_browser_extension_runtime_is_linked_into_desktop_sources() {
    let sources = [
        include_str!("../../src/app.ts"),
        include_str!("../../src/bridge.ts"),
        include_str!("sync.rs"),
        include_str!("commands.rs"),
    ]
    .join("\n");
    assert!(!sources.contains("chrome."));
    assert!(!sources.contains("browser.runtime"));
}

#[test]
fn buzz_ledger_is_typed_and_stays_on_demand() {
    let commands = include_str!("commands.rs");
    let client = include_str!("civitai.rs");
    let frontend = include_str!("../../src/app.ts");
    assert!(commands.contains("account_type: BuzzAccountType"));
    assert!(client.contains("accountTypes\": [account_type.as_str()]"));
    assert!(client.contains("image.getEntitiesCoverImage"));
    assert!(frontend.contains("void this.loadBuzzTransactions(false)"));
    assert!(!frontend.contains("innerHTML"));
}
