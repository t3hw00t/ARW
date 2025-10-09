use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::{extract::State, Json};
use serde_json::{json, Value};
use sysinfo::System;

use crate::{admin_ok, util, AppState};
use arw_topics as topics;

// Optional Windows DXCore interop for NPU detection (opt-in).
#[cfg(all(target_os = "windows", feature = "npu_dxcore"))]
mod win_npu_dxcore {
    #![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
    use serde_json::json;
    use windows::Win32::Graphics::DxCore::*;

    pub fn probe() -> Vec<serde_json::Value> {
        unsafe {
            let mut out: Vec<serde_json::Value> = Vec::new();
            let Ok(factory) = CreateDXCoreAdapterFactory() else {
                return out;
            };
            let attrs = [DXCORE_ADAPTER_ATTRIBUTE_D3D12_CORE_COMPUTE];
            let Ok(list) = factory.CreateAdapterList(&attrs) else {
                return out;
            };
            let count = list.GetAdapterCount();
            for i in 0..count {
                if let Ok(adapter) = list.GetAdapter(i) {
                    if adapter.IsAttributeSupported(&DXCORE_ADAPTER_ATTRIBUTE_D3D12_CORE_COMPUTE) {
                        // vendor/device ids when available
                        let mut ven = 0u32;
                        let mut dev = 0u32;
                        let mut sz: usize = 0;
                        if adapter.IsPropertySupported(DXCoreAdapterProperty::HardwareID) {
                            if adapter
                                .GetPropertySize(DXCoreAdapterProperty::HardwareID, &mut sz)
                                .is_ok()
                                && sz >= std::mem::size_of::<DXCoreHardwareID>()
                            {
                                let mut hwid: DXCoreHardwareID = std::mem::zeroed();
                                if adapter
                                    .GetProperty(
                                        DXCoreAdapterProperty::HardwareID,
                                        std::mem::size_of::<DXCoreHardwareID>(),
                                        &mut hwid as *mut _ as *mut std::ffi::c_void,
                                    )
                                    .is_ok()
                                {
                                    ven = hwid.VendorID;
                                    dev = hwid.DeviceID;
                                }
                            }
                        }
                        out.push(json!({
                            "index": format!("adapter{}", i),
                            "vendor_id": format!("0x{:04x}", ven),
                            "device_id": format!("0x{:04x}", dev),
                        }));
                    }
                }
            }
            out
        }
    }
}

fn unauthorized() -> Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

/// Effective path probe (successor to `/admin/probe`).
#[utoipa::path(
    get,
    path = "/admin/probe",
    tag = "Admin/Introspect",
    responses(
        (status = 200, description = "Effective paths", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn probe_effective_paths(
    headers: HeaderMap,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let ep = arw_core::load_effective_paths();
    Json(ep).into_response()
}

/// Hardware/software probe (`/admin/probe/hw`).
#[utoipa::path(
    get,
    path = "/admin/probe/hw",
    tag = "Admin/Introspect",
    responses(
        (status = 200, description = "Hardware and software info", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn probe_hw(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }

    let mut sys = System::new_all();
    sys.refresh_all();

    // CPU
    let cpus_logical = sys.cpus().len() as u64;
    let cpus_physical = sys.physical_core_count().unwrap_or(0) as u64;
    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();

    // Memory (bytes)
    let total_mem = sys.total_memory();
    let avail_mem = sys.available_memory();

    // OS
    let info = os_info::get();
    let os_name = info.os_type().to_string();
    let os_version = info.version().to_string();
    let kernel = System::kernel_version().unwrap_or_default();
    let arch = std::env::consts::ARCH.to_string();

    // Disks (system view, not just app paths)
    let disks: Vec<Value> = probe_disks_best_effort();

    // Boot/virt/container hints (Linux-only paths are best-effort)
    let mut boot = serde_json::Map::new();
    boot.insert(
        "uefi".into(),
        Value::Bool(std::path::Path::new("/sys/firmware/efi").exists()),
    );
    let mut virt = serde_json::Map::new();
    virt.insert(
        "hypervisor_flag".into(),
        Value::Bool(read_cpuinfo_has_flag("hypervisor")),
    );
    if let Some(pname) = read_small("/sys/devices/virtual/dmi/id/product_name") {
        virt.insert("product_name".into(), Value::String(pname));
    }
    let mut container = serde_json::Map::new();
    container.insert(
        "dockerenv".into(),
        Value::Bool(std::path::Path::new("/.dockerenv").exists()),
    );
    container.insert(
        "containerenv".into(),
        Value::Bool(std::path::Path::new("/run/.containerenv").exists()),
    );
    if let Ok(v) = std::env::var("container") {
        container.insert("env".into(), Value::String(v));
    }
    let wsl = read_small("/proc/sys/kernel/osrelease")
        .map(|s| s.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false);

    // Env hints
    let mut env = serde_json::Map::new();
    for k in [
        "CUDA_VISIBLE_DEVICES",
        "NVIDIA_VISIBLE_DEVICES",
        "ROCR_VISIBLE_DEVICES",
        "HSA_VISIBLE_DEVICES",
    ] {
        if let Ok(v) = std::env::var(k) {
            env.insert(k.to_string(), Value::String(v));
        }
    }

    // GPUs / NPUs
    let gpus = probe_gpus_best_effort();
    let npus = probe_npus_best_effort();
    #[cfg(feature = "gpu_wgpu")]
    let gpus_wgpu = probe_gpus_wgpu();
    #[cfg(not(feature = "gpu_wgpu"))]
    let gpus_wgpu: Vec<Value> = Vec::new();
    #[cfg(feature = "gpu_nvml")]
    let gpus_nvml = probe_gpu_nvml();
    #[cfg(not(feature = "gpu_nvml"))]
    let gpus_nvml: Vec<Value> = Vec::new();

    let out = json!({
        "cpu": {"brand": cpu_brand, "logical": cpus_logical, "physical": cpus_physical, "features": cpu_features()},
        "memory": {"total": total_mem, "available": avail_mem},
        "os": {"name": os_name, "version": os_version, "kernel": kernel, "arch": arch},
        "disks": disks,
        "boot": boot,
        "virt": virt,
        "container": container,
        "wsl": wsl,
        "env": env,
        "gpus": gpus,
        "gpus_wgpu": gpus_wgpu,
        "gpus_nvml": gpus_nvml,
        "npus": npus,
    });

    state.bus().publish(
        topics::TOPIC_PROBE_HW,
        &json!({"cpus": cpus_logical, "gpus": out["gpus"].as_array().map(|a| a.len()).unwrap_or(0)}),
    );
    Json(out).into_response()
}

/// Metrics snapshot probe (`/admin/probe/metrics`).
#[utoipa::path(
    get,
    path = "/admin/probe/metrics",
    tag = "Admin/Introspect",
    responses(
        (status = 200, description = "System metrics", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn probe_metrics(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let out = collect_metrics_snapshot().await;
    state.bus().publish(
        topics::TOPIC_PROBE_METRICS,
        &json!({
            "cpu": out["cpu"]["avg"],
            "mem": {
                "used": out["memory"]["used"],
                "total": out["memory"]["total"]
            },
            "disk": out["disk"],
            "gpus": out["gpus"],
            "npus": out["npus"],
        }),
    );
    Json(out).into_response()
}

async fn collect_metrics_snapshot() -> Value {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu();
    tokio::time::sleep(std::time::Duration::from_millis(180)).await;
    sys.refresh_cpu();
    let per_core: Vec<f64> = sys.cpus().iter().map(|c| c.cpu_usage() as f64).collect();
    let avg = if per_core.is_empty() {
        0.0
    } else {
        per_core.iter().sum::<f64>() / (per_core.len() as f64)
    };
    let total_mem = sys.total_memory();
    let avail_mem = sys.available_memory();
    let used_mem = total_mem.saturating_sub(avail_mem);
    let swap_total = sys.total_swap();
    let swap_used = sys.used_swap();
    let sdir = util::state_dir();
    let (disk_total, disk_avail) = (
        fs2::total_space(&sdir).unwrap_or(0),
        fs2::available_space(&sdir).unwrap_or(0),
    );
    let gpus = probe_gpu_metrics_best_effort_async().await;
    let npus = probe_npus_best_effort();
    json!({
        "cpu": {"avg": avg, "per_core": per_core},
        "memory": {"total": total_mem, "used": used_mem, "available": avail_mem, "swap_total": swap_total, "swap_used": swap_used},
        "disk": {"state_dir": sdir, "total": disk_total, "available": disk_avail},
        "gpus": gpus,
        "npus": npus,
    })
}

#[cfg(target_os = "linux")]
fn probe_gpus_best_effort() -> Vec<Value> {
    probe_gpus_linux()
}

#[cfg(target_os = "windows")]
fn probe_gpus_best_effort() -> Vec<Value> {
    use serde_json::json;
    use std::os::windows::ffi::OsStringExt as _;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    unsafe {
        let factory: IDXGIFactory1 = match CreateDXGIFactory1::<IDXGIFactory1>() {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        let mut i: u32 = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            if let Ok(desc) = adapter.GetDesc1() {
                let wname = &desc.Description;
                let len = wname.iter().position(|&c| c == 0).unwrap_or(wname.len());
                let name = std::ffi::OsString::from_wide(&wname[..len])
                    .to_string_lossy()
                    .to_string();
                out.push(json!({
                    "name": name,
                    "vendor_id": format!("0x{:04x}", desc.VendorId),
                    "device_id": format!("0x{:04x}", desc.DeviceId),
                    "dedicated_vram": (desc.DedicatedVideoMemory as u64),
                }));
            }
            i += 1;
        }
        out
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn probe_gpus_best_effort() -> Vec<Value> {
    Vec::new()
}

#[cfg(feature = "gpu_wgpu")]
fn probe_gpus_wgpu() -> Vec<Value> {
    use serde_json::json;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let mut out = Vec::new();
    for adapter in instance.enumerate_adapters(wgpu::Backends::all()) {
        let info = adapter.get_info();
        let backend = match info.backend {
            wgpu::Backend::Empty => "empty",
            wgpu::Backend::Vulkan => "vulkan",
            wgpu::Backend::Metal => "metal",
            wgpu::Backend::Dx12 => "dx12",
            wgpu::Backend::Gl => "gl",
            wgpu::Backend::BrowserWebGpu => "webgpu",
        };
        out.push(json!({
            "name": info.name,
            "vendor": format!("0x{:04x}", info.vendor),
            "device": format!("0x{:04x}", info.device),
            "device_type": format!("{:?}", info.device_type).to_lowercase(),
            "backend": backend,
        }));
    }
    out
}

#[cfg(feature = "gpu_nvml")]
fn probe_gpu_nvml() -> Vec<Value> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn probe_gpus_linux() -> Vec<Value> {
    use std::fs;
    use std::path::Path;
    let mut out: Vec<Value> = Vec::new();
    let drm = Path::new("/sys/class/drm");
    if let Ok(entries) = fs::read_dir(drm) {
        for ent in entries.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if !name.starts_with("card") || name.contains('-') {
                continue; // skip renderD* and control* symlinks
            }
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            let dev = path.join("device");
            let vendor = fs::read_to_string(dev.join("vendor")).unwrap_or_default();
            let device = fs::read_to_string(dev.join("device")).unwrap_or_default();
            let vendor = vendor.trim().to_string();
            let device = device.trim().to_string();
            let vendor_name = match vendor.as_str() {
                "0x10de" => "NVIDIA",
                "0x1002" => "AMD",
                "0x8086" => "Intel",
                _ => "Unknown",
            };
            // PCI bus id from uevent
            let mut pci_bus = String::new();
            if let Ok(ue) = fs::read_to_string(dev.join("uevent")) {
                for line in ue.lines() {
                    if let Some(val) = line.strip_prefix("PCI_SLOT_NAME=") {
                        pci_bus = val.trim().to_string();
                        break;
                    }
                }
            }
            // driver name
            let mut driver = String::new();
            if let Ok(link) = fs::read_link(dev.join("driver")) {
                if let Some(b) = link.file_name() {
                    driver = b.to_string_lossy().to_string();
                }
            }
            // Extra per-vendor hints
            let mut model = String::new();
            let mut vram_total: Option<u64> = None;
            // NVIDIA: parse /proc/driver/nvidia/gpus/<pci>/information
            if vendor == "0x10de" && !pci_bus.is_empty() {
                let info_path = format!("/proc/driver/nvidia/gpus/{}/information", pci_bus);
                if let Ok(body) = fs::read_to_string(&info_path) {
                    for line in body.lines() {
                        if let Some(val) = line.strip_prefix("Model:") {
                            model = val.trim().to_string();
                        }
                        if let Some(val) = line.strip_prefix("FB Memory Total:") {
                            let txt = val.trim();
                            let parts: Vec<&str> = txt.split_whitespace().collect();
                            if parts.len() >= 2 {
                                if let Ok(num) = parts[0].parse::<u64>() {
                                    let bytes = match parts[1].to_ascii_lowercase().as_str() {
                                        "mib" => num * 1024 * 1024,
                                        "gib" => num * 1024 * 1024 * 1024,
                                        _ => 0,
                                    };
                                    if bytes > 0 {
                                        vram_total = Some(bytes);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // AMD: try mem_info_vram_total
            if vendor == "0x1002" {
                let vpath = dev.join("mem_info_vram_total");
                if let Ok(s) = fs::read_to_string(&vpath) {
                    if let Ok(num) = s.trim().parse::<u64>() {
                        vram_total = Some(num);
                    }
                }
                let name_path = dev.join("product_name");
                if model.is_empty() {
                    if let Ok(s) = fs::read_to_string(&name_path) {
                        model = s.trim().to_string();
                    }
                }
            }
            out.push(json!({
                "index": name,
                "vendor_id": vendor,
                "vendor": vendor_name,
                "device_id": device,
                "pci_bus": pci_bus,
                "driver": driver,
                "model": model,
                "vram_total": vram_total,
            }));
        }
    }
    out
}

fn read_small(p: &str) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

fn read_cpuinfo_has_flag(flag: &str) -> bool {
    if let Ok(body) = std::fs::read_to_string("/proc/cpuinfo") {
        for line in body.lines() {
            if let Some(rest) = line.strip_prefix("flags") {
                if rest.contains(flag) {
                    return true;
                }
            }
        }
    }
    false
}

fn cpu_features() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse4.2") {
            out.push("sse4.2".into());
        }
        if std::is_x86_feature_detected!("avx") {
            out.push("avx".into());
        }
        if std::is_x86_feature_detected!("avx2") {
            out.push("avx2".into());
        }
        if std::is_x86_feature_detected!("fma") {
            out.push("fma".into());
        }
        if std::is_x86_feature_detected!("aes") {
            out.push("aes".into());
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            out.push("neon".into());
        }
        if std::arch::is_aarch64_feature_detected!("asimd") {
            out.push("asimd".into());
        }
        if std::arch::is_aarch64_feature_detected!("pmull") {
            out.push("pmull".into());
        }
        if std::arch::is_aarch64_feature_detected!("aes") {
            out.push("aes".into());
        }
        if std::arch::is_aarch64_feature_detected!("sha2") {
            out.push("sha2".into());
        }
        if std::arch::is_aarch64_feature_detected!("sha3") {
            out.push("sha3".into());
        }
    }
    out
}

fn probe_disks_best_effort() -> Vec<Value> {
    #[cfg(target_os = "linux")]
    {
        return probe_disks_linux();
    }
    #[cfg(target_os = "macos")]
    {
        return probe_disks_macos();
    }
    #[cfg(target_os = "windows")]
    {
        return probe_disks_windows();
    }
    #[allow(unreachable_code)]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn probe_disks_linux() -> Vec<Value> {
    use std::collections::HashSet;
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut out: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let allowed_fs: HashSet<&str> = [
        "ext4", "ext3", "ext2", "xfs", "btrfs", "zfs", "f2fs", "reiserfs", "ntfs", "vfat", "exfat",
        "overlay",
    ]
    .into_iter()
    .collect();
    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let mount = parts[1];
        let fstype = parts[2];
        if !allowed_fs.contains(fstype) && mount != "/" {
            continue;
        }
        if seen.contains(mount) {
            continue;
        }
        seen.insert(mount.to_string());
        let p = std::path::Path::new(mount);
        let (total, avail) = (
            fs2::total_space(p).unwrap_or(0),
            fs2::available_space(p).unwrap_or(0),
        );
        out.push(json!({"mount": mount, "total": total, "available": avail}));
    }
    out
}

#[cfg(target_os = "macos")]
fn probe_disks_macos() -> Vec<Value> {
    let paths = ["/", "/System/Volumes/Data"]
        .iter()
        .filter_map(|p| {
            let pb = std::path::Path::new(p);
            if std::fs::metadata(&pb).is_ok() {
                Some(pb.to_path_buf())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    for p in paths {
        let (total, avail) = (
            fs2::total_space(&p).unwrap_or(0),
            fs2::available_space(&p).unwrap_or(0),
        );
        out.push(json!({"mount": p.display().to_string(), "total": total, "available": avail}));
    }
    out
}

#[cfg(target_os = "windows")]
fn probe_disks_windows() -> Vec<Value> {
    let mut out = Vec::new();
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        let p = std::path::Path::new(&root);
        if std::fs::metadata(p).is_ok() {
            let total = fs2::total_space(p).unwrap_or(0);
            let avail = fs2::available_space(p).unwrap_or(0);
            if total > 0 {
                out.push(json!({"mount": root, "total": total, "available": avail}));
            }
        }
    }
    out
}

#[cfg(target_os = "linux")]
async fn probe_gpu_metrics_best_effort_async() -> Vec<Value> {
    let mut base = probe_gpu_metrics_best_effort();
    if crate::util::env_bool("ARW_ROCM_SMI").unwrap_or(false) {
        if let Some(extra) = rocm_smi_json().await {
            if let Some(obj) = extra.as_object() {
                for (k, v) in obj.iter() {
                    if !k.starts_with("card") {
                        continue;
                    }
                    if let Some(gpu) = base.iter_mut().find(|g| g["index"].as_str() == Some(k)) {
                        if let Some(map) = v.as_object() {
                            if gpu["busy_percent"].is_null() {
                                if let Some(bp) = pick_number(
                                    map,
                                    &["GPU use (%)", "GPU Utilization (%)", "GPU_Util"],
                                ) {
                                    gpu["busy_percent"] = json!(bp as u64);
                                }
                            }
                            if gpu["mem_total"].is_null() {
                                if let Some(mt) =
                                    pick_number(map, &["VRAM Total (B)", "VRAM_Total_Bytes"])
                                {
                                    gpu["mem_total"] = json!(mt as u64);
                                }
                            }
                            if gpu["mem_used"].is_null() {
                                if let Some(mu) =
                                    pick_number(map, &["VRAM Used (B)", "VRAM_Used_Bytes"])
                                {
                                    gpu["mem_used"] = json!(mu as u64);
                                }
                            }
                            gpu["extra"]["rocm_smi"] = v.clone();
                        }
                    } else {
                        base.push(json!({"index": k, "vendor":"AMD","vendor_id":"0x1002","extra": {"rocm_smi": v}}));
                    }
                }
            }
        }
    }
    base
}

#[cfg(not(target_os = "linux"))]
async fn probe_gpu_metrics_best_effort_async() -> Vec<Value> {
    probe_gpu_metrics_best_effort()
}

#[cfg(target_os = "linux")]
async fn rocm_smi_json() -> Option<Value> {
    use tokio::time::{timeout, Duration};
    let mut cmd = tokio::process::Command::new("rocm-smi");
    cmd.arg("--showuse")
        .arg("--showmeminfo")
        .arg("vram")
        .arg("--showtemp")
        .arg("--showclocks")
        .arg("--showpower")
        .arg("--json");
    match timeout(Duration::from_millis(1200), cmd.output()).await {
        Ok(Ok(out)) if out.status.success() => {
            let txt = String::from_utf8_lossy(&out.stdout);
            serde_json::from_str::<Value>(&txt).ok()
        }
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn pick_number(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    for k in keys {
        if let Some(v) = map.get(*k) {
            if v.is_number() {
                return v.as_f64();
            }
            if let Some(s) = v.as_str() {
                let s = s.trim_end_matches('%');
                if let Ok(x) = s.parse::<f64>() {
                    return Some(x);
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn probe_gpu_metrics_best_effort() -> Vec<Value> {
    use std::fs;
    use std::path::Path;
    let mut out: Vec<Value> = Vec::new();
    let drm = Path::new("/sys/class/drm");
    if let Ok(entries) = fs::read_dir(drm) {
        for ent in entries.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if !name.starts_with("card") || name.contains('-') {
                continue;
            }
            let dev = ent.path().join("device");
            let vendor = fs::read_to_string(dev.join("vendor")).unwrap_or_default();
            let vendor = vendor.trim().to_string();
            let vendor_name = match vendor.as_str() {
                "0x10de" => "NVIDIA",
                "0x1002" => "AMD",
                "0x8086" => "Intel",
                _ => "Unknown",
            };
            let mut mem_used = None;
            let mut mem_total = None;
            let mut busy = None;
            if vendor == "0x1002" {
                if let Ok(s) = fs::read_to_string(dev.join("mem_info_vram_used")) {
                    if let Ok(n) = s.trim().parse::<u64>() {
                        mem_used = Some(n);
                    }
                }
                if let Ok(s) = fs::read_to_string(dev.join("mem_info_vram_total")) {
                    if let Ok(n) = s.trim().parse::<u64>() {
                        mem_total = Some(n);
                    }
                }
                if let Ok(s) = fs::read_to_string(dev.join("gpu_busy_percent")) {
                    if let Ok(n) = s.trim().parse::<u64>() {
                        busy = Some(n);
                    }
                }
            }
            out.push(json!({
                "index": name,
                "vendor": vendor_name,
                "vendor_id": vendor,
                "mem_used": mem_used,
                "mem_total": mem_total,
                "busy_percent": busy,
            }));
        }
    }
    out
}

#[cfg(target_os = "windows")]
fn probe_gpu_metrics_best_effort() -> Vec<Value> {
    use serde_json::json;
    use std::os::windows::ffi::OsStringExt as _;
    use windows::core::Interface as _;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory1, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
        DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
    };
    unsafe {
        let factory: IDXGIFactory1 = match CreateDXGIFactory1::<IDXGIFactory1>() {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        let mut i: u32 = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            if let Ok(desc) = adapter.GetDesc1() {
                let wname = &desc.Description;
                let len = wname.iter().position(|&c| c == 0).unwrap_or(wname.len());
                let name = std::ffi::OsString::from_wide(&wname[..len])
                    .to_string_lossy()
                    .to_string();
                let mut used_local: Option<u64> = None;
                if let Ok(adapter3) = adapter.cast::<IDXGIAdapter3>() {
                    let mut info: DXGI_QUERY_VIDEO_MEMORY_INFO = std::mem::zeroed();
                    if adapter3
                        .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                        .is_ok()
                    {
                        used_local = Some(info.CurrentUsage as u64);
                    }
                    if used_local.is_none() {
                        let mut info2: DXGI_QUERY_VIDEO_MEMORY_INFO = std::mem::zeroed();
                        if adapter3
                            .QueryVideoMemoryInfo(
                                0,
                                DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
                                &mut info2,
                            )
                            .is_ok()
                        {
                            used_local = Some(info2.CurrentUsage as u64);
                        }
                    }
                }
                out.push(json!({
                    "index": format!("adapter{}", i),
                    "vendor": "windows",
                    "vendor_id": format!("0x{:04x}", desc.VendorId),
                    "name": name,
                    "mem_total": (desc.DedicatedVideoMemory as u64),
                    "mem_used": used_local,
                }));
            }
            i += 1;
        }
        out
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn probe_gpu_metrics_best_effort() -> Vec<Value> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn probe_npus_best_effort() -> Vec<Value> {
    use std::fs;
    use std::path::Path;
    let mut out: Vec<Value> = Vec::new();
    let accel = Path::new("/sys/class/accel");
    if let Ok(entries) = fs::read_dir(accel) {
        for ent in entries.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            let dev = ent.path().join("device");
            let vendor = fs::read_to_string(dev.join("vendor")).unwrap_or_default();
            let device = fs::read_to_string(dev.join("device")).unwrap_or_default();
            let vendor = vendor.trim().to_string();
            let device = device.trim().to_string();
            let mut driver = String::new();
            if let Ok(link) = fs::read_link(dev.join("driver")) {
                if let Some(b) = link.file_name() {
                    driver = b.to_string_lossy().to_string();
                }
            }
            let mut pci_bus = String::new();
            if let Ok(ue) = fs::read_to_string(dev.join("uevent")) {
                for line in ue.lines() {
                    if let Some(val) = line.strip_prefix("PCI_SLOT_NAME=") {
                        pci_bus = val.trim().to_string();
                        break;
                    }
                }
            }
            out.push(json!({
                "index": name,
                "vendor_id": vendor,
                "device_id": device,
                "driver": driver,
                "pci_bus": pci_bus,
            }));
        }
    }
    if let Ok(mods) = std::fs::read_to_string("/proc/modules") {
        let has_intel_vpu = mods.lines().any(|l| l.starts_with("intel_vpu "));
        let has_amd_xdna = mods.lines().any(|l| l.starts_with("amdxdna "));
        if has_intel_vpu || has_amd_xdna {
            out.push(json!({"modules": {"intel_vpu": has_intel_vpu, "amdxdna": has_amd_xdna}}));
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn probe_npus_best_effort() -> Vec<Value> {
    let mut out = Vec::new();
    if std::env::consts::ARCH == "aarch64" {
        out.push(json!({
            "vendor": "Apple",
            "name": "Neural Engine",
            "present": true
        }));
    }
    out
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn probe_npus_best_effort() -> Vec<Value> {
    #[cfg(all(target_os = "windows", feature = "npu_dxcore"))]
    {
        if crate::util::env_bool("ARW_DXCORE_NPU").unwrap_or(false) {
            return win_npu_dxcore::probe();
        }
    }
    Vec::new()
}
