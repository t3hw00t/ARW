use arw_tauri::{plugin as arw_plugin, ServiceState};
use tauri::Manager;

#[cfg(all(desktop, not(test)))]
fn create_tray<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> tauri::Result<()> {
    use std::time::Duration;
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let start_i = MenuItem::with_id(app, "start", "Start Service", true, None::<&str>)?;
    let stop_i = MenuItem::with_id(app, "stop", "Stop Service", true, None::<&str>)?;
    let open_i = MenuItem::with_id(app, "open-debug", "Open Debug UI", true, None::<&str>)?;
    let events_i = MenuItem::with_id(app, "events", "Events Window", true, None::<&str>)?;
    let logs_i = MenuItem::with_id(app, "logs", "Logs", true, None::<&str>)?;
    let models_i = MenuItem::with_id(app, "models", "Models", true, None::<&str>)?;
    let conns_i = MenuItem::with_id(app, "connections", "Connections", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &start_i, &stop_i, &open_i, &events_i, &logs_i, &models_i, &conns_i, &quit_i,
        ],
    )?;

    let _ = TrayIconBuilder::with_id("arw-tray")
        .tooltip("ARW")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "start" => {
                let st = app.state::<ServiceState>();
                tauri::async_runtime::spawn(arw_tauri::start_service(st, None));
            }
            "stop" => {
                let st = app.state::<ServiceState>();
                tauri::async_runtime::spawn(arw_tauri::stop_service(st, None));
            }
            "open-debug" => {
                let _ = arw_tauri::open_debug_ui(None);
            }
            "events" => {
                let _ = arw_tauri::open_events_window(app.clone());
            }
            "logs" => {
                let _ = arw_tauri::open_logs_window(app.clone());
            }
            "models" => {
                let _ = arw_tauri::open_models_window(app.clone());
            }
            "connections" => {
                let _ = arw_tauri::open_connections_window(app.clone());
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app);

    // Background health polling to update tray state + notifications on change
    let start_h = start_i.clone();
    let stop_h = stop_i.clone();
    let app_h = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut prev = None;
        let mut delay = Duration::from_secs(2);
        let mut last_prefs = std::time::Instant::now() - Duration::from_secs(60);
        let mut port_pref: Option<u16> = None;
        let mut notify_pref = true;
        loop {
            // refresh prefs every 10s
            if last_prefs.elapsed() >= Duration::from_secs(10) {
                let prefs = arw_tauri::load_prefs(Some("launcher"));
                port_pref = prefs
                    .get("port")
                    .and_then(|v| v.as_u64())
                    .and_then(|n| u16::try_from(n).ok());
                notify_pref = prefs
                    .get("notifyOnStatus")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                last_prefs = std::time::Instant::now();
            }
            let is_up = arw_tauri::check_service_health(port_pref)
                .await
                .unwrap_or(false);
            let _ = start_h.set_enabled(!is_up);
            let _ = stop_h.set_enabled(is_up);
            if let Some(tray) = app_h.tray_by_id("arw-tray") {
                let _ = tray.set_tooltip(Some(if is_up { "ARW: online" } else { "ARW: offline" }));
            }
            if prev != Some(is_up) {
                // Only notify on real changes and if enabled in prefs
                prev = Some(is_up);
                if notify_pref {
                    use tauri_plugin_notification::NotificationExt;
                    let _ = app_h
                        .notification()
                        .builder()
                        .title("ARW Service")
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
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("ARW Launcher")
            .inner_size(480.0, 320.0)
            .build()?;
            #[cfg(all(desktop, not(test)))]
            {
                create_tray(app.handle())?;
            }
            // Auto-start service if ARW_AUTOSTART=1 or prefs say so
            let auto_env = std::env::var("ARW_AUTOSTART")
                .ok()
                .map(|v| v == "1")
                .unwrap_or(false);
            let prefs = arw_tauri::load_prefs(Some("launcher"));
            let auto_pref = prefs
                .get("autostart")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if auto_env || auto_pref {
                let st = app.state::<ServiceState>();
                let _ = arw_tauri::start_service(st, None);
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
