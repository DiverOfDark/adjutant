//! Desktop integration: tray, quick-capture window, global hotkey,
//! autostart and close-to-tray.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::state::{CmdResult, err};

pub const CAPTURE: &str = "capture";

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    build_tray(app)?;
    WebviewWindowBuilder::new(app, CAPTURE, WebviewUrl::App("capture".into()))
        .title("Quick capture")
        .inner_size(560.0, 132.0)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .visible(false)
        .build()?;
    if let Some(w) = app.get_webview_window(CAPTURE) {
        let w2 = w.clone();
        w.on_window_event(move |e| {
            if let WindowEvent::Focused(false) = e {
                let _ = w2.hide();
            }
        });
    }

    let settings = crate::settings::load(app);
    if let Err(e) = register_capture_hotkey(app, &settings.capture_hotkey) {
        tracing::warn!("quick capture hotkey: {e}");
    }
    apply_autostart(app, settings.autostart);

    if let Some(main) = app.get_webview_window("main") {
        let handle = app.clone();
        main.on_window_event(move |e| {
            if let WindowEvent::CloseRequested { api, .. } = e
                && crate::settings::load(&handle).close_to_tray
            {
                api.prevent_close();
                if let Some(w) = handle.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
        });
    }
    Ok(())
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Adjutant", true, None::<&str>)?;
    let capture = MenuItem::with_id(app, "capture", "Quick capture", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Adjutant", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &capture, &PredefinedMenuItem::separator(app)?, &quit],
    )?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Adjutant")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => show_main(app),
            "capture" => toggle_capture(app),
            "quit" => {
                // The page installs a downloaded update first, then exits; this is the fallback.
                let _ = app.emit("quit-requested", ());
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                show_main(tray.app_handle());
            }
        });
    // Same artwork as the window and installer icons (assets/icon.svg).
    match tauri::image::Image::from_bytes(include_bytes!("../icons/64x64.png")) {
        Ok(icon) => tray = tray.icon(icon),
        Err(_) => {
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
        }
    }
    tray.build(app)?;
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn toggle_capture(app: &AppHandle) {
    let Some(w) = app.get_webview_window(CAPTURE) else {
        return;
    };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
    } else {
        let _ = w.center();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn register_capture_hotkey(app: &AppHandle, hotkey: &str) -> CmdResult<()> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if hotkey.trim().is_empty() {
        return Ok(());
    }
    gs.on_shortcut(hotkey, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            toggle_capture(app);
        }
    })
    .map_err(|e| format!("could not register {hotkey}: {e}"))
}

pub fn apply_autostart(app: &AppHandle, enabled: bool) {
    let al = app.autolaunch();
    let result = match (enabled, al.is_enabled().unwrap_or(false)) {
        (true, false) => al.enable(),
        (false, true) => al.disable(),
        _ => Ok(()),
    };
    if let Err(e) = result {
        tracing::warn!("autostart: {e}");
    }
}

#[tauri::command]
pub fn capture_hide(app: AppHandle) -> CmdResult<()> {
    app.get_webview_window(CAPTURE)
        .ok_or_else(|| "no capture window".to_string())?
        .hide()
        .map_err(err)
}
