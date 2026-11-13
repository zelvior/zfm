use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

#[derive(Debug, PartialEq, Eq)]
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
    /// Search stopped at an entry, result or depth limit.
    pub truncated: bool,
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
    Ok(Some(Listing { entries, skipped, truncated: false }))
}

/// Hard bounds keep recursive searches from consuming unbounded time/memory.
pub const SEARCH_MAX_ENTRIES: usize = 100_000;
pub const SEARCH_MAX_RESULTS: usize = 5_000;
pub const SEARCH_MAX_DEPTH: usize = 64;

/// Find names beneath `root` without descending into symbolic links or Windows
/// reparse points. This is a best-effort walk, not a defense against a hostile
/// process concurrently replacing path components. Cancellation is cooperative.
pub fn search_recursive(root: &Path, show_hidden: bool, query: &str, case_sensitive: bool,
    generation: &AtomicU64, expected: u64) -> io::Result<Option<Listing>> {
    search_with_limits(root, show_hidden, query, case_sensitive, generation, expected,
        SearchLimits { entries: SEARCH_MAX_ENTRIES, results: SEARCH_MAX_RESULTS, depth: SEARCH_MAX_DEPTH })
}

#[derive(Clone, Copy)]
struct SearchLimits { entries: usize, results: usize, depth: usize }
fn search_with_limits(root: &Path, show_hidden: bool, query: &str, case_sensitive: bool,
    generation: &AtomicU64, expected: u64, limits: SearchLimits) -> io::Result<Option<Listing>> {
    if query.is_empty() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "Enter a search term first")); }
    if generation.load(Ordering::Relaxed) != expected { return Ok(None); }
    let query = if case_sensitive { query.to_owned() } else { query.to_lowercase() };
    let mut todo = vec![(root.to_path_buf(), 0usize)];
    let mut entries = Vec::new();
    let mut visited = 0usize;
    let mut skipped = 0usize;
    let mut truncated = false;
    while let Some((directory, depth)) = todo.pop() {
        if generation.load(Ordering::Relaxed) != expected { return Ok(None); }
        let children = match fs::read_dir(&directory) {
            Ok(children) => children,
            Err(error) if depth == 0 => return Err(error),
            Err(_) => { skipped += 1; continue; }
        };
        for child in children {
            if generation.load(Ordering::Relaxed) != expected { return Ok(None); }
            if visited >= limits.entries { truncated = true; break; }
            visited += 1;
            let child = match child { Ok(child) => child, Err(_) => { skipped += 1; continue; } };
            let name = child.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') { continue; }
            let path = child.path();
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta, Err(_) => { skipped += 1; continue; }
            };
            #[cfg(windows)]
            let (hidden, reparse) = {
                use std::os::windows::fs::MetadataExt;
                (meta.file_attributes() & 0x2 != 0, meta.file_attributes() & 0x400 != 0)
            };
            #[cfg(not(windows))]
            let (hidden, reparse) = (false, false);
            if !show_hidden && hidden { continue; }
            let link = meta.file_type().is_symlink() || reparse;
            if meta.is_dir() && !link {
                if depth < limits.depth { todo.push((path.clone(), depth + 1)); }
                else { truncated = true; }
            }
            let matches = if case_sensitive { name.contains(&query) } else { name.to_lowercase().contains(&query) };
            if !matches { continue; }
            if entries.len() >= limits.results { truncated = true; break; }
            let is_dir = if link { fs::metadata(&path).map_or(false, |m| m.is_dir()) } else { meta.is_dir() };
            entries.push(Entry { path, name, is_dir, is_file: !link && meta.is_file(),
                size: if is_dir { None } else { Some(meta.len()) }, modified: meta.modified().ok() });
        }
        if visited >= limits.entries || entries.len() >= limits.results { truncated = true; break; }
    }
    if generation.load(Ordering::Relaxed) != expected { return Ok(None); }
    Ok(Some(Listing { entries, skipped, truncated }))
}

/// Display search hits relative to their search root; preserve the actual path
/// and name for file actions, even when different folders contain the same name.
pub fn display_name(entry: &Entry, root: &Path, recursive: bool) -> String {
    if recursive { entry.path.strip_prefix(root).unwrap_or(&entry.path).to_string_lossy().into_owned() }
    else { entry.name.clone() }
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
    fn recursive_search_matches_nested_names_and_reports_relative_paths() {
        let t = Temp::new();
        fs::create_dir_all(t.0.join("nested/deep")).unwrap();
        fs::create_dir_all(t.0.join(".hidden")).unwrap();
        fs::write(t.0.join("needle.txt"), b"root").unwrap();
        fs::write(t.0.join("nested/deep/Needle.txt"), b"nested").unwrap();
        fs::write(t.0.join(".hidden/needle.txt"), b"hidden").unwrap();
        let generation = AtomicU64::new(1);
        let found = search_recursive(&t.0, false, "needle", false, &generation, 1).unwrap().unwrap();
        assert_eq!(found.entries.len(), 2);
        assert!(!found.truncated);
        assert!(found.entries.iter().any(|e| display_name(e, &t.0, true) == Path::new("nested").join("deep").join("Needle.txt").to_string_lossy()));
        assert_eq!(search_recursive(&t.0, true, "needle", false, &generation, 1).unwrap().unwrap().entries.len(), 3);
        assert_eq!(search_recursive(&t.0, true, "needle", true, &generation, 1).unwrap().unwrap().entries.len(), 2);
        assert!(search_recursive(&t.0, true, "", false, &generation, 1).is_err());
        assert!(search_recursive(&t.0, false, "needle", false, &generation, 0).unwrap().is_none());
        assert!(search_recursive(&t.0.join("missing"), false, "x", false, &generation, 1).is_err());
    }
    #[test]
    fn recursive_search_reports_entry_and_result_limits() {
        let t = Temp::new();
        for i in 0..5 { fs::write(t.0.join(format!("hit-{i}")), b"x").unwrap(); }
        let generation = AtomicU64::new(1);
        let results = search_with_limits(&t.0, false, "hit", false, &generation, 1,
            SearchLimits { entries: 100, results: 2, depth: 64 }).unwrap().unwrap();
        assert_eq!(results.entries.len(), 2);
        assert!(results.truncated);
        let entries = search_with_limits(&t.0, false, "absent", false, &generation, 1,
            SearchLimits { entries: 2, results: 100, depth: 64 }).unwrap().unwrap();
        assert!(entries.truncated);
    }
    #[cfg(unix)]
    #[test]
    fn recursive_search_does_not_traverse_symlinks() {
        use std::os::unix::fs::symlink;
        let t = Temp::new();
        let external = Temp::new();
        fs::write(external.0.join("only-external-hit"), b"x").unwrap();
        symlink(&external.0, t.0.join("external")).unwrap();
        symlink(&t.0, t.0.join("cycle")).unwrap();
        let result = search_recursive(&t.0, true, "hit", false, &AtomicU64::new(1), 1).unwrap().unwrap();
        assert!(result.entries.is_empty());
    }
    #[test]
    fn recursive_search_limits_depth() {
        let t = Temp::new();
        let mut path = t.0.clone();
        for _ in 0..4 { path.push("a"); fs::create_dir(&path).unwrap(); }
        let result = search_with_limits(&t.0, false, "absent", false, &AtomicU64::new(1), 1,
            SearchLimits { entries: 100, results: 100, depth: 2 }).unwrap().unwrap();
        assert!(result.truncated);
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
        assert_eq!(search_recursive(&t.0, false, "invisible", false, &generation, 1).unwrap().unwrap().entries.len(), 0);
        assert_eq!(search_recursive(&t.0, true, "invisible", false, &generation, 1).unwrap().unwrap().entries.len(), 1);
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
