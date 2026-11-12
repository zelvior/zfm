// Only the Windows setup program reads these constants; other hosts build the
// payload for the installer unit tests.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

//! The files the installer carries inside its own executable.
//!
//! The payload is embedded at build time, so setup needs no network access,
//! no separate archive and no third-party runtime.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The release `zfm.exe` for the architecture this installer was built for.
pub const APP: &[u8] = include_bytes!(env!("ZFM_PAYLOAD_PATH"));
/// Multi-size monochrome application icon installed next to the executable.
pub const ICON: &[u8] = include_bytes!(env!("ZFM_ICON_PATH"));
/// PNG logo used for the installer window icon.
pub const LOGO: &[u8] = include_bytes!(env!("ZFM_LOGO_PATH"));
pub const LICENSE: &str = include_str!(env!("ZFM_LICENSE_PATH"));
pub const README: &str = include_str!(env!("ZFM_README_PATH"));
/// Release tag supplied to the build, or `dev` for a local build.
pub const VERSION: &str = env!("ZFM_VERSION");

pub const APP_NAME: &str = "ZFM File Manager";
pub const PUBLISHER: &str = "ZFM contributors";

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

/// Write the application files into `dir`, creating the folder if needed.
///
/// Each file is staged under a temporary name and then renamed into place, so
/// a failed or interrupted install does not leave a half-written executable.
pub fn install_files(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| {
        format!(
            "Could not create the installation folder {}: {error}",
            dir.display()
        )
    })?;
    for (name, data) in payload_files() {
        write_file(&dir.join(name), data)?;
    }
    Ok(())
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), String> {
    let staged = staged_path(path);
    fs::write(&staged, data)
        .map_err(|error| format!("Could not write {}: {error}", staged.display()))?;
    if let Err(error) = fs::rename(&staged, path) {
        let _ = fs::remove_file(&staged);
        return Err(format!("Could not install {}: {error}", path.display()));
    }
    Ok(())
}

fn staged_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "zfm".to_string());
    path.with_file_name(format!(".{name}.zfm-install"))
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
