#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod linux_ui;
#[cfg(target_os = "windows")]
mod windows_ui;

fn main() {
    #[cfg(target_os = "linux")]
    let result = linux_ui::run();
    #[cfg(target_os = "windows")]
    let result = windows_ui::run();
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let result: Result<(), String> = Err("ZFM supports Linux and Windows interfaces only".into());
    if let Err(error) = result {
        eprintln!("ZFM: {error}");
        #[cfg(target_os = "windows")]
        windows_ui::show_startup_error(&error);
        #[cfg(target_os = "linux")]
        show_linux_startup_error(&error);
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn show_linux_startup_error(error: &str) {
    // A double-clicked executable may have no visible stderr. If a display and
    // a standard desktop alert are available, also show the actual error.
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    let message = format!("ZFM could not start: {error}");
    if std::process::Command::new("zenity")
        .args(["--error", "--title=ZFM", "--text", &message])
        .spawn()
        .is_err()
    {
        let _ = std::process::Command::new("xmessage")
            .args(["-center", "-title", "ZFM", &message])
            .spawn();
    }
}
