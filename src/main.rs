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
        std::process::exit(1);
    }
}
