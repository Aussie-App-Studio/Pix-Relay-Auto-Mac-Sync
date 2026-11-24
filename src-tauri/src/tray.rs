use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::AppState;

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(app, "open", "Open Pix Relay", true, None::<&str>)?;
    let sync_item = MenuItem::with_id(app, "sync_now", "Sync Auto Folders Now", true, None::<&str>)?;
    let toggle_auto = MenuItem::with_id(app, "toggle_auto", "Toggle Auto-Sync", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Pix Relay", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&open_item, &sync_item, &toggle_auto, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("pixrelay-tray")
        .menu(&menu)
        .tooltip("Pix Relay: Auto Mac Sync")
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                show_or_recreate_main_window(app);
            }
            "sync_now" => {
                let app_c = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_c.state::<AppState>();
                    let _ = state.trigger_auto_sync(&app_c).await;
                });
            }
            "toggle_auto" => {
                let state = app.state::<AppState>();
                if let Ok(mut s) = state.db.get_settings() {
                    s.auto_sync_enabled = !s.auto_sync_enabled;
                    let _ = state.db.update_settings(&s);
                    let _ = state.db.append_activity(
                        "AUTO",
                        if s.auto_sync_enabled {
                            "Auto-Sync enabled from menu bar"
                        } else {
                            "Auto-Sync paused from menu bar"
                        },
                    );
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if !cfg!(target_os = "macos") {
                    show_or_recreate_main_window(tray.app_handle());
                }
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}

pub fn show_or_recreate_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    }

    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }

    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Pix Relay: Auto Mac Sync")
        .inner_size(1220.0, 780.0)
        .min_inner_size(960.0, 640.0)
        .resizable(true)
        .build();
}
