#[cfg(target_os = "linux")]
mod linux_ui;

fn main() {
    #[cfg(target_os = "linux")]
    if let Err(error) = linux_ui::run() {
        eprintln!("ZFM: {error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("ZFM: the native interface is currently implemented for Linux only.");
        std::process::exit(1);
    }
}
