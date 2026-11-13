//! Small, versioned persistent user preferences. No directories are created on read.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_BYTES: u64 = 4096;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Settings {
    pub show_hidden: bool,
    pub sort_ascending: bool,
    /// Windows-specific preferences; older version 1 files use these defaults.
    pub windows_live_refresh: bool,
    pub windows_drive_labels: bool,
    pub windows_compact_view: bool,
    pub windows_expand_drives: bool,
}
impl Default for Settings {
    fn default() -> Self { Self { show_hidden: false, sort_ascending: true,
        windows_live_refresh: true, windows_drive_labels: true,
        windows_compact_view: false, windows_expand_drives: true } }
}

/// The user's configuration directory, never the installation directory.
pub fn user_path() -> io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    let base = match std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        Some(path) if path.is_absolute() => Some(path),
        _ => std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_absolute()).map(|p| p.join(".config")),
    };
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from).filter(|p| p.is_absolute());
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let base: Option<PathBuf> = None;
    base.map(|p| p.join("zfm").join("settings.conf"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "User configuration directory is unavailable"))
}

fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
fn parse(text: &str) -> io::Result<Settings> {
    let mut version = None;
    let mut hidden = None;
    let mut ascending = None;
    let mut live_refresh = None;
    let mut drive_labels = None;
    let mut compact_view = None;
    let mut expand_drives = None;
    for line in text.lines() {
        let (key, value) = line.split_once('=').ok_or_else(|| invalid("Malformed settings"))?;
        let slot = match key {
            "version" if version.is_none() => { version = Some(value); continue; }
            "show_hidden" => &mut hidden,
            "sort_ascending" => &mut ascending,
            "windows_live_refresh" => &mut live_refresh,
            "windows_drive_labels" => &mut drive_labels,
            "windows_compact_view" => &mut compact_view,
            "windows_expand_drives" => &mut expand_drives,
            _ => return Err(invalid("Unknown or duplicate settings key")),
        };
        if slot.is_some() { return Err(invalid("Duplicate settings key")); }
        *slot = Some(match value { "true" => true, "false" => false, _ => return Err(invalid("Invalid settings value")) });
    }
    if version != Some("1") { return Err(invalid("Unsupported settings version")); }
    Ok(Settings {
        show_hidden: hidden.ok_or_else(|| invalid("Missing settings key"))?,
        sort_ascending: ascending.ok_or_else(|| invalid("Missing settings key"))?,
        windows_live_refresh: live_refresh.unwrap_or(true),
        windows_drive_labels: drive_labels.unwrap_or(true),
        windows_compact_view: compact_view.unwrap_or(false),
        windows_expand_drives: expand_drives.unwrap_or(true),
    })
}

fn safe_metadata(meta: &fs::Metadata, want_dir: bool) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 { return false; }
    }
    !meta.file_type().is_symlink() && if want_dir { meta.is_dir() } else { meta.is_file() }
}
pub(crate) fn check_directory(dir: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if safe_metadata(&meta, true) => Ok(true),
        Ok(_) => Err(invalid("Settings directory is not a regular directory")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
pub(crate) fn open_existing(path: &Path) -> io::Result<File> {
    #[cfg(all(target_os = "linux", not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))))]
    return Err(io::Error::new(io::ErrorKind::Unsupported, "Safe settings access is unavailable on this architecture"));
    let meta = fs::symlink_metadata(path)?;
    if !safe_metadata(&meta, false) { return Err(invalid("Settings file is not a regular file")); }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x800 | crate::linux_flags::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    if !safe_metadata(&file.metadata()?, false) { return Err(invalid("Settings file is not a regular file")); }
    Ok(file)
}

/// Missing files use defaults. Existing malformed or unsupported files are errors.
pub fn load(path: &Path) -> io::Result<Settings> {
    let dir = path.parent().ok_or_else(|| invalid("Invalid settings path"))?;
    if !check_directory(dir)? { return Ok(Settings::default()); }
    let file = match open_existing(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() > MAX_BYTES { return Err(invalid("Settings file is too large")); }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES { return Err(invalid("Settings file is too large")); }
    parse(std::str::from_utf8(&bytes).map_err(|_| invalid("Settings are not UTF-8"))?)
}

#[cfg(not(windows))]
pub(crate) fn replace(from: &Path, to: &Path) -> io::Result<()> { fs::rename(from, to) }
#[cfg(windows)]
pub(crate) fn replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" { fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32; }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Write a complete settings snapshot via a unique sibling file and atomic replacement.
/// Refuses to overwrite an unrecognized or malformed existing settings file.
pub fn save(path: &Path, settings: Settings) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| invalid("Invalid settings path"))?;
    if !check_directory(dir)? {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700).create(dir)?;
        }
        #[cfg(not(unix))]
        fs::create_dir_all(dir)?;
        if !check_directory(dir)? { return Err(invalid("Settings directory is missing")); }
    }
    let _ = load(path)?;
    let text = format!(concat!("version=1\nshow_hidden={}\nsort_ascending={}\n",
        "windows_live_refresh={}\nwindows_drive_labels={}\nwindows_compact_view={}\nwindows_expand_drives={}\n"),
        settings.show_hidden, settings.sort_ascending, settings.windows_live_refresh,
        settings.windows_drive_labels, settings.windows_compact_view, settings.windows_expand_drives);
    for _ in 0..100 {
        let temp = dir.join(format!(".settings-{}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        match options.open(&temp) {
            Ok(mut file) => {
                let result = (|| {
                    file.write_all(text.as_bytes())?;
                    file.sync_all()?;
                    drop(file);
                    // Recheck that we are not replacing an invalid file.
                    let _ = load(path)?;
                    replace(&temp, path)
                })();
                if result.is_err() { let _ = fs::remove_file(&temp); }
                return result;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "Could not allocate a settings staging file"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_round_trip_and_replace() {
        let dir = crate::test_support::scratch_dir("settings");
        let path = dir.join("config").join("zfm").join("settings.conf");
        assert_eq!(load(&path).unwrap(), Settings::default());
        assert!(!path.parent().unwrap().exists());
        let selected = Settings { show_hidden: true, sort_ascending: false, windows_live_refresh: false,
            windows_drive_labels: false, windows_compact_view: true, windows_expand_drives: false };
        save(&path, selected).unwrap();
        assert_eq!(load(&path).unwrap(), selected);
        save(&path, Settings::default()).unwrap();
        assert_eq!(load(&path).unwrap(), Settings::default());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn older_files_receive_new_defaults_and_duplicates_are_rejected() {
        let old = parse("version=1\nshow_hidden=true\nsort_ascending=false\n").unwrap();
        assert!(old.show_hidden && !old.sort_ascending && old.windows_live_refresh && old.windows_drive_labels && old.windows_expand_drives);
        assert!(!old.windows_compact_view);
        assert!(parse("version=1\nshow_hidden=true\nsort_ascending=true\nwindows_live_refresh=true\nwindows_live_refresh=false\n").is_err());
    }
    #[test]
    fn rejects_bad_files_without_overwriting() {
        let dir = crate::test_support::scratch_dir("settings-invalid");
        let path = dir.join("settings.conf");
        for value in ["version=2\n", "version=1\nshow_hidden=true\nshow_hidden=false\nsort_ascending=true\n", "invalid", "version=1\nshow_hidden=true\nsort_ascending=no\n"] {
            fs::write(&path, value).unwrap();
            assert!(load(&path).is_err());
            assert!(save(&path, Settings::default()).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), value);
        }
        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        assert!(load(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlink_files_and_directories() {
        use std::os::unix::fs::symlink;
        let dir = crate::test_support::scratch_dir("settings-links");
        let target = dir.join("target");
        fs::write(&target, "keep").unwrap();
        let link = dir.join("settings.conf");
        symlink(&target, &link).unwrap();
        assert!(load(&link).is_err());
        assert!(save(&link, Settings::default()).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
        let linked_dir = dir.join("linked");
        symlink(&dir, &linked_dir).unwrap();
        assert!(save(&linked_dir.join("other"), Settings::default()).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
