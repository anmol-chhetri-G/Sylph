//! Redraws asked for while GPUI draws, and the frame trace (plan card B2).
//!
//! GPUI 0.2.2 ignores `Window::refresh` during prepaint and paint (it
//! returns at once while drawing). What layout finds out (a caret-follow
//! scroll, a new page count) must ask for the next frame instead, or it
//! shows only after the next input event.

use gpui::Window;
use std::sync::OnceLock;

/// Draw the whole window again on the next frame. Safe in prepaint and
/// paint: next-frame callbacks run before the next draw, outside it.
pub(crate) fn next_frame(window: &Window) {
    window.on_next_frame(|window, _| window.refresh());
}

/// With `SYLPH_TRACE_FRAMES=1`, print `event` to stderr: formatting
/// toggles and paints with their content revision, to tell a wrong
/// decision from a late frame when bold "doesn't show".
pub(crate) fn trace(event: impl FnOnce() -> String) {
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("SYLPH_TRACE_FRAMES").is_some_and(|v| v == "1")) {
        eprintln!("[frame] {}", event());
    }
}

#[cfg(test)]
mod tests {
    /// `Window::refresh` does nothing while GPUI draws, so a call in
    /// prepaint or paint silently drops the redraw (B2). Draw code uses
    /// `redraw::next_frame`; handlers use `cx.notify()`.
    #[test]
    fn no_window_refresh_in_the_app() {
        let needle = concat!("window", ".refresh()");
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&src).expect("src dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("redraw.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("source file");
            for (n, line) in text.lines().enumerate() {
                if line.contains(needle) {
                    found.push(format!("{}:{}", path.display(), n + 1));
                }
            }
        }
        assert!(found.is_empty(), "use redraw::next_frame: {found:?}");
    }
}
