use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

#[derive(Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// True only for a regular file, not a symlink to one.
    pub is_file: bool,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

#[derive(Debug)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub skipped: usize,
}

/// Read metadata on the worker, not the UI thread. Symlinks are not traversed
/// during enumeration; a broken link remains visible. Cancellation is checked
/// between entries. A blocked OS filesystem call itself cannot be interrupted.
pub fn read(
    path: &Path,
    show_hidden: bool,
    generation: &AtomicU64,
    expected: u64,
) -> io::Result<Option<Listing>> {
    read_filtered(path, show_hidden, "", false, generation, expected)
}

/// Filter by a literal substring of the displayed filename, before loading
/// metadata. The caller controls when a search starts; typing does not scan.
pub fn read_filtered(
    path: &Path,
    show_hidden: bool,
    query: &str,
    case_sensitive: bool,
    generation: &AtomicU64,
    expected: u64,
) -> io::Result<Option<Listing>> {
    let query = if case_sensitive {
        query.to_owned()
    } else {
        query.to_lowercase()
    };
    let mut entries = Vec::new();
    let mut skipped = 0;
    for item in fs::read_dir(path)? {
        if generation.load(Ordering::Relaxed) != expected {
            return Ok(None);
        }
        let item = match item {
            Ok(v) => v,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let name = item.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if !query.is_empty() {
            let matches = if case_sensitive {
                name.contains(&query)
            } else {
                name.to_lowercase().contains(&query)
            };
            if !matches {
                continue;
            }
        }
        let metadata = match item.metadata() {
            Ok(m) => Some(m),
            Err(_) => {
                skipped += 1;
                None
            }
        };
        #[cfg(windows)]
        if !show_hidden {
            use std::os::windows::fs::MetadataExt;
            // Windows hidden attributes are separate from Unix-style dotfiles.
            if metadata
                .as_ref()
                .map_or(false, |m| m.file_attributes() & 0x2 != 0)
            {
                continue;
            }
        }
        // Follow links for folder navigation, but never recurse while listing.
        let is_dir = match metadata.as_ref() {
            Some(m) if m.file_type().is_symlink() => {
                fs::metadata(item.path()).map_or(false, |target| target.is_dir())
            }
            Some(m) => m.is_dir(),
            None => false,
        };
        let is_file = metadata.as_ref().map_or(false, |m| m.is_file());
        entries.push(Entry {
            path: item.path(),
            name,
            is_dir,
            is_file,
            size: metadata.as_ref().filter(|_| !is_dir).map(|m| m.len()),
            modified: metadata.and_then(|m| m.modified().ok()),
        });
    }
    entries.sort_by_cached_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
    if generation.load(Ordering::Relaxed) != expected {
        return Ok(None);
    }
    Ok(Some(Listing { entries, skipped }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(crate::test_support::scratch_dir("zfm-test"))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn empty_hidden_unicode_and_sorting() {
        let t = Temp::new();
        let gen = AtomicU64::new(1);
        assert!(read(&t.0, false, &gen, 1)
            .unwrap()
            .unwrap()
            .entries
            .is_empty());
        fs::write(t.0.join("a file.txt"), b"123").unwrap();
        fs::write(t.0.join(".secret"), b"").unwrap();
        fs::write(t.0.join("é.txt"), b"").unwrap();
        fs::create_dir(t.0.join("z folder")).unwrap();
        let l = read(&t.0, false, &gen, 1).unwrap().unwrap();
        assert_eq!(l.entries.len(), 3);
        assert_eq!(l.entries[0].name, "z folder");
        assert_eq!(l.entries[1].size, Some(3));
        assert_eq!(read(&t.0, true, &gen, 1).unwrap().unwrap().entries.len(), 4);
    }
    #[test]
    fn cancellation_and_errors() {
        let t = Temp::new();
        fs::write(t.0.join("x"), b"").unwrap();
        let gen = AtomicU64::new(2);
        assert!(read(&t.0, false, &gen, 1).unwrap().is_none());
        assert!(read(&t.0.join("missing"), false, &gen, 2).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn broken_symlink_is_visible() {
        let t = Temp::new();
        std::os::unix::fs::symlink("no-target", t.0.join("broken")).unwrap();
        let l = read(&t.0, false, &AtomicU64::new(1), 1).unwrap().unwrap();
        assert_eq!(l.entries.len(), 1);
        assert!(!l.entries[0].is_dir);
    }
    #[cfg(windows)]
    #[test]
    fn windows_hidden_attribute_is_respected() {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn SetFileAttributesW(path: *const u16, attributes: u32) -> i32;
        }
        let t = Temp::new();
        let path = t.0.join("invisible.txt");
        fs::write(&path, b"hidden").unwrap();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        assert_ne!(unsafe { SetFileAttributesW(wide.as_ptr(), 2) }, 0);
        let generation = AtomicU64::new(1);
        assert_eq!(
            read(&t.0, false, &generation, 1)
                .unwrap()
                .unwrap()
                .entries
                .len(),
            0
        );
        assert_eq!(
            read(&t.0, true, &generation, 1)
                .unwrap()
                .unwrap()
                .entries
                .len(),
            1
        );
        assert_ne!(unsafe { SetFileAttributesW(wide.as_ptr(), 0x80) }, 0);
    }
    #[test]
    fn thousands_of_entries() {
        let t = Temp::new();
        for i in 0..2000 {
            fs::write(t.0.join(format!("file-{i:04}")), b"").unwrap();
        }
        assert_eq!(
            read(&t.0, false, &AtomicU64::new(1), 1)
                .unwrap()
                .unwrap()
                .entries
                .len(),
            2000
        );
    }
    #[test]
    fn filters_names_and_extensions_before_metadata() {
        let t = Temp::new();
        fs::write(t.0.join("Report.TXT"), b"a").unwrap();
        fs::write(t.0.join("other.txt"), b"b").unwrap();
        fs::write(t.0.join("image.png"), b"c").unwrap();
        let generation = AtomicU64::new(3);
        let listing = read_filtered(&t.0, false, ".txt", false, &generation, 3)
            .unwrap()
            .unwrap();
        assert_eq!(listing.entries.len(), 2);
        let listing = read_filtered(&t.0, false, "report", true, &generation, 3)
            .unwrap()
            .unwrap();
        assert!(listing.entries.is_empty());
        let listing = read_filtered(&t.0, false, "Report", true, &generation, 3)
            .unwrap()
            .unwrap();
        assert_eq!(listing.entries.len(), 1);
    }
}
