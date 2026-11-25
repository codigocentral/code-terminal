//! Platform-specific functionality

pub mod clipboard;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod unix;

#[cfg(target_os = "macos")]
mod macos;

pub use clipboard::{get_clipboard, ClipboardError, ClipboardManager};
