//! What to say when GPUI cannot open the window, instead of a panic.
//!
//! The known case: a laptop with Intel and NVIDIA GPUs under Wayland, where
//! NVIDIA's Vulkan driver fails `vkCreateWaylandSurfaceKHR` with
//! `ERROR_OUT_OF_HOST_MEMORY`. The Vulkan loader asks every installed
//! driver to create the surface, so choosing the Intel GPU is not enough:
//! NVIDIA's driver must be hidden from the process.

/// The message for a failed `open_window`, with the fix when the GPU
/// driver could not create a surface for the display.
pub(crate) fn message(error: &str) -> String {
    let mut out = format!("Sylph could not open its window: {error}");
    if error.contains("create surface") || error.contains("ERROR_OUT_OF_HOST_MEMORY") {
        out.push_str(
            "\n\nThe GPU driver could not draw to this display. If this computer \
             also has an Intel or AMD GPU, run Sylph without NVIDIA's Vulkan driver:\n\
             \n    VK_LOADER_DRIVERS_DISABLE='*nvidia*' cargo run -p sylph-desktop\n\
             \n`VK_LOADER_DRIVERS_DISABLE='*nvidia*' vulkaninfo --summary` shows \
             which GPU that leaves (scripts/doctor.sh checks this too).",
        );
    }
    out
}

/// Print `message` for `error` and exit with status 1.
pub(crate) fn exit(error: impl std::fmt::Display) -> ! {
    eprintln!("{}", message(&format!("{error:#}")));
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::message;

    #[test]
    fn a_surface_failure_says_how_to_run_on_the_other_gpu() {
        // As GPUI 0.2.2 reported it on the author's Intel + RTX 4050 laptop.
        let m = message("Failed to create surface: Platform(Init(ERROR_OUT_OF_HOST_MEMORY))");
        assert!(m.starts_with("Sylph could not open its window: Failed to create surface"));
        assert!(m.contains("VK_LOADER_DRIVERS_DISABLE='*nvidia*'"), "{m}");
        // Other failures are reported as they are, without a wrong hint.
        let m = message("no display: WAYLAND_DISPLAY and DISPLAY are unset");
        assert!(!m.contains("nvidia"), "{m}");
    }
}
