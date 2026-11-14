//! Previewable, bounded numbered renaming of regular files in one directory.
//! The operation is non-overwriting per item, but is not an atomic batch.
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_ITEMS: usize = 100;
const MAX_PREVIEW_UTF16: usize = 20_000;
fn invalid(message: impl Into<String>) -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, message.into()) }

#[derive(Debug)]
pub struct RenamePlan { pairs: Vec<(PathBuf, PathBuf)>, preview: String }
impl RenamePlan {
    pub fn preview(&self) -> &str { &self.preview }
    pub fn len(&self) -> usize { self.pairs.len() }
    pub fn is_empty(&self) -> bool { self.pairs.is_empty() }
}

/// Plan in the caller's displayed selection order, preserving the final file
/// extension. The preview includes every old and new name, escaped for clarity.
/// No filesystem access is done here; actual type and conflict checks happen at
/// execution time. The parent path and names must be UTF-8 so the preview is faithful.
pub fn plan(paths: &[PathBuf], base: &OsStr) -> io::Result<RenamePlan> {
    if !(2..=MAX_ITEMS).contains(&paths.len()) { return Err(invalid("Select between 2 and 100 files")); }
    crate::operations::validate_batch_selection(paths)?;
    let base_text = base.to_str().ok_or_else(|| invalid("Base name must be UTF-8"))?;
    if base_text.chars().any(char::is_control) { return Err(invalid("Base name cannot contain control characters")); }
    crate::operations::validate_name(base)?;
    let parent = paths[0].parent().ok_or_else(|| invalid("File has no parent"))?;
    let parent_text = parent.to_str().ok_or_else(|| invalid("The parent folder path must be UTF-8 for the preview"))?;
    let mut pairs = Vec::with_capacity(paths.len());
    let mut preview = format!("Numbered rename — no overwrites. Earlier renames remain if an error or cancellation occurs.\nFolder: {:?}\n\n", parent_text);
    for (index, path) in paths.iter().enumerate() {
        if !path.is_absolute() || path.parent() != Some(parent)
            || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(invalid("All files must have the same absolute parent folder"));
        }
        let old = path.file_name().and_then(OsStr::to_str).ok_or_else(|| invalid("Each filename must be UTF-8"))?;
        let mut name = OsString::from(base);
        name.push(format!(" ({})", index + 1));
        if let Some(ext) = path.extension() { name.push("."); name.push(ext); }
        crate::operations::validate_name(&name)?;
        let target = parent.join(&name);
        let new = name.to_str().ok_or_else(|| invalid("Each filename must be UTF-8"))?;
        // Debug formatting quotes names and escapes embedded newlines/control characters.
        preview.push_str(&format!("{:?} → {:?}\n", old, new));
        if preview.encode_utf16().count() > MAX_PREVIEW_UTF16 { return Err(invalid("Preview is too long; select fewer files")); }
        pairs.push((path.clone(), target));
    }
    Ok(RenamePlan { pairs, preview })
}

#[derive(Debug)]
pub struct RenameResult { pub completed: usize, pub failure: Option<(PathBuf, String)> }
impl RenameResult {
    pub fn message(&self, total: usize) -> String {
        match &self.failure {
            None => format!("Renamed {} files.", self.completed),
            Some((path, error)) => format!("Renamed {} of {total} files; stopped at {}: {error}. Earlier renames remain; later files were not renamed.", self.completed, path.display()),
        }
    }
}
fn check_regular(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 { return Err(invalid("Links and reparse points are not supported")); }
    }
    if !meta.file_type().is_file() { return Err(invalid("Only regular files can be batch renamed")); }
    Ok(())
}

/// Recheck every source and destination before starting, then rename in order
/// with no-replace semantics. Conflicts/races can still stop a partially
/// completed batch. Use trusted directories; this is not a snapshot or sandbox.
pub fn apply<F: FnMut(usize)>(plan: &RenamePlan, cancel: &AtomicBool, mut progress: F) -> io::Result<RenameResult> {
    for (source, destination) in &plan.pairs {
        if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(io::ErrorKind::Interrupted, "Rename cancelled before any changes")); }
        check_regular(source).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", source.display())))?;
        match fs::symlink_metadata(destination) {
            Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("Destination already exists: {}", destination.display()))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(io::Error::new(e.kind(), format!("Cannot check {}: {e}", destination.display()))),
        }
    }
    let mut completed = 0;
    for (source, destination) in &plan.pairs {
        if cancel.load(Ordering::Relaxed) { return Ok(RenameResult { completed, failure: Some((source.clone(), "Rename cancelled".into())) }); }
        let result = check_regular(source).and_then(|_| crate::operations::rename_child(source, destination.file_name().unwrap()));
        if let Err(error) = result { return Ok(RenameResult { completed, failure: Some((source.clone(), error.to_string())) }); }
        completed += 1;
        progress(completed);
    }
    Ok(RenameResult { completed, failure: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp { fn new() -> Self { Self(crate::test_support::scratch_dir("zfm-rename")) } }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[test]
    fn previews_and_renames_in_selection_order_with_extensions() {
        let t = Temp::new();
        let paths = [t.0.join("one.txt"), t.0.join("two.tar.gz"), t.0.join(".hidden")];
        for (i, path) in paths.iter().enumerate() { fs::write(path, [i as u8]).unwrap(); }
        let plan = plan(&paths, OsStr::new("Trip ü")).unwrap();
        assert_eq!(plan.len(), 3);
        assert!(plan.preview().contains("\"one.txt\" → \"Trip ü (1).txt\""));
        assert!(plan.preview().contains("\"two.tar.gz\" → \"Trip ü (2).gz\""));
        assert!(plan.preview().contains("\".hidden\" → \"Trip ü (3)\""));
        let result = apply(&plan, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(result.completed, 3); assert!(result.failure.is_none());
        for (i, name) in ["Trip ü (1).txt", "Trip ü (2).gz", "Trip ü (3)"].iter().enumerate() {
            assert_eq!(fs::read(t.0.join(name)).unwrap(), [i as u8]);
        }
        assert!(paths.iter().all(|p| !p.exists()));
    }
    #[test]
    fn conflicts_are_preflighted_without_changes() {
        let t = Temp::new(); let paths = [t.0.join("a.txt"), t.0.join("b.txt")];
        fs::write(&paths[0], b"a").unwrap(); fs::write(&paths[1], b"b").unwrap();
        fs::write(t.0.join("x (2).txt"), b"keep").unwrap();
        let plan = plan(&paths, OsStr::new("x")).unwrap();
        assert_eq!(apply(&plan, &AtomicBool::new(false), |_| {}).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert!(paths.iter().all(|p| p.exists()));
        assert_eq!(fs::read(t.0.join("x (2).txt")).unwrap(), b"keep");
        assert!(!t.0.join("x (1).txt").exists());
    }
    #[test]
    fn stops_on_cancel_or_racing_conflict_without_rollback() {
        let t = Temp::new(); let paths = [t.0.join("a"), t.0.join("b"), t.0.join("c")];
        for path in &paths { fs::write(path, b"src").unwrap(); }
        let plan = plan(&paths, OsStr::new("new")).unwrap();
        let cancel = AtomicBool::new(false);
        let result = apply(&plan, &cancel, |_| cancel.store(true, Ordering::Relaxed)).unwrap();
        assert_eq!(result.completed, 1); assert!(result.failure.unwrap().1.contains("cancelled"));
        assert!(t.0.join("new (1)").exists() && paths[1].exists() && paths[2].exists());
        fs::rename(t.0.join("new (1)"), &paths[0]).unwrap(); cancel.store(false, Ordering::Relaxed);
        let result = apply(&plan, &cancel, |_| { fs::write(t.0.join("new (2)"), b"external").unwrap(); }).unwrap();
        assert_eq!(result.completed, 1); assert_eq!(result.failure.unwrap().0, paths[1]);
        assert_eq!(fs::read(t.0.join("new (2)")).unwrap(), b"external");
        assert!(paths[1].exists() && paths[2].exists());
    }
    #[test]
    fn rejects_invalid_plans_and_non_files_before_renaming() {
        let t = Temp::new(); let a = t.0.join("a"); let b = t.0.join("b");
        fs::write(&a, b"a").unwrap(); fs::create_dir(&b).unwrap();
        assert!(plan(&[a.clone()], OsStr::new("x")).is_err());
        assert!(plan(&[a.clone(), a.clone()], OsStr::new("x")).is_err());
        assert!(plan(&[a.clone(), b.clone()], OsStr::new("../x")).is_err());
        assert!(plan(&[a.clone(), b.clone()], OsStr::new("x\n")).is_err());
        assert!(plan(&[a.clone(), t.0.join("other/b")], OsStr::new("x")).is_err());
        assert!(plan(&vec![a.clone(); MAX_ITEMS + 1], OsStr::new("x")).is_err());
        let plan = plan(&[a.clone(), b], OsStr::new("x")).unwrap();
        assert!(apply(&plan, &AtomicBool::new(false), |_| {}).is_err());
        assert!(a.exists() && !t.0.join("x (1)").exists());
    }
    #[cfg(unix)]
    #[test]
    fn rejects_links_and_non_utf8_names() {
        use std::os::unix::ffi::OsStringExt;
        let t = Temp::new(); let a = t.0.join("a"); let link = t.0.join("link");
        fs::write(&a, b"safe").unwrap(); std::os::unix::fs::symlink(&a, &link).unwrap();
        let plan = plan(&[a.clone(), link], OsStr::new("x")).unwrap();
        assert!(apply(&plan, &AtomicBool::new(false), |_| {}).is_err());
        assert_eq!(fs::read(&a).unwrap(), b"safe");
        let non_utf = t.0.join(OsString::from_vec(b"bad\xff".to_vec()));
        assert!(super::plan(&[a, non_utf], OsStr::new("x")).is_err());
    }
}
