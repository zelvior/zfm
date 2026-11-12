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
        let metadata = match item.metadata() {
            Ok(m) => Some(m),
            Err(_) => {
                skipped += 1;
                None
            }
        };
        // Follow links for folder navigation, but never recurse while listing.
        let is_dir = match metadata.as_ref() {
            Some(m) if m.file_type().is_symlink() => {
                fs::metadata(item.path()).map_or(false, |target| target.is_dir())
            }
            Some(m) => m.is_dir(),
            None => false,
        };
        entries.push(Entry {
            path: item.path(),
            name,
            is_dir,
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
    use std::time::{SystemTime, UNIX_EPOCH};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "zfm-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
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
}
