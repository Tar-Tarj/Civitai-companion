fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_snapshot",
            "configure_api_key",
            "remove_api_key",
            "test_connection",
            "sync_now",
            "update_settings",
            "mark_notification_read",
            "mark_all_notifications_read",
            "get_buzz_transactions",
            "preview_sound",
            "fetch_civitai_image",
            "open_civitai_url",
            "export_preferences",
            "import_preferences",
            "reset_cached_data",
            "clear_account_data",
        ]),
    ))
    .expect("tauri-build failed")
}
