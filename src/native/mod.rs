//! Platform-specific native implementation.
//!
//! Only the Windows backend exists (`windows.rs`); other platforms get a stub
//! in `desktop.rs` that returns `UnsupportedPlatform`.

#[cfg(target_os = "windows")]
pub mod windows;
