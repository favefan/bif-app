// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(not(test), windows_subsystem = "windows")]
mod gateway;
mod platform;
use gateway::{Gateway, Paths};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

struct Runtime {
    gateway: Mutex<Option<Gateway>>,
    port: Mutex<Option<u16>>,
    quitting: AtomicBool,
}
fn show(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
fn fail(app: &tauri::AppHandle, message: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let text = serde_json::to_string(message).unwrap();
        let _ = w.eval(format!(
            "document.getElementById('status').textContent={text}"
        ));
        show(app);
    }
    platform::error_dialog(message);
}
fn quit(app: &tauri::AppHandle, runtime: &Arc<Runtime>) {
    if runtime.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let runtime = runtime.clone();
    thread::spawn(move || {
        if let Some(mut gateway) = runtime.gateway.lock().unwrap().take() {
            let _ = gateway.stop(Duration::from_secs(40));
        }
        app.exit(0);
    });
}
fn main() {
    let exe_dir = match std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        Some(p) => p,
        None => return,
    };
    // The portable ZIP carries an official Microsoft fixed runtime. Do not rely
    // on an installed Evergreen runtime or change system-wide WebView settings.
    let webview = exe_dir.join("WebView2");
    if webview.join("msedgewebview2.exe").exists() {
        std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &webview);
    }
    let runtime = Arc::new(Runtime {
        gateway: Mutex::new(None),
        port: Mutex::new(None),
        quitting: AtomicBool::new(false),
    });
    let setup_state = runtime.clone();
    let event_state = runtime.clone();
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            let paths = Paths::from_local(&app.path().local_data_dir()?);
            paths.create()?;
            let nav_app = app.handle().clone(); let nav_state = setup_state.clone();
            let popup_app = app.handle().clone();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("bif-app").inner_size(1280.0, 850.0).min_inner_size(900.0, 600.0)
                .data_directory(paths.desktop.join("webview"))
                .on_navigation(move |url| {
                    if url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost") || url.as_str() == "about:blank" { return true; }
                    if url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port() == *nav_state.port.lock().unwrap() { return true; }
                    if matches!(url.scheme(), "https" | "http" | "mailto") { let _ = nav_app.opener().open_url(url.as_str(), None::<&str>); }
                    false
                })
                .on_new_window(move |url, _| {
                    if matches!(url.scheme(), "https" | "http" | "mailto") { let _ = popup_app.opener().open_url(url.as_str(), None::<&str>); }
                    tauri::webview::NewWindowResponse::Deny
                }).build()?;
            let show_item = MenuItem::with_id(app, "show", "Show bif-app", true, None::<&str>)?;
            let browser = MenuItem::with_id(app, "browser", "Open Bifrost UI in browser", true, None::<&str>)?;
            let copy = MenuItem::with_id(app, "copy", "Copy API Base URL", true, None::<&str>)?;
            let login = CheckMenuItem::with_id(app, "login", "Start at Login", true, app.autolaunch().is_enabled()?, None::<&str>)?;
            let about = MenuItem::with_id(app, "about", "About bif-app", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &browser, &copy, &login, &about, &quit_item])?;
            let tray_state = setup_state.clone();
            TrayIconBuilder::new().icon(tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?)
                .tooltip("bif-app — local Bifrost gateway").menu(&menu).show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| { if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) { show(tray.app_handle()); } })
                .on_menu_event(move |app, event| {
                    let port = *tray_state.port.lock().unwrap();
                    match event.id.as_ref() {
                        "show" => show(app),
                        "browser" => if let Some(p) = port { let _ = app.opener().open_url(format!("http://127.0.0.1:{p}"), None::<&str>); },
                        "copy" => if let Some(p) = port { if let Err(e) = app.clipboard().write_text(format!("http://127.0.0.1:{p}/v1")) { fail(app, &format!("Cannot copy API URL: {e}")); } },
                        "login" => {
                            let manager = app.autolaunch();
                            let change = manager.is_enabled().and_then(|enabled| if enabled { manager.disable() } else { manager.enable() });
                            if let Err(e) = change { fail(app, &format!("Cannot change Start at Login: {e}")); }
                            let _ = login.set_checked(manager.is_enabled().unwrap_or(false));
                        },
                        "about" => platform::error_dialog("bif-app 0.1.0-alpha.1\n\nbif-app is an independent desktop wrapper based on the open-source Bifrost AI Gateway by Maxim.\n\nBifrost is developed by Maxim.\nbif-app is not affiliated with or endorsed by Maxim.\n\nApache-2.0. See LICENSE and THIRD_PARTY_NOTICES.md in the portable folder."),
                        "quit" => quit(app, &tray_state), _ => ()
                    }
                }).build(app)?;
            let app_handle = app.handle().clone(); let state = setup_state.clone();
            thread::spawn(move || {
                let startup = (|| -> Result<u16, String> {
                    let store = platform::WindowsCredentials("bif-app/gateway-encryption-key/v1".into());
                    let key = platform::encryption_key(&store, paths.has_data().map_err(|e| e.to_string())?)?;
                    let client = gateway::http_client().map_err(|e| e.to_string())?;
                    let reserved = gateway::reserve_port(gateway::read_state(&paths.desktop).port, &client).map_err(|e| e.to_string())?;
                    let port = reserved.local_addr().map_err(|e| e.to_string())?.port();
                    let mut slot = state.gateway.lock().unwrap();
                    if state.quitting.load(Ordering::SeqCst) { return Err("Startup cancelled".into()); }
                    drop(reserved);
                    *slot = Some(Gateway::spawn(&exe_dir.join("bifrost-http.exe"), &paths, port, key)?);
                    let gateway = slot.as_mut().unwrap();
                    gateway.wait_ready(&client, Duration::from_secs(90))?;
                    gateway::save_state(&paths.desktop, port).map_err(|e| e.to_string())?;
                    *state.port.lock().unwrap() = Some(port);
                    Ok(port)
                })();
                match startup {
                    Ok(port) => {
                        if let Some(w) = app_handle.get_webview_window("main") { if let Err(e) = w.navigate(format!("http://127.0.0.1:{port}").parse().unwrap()) { fail(&app_handle, &format!("Cannot open Bifrost UI: {e}")); } }
                        loop {
                            thread::sleep(Duration::from_secs(1));
                            if state.quitting.load(Ordering::SeqCst) { break; }
                            let exited = state.gateway.lock().unwrap().as_mut().and_then(|g| g.child.try_wait().ok().flatten()).is_some();
                            if exited { *state.port.lock().unwrap() = None; fail(&app_handle, "Bifrost stopped unexpectedly. See %LOCALAPPDATA%\\bif-app\\desktop\\gateway.log, then Quit and restart bif-app."); break; }
                        }
                    },
                    Err(e) => {
                        if let Some(mut g) = state.gateway.lock().unwrap().take() { let _ = g.stop(Duration::from_secs(5)); }
                        if !state.quitting.load(Ordering::SeqCst) { fail(&app_handle, &format!("{e}\n\nLogs: {}", paths.desktop.display())); }
                    }
                }
            });
            Ok(())
        })
        .on_window_event(move |window, event| { if let tauri::WindowEvent::CloseRequested { api, .. } = event { if !event_state.quitting.load(Ordering::SeqCst) { api.prevent_close(); let _ = window.hide(); } } })
        .build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(move |handle, event| { if let tauri::RunEvent::ExitRequested { api, .. } = event { if !runtime.quitting.load(Ordering::SeqCst) { api.prevent_exit(); quit(handle, &runtime); } } }),
        Err(e) => platform::error_dialog(&format!("bif-app could not start: {e}\nKeep the portable ZIP contents together. This build needs an installed Microsoft WebView2 runtime, or the optional with-webview2 package."))
    }
}
