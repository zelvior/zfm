// Only the Windows setup program reads these constants; other hosts build the
// payload for the installer unit tests.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

//! The files the installer carries inside its own executable.
//!
//! The payload is embedded at build time, so setup needs no network access,
//! no separate archive and no third-party runtime.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The release `zfm.exe` for the architecture this installer was built for.
pub const APP: &[u8] = include_bytes!(env!("ZFM_PAYLOAD_PATH"));
/// Multi-size monochrome application icon installed next to the executable.
pub const ICON: &[u8] = include_bytes!(env!("ZFM_ICON_PATH"));
/// PNG logo used for the installer window icon.
pub const LOGO: &[u8] = include_bytes!(env!("ZFM_LOGO_PATH"));
pub const LICENSE: &str = include_str!(env!("ZFM_LICENSE_PATH"));
pub const README: &str = concat!(
    include_str!(env!("ZFM_README_PATH")),
    "\n\n--- Upstream Rust 1.77.2 COPYRIGHT (reference for pinned release builds) ---\n",
    include_str!(env!("ZFM_RUST_COPYRIGHT_PATH")),
    "\n\n--- Upstream Rust 1.77.2 LICENSE-MIT ---\n",
    include_str!(env!("ZFM_RUST_MIT_PATH")),
    "\n\n--- Upstream Rust 1.77.2 LICENSE-APACHE ---\n",
    include_str!(env!("ZFM_RUST_APACHE_PATH")),
);
/// Release tag supplied to the build, or `dev` for a local build.
pub const VERSION: &str = env!("ZFM_VERSION");

pub const APP_NAME: &str = "ZFM File Manager";
pub const PUBLISHER: &str = "Zelvior Labs";

/// Every file the installer creates. The uninstaller removes exactly these and
/// never touches anything else in the selected folder.
pub const MANAGED_FILES: [&str; 5] = [
    "zfm.exe",
    "zfm.ico",
    "LICENSE.txt",
    "README.txt",
    "Uninstall.exe",
];

fn payload_files() -> [(&'static str, &'static [u8]); 4] {
    [
        ("zfm.exe", APP),
        ("zfm.ico", ICON),
        ("LICENSE.txt", LICENSE.as_bytes()),
        ("README.txt", README.as_bytes()),
    ]
}

/// Total size of the installed files in bytes, used for the Programs and
/// Features "size" value.
pub fn installed_size() -> u64 {
    payload_files()
        .iter()
        .map(|(_, data)| data.len() as u64)
        .sum()
}

/// A file installation error retains the OS error for retry decisions.
#[derive(Debug)]
pub struct InstallFileError {
    pub message: String,
    pub retryable: bool,
    pub blocked_path: Option<PathBuf>,
}

impl InstallFileError {
    fn new(action: &str, path: &Path, error: io::Error) -> Self {
        // Access denied can also be a permissions or security software problem,
        // not just an open executable. Never claim we know the exact cause.
        let retryable = error.kind() == io::ErrorKind::PermissionDenied
            || matches!(error.raw_os_error(), Some(5 | 32 | 33));
        let advice = if retryable {
            "\n\nClose ZFM and other programs using this folder, then retry. If the problem continues, check folder permissions or security software, or choose another folder."
        } else {
            ""
        };
        Self {
            message: format!("Could not {action} {}: {error}{advice}", path.display()),
            retryable,
            blocked_path: if retryable && action == "install" {
                Some(path.to_path_buf())
            } else {
                None
            },
        }
    }
}

static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write the application files into `dir`, creating the folder if needed.
/// Identical files are left untouched, allowing a same-version reinstall while
/// the application is running. Changed files are staged and replaced atomically.
pub fn install_files(dir: &Path) -> Result<(), InstallFileError> {
    fs::create_dir_all(dir)
        .map_err(|error| InstallFileError::new("create the installation folder", dir, error))?;
    for (name, data) in payload_files() {
        write_file(&dir.join(name), data)?;
    }
    Ok(())
}

/// Install setup itself as the uninstaller, also using staged replacement.
pub fn install_uninstaller(dir: &Path, source: &Path) -> Result<(), InstallFileError> {
    let data = fs::read(source)
        .map_err(|error| InstallFileError::new("read the setup program", source, error))?;
    write_file(&dir.join("Uninstall.exe"), &data)
}

fn contents_match(path: &Path, expected: &[u8]) -> bool {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_file() && m.len() == expected.len() as u64 => (),
        _ => return false,
    }
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut buf = [0u8; 64 * 1024];
    let mut offset = 0;
    while offset < expected.len() {
        let length = (expected.len() - offset).min(buf.len());
        if file.read_exact(&mut buf[..length]).is_err()
            || buf[..length] != expected[offset..offset + length]
        {
            return false;
        }
        offset += length;
    }
    // An additional read catches a concurrently growing file.
    matches!(file.read(&mut buf[..1]), Ok(0))
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), InstallFileError> {
    if contents_match(path, data) {
        return Ok(());
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut created = None;
    for _ in 0..100 {
        let counter = STAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let staged = path.with_file_name(format!(
            ".{name}.zfm-install-{}-{counter}",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
        {
            Ok(file) => {
                created = Some((staged, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(InstallFileError::new("write", &staged, error)),
        }
    }
    let (staged, mut file) = created.ok_or_else(|| {
        InstallFileError::new(
            "stage",
            path,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Cannot allocate a temporary file",
            ),
        )
    })?;
    if let Err(error) = file.write_all(data).and_then(|_| file.flush()) {
        drop(file);
        let _ = fs::remove_file(&staged);
        return Err(InstallFileError::new("write", &staged, error));
    }
    drop(file);
    if let Err(error) = fs::rename(&staged, path) {
        let _ = fs::remove_file(&staged);
        return Err(InstallFileError::new("install", path, error));
    }
    Ok(())
}

/// Remove the installed files. Missing files are not an error; failures are
/// returned so the uninstaller can report an incomplete removal.
pub fn remove_files(dir: &Path) -> Vec<String> {
    remove_files_except(dir, None)
}

/// Remove the installed files except `skip`, which the uninstaller handles
/// while the executable is still running.
pub fn remove_files_except(dir: &Path, skip: Option<&str>) -> Vec<String> {
    let mut problems = Vec::new();
    for name in MANAGED_FILES {
        if skip == Some(name) {
            continue;
        }
        let path = dir.join(name);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => problems.push(format!("{}: {error}", path.display())),
        }
    }
    problems
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

/// Recognize a folder whose managed files came from an earlier ZFM setup run
/// that failed before recording its registry entry (for example when shortcut
/// creation failed after the payload was already copied).
///
/// Unrelated folders are still rejected: every payload file must be a regular
/// file (never a symlink) and both `zfm.ico` and `LICENSE.txt` must match the
/// embedded ZFM assets byte-for-byte.
pub fn is_previous_install(dir: &Path) -> bool {
    if !payload_files()
        .iter()
        .all(|(name, _)| is_regular_file(&dir.join(*name)))
    {
        return false;
    }
    if let Ok(metadata) = fs::symlink_metadata(dir.join("Uninstall.exe")) {
        if !metadata.file_type().is_file() {
            return false;
        }
    }
    fs::read(dir.join("zfm.ico")).ok().as_deref() == Some(ICON)
        && fs::read(dir.join("LICENSE.txt")).ok().as_deref() == Some(LICENSE.as_bytes())
}

/// Remove the installation folder when nothing else lives in it. A folder that
/// still contains user files is deliberately left in place.
pub fn remove_folder_if_empty(dir: &Path) {
    if let Ok(mut entries) = fs::read_dir(dir) {
        if entries.next().is_none() {
            let _ = fs::remove_dir(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        let unique = format!(
            "zfm-installer-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn installs_and_removes_only_managed_files() {
        let dir = scratch();
        install_files(&dir).expect("install");
        for name in MANAGED_FILES.iter().take(4) {
            assert!(dir.join(name).is_file(), "{name} was not installed");
        }
        assert_eq!(fs::read(dir.join("zfm.exe")).unwrap(), APP);
        assert_eq!(fs::read(dir.join("LICENSE.txt")).unwrap(), LICENSE.as_bytes());
        assert_eq!(fs::read(dir.join("README.txt")).unwrap(), README.as_bytes());
        for text in [
            include_str!(env!("ZFM_RUST_COPYRIGHT_PATH")),
            include_str!(env!("ZFM_RUST_MIT_PATH")),
            include_str!(env!("ZFM_RUST_APACHE_PATH")),
        ] {
            assert!(README.contains(text), "missing upstream notice");
        }
        for entry in fs::read_dir(&dir).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.contains(".zfm-install"), "staging file left: {name}");
        }

        let unrelated = dir.join("notes.txt");
        fs::write(&unrelated, b"keep me").unwrap();
        assert!(remove_files(&dir).is_empty());
        assert!(!dir.join("zfm.exe").exists());
        assert!(unrelated.is_file());
        remove_folder_if_empty(&dir);
        assert!(unrelated.is_file(), "user data must survive");

        fs::remove_file(&unrelated).unwrap();
        remove_folder_if_empty(&dir);
        assert!(!dir.exists(), "empty folder should be removed");
    }

    #[test]
    fn preserves_old_file_and_replaces_only_when_needed() {
        let dir = scratch();
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("zfm.exe");
        fs::write(&dest, b"old executable").unwrap();
        let unrelated = dir.join(".zfm.exe.zfm-install");
        fs::write(&unrelated, b"do not touch").unwrap();
        write_file(&dest, b"new executable").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new executable");
        assert_eq!(fs::read(&unrelated).unwrap(), b"do not touch");
        write_file(&dest, b"new executable").unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn locked_file_can_retry_without_damaging_original() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = scratch();
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("zfm.exe");
        fs::write(&dest, b"old executable").unwrap();
        // Same-version reinstall can read the image even though deletion is blocked.
        let read_lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&dest)
            .unwrap();
        write_file(&dest, b"old executable").unwrap();
        let error = write_file(&dest, b"new executable").unwrap_err();
        assert!(error.retryable, "{error:?}");
        assert!(error.message.contains("Close ZFM"), "{error:?}");
        drop(read_lock);
        assert_eq!(fs::read(&dest).unwrap(), b"old executable");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "staging file must be removed"
        );
        write_file(&dest, b"new executable").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new executable");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reports_missing_files_as_removed() {
        let dir = scratch();
        fs::create_dir_all(&dir).unwrap();
        assert!(remove_files(&dir).is_empty());
        fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn installed_size_counts_the_payload() {
        assert_eq!(
            installed_size(),
            APP.len() as u64 + ICON.len() as u64 + LICENSE.len() as u64 + README.len() as u64
        );
    }
}
