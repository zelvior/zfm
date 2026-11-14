//! Integration with a separately installed WinCDEmu batch mounter and driver.
//! We neither bundle nor install software or drivers. The selected executable
//! runs unsandboxed and may launch an elevated descendant.
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug)]
pub enum Action { Mount, Dismount, List }

pub fn supports(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).map_or(false, |s| {
        ["iso", "img", "cue", "nrg", "ccd", "mds"].iter().any(|ext| s.eq_ignore_ascii_case(ext))
    })
}

fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, message) }

fn validate_program(program: &Path) -> io::Result<()> {
    if !program.is_absolute() || !program.file_name().and_then(|s| s.to_str()).map_or(false, |s| s.eq_ignore_ascii_case("batchmnt.exe")) {
        return Err(invalid("Choose the absolute path to the installed WinCDEmu batchmnt.exe"));
    }
    let _file = crate::settings::open_existing(program)?;
    Ok(())
}
fn validate_image(path: &Path) -> io::Result<()> {
    if !path.is_absolute() || !supports(path) { return Err(invalid("Select an absolute ISO, IMG, CUE, NRG, CCD or MDS image")); }
    let _file = crate::settings::open_existing(path)?;
    Ok(())
}
fn build_command(program: &Path, image: Option<&Path>, action: Action) -> io::Result<std::process::Command> {
    let mut cmd = std::process::Command::new(program);
    cmd.current_dir(program.parent().ok_or_else(|| invalid("Missing executable directory"))?);
    match action {
        Action::Mount => { cmd.arg(image.ok_or_else(|| invalid("Missing image"))?); }
        Action::Dismount => { cmd.arg("/unmount").arg(image.ok_or_else(|| invalid("Missing image"))?); }
        Action::List => { cmd.arg("/list"); }
    }
    Ok(cmd)
}

/// Executes only a user-selected installed batchmnt; no shell is involved.
/// The driver reopens the image and potentially sidecar files by path, so
/// validation is not race-free and images must come from trusted directories.
pub fn run(program: &Path, image: Option<&Path>, action: Action, cancel: &AtomicBool) -> io::Result<String> {
    validate_program(program)?;
    match (action, image) {
        (Action::List, None) => (),
        (Action::Mount | Action::Dismount, Some(path)) => validate_image(path)?,
        _ => return Err(invalid("Image is required only for mount or dismount")),
    }
    #[cfg(windows)] {
        let command = build_command(program, image, action)?;
        let result = crate::tools::run_command_with_timeout(command, cancel, std::time::Duration::from_secs(60))?;
        Ok(format!("WinCDEmu request: {action:?}\n\n{result}\n\nVerify the operation in WinCDEmu or Windows drive list; batchmnt may report a zero exit status even if unmount fails. The driver and mounts may persist, including after cancellation or timeout. Elevated descendants may continue running."))
    }
    #[cfg(not(windows))] {
        let _ = (cancel, build_command(program, image, action)?);
        Err(io::Error::new(io::ErrorKind::Unsupported, "WinCDEmu is available on Windows only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn formats_and_literal_arguments() {
        for name in ["a.iso", "a.IMG", "a.cue", "a.nrg", "a.ccd", "a.mds"] { assert!(supports(Path::new(name))); }
        for name in ["a.zip", "a.bin", "a.mdf", "a.iso.exe"] { assert!(!supports(Path::new(name))); }
        let program = Path::new("/trusted/batchmnt.exe");
        let image = Path::new("/trusted/a & test.iso");
        let mount = build_command(program, Some(image), Action::Mount).unwrap();
        assert_eq!(mount.get_args().collect::<Vec<_>>(), vec![image.as_os_str()]);
        let unmount = build_command(program, Some(image), Action::Dismount).unwrap();
        assert_eq!(unmount.get_args().collect::<Vec<_>>(), vec![std::ffi::OsStr::new("/unmount"), image.as_os_str()]);
        let list = build_command(program, None, Action::List).unwrap();
        assert_eq!(list.get_args().collect::<Vec<_>>(), vec![std::ffi::OsStr::new("/list")]);
        assert!(build_command(program, None, Action::Mount).is_err());
    }
    #[test] fn selection_checks() {
        let root = crate::test_support::scratch_dir("wincdemu");
        let program = root.join("batchmnt.exe"); let image = root.join("image.cue");
        std::fs::write(&program, b"fake").unwrap(); std::fs::write(&image, b"fake").unwrap();
        assert!(validate_program(&program).is_ok()); assert!(validate_image(&image).is_ok());
        assert!(validate_program(&root.join("else.exe")).is_err());
        assert!(validate_image(&root.join("missing.iso")).is_err());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(&image, root.join("link.iso")).unwrap();
            assert!(validate_image(&root.join("link.iso")).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
