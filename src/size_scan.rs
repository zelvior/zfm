//! Bounded, cancellable apparent-size scan. Not a filesystem snapshot.
use std::fs::{self, ReadDir};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_ENTRIES: usize = 100_000;
pub const MAX_DEPTH: usize = 128;

#[derive(Debug, Default)]
pub struct SizeResult {
    pub selection: String,
    pub bytes: u64,
    pub files: usize,
    pub folders: usize,
    pub skipped: usize,
    /// None means enumeration finished (although links and special files may have been skipped).
    pub stopped: Option<String>,
}
impl SizeResult {
    pub fn message(&self) -> String {
        let state = match &self.stopped {
            Some(reason) => format!("Incomplete: {reason}"),
            None => "Scan finished.".to_string(),
        };
        format!("{state}\n{}\nApparent file size: {} bytes\nFiles: {}\nFolders: {}\nLinks/reparse points and special entries skipped: {}\n\nHard links are counted per path. Hidden entries are included. This is not disk usage or a snapshot.", self.selection, self.bytes, self.files, self.folders, self.skipped)
    }
}

/// Calculate summed apparent lengths, counting each regular-file path once.
/// Does not intentionally follow links/reparse points. Enumeration stops on
/// errors, cancellation, overflow, entry or depth limits and reports partial
/// results. Concurrent path replacement can defeat no-follow checks; use only
/// trusted directories. Mount points are crossed. An OS call can block.
pub fn calculate<F: FnMut(usize)>(paths: &[PathBuf], cancel: &AtomicBool, progress: F) -> io::Result<SizeResult> {
    calculate_with_limits(paths, cancel, MAX_ENTRIES, MAX_DEPTH, progress)
}
fn calculate_with_limits<F: FnMut(usize)>(paths: &[PathBuf], cancel: &AtomicBool, max_entries: usize, max_depth: usize, mut progress: F) -> io::Result<SizeResult> {
    crate::operations::validate_batch_selection(paths)?;
    let mut result = SizeResult { selection: format!("Selection: {:?} ({} selected)", paths[0], paths.len()), ..SizeResult::default() };
    let mut stack: Vec<(ReadDir, usize)> = Vec::new();
    let mut roots = paths.iter();
    let mut count = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) { result.stopped = Some("Cancelled".into()); break; }
        let next = if let Some((entries, depth)) = stack.last_mut() {
            match entries.next() {
                Some(Ok(entry)) => Some((entry.path(), *depth + 1)),
                Some(Err(error)) => { result.stopped = Some(format!("Could not read directory entry: {error}")); break; }
                None => { stack.pop(); continue; }
            }
        } else { roots.next().map(|p| (p.clone(), 0)) };
        let (path, depth) = match next { Some(item) => item, None => break };
        if count >= max_entries { result.stopped = Some(format!("Entry limit of {max_entries} reached")); break; }
        if depth > max_depth { result.stopped = Some(format!("Depth limit of {max_depth} reached")); break; }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(error) => { result.stopped = Some(format!("Could not inspect {:?}: {error}", path)); break; }
        };
        count += 1;
        #[cfg(windows)]
        let is_link = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let is_link = metadata.file_type().is_symlink();
        if is_link {
            result.skipped += 1;
        } else if metadata.is_file() {
            match result.bytes.checked_add(metadata.len()) {
                Some(bytes) => result.bytes = bytes,
                None => { result.stopped = Some("File size total overflowed".into()); break; }
            }
            result.files += 1;
        } else if metadata.is_dir() {
            result.folders += 1;
            match fs::read_dir(&path) {
                Ok(entries) => stack.push((entries, depth)),
                Err(error) => { result.stopped = Some(format!("Could not read {:?}: {error}", path)); break; }
            }
        } else {
            result.skipped += 1;
        }
        progress(count);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp { fn new() -> Self { Self(crate::test_support::scratch_dir("zfm-size")) } }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[test]
    fn totals_hidden_nested_and_hard_links() {
        let t = Temp::new();
        fs::create_dir(t.0.join("folder")).unwrap();
        fs::write(t.0.join("folder/.hidden"), b"abc").unwrap();
        fs::hard_link(t.0.join("folder/.hidden"), t.0.join("second")).unwrap();
        let result = calculate(&[t.0.clone()], &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!((result.bytes, result.files, result.folders, result.skipped), (6, 2, 2, 0));
        assert!(result.stopped.is_none());
    }
    #[test]
    fn limits_cancellation_and_errors_are_incomplete() {
        let t = Temp::new(); fs::create_dir(t.0.join("nested")).unwrap(); fs::write(t.0.join("nested/file"), b"abc").unwrap();
        let paths = [t.0.clone()]; let cancel = AtomicBool::new(false);
        let limited = calculate_with_limits(&paths, &cancel, 1, 128, |_| {}).unwrap();
        assert!(limited.stopped.unwrap().contains("Entry limit"));
        let depth = calculate_with_limits(&paths, &cancel, 100, 0, |_| {}).unwrap();
        assert!(depth.stopped.unwrap().contains("Depth limit"));
        let cancelled = calculate(&paths, &cancel, |_| cancel.store(true, Ordering::Relaxed)).unwrap();
        assert!(cancelled.stopped.unwrap().contains("Cancelled"));
        let missing = calculate(&[t.0.join("missing")], &AtomicBool::new(false), |_| {}).unwrap();
        assert!(missing.stopped.unwrap().contains("Could not inspect"));
        assert!(calculate(&[t.0.clone(), t.0.join("nested")], &AtomicBool::new(false), |_| {}).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks() {
        let t = Temp::new(); fs::write(t.0.join("file"), b"1234").unwrap();
        std::os::unix::fs::symlink(&t.0, t.0.join("cycle")).unwrap();
        let result = calculate(&[t.0.clone()], &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!((result.bytes, result.files, result.folders, result.skipped), (4, 1, 1, 1));
        assert!(result.stopped.is_none());
    }
}
