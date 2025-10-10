#[cfg(all(target_os = "linux", not(feature = "launcher-linux-ui"), not(test)))]
compile_error!(
    "Linux builds of the ARW launcher require enabling the `launcher-linux-ui` feature. \
Run `cargo build -p arw-launcher --features launcher-linux-ui` or exclude the launcher crate \
(`cargo build --workspace --exclude apps/arw-launcher/src-tauri`)."
);

use arw_core::util::env_bool;
use arw_tauri::{plugin as arw_plugin, ServiceState};
use tauri::Manager;

#[cfg(all(desktop, not(test)))]
fn create_tray<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    use std::time::Duration;
    use tauri::menu::{Menu, MenuItem, Submenu};
    use tauri::tray::TrayIconBuilder;

    // Service submenu
    let svc_start = MenuItem::with_id(app, "svc-start", "Start Service", true, None::<&str>)?;
    let svc_stop = MenuItem::with_id(app, "svc-stop", "Stop Service", true, None::<&str>)?;
    let svc_sub =
        Submenu::with_id_and_items(app, "svc", "Service", true, &[&svc_start, &svc_stop])?;

    // Debug submenu
    let dbg_browser = MenuItem::with_id(
        app,
        "dbg-browser",
        "Open Debug (Browser)",
        true,
        None::<&str>,
    )?;
    let dbg_window =
        MenuItem::with_id(app, "dbg-window", "Open Debug (Window)", true, None::<&str>)?;
    let dbg_sub =
        Submenu::with_id_and_items(app, "dbg", "Debug", true, &[&dbg_browser, &dbg_window])?;

    // Windows submenu
    let w_events = MenuItem::with_id(app, "win-events", "Events", true, None::<&str>)?;
    let w_logs = MenuItem::with_id(app, "win-logs", "Logs", true, None::<&str>)?;
    let w_models = MenuItem::with_id(app, "win-models", "Models", true, None::<&str>)?;
    let w_conns = MenuItem::with_id(app, "win-conns", "Connections", true, None::<&str>)?;
    let w_hub = MenuItem::with_id(app, "win-hub", "Project Hub", true, None::<&str>)?;
    let w_chat = MenuItem::with_id(app, "win-chat", "Chat", true, None::<&str>)?;
    let w_training = MenuItem::with_id(app, "win-training", "Training Park", true, None::<&str>)?;
    let w_trial = MenuItem::with_id(app, "win-trial", "Trial Control", true, None::<&str>)?;
    let w_settings = MenuItem::with_id(app, "win-settings", "Settings", true, None::<&str>)?;
    let w_mascot = MenuItem::with_id(app, "win-mascot", "Mascot (overlay)", true, None::<&str>)?;
    let windows_sub = Submenu::with_id_and_items(
        app,
        "windows",
        "Windows",
        true,
        &[
            &w_events,
            &w_logs,
            &w_models,
            &w_conns,
            &w_hub,
            &w_chat,
            &w_training,
            &w_trial,
            &w_settings,
            &w_mascot,
        ],
    )?;

    // Quit
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&svc_sub, &dbg_sub, &windows_sub, &quit_i])?;

    let _ = TrayIconBuilder::with_id("arw-launcher-tray")
        .tooltip("Agent Hub (ARW)")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            // Service
            "svc-start" => {
                let st = app.state::<ServiceState>();
                let _ = arw_tauri::start_service(app.clone(), st, None);
            }
            "svc-stop" => {
                let app_c = app.clone();
                tauri::async_runtime::spawn(async move {
                    let st = app_c.state::<ServiceState>();
                    let _ = arw_tauri::stop_service(app_c.clone(), st, None).await;
                });
            }
            // Debug
            "dbg-browser" => {
                let _ = arw_tauri::open_debug_ui(None);
            }
            "dbg-window" => {
                let _ = arw_tauri::open_debug_window(app.clone(), None);
            }
            // Windows
            "win-events" => {
                let _ = arw_tauri::open_events_window(app.clone());
            }
            "win-logs" => {
                let _ = arw_tauri::open_logs_window(app.clone());
            }
            "win-models" => {
                let _ = arw_tauri::open_models_window(app.clone());
            }
            "win-conns" => {
                let _ = arw_tauri::open_connections_window(app.clone());
            }
            "win-hub" => {
                let _ = arw_tauri::open_hub_window(app.clone());
            }
            "win-chat" => {
                let _ = arw_tauri::open_chat_window(app.clone());
            }
            "win-training" => {
                let _ = arw_tauri::open_training_window(app.clone());
            }
            "win-trial" => {
                let _ = arw_tauri::open_trial_window(app.clone());
            }
            "win-settings" => {
                let _ = arw_tauri::open_settings_window(app.clone());
            }
            "win-mascot" => {
                let _ = arw_tauri::open_mascot_window(app.clone());
            }
            // App
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app);

    // Background health polling to update tray state + notifications on change
    let start_h = svc_start.clone();
    let stop_h = svc_stop.clone();
    let app_h = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut prev = None;
        let mut delay = Duration::from_secs(2);
        let mut last_prefs = std::time::Instant::now() - Duration::from_secs(60);
        let mut port_pref: Option<u16> = None;
        let mut base_pref: Option<String> = None;
        let mut notify_pref = true;
        loop {
            // refresh prefs every 10s
            if last_prefs.elapsed() >= Duration::from_secs(10) {
                let prefs = arw_tauri::load_prefs(Some("launcher"));
                port_pref = prefs
                    .get("port")
                    .and_then(|v| v.as_u64())
                    .and_then(|n| u16::try_from(n).ok());
                base_pref = prefs
                    .get("baseOverride")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                notify_pref = prefs
                    .get("notifyOnStatus")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                last_prefs = std::time::Instant::now();
            }
            let is_up = arw_tauri::check_service_health(base_pref.clone(), port_pref)
                .await
                .unwrap_or(false);
            let _ = start_h.set_enabled(!is_up);
            let _ = stop_h.set_enabled(is_up);
            if let Some(tray) = app_h.tray_by_id("arw-launcher-tray") {
                let _ = tray.set_tooltip(Some(if is_up {
                    "Agent Hub (ARW): online"
                } else {
                    "Agent Hub (ARW): offline"
                }));
            }
            if prev != Some(is_up) {
                // Only notify on real changes and if enabled in prefs
                prev = Some(is_up);
                if notify_pref {
                    use tauri_plugin_notification::NotificationExt;
                    let _ = app_h
                        .notification()
                        .builder()
                        .title("Agent Hub (ARW) Service")
                        .body(if is_up {
                            "Service is online"
                        } else {
                            "Service is offline"
                        })
                        .show();
                }
                // when state changes, reset polling delay
                delay = Duration::from_secs(2);
            } else {
                // modest backoff while state is stable
                let next = delay.as_secs().saturating_mul(2).min(16);
                delay = Duration::from_secs(next);
            }
            tokio::time::sleep(delay).await;
        }
    });

    Ok(())
}

fn main() {
    tauri::Builder::<tauri::Wry>::default()
        .plugin(tauri_plugin_window_state::Builder::default().build::<tauri::Wry>())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&'static str>>,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            // Focus the existing window on second-instance attempt
            #[allow(unused)]
            if let Some(w) = _app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .plugin(arw_plugin::<tauri::Wry>())
        .manage(ServiceState::default())
        .setup(|app| {
            // Create a minimal window; tray does most of the work for now
            let main = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Agent Hub (ARW) Launcher")
            .inner_size(480.0, 320.0)
            .build()?;
            #[cfg(all(desktop, not(test)))]
            {
                create_tray(app.handle())?;
            }
            // Show mascot-only mode when ARW_MASCOT_ONLY=1
            if env_bool("ARW_MASCOT_ONLY").unwrap_or(false) {
                let _ = main.hide();
                let _ = arw_tauri::open_mascot_window(app.handle().clone());
            }

            // Auto-start service if ARW_AUTOSTART=1 or prefs say so
            let auto_env = env_bool("ARW_AUTOSTART").unwrap_or(false);
            let prefs = arw_tauri::load_prefs(Some("launcher"));
            let auto_pref = prefs
                .get("autostart")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if auto_env || auto_pref {
                let st = app.state::<ServiceState>();
                let _ = arw_tauri::start_service(app.handle().clone(), st, None);
            }
            // Optionally, register updater plugin (no-op without config)
            #[cfg(all(desktop, not(test)))]
            {
                let _ = app
                    .handle()
                    .plugin(tauri_plugin_updater::Builder::new().build::<tauri::Wry>());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
