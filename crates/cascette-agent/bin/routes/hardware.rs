//! GET /hardware -- System hardware information.
//!
//! The real agent reports CPU, memory, GPU, and OS info via Win32 APIs.
//! We use the sysinfo crate for cross-platform CPU/memory/OS detection
//! and platform-specific GPU probes:
//!
//! - Linux: `/sys/class/drm` sysfs (fast, no subprocess) with `lspci -mm`
//!   fallback for human-readable vendor/device names.
//! - macOS: `system_profiler SPDisplaysDataType -json`
//! - Windows: `wmic path win32_VideoController get Name /format:list`
//!
//! All GPU probes are best-effort; an empty string is returned when
//! detection fails or the platform has no known probe path.

use axum::Json;

use cascette_agent::handlers::hardware;

/// GET /hardware
///
/// Returns hardware information in Agent.exe wire format.
///
/// Serializes a `Hardware` protobuf message to JSON with flat fields:
/// `cpu_arch`, `cpu_num_cores`, `cpu_speed`, `memory`, `num_gpus`,
/// `gpu_1`/`gpu_2`/`gpu_3` (each a `Gpu` sub-message), `cpu_vendor`, `cpu_brand`.
pub async fn get_hardware() -> Json<serde_json::Value> {
    Json(hardware::hardware().await)
}
