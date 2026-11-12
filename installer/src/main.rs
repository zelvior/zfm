#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! ZFM setup and uninstaller.
//!
//! The same executable installs the application, acts as the uninstaller when
//! it is named `Uninstall.exe`, and supports silent `/S` runs. The release
//! executable is embedded at build time, so setup needs no downloads and no
//! third-party installer framework.

mod payload;
mod plan;

#[cfg(windows)]
mod win32;
#[cfg(windows)]
mod windows;

fn main() {
    #[cfg(windows)]
    {
        std::process::exit(windows::main());
    }
    #[cfg(not(windows))]
    {
        eprintln!("ZFM setup runs on Windows only.");
        std::process::exit(2);
    }
}
