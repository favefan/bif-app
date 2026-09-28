// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(not(test), windows_subsystem = "windows")]
mod controller;
mod gateway;
mod platform;
mod settings;
use controller::Controller;
use gateway::Paths;
use serde::Serialize;
use settings::Settings;
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
    controller: Mutex<Controller>,
    port: Mutex<Option<u16>>,
    busy: AtomicBool,
    quitting: AtomicBool,
    credential: platform::WindowsCredentials,
}

struct Busy(Arc<Runtime>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::SeqCst);
    }
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
            "document.getElementById('status')?.replaceChildren(document.createTextNode({text}))"
        ));
        show(app);
    }
    platform::error_dialog(message);
}
fn navigate(app: &tauri::AppHandle, port: Option<u16>) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        let url = match port {
            Some(p) => format!("http://127.0.0.1:{p}"),
            None => "http://tauri.localhost/index.html".into(),
        };
        w.navigate(url.parse().map_err(|e| format!("Invalid UI URL: {e}"))?)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn quit(app: &tauri::AppHandle, runtime: &Arc<Runtime>) {
    if runtime.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let runtime = runtime.clone();
    thread::spawn(move || {
        let _ = runtime.controller.lock().unwrap().stop();
        app.exit(0);
    });
}
fn local_settings_origin(label: &str, url: &tauri::Url) -> bool {
    label == "desktop-settings"
        && url.host_str() == Some("tauri.localhost")
        && matches!(url.scheme(), "http" | "https" | "tauri")
        && url.path() == "/settings.html"
}
fn authorize_settings(window: &tauri::WebviewWindow) -> Result<(), String> {
    if local_settings_origin(window.label(), &window.url().map_err(|e| e.to_string())?) {
        Ok(())
    } else {
        Err("Desktop settings are accessible only from the bundled settings window.".into())
    }
}
fn open_settings(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("desktop-settings") {
        w.show().map_err(|e| e.to_string())?;
        let _ = w.unminimize();
        return w.set_focus().map_err(|e| e.to_string());
    }
    let runtime = app.state::<Arc<Runtime>>();
    let data = runtime
        .controller
        .try_lock()
        .map_err(|_| "Gateway 正在启动或重启，请稍后再打开设置。")?
        .paths
        .desktop
        .join("webview");
    WebviewWindowBuilder::new(
        app,
        "desktop-settings",
        WebviewUrl::App("settings.html".into()),
    )
    .title("bif-app · Desktop Settings")
    .inner_size(600.0, 680.0)
    .min_inner_size(480.0, 580.0)
    .data_directory(data)
    .on_navigation(|url| local_settings_origin("desktop-settings", url))
    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}
#[derive(Serialize)]
struct Snapshot {
    settings: Settings,
    actual_port: Option<u16>,
    api_url: Option<String>,
    busy: bool,
}
fn snapshot(runtime: &Runtime) -> Snapshot {
    let c = runtime.controller.lock().unwrap();
    let port = c.port();
    Snapshot {
        settings: c.saved.settings.clone(),
        actual_port: port,
        api_url: port.map(|p| format!("http://127.0.0.1:{p}/v1")),
        busy: runtime.busy.load(Ordering::SeqCst),
    }
}
#[tauri::command]
async fn get_desktop_settings(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, Arc<Runtime>>,
) -> Result<Snapshot, String> {
    authorize_settings(&window)?;
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || snapshot(&runtime))
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn apply_desktop_settings(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Runtime>>,
    settings: Settings,
    allow_lan: bool,
) -> Result<Snapshot, String> {
    authorize_settings(&window)?;
    settings.validate()?;
    if settings.host == "0.0.0.0" && !allow_lan {
        return Err("请先确认局域网访问提示。".into());
    }
    if runtime.quitting.load(Ordering::SeqCst) {
        return Err("应用正在退出。".into());
    }
    runtime
        .busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "Gateway 正在启动或重启，请稍候。")?;
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = Busy(runtime.clone());
        let mut c = runtime.controller.lock().unwrap();
        let key = platform::encryption_key(
            &runtime.credential,
            c.paths.has_data().map_err(|e| e.to_string())?,
        )?;
        // Settings renderer stays available while the original UI returns to the
        // local loading screen; it never receives desktop IPC capabilities.
        navigate(&app, None)?;
        *runtime.port.lock().unwrap() = None;
        let changed = c.apply(settings, &key, || runtime.quitting.load(Ordering::SeqCst));
        let port = c.port();
        *runtime.port.lock().unwrap() = port;
        drop(c);
        if !runtime.quitting.load(Ordering::SeqCst) {
            navigate(&app, port)?;
        }
        changed?;
        let mut reply = snapshot(&runtime);
        reply.busy = false;
        Ok(reply)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn main() {
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    else {
        return;
    };
    let webview = exe_dir.join("WebView2");
    if webview.join("msedgewebview2.exe").exists() {
        std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &webview);
    }
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_opener::init()).plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![get_desktop_settings, apply_desktop_settings])
        .setup(move |app| {
            let paths = Paths::from_local(&app.path().local_data_dir()?); paths.create()?;
            let credential = platform::WindowsCredentials("bif-app/gateway-encryption-key/v1".into());
            let runtime = Arc::new(Runtime { controller: Mutex::new(Controller::new(paths.clone(), exe_dir.join("bifrost-http.exe"))?),
                port: Mutex::new(None), busy: AtomicBool::new(true), quitting: AtomicBool::new(false), credential });
            app.manage(runtime.clone());
            let nav_app = app.handle().clone(); let nav_state = runtime.clone(); let popup_app = app.handle().clone();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("bif-app").inner_size(1280.0, 850.0).min_inner_size(900.0, 600.0)
                .data_directory(paths.desktop.join("webview"))
                .on_navigation(move |url| {
                    if url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost") || url.as_str() == "about:blank" { return true; }
                    if url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port() == *nav_state.port.lock().unwrap() { return true; }
                    if matches!(url.scheme(), "https" | "http" | "mailto") { let _ = nav_app.opener().open_url(url.as_str(), None::<&str>); }
                    false
                }).on_new_window(move |url, _| {
                    if matches!(url.scheme(), "https" | "http" | "mailto") { let _ = popup_app.opener().open_url(url.as_str(), None::<&str>); }
                    tauri::webview::NewWindowResponse::Deny
                }).build()?;
            let show_item = MenuItem::with_id(app, "show", "Show bif-app", true, None::<&str>)?;
            let browser = MenuItem::with_id(app, "browser", "Open Bifrost UI in browser", true, None::<&str>)?;
            let copy = MenuItem::with_id(app, "copy", "Copy API Base URL", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "Desktop Settings…", true, None::<&str>)?;
            let login = CheckMenuItem::with_id(app, "login", "Start at Login", true, app.autolaunch().is_enabled()?, None::<&str>)?;
            let about = MenuItem::with_id(app, "about", "About bif-app", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &browser, &copy, &settings_item, &login, &about, &quit_item])?;
            let tray_state = runtime.clone();
            TrayIconBuilder::new().icon(tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?)
                .tooltip("bif-app — local Bifrost gateway").menu(&menu).show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| { if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) { show(tray.app_handle()); } })
                .on_menu_event(move |app, event| {
                    let port = *tray_state.port.lock().unwrap();
                    match event.id.as_ref() {
                        "show" => show(app),
                        "settings" => if let Err(e) = open_settings(app) { platform::error_dialog(&e); },
                        "browser" => if let Some(p) = port { let _ = app.opener().open_url(format!("http://127.0.0.1:{p}"), None::<&str>); },
                        "copy" => if let Some(p) = port { if let Err(e) = app.clipboard().write_text(format!("http://127.0.0.1:{p}/v1")) { fail(app, &format!("Cannot copy API URL: {e}")); } },
                        "login" => {
                            let manager = app.autolaunch();
                            let change = manager.is_enabled().and_then(|enabled| if enabled { manager.disable() } else { manager.enable() });
                            if let Err(e) = change { fail(app, &format!("Cannot change Start at Login: {e}")); }
                            let _ = login.set_checked(manager.is_enabled().unwrap_or(false));
                        },
                        "about" => platform::error_dialog(concat!("bif-app ", env!("CARGO_PKG_VERSION"), "\n\nbif-app is an independent desktop wrapper based on the open-source Bifrost AI Gateway by Maxim.\n\nBifrost is developed by Maxim.\nbif-app is not affiliated with or endorsed by Maxim.\n\nApache-2.0. See LICENSE and THIRD_PARTY_NOTICES.md in the portable folder.")),
                        "quit" => quit(app, &tray_state), _ => ()
                    }
                }).build(app)?;
            // CI can exercise the same tray window entry point in a clean
            // profile. Production release builds do not expose this test flag.
            #[cfg(debug_assertions)]
            if std::env::args().any(|arg| arg == "--test-settings-window") { open_settings(app.handle())?; }
            let handle = app.handle().clone(); let state = runtime.clone();
            thread::spawn(move || {
                let startup = (|| -> Result<(), String> {
                    let mut c = state.controller.lock().unwrap();
                    let key = platform::encryption_key(&state.credential, c.paths.has_data().map_err(|e| e.to_string())?)?;
                    c.start(&key, || state.quitting.load(Ordering::SeqCst))?;
                    *state.port.lock().unwrap() = c.port();
                    Ok(())
                })();
                state.busy.store(false, Ordering::SeqCst);
                match startup {
                    Ok(()) => { let port = *state.port.lock().unwrap(); if let Err(e) = navigate(&handle, port) { fail(&handle, &e); } },
                    Err(e) if !state.quitting.load(Ordering::SeqCst) => fail(&handle, &format!("{e}\n\nLogs: {}", paths.desktop.display())),
                    Err(_) => (),
                }
                loop {
                    thread::sleep(Duration::from_secs(1));
                    if state.quitting.load(Ordering::SeqCst) { break; }
                    if state.busy.load(Ordering::SeqCst) { continue; }
                    let stopped = if let Ok(mut c) = state.controller.try_lock() {
                        let exited = c.gateway.as_mut().and_then(|g| g.child.try_wait().ok().flatten()).is_some();
                        let lost_listener = c.gateway.as_ref().is_some_and(|g| !g.owns_listener());
                        if exited || lost_listener { let _ = c.stop(); *state.port.lock().unwrap() = None; }
                        exited || lost_listener
                    } else { false };
                    if stopped {
                        let _ = navigate(&handle, None);
                        fail(&handle, "Bifrost stopped or no longer exclusively owns its listening address. Open Desktop Settings to restart, or Quit and relaunch. See desktop/gateway.log.");
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if let Some(runtime) = window.app_handle().try_state::<Arc<Runtime>>() {
                        if !runtime.quitting.load(Ordering::SeqCst) { api.prevent_close(); let _ = window.hide(); }
                    }
                }
            }
        }).build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(|handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if let Some(runtime) = handle.try_state::<Arc<Runtime>>() {
                    if !runtime.quitting.load(Ordering::SeqCst) { api.prevent_exit(); quit(handle, &runtime); }
                }
            }
        }),
        Err(e) => platform::error_dialog(&format!("bif-app could not start: {e}\nKeep the portable ZIP contents together. This build needs an installed Microsoft WebView2 runtime, or the optional with-webview2 package."))
    }
}
#[cfg(test)]
mod ipc_tests {
    use super::*;
    #[test]
    fn settings_commands_require_the_bundled_settings_origin() {
        assert!(local_settings_origin(
            "desktop-settings",
            &"http://tauri.localhost/settings.html".parse().unwrap()
        ));
        for (label, url) in [
            ("main", "http://tauri.localhost/settings.html"),
            ("desktop-settings", "http://127.0.0.1:8080/settings.html"),
            ("desktop-settings", "https://example.com/settings.html"),
            ("desktop-settings", "http://tauri.localhost/index.html"),
        ] {
            assert!(!local_settings_origin(label, &url.parse().unwrap()));
        }
    }
}
