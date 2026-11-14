// Modified: expose bounded viewer and persistent settings handling.
//! Filesystem and navigation logic independent of the user interface.
pub mod directory;
pub mod navigation;
pub mod bookmarks;
pub mod ui_theme;

pub mod converter;
pub mod image_convert;
pub mod audio_convert;

pub mod operations;
pub mod batch_rename;
pub mod size_scan;

pub mod archive;
pub mod archive_actions;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod archive_engine;

pub mod trash;

#[cfg(target_os = "linux")]
pub mod linux_flags;

#[cfg(target_os = "linux")]
pub mod packed_archive;

#[cfg(unix)]
pub mod file_clipboard;

pub mod json_csv;

#[cfg(test)]
mod test_support;

pub mod viewer;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod pdf;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod office;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod office_render;

pub mod settings;

pub mod insights;
pub mod duplicate_finder;
pub mod mobile;
pub mod scratchpad;

pub mod tags;

pub mod hashing;

pub mod local_server;

pub mod tools;
