//! Installer decisions that do not depend on Windows: option parsing, the
//! default folder, and the rule that protects unrelated files from being
//! replaced. Keeping these here means they can be tested on any host.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::payload;

/// Command line of setup and of the uninstaller.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `/S`: no windows, no questions.
    pub silent: bool,
    /// `/UNINSTALL`: act as the uninstaller.
    pub uninstall: bool,
    /// `/D=<folder>`: installation folder.
    pub directory: Option<PathBuf>,
}

/// Parse the arguments that follow the program name.
///
/// The syntax follows the familiar installer conventions: options start with a
/// slash and are case-insensitive, `/D=` takes the rest of the argument, and a
/// quoted folder is accepted as well. Unknown options are rejected instead of
/// being silently ignored.
pub fn parse<I>(args: I) -> Result<Options, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut options = Options::default();
    for argument in args {
        let text = argument.to_string_lossy().into_owned();
        let lower = text.to_ascii_lowercase();
        if lower == "/s" || lower == "/silent" {
            options.silent = true;
        } else if lower == "/uninstall" {
            options.uninstall = true;
        } else if let Some(value) = value_of(&text, "/d=") {
            if value.is_empty() {
                return Err("The /D option needs a folder.".to_string());
            }
            options.directory = Some(PathBuf::from(value));
        } else if text.starts_with('/') {
            return Err(format!("Unknown option: {text}"));
        } else {
            return Err(format!("Unexpected argument: {text}"));
        }
    }
    Ok(options)
}

fn value_of<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let value = &text[prefix.len()..];
    let value = value.strip_prefix('"').unwrap_or(value);
    let value = value.strip_suffix('"').unwrap_or(value);
    Some(value.trim())
}

/// Default per-user installation folder. No administrator rights and no
/// machine-wide change are needed.
pub fn default_directory(
    local_app_data: Option<&Path>,
    user_profile: Option<&Path>,
) -> Option<PathBuf> {
    local_app_data
        .map(|base| base.join("Programs").join("ZFM"))
        .or_else(|| user_profile.map(|base| base.join("Programs").join("ZFM")))
}

/// The folder the uninstaller must clean: the folder recorded by setup when it
/// still holds an uninstaller, otherwise the folder the uninstaller runs from.
pub fn uninstall_directory(
    running_in: Option<&Path>,
    registered: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(registered) = registered {
        if registered.join("Uninstall.exe").is_file() {
            return Some(registered.to_path_buf());
        }
    }
    running_in.map(Path::to_path_buf)
}

/// Validate the folder typed into the wizard.
pub fn validate_directory(text: &str) -> Result<PathBuf, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Choose an installation folder.".to_string());
    }
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err(
            "The installation folder must be a full path such as C:\\Users\\You\\AppData\\Local\\Programs\\ZFM."
                .to_string(),
        );
    }
    Ok(path)
}

/// Refuse to install over files the installer does not own.
///
/// Reinstalling into the folder recorded in the registry — or completing an
/// earlier ZFM installation that copied its payload before registration could
/// finish — is an upgrade and is allowed; any other folder that already
/// contains a managed file is not.
pub fn may_install(dir: &Path, registered: Option<&Path>) -> Result<(), String> {
    if let Some(registered) = registered {
        if same_directory(registered, dir) {
            return Ok(());
        }
    }
    let existing: Vec<&str> = payload::MANAGED_FILES
        .iter()
        .copied()
        .filter(|name| fs::symlink_metadata(dir.join(name)).is_ok())
        .collect();
    if existing.is_empty() || payload::is_previous_install(dir) {
        return Ok(());
    }
    Err(format!(
        "The folder {} already contains {}. Choose an empty folder; setup will not replace unrelated files.",
        dir.display(),
        existing.join(", ")
    ))
}

/// Compare two folders without requiring either to exist.
pub fn same_directory(left: &Path, right: &Path) -> bool {
    if let (Ok(left), Ok(right)) = (fs::canonicalize(left), fs::canonicalize(right)) {
        return left == right;
    }
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(|value| OsString::from(*value)).collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zfm-plan-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_silent_directory_and_uninstall() {
        let options = parse(args(&["/S", "/D=C:\\ZFM"])).unwrap();
        assert!(options.silent);
        assert_eq!(
            options.directory,
            Some(PathBuf::from("C:\\ZFM")),
            "unquoted folders are accepted"
        );

        let options = parse(args(&["/uninstall", "/S", "/d=\"C:\\ZFM\""])).unwrap();
        assert!(options.uninstall);
        assert!(options.silent);
        assert_eq!(options.directory, Some(PathBuf::from("C:\\ZFM")));
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(parse(args(&["--help"])).is_err());
        assert!(parse(args(&["/D"])).is_err());
        assert!(parse(args(&["/NOPE"])).is_err());
        assert!(parse(args(&["/target=C:\\b"])).is_err());
    }

    #[test]
    fn default_folder_is_per_user() {
        let base = Path::new("C:\\Users\\You\\AppData\\Local");
        assert_eq!(
            default_directory(Some(base), None),
            Some(base.join("Programs").join("ZFM"))
        );
        assert_eq!(default_directory(None, None), None);
    }

    #[test]
    fn uninstall_folder_prefers_the_installed_copy() {
        let registered = scratch("registered");
        fs::write(registered.join("Uninstall.exe"), b"uninstaller").unwrap();
        assert_eq!(
            uninstall_directory(Some(Path::new("/tmp/running")), Some(&registered)),
            Some(registered.clone())
        );
        assert_eq!(
            uninstall_directory(Some(Path::new("/tmp/running")), Some(Path::new("/tmp/other"))),
            Some(PathBuf::from("/tmp/running"))
        );
        assert_eq!(uninstall_directory(None, None), None);
        fs::remove_dir_all(&registered).unwrap();
    }

    #[test]
    fn validates_absolute_folders() {
        let base = scratch("validate");
        let valid = base.to_string_lossy().into_owned();
        assert_eq!(validate_directory(&valid).unwrap(), base);
        assert!(validate_directory("").is_err());
        assert!(validate_directory("relative\\folder").is_err());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn refuses_to_replace_unrelated_files() {
        let dir = scratch("conflict");
        assert!(may_install(&dir, None).is_ok());
        fs::write(dir.join("zfm.exe"), b"not ours").unwrap();
        let error = may_install(&dir, None).unwrap_err();
        assert!(error.contains("zfm.exe"), "{error}");
        // A folder previously installed by setup may be upgraded in place.
        assert!(may_install(&dir, Some(&dir)).is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recovers_incomplete_zfm_install_without_trusting_foreign_files() {
        let dir = scratch("incomplete");
        payload::install_files(&dir).unwrap();
        fs::write(dir.join("Uninstall.exe"), b"previous uninstaller").unwrap();
        // A prior ZFM install that failed before writing its registry key may
        // be completed in place.
        assert!(may_install(&dir, None).is_ok());

        // Tampering with the ZFM assets disqualifies the folder so unrelated
        // files are still protected.
        fs::write(dir.join("LICENSE.txt"), b"custom license").unwrap();
        assert!(may_install(&dir, None).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
