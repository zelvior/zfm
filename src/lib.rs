//! Filesystem and navigation logic independent of the user interface.
pub mod directory;
pub mod navigation;

pub mod converter;

pub mod operations;

pub mod archive;

pub mod trash;

#[cfg(target_os = "linux")]
pub mod linux_flags;
