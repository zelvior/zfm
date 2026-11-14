//! Review-only folder insights. These reports are not filesystem snapshots in the
//! atomic/security sense: concurrent changes and parent symlinks are possible.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_ENTRIES: usize = 5000;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_REPORT: usize = 250;
static NEXT: AtomicU64 = AtomicU64::new(0);
fn invalid(s: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, s) }
fn cancelled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) { Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled")) } else { Ok(()) }
}
fn link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; meta.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))] { meta.file_type().is_symlink() }
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Item { kind: char, size: u64, modified: i128 }
#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot { root: String, entries: BTreeMap<String, Item> }
fn stamp(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) { Ok(d) => d.as_nanos() as i128, Err(e) => -(e.duration().as_nanos() as i128) }
}
fn scan(root: &Path, cancel: &AtomicBool) -> io::Result<Snapshot> {
    cancelled(cancel)?;
    if !root.is_absolute() { return Err(invalid("Folder must be an absolute path")); }
    let root_text = root.to_str().ok_or_else(|| invalid("Folder path must be UTF-8"))?;
    if root_text.len() > 16384 { return Err(invalid("Folder path is too long for snapshot storage")); }
    let meta = fs::symlink_metadata(root)?;
    if link(&meta) || !meta.is_dir() { return Err(invalid("Folder must be a real directory, not a link")); }
    let mut entries = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        cancelled(cancel)?;
        let entry = entry?;
        if entries.len() >= MAX_ENTRIES { return Err(invalid("Folder exceeds the 5,000-entry limit; no partial snapshot was saved")); }
        let name = entry.file_name().into_string().map_err(|_| invalid("Folder contains a non-Unicode filename"))?;
        if name.len() > 16384 { return Err(invalid("Filename is too long for snapshot storage")); }
        let meta = fs::symlink_metadata(entry.path())?;
        let kind = if link(&meta) { 'l' } else if meta.is_file() { 'f' } else if meta.is_dir() { 'd' } else { 's' };
        let item = Item { kind, size: if kind == 'f' { meta.len() } else { 0 }, modified: stamp(meta.modified()?) };
        entries.insert(name, item);
    }
    Ok(Snapshot { root: root_text.to_owned(), entries })
}
fn hex(s: &str) -> String { s.as_bytes().iter().map(|b| format!("{b:02x}")).collect() }
fn unhex(s: &str) -> io::Result<String> {
    if s.len() % 2 != 0 || s.len() > 32768 { return Err(invalid("Invalid snapshot text")); }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for pair in s.as_bytes().chunks_exact(2) {
        let n = |b: u8| match b { b'0'..=b'9' => Some(b-b'0'), b'a'..=b'f' => Some(b-b'a'+10), _ => None };
        bytes.push(n(pair[0]).zip(n(pair[1])).map(|(a,b)| a*16+b).ok_or_else(|| invalid("Invalid snapshot encoding"))?);
    }
    String::from_utf8(bytes).map_err(|_| invalid("Invalid snapshot UTF-8"))
}
fn encode(s: &Snapshot) -> String {
    let mut out = format!("zfm-folder-snapshot-1\n{}\n", hex(&s.root));
    for (name, item) in &s.entries { out.push_str(&format!("{}\t{}\t{}\t{}\n", item.kind, item.size, item.modified, hex(name))); }
    out
}
fn decode(text: &str) -> io::Result<Snapshot> {
    let mut lines = text.lines();
    if lines.next() != Some("zfm-folder-snapshot-1") { return Err(invalid("Unrecognized snapshot format")); }
    let root = unhex(lines.next().ok_or_else(|| invalid("Missing snapshot folder"))?)?;
    if !Path::new(&root).is_absolute() { return Err(invalid("Invalid snapshot folder")); }
    let mut entries = BTreeMap::new();
    for line in lines {
        if entries.len() >= MAX_ENTRIES { return Err(invalid("Snapshot has too many entries")); }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 4 { return Err(invalid("Invalid snapshot entry")); }
        let kind = match fields[0] { "f" => 'f', "d" => 'd', "l" => 'l', "s" => 's', _ => return Err(invalid("Invalid snapshot kind")) };
        let size = fields[1].parse().map_err(|_| invalid("Invalid snapshot size"))?;
        let modified = fields[2].parse().map_err(|_| invalid("Invalid snapshot time"))?;
        let name = unhex(fields[3])?;
        crate::operations::validate_name(std::ffi::OsStr::new(&name)).map_err(|_| invalid("Invalid snapshot name"))?;
        if entries.insert(name, Item { kind, size, modified }).is_some() { return Err(invalid("Duplicate snapshot entry")); }
    }
    Ok(Snapshot { root, entries })
}
pub fn snapshot_path() -> io::Result<PathBuf> { Ok(crate::settings::user_path()?.with_file_name("folder-snapshot-v1")) }
fn load(path: &Path) -> io::Result<Snapshot> {
    let parent = path.parent().ok_or_else(|| invalid("Invalid snapshot location"))?;
    if !crate::settings::check_directory(parent)? { return Err(io::Error::new(io::ErrorKind::NotFound, "No saved folder snapshot")); }
    let file = crate::settings::open_existing(path)?;
    if file.metadata()?.len() > MAX_BYTES { return Err(invalid("Snapshot too large")); }
    let mut bytes = Vec::new(); file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES { return Err(invalid("Snapshot too large")); }
    decode(std::str::from_utf8(&bytes).map_err(|_| invalid("Invalid snapshot UTF-8"))?)
}
/// Stores one baseline per user. The caller must explicitly confirm replacement.
pub fn save_snapshot(root: &Path, cancel: &AtomicBool) -> io::Result<String> {
    let snapshot = scan(root, cancel)?;
    let contents = encode(&snapshot);
    if contents.len() as u64 > MAX_BYTES { return Err(invalid("Snapshot exceeds storage limit")); }
    let path = snapshot_path()?;
    let dir = path.parent().ok_or_else(|| invalid("Invalid snapshot location"))?;
    if !crate::settings::check_directory(dir)? {
        #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?; }
        #[cfg(not(unix))] fs::create_dir_all(dir)?;
        if !crate::settings::check_directory(dir)? { return Err(invalid("Configuration folder missing")); }
    }
    match load(&path) { Ok(_) => (), Err(e) if e.kind() == io::ErrorKind::NotFound => (), Err(e) => return Err(e) }
    for _ in 0..100 {
        cancelled(cancel)?;
        let temp = dir.join(format!(".snapshot-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut options = OpenOptions::new(); options.write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        match options.open(&temp) {
            Ok(mut f) => {
                let result = (|| {
                    f.write_all(contents.as_bytes())?; f.sync_all()?; drop(f); cancelled(cancel)?;
                    match load(&path) { Ok(_) => (), Err(e) if e.kind() == io::ErrorKind::NotFound => (), Err(e) => return Err(e) }
                    crate::settings::replace(&temp, &path)
                })();
                if result.is_err() { let _ = fs::remove_file(&temp); }
                result?;
                return Ok(format!("Saved a baseline for {} ({} immediate entries).\n\nStored in your ZFM configuration directory; filenames and metadata can be sensitive. This is not a backup.", root.display(), snapshot.entries.len()));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(invalid("Could not create a snapshot staging file"))
}
pub fn compare_snapshot(root: &Path, cancel: &AtomicBool) -> io::Result<String> {
    let saved = load(&snapshot_path()?)?;
    let now = scan(root, cancel)?;
    if saved.root != now.root { return Err(invalid("The saved baseline belongs to a different folder. Open that folder or save a new baseline.")); }
    let mut changes = Vec::new();
    for (name, item) in &saved.entries {
        match now.entries.get(name) { None => changes.push(format!("Removed: {name:?}")), Some(new) if new != item => changes.push(format!("Metadata changed: {name:?}")), _ => () }
    }
    for name in now.entries.keys() { if !saved.entries.contains_key(name) { changes.push(format!("Added: {name:?}")); } }
    let total = changes.len();
    let mut result = format!("Folder comparison: {}\n{total} change(s) among immediate entries.\n\n", root.display());
    if total == 0 { result.push_str("No metadata changes detected.\n"); }
    for line in changes.iter().take(MAX_REPORT) { result.push_str(line); result.push('\n'); }
    if total > MAX_REPORT { result.push_str(&format!("… {} additional changes not shown.\n", total-MAX_REPORT)); }
    result.push_str("\nNames, type, file size and modification time only; content is not hashed. Renames appear as removal and addition. Subfolders are not scanned. Concurrent changes may be missed. The baseline is unchanged.");
    Ok(result)
}
pub fn cleanup_report(root: &Path, cancel: &AtomicBool) -> io::Result<String> {
    let snapshot = scan(root, cancel)?;
    let now = stamp(SystemTime::now());
    let old = 180i128 * 24 * 60 * 60 * 1_000_000_000;
    let mut suggestions = Vec::new();
    for (name, item) in &snapshot.entries {
        if item.kind != 'f' { continue; }
        let lower = name.to_ascii_lowercase();
        let mut reasons = Vec::new();
        if item.size == 0 { reasons.push("empty file"); }
        if lower.ends_with(".tmp") || lower.ends_with(".bak") || lower.ends_with('~') { reasons.push("temporary/backup-like name"); }
        if item.size >= 100 * 1024 * 1024 && now.saturating_sub(item.modified) >= old { reasons.push("at least 100 MiB and not modified for 180 days"); }
        if !reasons.is_empty() { suggestions.push(format!("{name:?} — {} bytes — {}", item.size, reasons.join(", "))); }
    }
    let total = suggestions.len();
    let mut out = format!("Cleanup review: {}\n{total} possible candidate(s), from immediate entries only.\n\n", root.display());
    for line in suggestions.iter().take(MAX_REPORT) { out.push_str(line); out.push('\n'); }
    if total > MAX_REPORT { out.push_str(&format!("… {} more not shown.\n", total-MAX_REPORT)); }
    if total == 0 { out.push_str("No candidates matched these rules.\n"); }
    out.push_str("\nThese are clues, not proof of clutter or safe deletion. Review each item yourself. Nothing was deleted or changed; links and subfolders are not inspected.");
    Ok(out)
}
#[derive(Clone, Copy)]
pub enum PreviewKind { Copy, Move, Rename, Archive }
pub fn impact(kind: PreviewKind, paths: &[PathBuf], destination: &Path, cancel: &AtomicBool) -> io::Result<String> {
    if paths.is_empty() || paths.len() > crate::operations::MAX_COPY_ITEMS || !destination.is_absolute() { return Err(invalid("Select 1–1024 items and an absolute destination")); }
    if matches!(kind, PreviewKind::Rename) && paths.len() != 1 { return Err(invalid("Rename preview requires exactly one item")); }
    let label = match kind { PreviewKind::Copy => "Copy", PreviewKind::Move => "Move", PreviewKind::Rename => "Rename", PreviewKind::Archive => "Create archive" };
    let mut out = format!("{label} impact preview — no changes made\n\n");
    let parent = if matches!(kind, PreviewKind::Copy | PreviewKind::Move) { destination } else { destination.parent().ok_or_else(|| invalid("Destination has no parent"))? };
    let parent_meta = fs::symlink_metadata(parent)?;
    if link(&parent_meta) || !parent_meta.is_dir() { return Err(invalid("Destination folder must be an existing real directory")); }
    if matches!(kind, PreviewKind::Rename | PreviewKind::Archive) { crate::operations::validate_name(destination.file_name().ok_or_else(|| invalid("Missing destination name"))?)?; }
    if matches!(kind, PreviewKind::Archive) && !crate::archive_actions::Format::ALL.iter().any(|f| f.matches(destination)) { out.push_str("Warning: unsupported archive extension.\n"); }
    let mut seen = BTreeSet::new();
    for (i, path) in paths.iter().enumerate() {
        cancelled(cancel)?;
        let target = if matches!(kind, PreviewKind::Copy | PreviewKind::Move) { parent.join(path.file_name().ok_or_else(|| invalid("Source has no name"))?) } else { destination.to_owned() };
        let mut warnings = Vec::new();
        match fs::symlink_metadata(path) {
            Ok(meta) if link(&meta) || (!meta.is_file() && !meta.is_dir()) => warnings.push("link or special entry; supported operations may reject it"),
            Err(_) => warnings.push("source cannot be inspected"), _ => ()
        }
        if path == &target || (matches!(kind, PreviewKind::Copy | PreviewKind::Move) && target.starts_with(path)) { warnings.push("destination is the source or inside it"); }
        if !matches!(kind, PreviewKind::Archive) && !seen.insert(target.clone()) { warnings.push("multiple sources have the same destination"); }
        match fs::symlink_metadata(&target) { Ok(_) => warnings.push("destination already exists; no overwrite"), Err(e) if e.kind() == io::ErrorKind::NotFound => (), Err(_) => warnings.push("destination cannot be inspected") }
        if i < MAX_REPORT { out.push_str(&format!("{:?} → {:?}{}\n", path, target, if warnings.is_empty() { String::new() } else { format!("  ⚠ {}", warnings.join("; ")) })); }
    }
    if paths.len() > MAX_REPORT { out.push_str(&format!("… {} additional mappings not shown.\n", paths.len()-MAX_REPORT)); }
    out.push_str("\nAdvisory only: does not inspect folder contents, test permissions, free space, archive limits, cross-volume moves or predict all failures. Files may change after this preview. Use the normal action to proceed; nothing was executed.");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn scan_compare_codec_cleanup_and_preview() {
        let root = crate::test_support::scratch_dir("insights");
        fs::write(root.join("empty.tmp"), b"").unwrap();
        let cancel = AtomicBool::new(false);
        let snapshot = scan(&root, &cancel).unwrap();
        assert_eq!(decode(&encode(&snapshot)).unwrap(), snapshot);
        assert!(decode("zfm-folder-snapshot-1\n2f\nf\t0\t1\t2e2e\n").is_err());
        let cleanup = cleanup_report(&root, &cancel).unwrap();
        assert!(cleanup.contains("empty file") && cleanup.contains("temporary/backup-like name"));
        let preview = impact(PreviewKind::Copy, &[root.join("empty.tmp")], &root, &cancel).unwrap();
        assert!(preview.contains("destination already exists") && preview.contains("no changes made"));
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(scan(&root, &cancel).unwrap_err().kind(), io::ErrorKind::Interrupted);
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)] #[test] fn refuses_root_symlink() {
        let root = crate::test_support::scratch_dir("insight-link");
        std::os::unix::fs::symlink(&root, root.join("link")).unwrap();
        assert!(scan(&root.join("link"), &AtomicBool::new(false)).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

pub enum Job {
    Save(PathBuf),
    Compare(PathBuf),
    Cleanup(PathBuf),
    Impact(PreviewKind, Vec<PathBuf>, PathBuf),
}
impl Job {
    pub fn title(&self) -> &'static str {
        match self { Self::Save(_) => "Folder snapshot", Self::Compare(_) => "Folder changes", Self::Cleanup(_) => "Cleanup review", Self::Impact(..) => "Operation impact preview" }
    }
    pub fn run(self, cancel: &AtomicBool) -> io::Result<String> {
        match self { Self::Save(p) => save_snapshot(&p, cancel), Self::Compare(p) => compare_snapshot(&p, cancel), Self::Cleanup(p) => cleanup_report(&p, cancel), Self::Impact(kind, paths, target) => impact(kind, &paths, &target, cancel) }
    }
}
