//! Progress reporting module for Tauri GUI.

#[cfg(feature = "gui")]
use once_cell::sync::OnceCell;
#[cfg(feature = "gui")]
use serde_json::json;
#[cfg(feature = "gui")]
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(feature = "gui")]
use tauri::{Emitter, WebviewWindow};

#[cfg(feature = "gui")]
pub static MAIN_WINDOW: OnceCell<WebviewWindow> = OnceCell::new();

#[cfg(feature = "gui")]
static PROGRESS_FLOOR: AtomicU32 = AtomicU32::new(0);

#[cfg(feature = "gui")]
static SUPPRESS_COUNT: AtomicU32 = AtomicU32::new(0);

/// Mutes/unmutes progress emits; used by the 3D terrain preview fetches.
pub fn set_progress_suppressed(_suppressed: bool) {
    #[cfg(feature = "gui")]
    {
        if _suppressed {
            SUPPRESS_COUNT.fetch_add(1, Ordering::Relaxed);
        } else {
            let mut v = SUPPRESS_COUNT.load(Ordering::Relaxed);
            while let Err(now) = SUPPRESS_COUNT.compare_exchange_weak(
                v,
                v.saturating_sub(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                v = now;
            }
        }
    }
}

#[cfg(feature = "gui")]
fn emits_suppressed(progress: f64, message: &str) -> bool {
    SUPPRESS_COUNT.load(Ordering::Relaxed) > 0 && progress < 100.0 && !message.starts_with("Error!")
}

/// Resets the monotonic progress floor at the start of a generation.
pub fn reset_progress_floor() {
    #[cfg(feature = "gui")]
    PROGRESS_FLOOR.store(0, Ordering::Relaxed);
}

pub const MESSAGE_ONLY: f64 = -1.0;

#[cfg(feature = "gui")]
fn clamp_progress(progress: f64) -> f64 {
    if progress <= 0.0 {
        return progress;
    }
    let p = (progress * 100.0) as u32;
    let prev = PROGRESS_FLOOR.fetch_max(p, Ordering::Relaxed);
    f64::from(prev.max(p)) / 100.0
}

#[cfg(feature = "gui")]
pub fn set_main_window(window: WebviewWindow) {
    MAIN_WINDOW.set(window).ok();
}

#[cfg(feature = "gui")]
pub fn get_main_window() -> Option<&'static WebviewWindow> {
    MAIN_WINDOW.get()
}

pub fn is_running_with_gui() -> bool {
    #[cfg(feature = "gui")]
    {
        get_main_window().is_some()
    }
    #[cfg(not(feature = "gui"))]
    {
        false
    }
}

pub fn emit_gui_progress_update(_progress: f64, _message: &str) {
    #[cfg(feature = "gui")]
    {
        if emits_suppressed(_progress, _message) {
            return;
        }
        if let Some(window) = get_main_window() {
            let payload = json!({
                "progress": clamp_progress(_progress),
                "message": _message
            });
            if let Err(e) = window.emit("progress-update", payload) {
                eprintln!("Failed to emit progress event: {e}");
            }
        }
    }
}

pub fn emit_gui_progress_update_ex(_progress: f64, _message: &str, _streaming: bool) {
    #[cfg(feature = "gui")]
    {
        if emits_suppressed(_progress, _message) {
            return;
        }
        if let Some(window) = get_main_window() {
            let payload = json!({
                "progress": clamp_progress(_progress),
                "message": _message,
                "streaming": _streaming
            });
            if let Err(e) = window.emit("progress-update", payload) {
                eprintln!("Failed to emit progress event: {e}");
            }
        }
    }
}

pub fn emit_gui_error(message: &str) {
    const MAX_CHARS: usize = 35;
    let truncated: String = message.chars().take(MAX_CHARS).collect();
    emit_gui_progress_update(0.0, &format!("Error! {truncated}"));
}

pub fn emit_world_name_update(_name: &str) {
    #[cfg(feature = "gui")]
    {
        if let Some(window) = get_main_window() {
            if let Err(e) = window.emit("world-name-update", _name) {
                eprintln!("Failed to emit world-name-update event: {e}");
            }
        }
    }
}

pub fn emit_map_preview_ready() {
    #[cfg(feature = "gui")]
    {
        if let Some(window) = get_main_window() {
            if let Err(e) = window.emit("map-preview-ready", ()) {
                eprintln!("Failed to emit map-preview-ready event: {}", e);
            }
        }
    }
}

pub fn emit_show_in_folder(_path: &str) {
    #[cfg(feature = "gui")]
    {
        if let Some(window) = get_main_window() {
            if let Err(e) = window.emit("show-in-folder", _path) {
                eprintln!("Failed to emit show-in-folder event: {}", e);
            }
        }
    }
}
