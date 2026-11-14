//! Optional, explicitly invoked Windows system-managed read-only ISO mounts.
//! This does not embed a driver, parse image contents, or preload an image.
use std::io;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy)]
pub enum Action { Mount, Dismount }
fn validate_image(path: &Path) -> io::Result<()> {
    if !path.is_absolute() || !path.extension().and_then(|s| s.to_str()).map_or(false, |s| s.eq_ignore_ascii_case("iso")) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Select an absolute ISO image filename"));
    }
    let _checked = crate::settings::open_existing(path)?;
    Ok(())
}
#[cfg(windows)]
fn system_powershell() -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "kernel32")]
    extern "system" { fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32; }
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() { return Err(io::Error::last_os_error()); }
    buffer.truncate(length);
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer)).join("WindowsPowerShell").join("v1.0").join("powershell.exe"))
}

#[cfg(windows)]
fn build_command(program: &Path, path: &Path, action: Action) -> std::process::Command {
    let script = match action {
        Action::Mount => "try { Import-Module Storage -ErrorAction Stop; Mount-DiskImage -ImagePath $env:ZFM_ISO_IMAGE -StorageType ISO -ErrorAction Stop | Format-List ImagePath,Attached,DevicePath; exit 0 } catch { Write-Error $_; exit 1 }",
        Action::Dismount => "try { Import-Module Storage -ErrorAction Stop; Dismount-DiskImage -ImagePath $env:ZFM_ISO_IMAGE -ErrorAction Stop; Write-Output 'Dismount command completed'; exit 0 } catch { Write-Error $_; exit 1 }",
    };
    let mut command = std::process::Command::new(program);
    command.current_dir(program.parent().unwrap()).env("ZFM_ISO_IMAGE", path)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script]);
    command

}

/// Runs the operating system's Storage module with a fixed command and the
/// image path passed as an environment value, not interpolated into a script.
/// Mounts may outlive ZFM or a timed-out client. Never invokes a shell with the
/// path as code. Use trusted directories: the OS reopens the image by path.
pub fn run(path: &Path, action: Action, cancel: &AtomicBool) -> io::Result<String> {
    validate_image(path)?;
    #[cfg(windows)] {
        use std::time::Duration;
        let program = system_powershell()?;
        if !program.is_file() { return Err(io::Error::new(io::ErrorKind::NotFound, "Windows PowerShell is not installed in the system directory")); }
        let command = build_command(&program, path, action);
        let result = crate::tools::run_command_with_timeout(command, cancel, Duration::from_secs(60))?;
        let verb = match action { Action::Mount => "Mount", Action::Dismount => "Dismount" };
        Ok(format!("Windows ISO {verb} request: {:?}\n\n{result}\n\nCheck the exit status and Windows' drive list. ISO mounts are read-only and may persist after ZFM exits, cancellation or a timeout. Dismount explicitly when finished. Other disk images and archive formats are not supported here.", path))
    }
    #[cfg(not(windows))] {
        let _ = (action, cancel);
        Err(io::Error::new(io::ErrorKind::Unsupported, "Windows ISO mounting is unavailable on this platform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test] fn fixed_powershell_script_does_not_embed_image_path() {
        let path = Path::new("C:\\tmp\\a'; throw 'oops.iso");
        let program = Path::new("C:\\Windows\\System32\\powershell.exe");
        for action in [Action::Mount, Action::Dismount] {
            let cmd = build_command(program, path, action);
            let args = cmd.get_args().map(|v| v.to_string_lossy().into_owned()).collect::<Vec<_>>();
            assert!(args.iter().any(|v| v.contains("$env:ZFM_ISO_IMAGE")));
            assert!(!args.iter().any(|v| v.contains("oops.iso")));
            assert!(cmd.get_envs().any(|(key, val)| key == "ZFM_ISO_IMAGE" && val == Some(path.as_os_str())));
        }
    }
    #[test] fn only_existing_regular_iso_files_are_accepted() {
        let root = crate::test_support::scratch_dir("disk-image");
        let iso = root.join("disk.ISO"); let other = root.join("disk.zip");
        std::fs::write(&iso, b"not necessarily a valid ISO").unwrap(); std::fs::write(&other, b"x").unwrap();
        assert!(validate_image(&iso).is_ok()); assert!(validate_image(&other).is_err());
        assert!(validate_image(Path::new("relative.iso")).is_err()); assert!(validate_image(&root.join("missing.iso")).is_err());
        #[cfg(unix)] { std::os::unix::fs::symlink(&iso, root.join("link.iso")).unwrap(); assert!(validate_image(&root.join("link.iso")).is_err()); }
        std::fs::remove_dir_all(root).unwrap();
    }
}
