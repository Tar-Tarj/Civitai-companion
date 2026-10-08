mod audio;
mod badge_icon;
mod civitai;
mod commands;
mod credentials;
mod error;
mod media;
mod model;
mod security;
mod storage;
mod sync;

#[cfg(test)]
mod security_contract;

use std::sync::Arc;

use storage::AppRuntime;
use tauri::{
    Manager, WebviewWindowBuilder, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    webview::NewWindowResponse,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::configure_api_key,
            commands::remove_api_key,
            commands::test_connection,
            commands::sync_now,
            commands::update_settings,
            commands::mark_notification_read,
            commands::mark_all_notifications_read,
            commands::get_buzz_transactions,
            commands::preview_sound,
            commands::fetch_civitai_image,
            commands::open_civitai_url,
            commands::export_preferences,
            commands::import_preferences,
            commands::reset_cached_data,
            commands::clear_account_data,
        ])
        .setup(|app| {
            let runtime = Arc::new(AppRuntime::load(app.handle())?);
            let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
            let initial_unread_count = tauri::async_runtime::block_on(async {
                let mut data = runtime.data.write().await;
                if data.settings.start_with_windows != autostart_enabled {
                    data.settings.start_with_windows = autostart_enabled;
                    runtime.persist(&data)?;
                }
                Ok::<usize, crate::error::AppError>(data.notifications.unread_count)
            })?;
            app.manage(runtime.clone());

            let mut window_config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .ok_or(crate::error::AppError::Operation)?;
            if std::env::args().any(|argument| argument == "--minimized") {
                window_config.visible = false;
            }
            WebviewWindowBuilder::from_config(app, &window_config)?
                .on_navigation(security::is_allowed_webview_navigation)
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .build()?;

            let show =
                MenuItem::with_id(app, "show", "Show Civitai Companion", true, None::<&str>)?;
            let sync_now = MenuItem::with_id(app, "sync", "Sync now", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &sync_now, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main-tray")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "sync" => {
                        let app = app.clone();
                        let runtime = app.state::<Arc<AppRuntime>>().inner().clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = sync::synchronize(&app, &runtime, "tray").await;
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            badge_icon::update_notification_badges(app.handle(), initial_unread_count);

            sync::start_scheduler(app.handle().clone(), runtime.clone());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = audio::warm_up_notification_audio().await;
                let _ = sync::synchronize(&handle, &runtime, "startup").await;
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let runtime = window.state::<Arc<AppRuntime>>();
                if runtime.close_to_tray() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Civitai Companion failed to start");
}
