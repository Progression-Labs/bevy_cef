//! Whether CEF's GPU process can paint pages on the GPU here (Linux `CefPlugin::gpu_paint`),
//! checked before CEF starts: when it cannot, Chromium restarts the GPU process until it gives
//! up and exits the whole app ("GPU process isn't usable. Goodbye."), stalling the app's main
//! thread on every restart until then.
//!
//! `gpu_paint` runs the GPU process on Wayland ozone (ANGLE on GL/EGL), so it needs the
//! compositor's socket.

use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use bevy::log::warn;
use bevy_cef_core::prelude::CommandLineConfig;

/// The switches the GPU path runs under, each replacing any value the app gave it (Chromium
/// keeps only the last value of a repeated switch).
///
/// - `ozone-platform=wayland`, `use-angle=gl-egl`: only Wayland ozone with ANGLE on GL/EGL hands
///   NVIDIA's tiled buffers over with the page in them: under X11 (XWayland) EGL rejects them
///   and Vulkan leaves them empty.
/// - `use-webgpu-adapter=opengles`: on Wayland ozone Chromium still starts Vulkan in the GPU
///   process for WebGPU (WebGPU on Vulkan through GL interop, on by default on Linux) and logs
///   an ERROR on every start, "'--ozone-platform=wayland' is not compatible with Vulkan".
///   `disable-features=Vulkan` does not stop it (that feature is already off); only taking
///   WebGPU off Vulkan does (Chrome 157, measured by the log line). Its price: a page's WebGPU
///   runs on Dawn's OpenGL ES backend, WebGPU's compatibility subset, instead of on Vulkan.
pub(crate) const SWITCHES: [(&str, &str); 3] = [
    ("ozone-platform", "wayland"),
    ("use-angle", "gl-egl"),
    ("use-webgpu-adapter", "opengles"),
];

/// Puts [`SWITCHES`] in `config`, each in place of the app's own value (said once if replaced).
pub(crate) fn apply_switches(config: &mut CommandLineConfig) {
    let values = &mut config.switch_values;
    for (name, value) in SWITCHES {
        if let Some(old) = values.iter().find(|(n, v)| *n == name && *v != value) {
            warn!("gpu_paint: {name}={} replaced by {name}={value}", old.1);
        }
        values.retain(|(n, _)| *n != name);
        values.push((name, value));
    }
}

/// Why the GPU process could not paint here, if it could not.
pub(crate) fn preflight() -> Result<(), String> {
    let display = std::env::var("WAYLAND_DISPLAY").map_err(|_| {
        "WAYLAND_DISPLAY is not set; gpu_paint's Wayland ozone needs it".to_string()
    })?;
    let socket = if display.starts_with('/') {
        PathBuf::from(&display)
    } else {
        let runtime = std::env::var("XDG_RUNTIME_DIR").map_err(|_| {
            "XDG_RUNTIME_DIR is not set; the Wayland socket lives there".to_string()
        })?;
        PathBuf::from(runtime).join(&display)
    };
    UnixStream::connect(&socket).map(drop).map_err(|e| {
        format!(
            "cannot reach the Wayland compositor at {}: {e}",
            socket.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The GPU path takes WebGPU off Vulkan (the cause of Chromium's Wayland/Vulkan ERROR) and
    /// its switches win over the app's.
    #[test]
    fn the_gpu_path_keeps_webgpu_off_vulkan_and_its_values_win() {
        let mut config = CommandLineConfig::default()
            .with_switch_value("ozone-platform", "x11")
            .with_switch_value("autoplay-policy", "no-user-gesture-required");
        apply_switches(&mut config);
        let value = |name: &str| {
            let all: Vec<_> = config
                .switch_values
                .iter()
                .filter(|(n, _)| *n == name)
                .collect();
            assert_eq!(all.len(), 1, "{name} given once: {all:?}");
            all[0].1
        };
        assert_eq!(value("use-webgpu-adapter"), "opengles");
        assert_eq!(value("ozone-platform"), "wayland");
        assert_eq!(value("use-angle"), "gl-egl");
        assert_eq!(value("autoplay-policy"), "no-user-gesture-required");
    }
}
