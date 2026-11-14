//! Bounded, review-only duplicate and junk candidate discovery.
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

const MAX_ENTRIES: usize = 20_000;
const MAX_DEPTH: usize = 64;
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_HASH_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_LINES: usize = 150;
fn cancelled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) { Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled")) } else { Ok(()) }
}
fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; meta.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))] { meta.file_type().is_symlink() }
}
fn display(path: &Path) -> String {
    let text = format!("{:?}", path); let mut chars = text.chars();
    let prefix: String = chars.by_ref().take(300).collect();
    if chars.next().is_some() { format!("{prefix}…") } else { prefix }
}
/// Recursively scan only after explicit invocation; reports are advisory and do
/// not delete files. Limits, omissions and errors are always reported.
pub fn report(root: &Path, cancel: &AtomicBool) -> io::Result<String> {
    cancelled(cancel)?;
    let meta = fs::symlink_metadata(root)?;
    if !root.is_absolute() || is_link(&meta) || !meta.is_dir() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "Select an absolute real directory")); }
    let mut dirs = vec![(root.to_path_buf(), 0usize)];
    let mut sizes: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    let mut junk = Vec::new(); let mut seen = 0usize; let mut skipped = 0usize; let mut truncated = false;
    let cutoff = SystemTime::now().checked_sub(Duration::from_secs(30 * 24 * 60 * 60));
    'scan: while let Some((dir, depth)) = dirs.pop() {
        cancelled(cancel)?;
        let reader = match fs::read_dir(&dir) { Ok(reader) => reader, Err(_) => { skipped += 1; continue; } };
        for entry in reader {
            cancelled(cancel)?;
            if seen >= MAX_ENTRIES { truncated = true; break 'scan; }
            seen += 1;
            let entry = match entry { Ok(e) => e, Err(_) => { skipped += 1; continue; } };
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) { Ok(m) => m, Err(_) => { skipped += 1; continue; } };
            if is_link(&meta) { skipped += 1; continue; }
            if meta.is_dir() {
                if depth < MAX_DEPTH { dirs.push((path, depth + 1)); } else { truncated = true; }
            } else if meta.is_file() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                let old = cutoff.zip(meta.modified().ok()).map_or(false, |(limit, modified)| modified <= limit);
                let reason = if meta.len() == 0 { Some("empty file") }
                    else if old && (name.ends_with(".tmp") || name.ends_with(".bak") || name.ends_with('~')) { Some("30+ day old temporary/backup-like name") }
                    else { None };
                if let Some(reason) = reason { junk.push((path.clone(), reason)); }
                sizes.entry(meta.len()).or_default().push(path);
            } else { skipped += 1; }
        }
    }
    let mut groups: BTreeMap<(u64, String), Vec<PathBuf>> = BTreeMap::new();
    let mut budget = 0u64; let mut hash_skipped = 0usize;
    for (size, mut paths) in sizes {
        if paths.len() < 2 { continue; }
        paths.sort();
        for path in paths {
            cancelled(cancel)?;
            if size > MAX_FILE_BYTES || budget.saturating_add(size) > MAX_HASH_BYTES { hash_skipped += 1; continue; }
            budget += size;
            match crate::hashing::hash_file(&path, cancel) {
                Ok(h) if h.bytes == size => groups.entry((size, h.sha256)).or_default().push(path),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
                _ => hash_skipped += 1,
            }
        }
    }
    junk.sort_by(|a,b| a.0.cmp(&b.0));
    let group_count = groups.values().filter(|g| g.len() > 1).count();
    let mut out = format!("Duplicate and junk review: {:?}\nEntries scanned: {seen}; duplicate SHA-256 groups: {group_count}; junk candidates: {}.\nSkipped unreadable, linked or special entries: {skipped}; skipped changed, unreadable or over-budget hash candidates: {hash_skipped}; scan limit reached: {truncated}.\n\n", root, junk.len());
    out.push_str("Duplicate hash groups (size and SHA-256):\n");
    let mut lines = 0usize; let mut omitted = 0usize;
    for ((size, hash), paths) in groups.into_iter().filter(|(_, paths)| paths.len() > 1) {
        for path in paths {
            if lines < MAX_LINES { out.push_str(&format!("{size} bytes {hash}  {}\n", display(&path))); lines += 1; } else { omitted += 1; }
        }
    }
    out.push_str("\nJunk candidates:\n");
    for (path, reason) in junk {
        if lines < MAX_LINES { out.push_str(&format!("{reason}: {}\n", display(&path))); lines += 1; } else { omitted += 1; }
    }
    if omitted > 0 { out.push_str(&format!("\n{omitted} additional paths omitted from this report.\n")); }
    out.push_str("\nReview only: nothing was deleted. Matching hashes are not byte-by-byte proof, hard links may appear separately, and junk names are only heuristics. Hidden files are included. Changes during scanning may be missed. Use only trusted folders; concurrent replacement of parent paths is not prevented. Limits: 20,000 entries, 64 folder levels, 1 GiB/file, 2 GiB total hashed, 150 reported paths. Incomplete scans cannot prove uniqueness.");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn duplicates_junk_and_cancellation() {
        let root = crate::test_support::scratch_dir("duplicates");
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("a"), b"same").unwrap(); fs::write(root.join("nested/b"), b"same").unwrap();
        fs::write(root.join("different"), b"diff").unwrap(); fs::write(root.join("empty"), b"").unwrap();
        let out = report(&root, &AtomicBool::new(false)).unwrap();
        assert!(out.contains("duplicate SHA-256 groups: 1")); assert!(out.contains("junk candidates: 1"));
        assert!(out.contains("nested") && out.contains("empty"));
        assert_eq!(report(&root, &AtomicBool::new(true)).unwrap_err().kind(), io::ErrorKind::Interrupted);
        fs::remove_dir_all(root).unwrap();
    }
}
