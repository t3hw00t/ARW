use crate::image_utils::png_to_rgba_image;
use anyhow::{anyhow, Context, Result};
use dbus::{
    arg::{AppendAll, Iter, IterAppend, PropMap, ReadAll, RefArg, TypeMismatchError, Variant},
    blocking::Connection,
    message::{MatchRule, SignalArgs},
};
use image::RgbaImage;
use libwayshot::{CaptureRegion, WayshotConnection};
use percent_encoding::percent_decode;
use std::{
    collections::HashMap,
    env,
    fs::{self},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub struct OrgFreedesktopPortalRequestResponse {
    pub status: u32,
    pub results: PropMap,
}

impl AppendAll for OrgFreedesktopPortalRequestResponse {
    fn append(&self, i: &mut IterAppend) {
        RefArg::append(&self.status, i);
        RefArg::append(&self.results, i);
    }
}

impl ReadAll for OrgFreedesktopPortalRequestResponse {
    fn read(i: &mut Iter) -> Result<Self, TypeMismatchError> {
        Ok(OrgFreedesktopPortalRequestResponse {
            status: i.read()?,
            results: i.read()?,
        })
    }
}

impl SignalArgs for OrgFreedesktopPortalRequestResponse {
    const NAME: &'static str = "Response";
    const INTERFACE: &'static str = "org.freedesktop.portal.Request";
}

fn load_rgba_and_cleanup(
    path: &String,
    crop_x: i32,
    crop_y: i32,
    width: i32,
    height: i32,
) -> Result<RgbaImage> {
    match png_to_rgba_image(path, crop_x, crop_y, width, height) {
        Ok(image) => {
            fs::remove_file(path)
                .with_context(|| format!("failed to remove temporary screenshot {path}"))?;
            Ok(image)
        }
        Err(err) => {
            let _ = fs::remove_file(path);
            Err(err)
        }
    }
}

fn org_gnome_shell_screenshot(
    conn: &Connection,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<RgbaImage> {
    let proxy = conn.with_proxy(
        "org.gnome.Shell.Screenshot",
        "/org/gnome/Shell/Screenshot",
        Duration::from_secs(10),
    );

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?;

    let dirname = env::temp_dir().join("screenshot");

    fs::create_dir_all(&dirname).with_context(|| {
        format!(
            "failed to ensure temporary screenshot dir {}",
            dirname.display()
        )
    })?;

    let mut path = dirname.join(timestamp.as_micros().to_string());
    path.set_extension("png");

    let filename = path.to_string_lossy().to_string();

    proxy.method_call::<(), _, _, _>(
        "org.gnome.Shell.Screenshot",
        "ScreenshotArea",
        (x, y, width, height, false, &filename),
    )?;

    load_rgba_and_cleanup(&filename, 0, 0, width, height)
}

fn org_freedesktop_portal_screenshot(
    conn: &Connection,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<RgbaImage> {
    let status: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
    let status_res = status.clone();
    let path: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let path_res = path.clone();

    let match_rule = MatchRule::new_signal("org.freedesktop.portal.Request", "Response");
    conn.add_match(
        match_rule,
        move |response: OrgFreedesktopPortalRequestResponse, _conn, _msg| {
            if let Ok(mut status) = status.lock() {
                *status = Some(response.status);
            }

            let uri = response.results.get("uri").and_then(|str| str.as_str());
            if let (Some(uri_str), Ok(mut path)) = (uri, path.lock()) {
                *path = uri_str[7..].to_string();
            }

            true
        },
    )?;

    let proxy = conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(10000),
    );

    let mut options: PropMap = HashMap::new();
    options.insert(
        String::from("handle_token"),
        Variant(Box::new(String::from("1234"))),
    );
    options.insert(String::from("modal"), Variant(Box::new(true)));
    options.insert(String::from("interactive"), Variant(Box::new(false)));

    proxy.method_call::<(), _, _, _>(
        "org.freedesktop.portal.Screenshot",
        "Screenshot",
        ("", options),
    )?;

    // wait 60 seconds for user interaction
    for _ in 0..60 {
        let result = conn.process(Duration::from_millis(1000))?;
        let status = status_res
            .lock()
            .map_err(|_| anyhow!("Get status lock failed"))?;

        if result && status.is_some() {
            break;
        }
    }

    let status = status_res
        .lock()
        .map_err(|_| anyhow!("Get status lock failed"))?;
    let status = *status;

    let path = path_res
        .lock()
        .map_err(|_| anyhow!("Get path lock failed"))?;
    let path = &*path;

    if status.ne(&Some(0)) || path.is_empty() {
        if !path.is_empty() {
            let _ = fs::remove_file(path);
        }
        return Err(anyhow!("Screenshot failed or canceled",));
    }

    let filename = percent_decode(path.as_bytes()).decode_utf8()?.to_string();
    load_rgba_and_cleanup(&filename, x, y, width, height)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum CompositorHint {
    GnomeLike,
    PlasmaLike,
    WlrootsLike,
    Unknown,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ScreenshotStrategyKind {
    Gnome,
    Portal,
    WlRoots,
}

impl ScreenshotStrategyKind {
    fn label(&self) -> &'static str {
        match self {
            ScreenshotStrategyKind::Gnome => "gnome-shell",
            ScreenshotStrategyKind::Portal => "xdg-desktop-portal",
            ScreenshotStrategyKind::WlRoots => "wlroots",
        }
    }
}

const ORDER_WLROOTS: &[ScreenshotStrategyKind; 3] = &[
    ScreenshotStrategyKind::WlRoots,
    ScreenshotStrategyKind::Portal,
    ScreenshotStrategyKind::Gnome,
];
const ORDER_GNOME: &[ScreenshotStrategyKind; 3] = &[
    ScreenshotStrategyKind::Gnome,
    ScreenshotStrategyKind::Portal,
    ScreenshotStrategyKind::WlRoots,
];
const ORDER_PLASMA: &[ScreenshotStrategyKind; 3] = &[
    ScreenshotStrategyKind::Portal,
    ScreenshotStrategyKind::Gnome,
    ScreenshotStrategyKind::WlRoots,
];
const ORDER_UNKNOWN: &[ScreenshotStrategyKind; 3] = &[
    ScreenshotStrategyKind::Portal,
    ScreenshotStrategyKind::Gnome,
    ScreenshotStrategyKind::WlRoots,
];

fn backend_order_for_hint(hint: CompositorHint) -> &'static [ScreenshotStrategyKind] {
    match hint {
        CompositorHint::WlrootsLike => ORDER_WLROOTS,
        CompositorHint::GnomeLike => ORDER_GNOME,
        CompositorHint::PlasmaLike => ORDER_PLASMA,
        CompositorHint::Unknown => ORDER_UNKNOWN,
    }
}

fn compositor_env_tokens() -> Vec<String> {
    const CANDIDATE_VARS: [&str; 3] = [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ];
    CANDIDATE_VARS
        .iter()
        .filter_map(|key| env::var(key).ok())
        .flat_map(|value| {
            value
                .split([':', ';', ','])
                .flat_map(|part| part.split_whitespace())
                .map(|token| token.trim().to_lowercase())
                .collect::<Vec<String>>()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn compositor_hint() -> CompositorHint {
    let tokens = compositor_env_tokens();

    if tokens.iter().any(|token| token.contains("gnome")) {
        return CompositorHint::GnomeLike;
    }

    const WLROOTS_HINTS: [&str; 7] = [
        "sway", "river", "hyprland", "wayfire", "labwc", "niri", "cage",
    ]; // best-effort markers
    if tokens
        .iter()
        .any(|token| WLROOTS_HINTS.iter().any(|hint| token.contains(hint)))
        || env::var_os("SWAYSOCK").is_some()
        || env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        || env::var_os("WAYFIRE_SOCKET").is_some()
        || env::var_os("LABWC_SOCKET").is_some()
    {
        return CompositorHint::WlrootsLike;
    }

    if tokens
        .iter()
        .any(|token| token.contains("plasma") || token.contains("kde"))
    {
        return CompositorHint::PlasmaLike;
    }

    CompositorHint::Unknown
}

fn wlr_screenshot(
    x_coordinate: i32,
    y_coordinate: i32,
    width: i32,
    height: i32,
) -> Result<RgbaImage> {
    let wayshot_connection = WayshotConnection::new()?;
    let capture_region = CaptureRegion {
        x_coordinate,
        y_coordinate,
        width,
        height,
    };
    let rgba_image = wayshot_connection.screenshot(capture_region, false)?;

    Ok(rgba_image)
}

enum ScreenshotBackend<'a> {
    Gnome(&'a Connection),
    Portal(&'a Connection),
    WlRoots,
}

fn instantiate_backends<'a>(
    conn: &'a Connection,
    hint: CompositorHint,
) -> Vec<ScreenshotBackend<'a>> {
    backend_order_for_hint(hint)
        .iter()
        .map(|kind| match kind {
            ScreenshotStrategyKind::Gnome => ScreenshotBackend::Gnome(conn),
            ScreenshotStrategyKind::Portal => ScreenshotBackend::Portal(conn),
            ScreenshotStrategyKind::WlRoots => ScreenshotBackend::WlRoots,
        })
        .collect()
}

impl<'a> ScreenshotBackend<'a> {
    fn label(&self) -> &'static str {
        self.kind().label()
    }

    fn kind(&self) -> ScreenshotStrategyKind {
        match self {
            ScreenshotBackend::Gnome(_) => ScreenshotStrategyKind::Gnome,
            ScreenshotBackend::Portal(_) => ScreenshotStrategyKind::Portal,
            ScreenshotBackend::WlRoots => ScreenshotStrategyKind::WlRoots,
        }
    }

    fn capture(&self, x: i32, y: i32, width: i32, height: i32) -> Result<RgbaImage> {
        match self {
            ScreenshotBackend::Gnome(conn) => org_gnome_shell_screenshot(conn, x, y, width, height)
                .with_context(|| format!("{backend} backend failed", backend = self.label())),
            ScreenshotBackend::Portal(conn) => {
                org_freedesktop_portal_screenshot(conn, x, y, width, height)
                    .with_context(|| format!("{backend} backend failed", backend = self.label()))
            }
            ScreenshotBackend::WlRoots => wlr_screenshot(x, y, width, height)
                .with_context(|| format!("{backend} backend failed", backend = self.label())),
        }
    }
}

pub fn wayland_screenshot(x: i32, y: i32, width: i32, height: i32) -> Result<RgbaImage> {
    let conn = Connection::new_session()?;
    let compositor = compositor_hint();

    let backends: Vec<ScreenshotBackend> = instantiate_backends(&conn, compositor);

    let mut errors = Vec::with_capacity(backends.len());
    for backend in &backends {
        match backend.capture(x, y, width, height) {
            Ok(image) => return Ok(image),
            Err(err) => errors.push((backend.label(), err)),
        }
    }

    let summary = errors
        .iter()
        .map(|(label, err)| format!("{label}: {err}"))
        .collect::<Vec<_>>()
        .join(" | ");

    Err(anyhow!(
        "All Wayland screenshot strategies failed. Attempts: {summary}",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        sync::{Mutex, OnceLock},
    };

    const ENV_RESET_KEYS: [&str; 7] = [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
        "SWAYSOCK",
        "HYPRLAND_INSTANCE_SIGNATURE",
        "WAYFIRE_SOCKET",
        "LABWC_SOCKET",
    ];

    fn with_sanitized_env(vars: &[(&str, Option<&str>)], f: impl FnOnce()) {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();

        let mut keys = ENV_RESET_KEYS.to_vec();
        for (key, _) in vars {
            if !keys.contains(key) {
                keys.push(key);
            }
        }

        let snapshot: Vec<(String, Option<String>)> = keys
            .iter()
            .map(|key| ((*key).to_string(), env::var(key).ok()))
            .collect();

        for key in &keys {
            env::remove_var(key);
        }

        for (key, value) in vars {
            match value {
                Some(val) => env::set_var(key, val),
                None => env::remove_var(key),
            }
        }

        f();

        for (key, value) in snapshot {
            match value {
                Some(val) => env::set_var(&key, val),
                None => env::remove_var(&key),
            }
        }
    }

    #[test]
    fn compositor_hint_detects_gnome() {
        with_sanitized_env(&[("XDG_CURRENT_DESKTOP", Some("GNOME:Classic"))], || {
            assert_eq!(compositor_hint(), CompositorHint::GnomeLike);
        });
    }

    #[test]
    fn compositor_hint_detects_wlroots_tokens() {
        with_sanitized_env(&[("XDG_SESSION_DESKTOP", Some("sway"))], || {
            assert_eq!(compositor_hint(), CompositorHint::WlrootsLike);
        });
    }

    #[test]
    fn compositor_hint_detects_wlroots_env_markers() {
        with_sanitized_env(&[("HYPRLAND_INSTANCE_SIGNATURE", Some("abc"))], || {
            assert_eq!(compositor_hint(), CompositorHint::WlrootsLike);
        });
    }

    #[test]
    fn compositor_hint_detects_plasma() {
        with_sanitized_env(&[("DESKTOP_SESSION", Some("KDE:PLASMA"))], || {
            assert_eq!(compositor_hint(), CompositorHint::PlasmaLike);
        });
    }

    #[test]
    fn compositor_hint_defaults_to_unknown() {
        with_sanitized_env(&[], || {
            assert_eq!(compositor_hint(), CompositorHint::Unknown);
        });
    }

    #[test]
    fn backend_order_prefers_wlroots_when_hint_is_wlroots() {
        assert_eq!(
            backend_order_for_hint(CompositorHint::WlrootsLike),
            ORDER_WLROOTS
        );
    }

    #[test]
    fn backend_order_prefers_gnome_when_hint_is_gnome_like() {
        assert_eq!(
            backend_order_for_hint(CompositorHint::GnomeLike),
            ORDER_GNOME
        );
    }

    #[test]
    fn backend_order_prefers_portal_when_hint_is_plasma_or_unknown() {
        assert_eq!(
            backend_order_for_hint(CompositorHint::PlasmaLike),
            ORDER_PLASMA
        );
        assert_eq!(
            backend_order_for_hint(CompositorHint::Unknown),
            ORDER_UNKNOWN
        );
    }
}
