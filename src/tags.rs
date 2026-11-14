//! Path-based personal labels. Tags never modify the labelled files.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PATHS: usize = 5000;
const MAX_TAGS: usize = 32;
/// Maximum number of paths accepted by a batch tag action.
pub const MAX_BATCH: usize = 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);
type Entries = BTreeMap<PathBuf, Vec<String>>;
fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
pub fn user_path() -> io::Result<PathBuf> { Ok(crate::settings::user_path()?.with_file_name("tags.conf")) }
fn valid_path(path: &Path) -> bool {
    path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir))
        && path.to_str().map_or(false, |s| s.len() <= 4096 && !s.chars().any(char::is_control))
}
fn valid_tag(tag: &str) -> bool { !tag.is_empty() && tag.len() <= 64 && !tag.chars().any(|c| c.is_control() || c == ',') && tag.trim() == tag }
fn encode(text: &str) -> String { text.as_bytes().iter().map(|b| format!("{b:02x}")).collect() }
fn decode(text: &str) -> io::Result<String> {
    if text.len() % 2 != 0 { return Err(invalid("Invalid tag encoding")); }
    let digit = |ch: u8| match ch { b'0'..=b'9' => Some(ch-b'0'), b'a'..=b'f' => Some(ch-b'a'+10), _ => None };
    let mut bytes = Vec::with_capacity(text.len()/2);
    for pair in text.as_bytes().chunks_exact(2) {
        bytes.push(digit(pair[0]).ok_or_else(|| invalid("Invalid tag encoding"))? * 16 + digit(pair[1]).ok_or_else(|| invalid("Invalid tag encoding"))?);
    }
    String::from_utf8(bytes).map_err(|_| invalid("Invalid tag UTF-8"))
}
fn load(file: &Path) -> io::Result<Entries> {
    let dir = file.parent().ok_or_else(|| invalid("Invalid tag file path"))?;
    if !crate::settings::check_directory(dir)? { return Ok(Entries::new()); }
    let input = match crate::settings::open_existing(file) { Ok(f) => f, Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Entries::new()), Err(e) => return Err(e) };
    if input.metadata()?.len() > MAX_BYTES { return Err(invalid("Tag database is too large")); }
    let mut text = String::new(); input.take(MAX_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_BYTES { return Err(invalid("Tag database is too large")); }
    let mut lines = text.lines();
    if lines.next() != Some("version=1") { return Err(invalid("Unsupported tag database")); }
    let mut result = Entries::new();
    for line in lines {
        let (path, tags) = line.split_once('\t').ok_or_else(|| invalid("Malformed tag entry"))?;
        let path = PathBuf::from(decode(path)?);
        let tags: Vec<String> = tags.split(',').map(decode).collect::<io::Result<_>>()?;
        let distinct: BTreeSet<_> = tags.iter().collect();
        if !valid_path(&path) || tags.is_empty() || tags.len() > MAX_TAGS || distinct.len() != tags.len() || tags.iter().any(|tag| !valid_tag(tag)) || result.len() >= MAX_PATHS || result.insert(path, tags).is_some() { return Err(invalid("Invalid or duplicate tag entry")); }
    }
    Ok(result)
}
/// Returns tags for an absolute UTF-8 path; names are case-sensitive.
pub fn get(file: &Path, path: &Path) -> io::Result<Vec<String>> {
    if !valid_path(path) { return Err(invalid("Tagging requires an absolute UTF-8 path without control characters")); }
    Ok(load(file)?.remove(path).unwrap_or_default())
}
/// Find an exact, case-sensitive tag across the local path database. Results
/// are path records, not verified file identities; records can be stale.
pub fn find(file: &Path, tag: &str) -> io::Result<Vec<PathBuf>> {
    if !valid_tag(tag) { return Err(invalid("Enter one nonempty tag of at most 64 bytes")); }
    Ok(load(file)?.into_iter().filter_map(|(path, tags)| {
        if tags.iter().any(|value| value == tag) { Some(path) } else { None }
    }).collect())
}
/// Produce a bounded, selectable report without touching paths on disk.
pub fn find_report(file: &Path, tag: &str) -> io::Result<String> {
    let matches = find(file, tag)?;
    let mut report = format!("Tag {:?}: {} saved path(s).\n\n", tag, matches.len());
    let mut shown = 0;
    for path in matches.iter().take(250) {
        let line = format!("{}\n", path.display());
        if report.len() + line.len() > 48 * 1024 { break; }
        report.push_str(&line); shown += 1;
    }
    if matches.len() > shown { report.push_str(&format!("\n{} more paths not shown.\n", matches.len() - shown)); }
    report.push_str("\nPaths may be stale after moves, renames or deletion. This search does not check whether files exist.");
    Ok(report)
}

/// Replace all tags on one path. Empty input removes the entry.
pub fn set(file: &Path, path: &Path, tags: &[String]) -> io::Result<()> {
    let distinct: BTreeSet<_> = tags.iter().collect();
    if !valid_path(path) || tags.len() > MAX_TAGS || distinct.len() != tags.len() || tags.iter().any(|tag| !valid_tag(tag)) {
        return Err(invalid("Invalid path or tags; use up to 32 unique comma-free tags of at most 64 bytes"));
    }
    let mut entries = load(file)?;
    if tags.is_empty() { entries.remove(path); } else { entries.insert(path.to_path_buf(), tags.to_vec()); }
    save(file, &entries)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchMode { Add, Remove }

/// Add or remove labels from all selected path records. Changes are validated
/// together and published as one database replacement. File contents are never
/// modified; as with single-item edits, concurrent processes can overwrite one
/// another's updates.
pub fn edit_batch(file: &Path, paths: &[PathBuf], tags: &[String], mode: BatchMode) -> io::Result<usize> {
    let distinct_paths: BTreeSet<_> = paths.iter().collect();
    let distinct_tags: BTreeSet<_> = tags.iter().collect();
    if paths.is_empty() || paths.len() > MAX_BATCH || distinct_paths.len() != paths.len() || paths.iter().any(|path| !valid_path(path)) {
        return Err(invalid("Select 1–1024 distinct absolute UTF-8 paths without control characters"));
    }
    if tags.is_empty() || tags.len() > MAX_TAGS || distinct_tags.len() != tags.len() || tags.iter().any(|tag| !valid_tag(tag)) {
        return Err(invalid("Enter 1–32 distinct nonempty tags of at most 64 bytes"));
    }
    let mut entries = load(file)?;
    let mut changed = 0;
    for path in paths {
        let mut values = entries.get(path).cloned().unwrap_or_default();
        let before = values.clone();
        match mode {
            BatchMode::Add => for tag in tags { if !values.contains(tag) { values.push(tag.clone()); } },
            BatchMode::Remove => values.retain(|tag| !distinct_tags.contains(tag)),
        }
        if values.len() > MAX_TAGS { return Err(invalid("An item would exceed the 32-tag limit; no tags were saved")); }
        if values != before {
            changed += 1;
            if values.is_empty() { entries.remove(path); } else { entries.insert(path.clone(), values); }
        }
    }
    if changed != 0 { save(file, &entries)?; }
    Ok(changed)
}

fn save(file: &Path, entries: &Entries) -> io::Result<()> {
    if entries.len() > MAX_PATHS { return Err(invalid("Tag database has too many paths")); }
    let mut text = String::from("version=1\n");
    for (path, tags) in entries { text.push_str(&encode(path.to_str().unwrap())); text.push('\t'); text.push_str(&tags.iter().map(|s| encode(s)).collect::<Vec<_>>().join(",")); text.push('\n'); }
    if text.len() as u64 > MAX_BYTES { return Err(invalid("Tag database is too large")); }
    let dir = file.parent().ok_or_else(|| invalid("Invalid tag file path"))?;
    if !crate::settings::check_directory(dir)? {
        #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?; }
        #[cfg(not(unix))] fs::create_dir_all(dir)?;
        if !crate::settings::check_directory(dir)? { return Err(invalid("Tag directory is missing")); }
    }
    for _ in 0..100 {
        let temp = dir.join(format!(".tags-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut options = OpenOptions::new(); options.write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        match options.open(&temp) {
            Ok(mut output) => {
                let result = (|| { output.write_all(text.as_bytes())?; output.sync_all()?; drop(output); let _ = load(file)?; crate::settings::replace(&temp, file) })();
                if result.is_err() { let _ = fs::remove_file(&temp); }
                return result;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(invalid("Could not allocate tag staging file"))
}
pub fn parse_input(input: &str) -> io::Result<Vec<String>> {
    let result: Vec<String> = if input.trim().is_empty() { Vec::new() } else { input.split(',').map(|s| s.trim().to_owned()).collect() };
    let distinct: BTreeSet<_> = result.iter().collect();
    if result.len() > MAX_TAGS || distinct.len() != result.len() || result.iter().any(|s| !valid_tag(s)) { return Err(invalid("Use up to 32 distinct, nonempty tags of at most 64 bytes")); }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_changes_are_atomic_and_preserve_other_labels() {
        let root = crate::test_support::scratch_dir("tag-batch");
        let file = root.join("config/tags.conf"); let a = root.join("a"); let b = root.join("b");
        assert_eq!(edit_batch(&file, &[a.clone()], &["missing".into()], BatchMode::Remove).unwrap(), 0);
        assert!(!file.parent().unwrap().exists());
        assert!(edit_batch(&file, &vec![a.clone(); MAX_BATCH + 1], &["x".into()], BatchMode::Add).is_err());
        set(&file, &a, &["old".into(), "shared".into()]).unwrap();
        set(&file, &b, &["other".into()]).unwrap();
        let paths = vec![a.clone(), b.clone()];
        assert_eq!(edit_batch(&file, &paths, &["shared".into(), "new".into()], BatchMode::Add).unwrap(), 2);
        assert_eq!(get(&file, &a).unwrap(), vec!["old", "shared", "new"]);
        assert_eq!(get(&file, &b).unwrap(), vec!["other", "shared", "new"]);
        assert_eq!(edit_batch(&file, &paths, &["shared".into()], BatchMode::Add).unwrap(), 0);
        assert_eq!(edit_batch(&file, &paths, &["shared".into(), "other".into()], BatchMode::Remove).unwrap(), 2);
        assert_eq!(get(&file, &a).unwrap(), vec!["old", "new"]);
        assert_eq!(get(&file, &b).unwrap(), vec!["new"]);
        let full: Vec<String> = (0..32).map(|i| format!("tag{i}")).collect();
        set(&file, &b, &full).unwrap(); let before = fs::read(&file).unwrap();
        assert!(edit_batch(&file, &paths, &["overflow".into()], BatchMode::Add).is_err());
        assert_eq!(fs::read(&file).unwrap(), before);
        assert!(edit_batch(&file, &[a.clone(), a.clone()], &["x".into()], BatchMode::Add).is_err());
        assert!(edit_batch(&file, &paths, &[], BatchMode::Remove).is_err());
        assert!(edit_batch(&file, &[a.clone(), PathBuf::from("relative")], &["x".into()], BatchMode::Add).is_err());
        assert_eq!(fs::read(&file).unwrap(), before);
        assert_eq!(edit_batch(&file, &[b.clone()], &full, BatchMode::Remove).unwrap(), 1);
        assert!(get(&file, &b).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn roundtrip_and_reject_corruption() {
        let root = crate::test_support::scratch_dir("tags"); let file = root.join("config/tags.conf"); let path = root.join("file ü.txt");
        let values = parse_input("project, work, 一").unwrap(); set(&file, &path, &values).unwrap();
        assert_eq!(get(&file, &path).unwrap(), values);
        let other = root.join("other"); set(&file, &other, &["work".into()]).unwrap();
        let found = find(&file, "work").unwrap();
        assert_eq!(found.len(), 2); assert!(found.contains(&path)); assert!(found.contains(&other));
        assert!(find_report(&file, "work").unwrap().contains("2 saved path(s)"));
        assert!(find(&file, "Work").unwrap().is_empty());
        assert!(find(&file, "").is_err());
        assert!(parse_input("x,x").is_err()); assert!(parse_input("x,,y").is_err());
        set(&file, &path, &[]).unwrap(); assert!(get(&file, &path).unwrap().is_empty());
        fs::write(&file, "version=99\n").unwrap(); assert!(set(&file, &path, &values).is_err()); assert_eq!(fs::read_to_string(&file).unwrap(), "version=99\n");
        fs::remove_dir_all(root).unwrap();
    }
}
