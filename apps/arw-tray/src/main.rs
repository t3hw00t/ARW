use anyhow::Result;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tray_icon::{menu, TrayIconBuilder, TrayIcon};
use tray_icon::menu::MenuEvent;

#[derive(Clone)]
struct Shared {
    svc: Arc<Mutex<Option<Child>>>,
    tray: Arc<Mutex<Option<TrayIcon>>>,
    port: u16,
}

fn main() -> Result<()> {
    let port: u16 = std::env::var("ARW_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8090);
    let shared = Shared { svc: Arc::new(Mutex::new(None)), tray: Arc::new(Mutex::new(None)), port };

    let menu = menu::Menu::new();
    let m_start = menu::MenuItem::new("Start Service", true, None);
    let m_stop = menu::MenuItem::new("Stop Service", true, None);
    let m_open = menu::MenuItem::new("Open Debug UI", true, None);
    let m_quit = menu::MenuItem::new("Quit", true, None);
    let start_id = m_start.id().clone();
    let stop_id = m_stop.id().clone();
    let open_id = m_open.id().clone();
    let quit_id = m_quit.id().clone();
    menu.append(&m_start)?;
    menu.append(&m_stop)?;
    menu.append(&menu::PredefinedMenuItem::separator())?;
    menu.append(&m_open)?;
    menu.append(&menu::PredefinedMenuItem::separator())?;
    menu.append(&m_quit)?;

    let mut tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("ARW: offline")
        .build()?;

    // Menu callbacks: use global MenuEvent receiver
    {
        let svc = shared.svc.clone();
        let port = shared.port;
        let start = start_id.clone();
        let stop = stop_id.clone();
        let open = open_id.clone();
        let quit = quit_id.clone();
        thread::spawn(move || loop {
            if let Ok(ev) = MenuEvent::receiver().recv() {
                if ev.id == start { let _ = start_service_inner(&svc, port); }
                if ev.id == stop { let _ = stop_service_inner(&svc, port); }
                if ev.id == open { let _ = open::that(format!("http://127.0.0.1:{}/debug", port)); }
                if ev.id == quit { let _ = stop_service_inner(&svc, port); std::process::exit(0); }
            }
        });
    }

    // store tray handle
    {
        let mut g = shared.tray.lock().unwrap();
        *g = Some(tray);
    }

    // Health monitor omitted (keeps tray on main thread only)

    // keep running
    loop { thread::sleep(Duration::from_secs(3600)); }
}

fn check_health(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/healthz", port);
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_millis(1000)).timeout_read(Duration::from_millis(1000)).build();
    agent.get(&url).call().ok().map(|r| r.status()==200).unwrap_or(false)
}

fn start_service(shared: &Shared) -> Result<()> {
    start_service_inner(&shared.svc, shared.port)
}

fn start_service_inner(svc: &Arc<Mutex<Option<Child>>>, port: u16) -> Result<()> {
    // already running?
    {
        let mut guard = svc.lock().unwrap();
        if let Some(child) = guard.as_mut() {
            if child.try_wait()?.is_none() { return Ok(()); }
        }
    }
    let svc_bin = locate_svc_binary();
    let svc_bin = svc_bin.ok_or_else(|| anyhow::anyhow!("arw-svc binary not found"))?;
    let mut cmd = Command::new(svc_bin);
    cmd.env("ARW_DEBUG", "1").env("ARW_PORT", format!("{}", port)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let child = cmd.spawn()?;
    *svc.lock().unwrap() = Some(child);
    Ok(())
}

fn stop_service(shared: &Shared) -> Result<()> {
    stop_service_inner(&shared.svc, shared.port)
}

fn stop_service_inner(svc: &Arc<Mutex<Option<Child>>>, port: u16) -> Result<()> {
    // try graceful shutdown
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_millis(1000)).timeout_read(Duration::from_millis(1000)).build();
    let _ = agent.get(&format!("http://127.0.0.1:{}/shutdown", port)).call();
    // then wait briefly and kill if needed
    thread::sleep(Duration::from_millis(500));
    if let Some(mut child) = svc.lock().unwrap().take() {
        let _ = child.kill();
    }
    Ok(())
}

fn locate_svc_binary() -> Option<PathBuf> {
    // 1) next to this exe (packaged dist)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("arw-svc");
            let candidate_win = dir.join("arw-svc.exe");
            if candidate.exists() { return Some(candidate); }
            if candidate_win.exists() { return Some(candidate_win); }
            // packaged layout may be bin/ alongside
            let bin = dir.join("bin");
            let c2 = bin.join("arw-svc");
            let c2w = bin.join("arw-svc.exe");
            if c2.exists() { return Some(c2); }
            if c2w.exists() { return Some(c2w); }
        }
    }
    // 2) workspace target/release
    let mut path = std::env::current_dir().ok()?;
    for _ in 0..3 { // walk up a bit
        let p = path.join("target").join("release").join(if cfg!(windows) { "arw-svc.exe" } else { "arw-svc" });
        if p.exists() { return Some(p); }
        path = path.parent()?.to_path_buf();
    }
    None
}
