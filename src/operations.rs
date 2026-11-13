//! Conservative, non-overwriting filesystem operations used by the UI.
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Accept exactly one ordinary filename, never a path or dot component.
pub fn validate_name(name: &OsStr) -> io::Result<()> {
    let mut parts = Path::new(name).components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return Err(invalid("Enter a single file or folder name"));
    }
    // Components normalizes trailing separators and a trailing '/.'. Reject both.
    if Path::new(name).file_name() != Some(name) {
        return Err(invalid("Enter a single file or folder name"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = name.encode_wide().collect();
        if wide
            .iter()
            .any(|&c| c < 32 || matches!(c, 34 | 42 | 47 | 58 | 60 | 62 | 63 | 92 | 124))
            || matches!(wide.last(), Some(32 | 46))
        {
            return Err(invalid("Invalid Windows filename"));
        }
        let base = name
            .to_string_lossy()
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((base.starts_with("COM") || base.starts_with("LPT"))
                && base.len() == 4
                && matches!(base.as_bytes()[3], b'1'..=b'9'));
        if reserved {
            return Err(invalid("Reserved Windows filename"));
        }
    }
    Ok(())
}

pub fn create_file(directory: &Path, name: &OsStr) -> io::Result<PathBuf> {
    validate_name(name)?;
    let path = directory.join(name);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(path)
}

pub fn create_directory(directory: &Path, name: &OsStr) -> io::Result<PathBuf> {
    validate_name(name)?;
    let path = directory.join(name);
    fs::create_dir(&path)?;
    Ok(path)
}

/// Rename a direct child, atomically refusing to replace an existing name.
/// Linux renameat2 requires kernel 3.15 or newer. Other targets fail safely.
pub fn rename_child(source: &Path, new_name: &OsStr) -> io::Result<PathBuf> {
    validate_name(new_name)?;
    let parent = source
        .parent()
        .ok_or_else(|| invalid("Source has no parent"))?;
    let dest = parent.join(new_name);
    if source == dest {
        return Err(invalid("Choose a different name"));
    }
    rename_no_replace(source, &dest)?;
    Ok(dest)
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
fn rename_no_replace(source: &Path, dest: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::raw::{c_int, c_long};
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn syscall(number: c_long, ...) -> c_long;
    }
    #[cfg(target_arch = "x86_64")]
    const SYS_RENAMEAT2: c_long = 316;
    #[cfg(target_arch = "x86")]
    const SYS_RENAMEAT2: c_long = 353;
    #[cfg(target_arch = "aarch64")]
    const SYS_RENAMEAT2: c_long = 276;
    const AT_FDCWD: c_int = -100;
    const RENAME_NOREPLACE: c_int = 1;
    let source =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| invalid("Invalid source path"))?;
    let dest = CString::new(dest.as_os_str().as_bytes())
        .map_err(|_| invalid("Invalid destination path"))?;
    // `renameat2` with RENAME_NOREPLACE avoids the check-then-rename race.
    let result = unsafe {
        syscall(
            SYS_RENAMEAT2,
            AT_FDCWD,
            source.as_ptr(),
            AT_FDCWD,
            dest.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
#[cfg(target_os = "windows")]
fn rename_no_replace(source: &Path, dest: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(source: *const u16, dest: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let dest: Vec<u16> = dest.as_os_str().encode_wide().chain(Some(0)).collect();
    // Flags zero: do not replace existing targets or copy across volumes.
    if unsafe { MoveFileExW(source.as_ptr(), dest.as_ptr(), 0) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(any(
    target_os = "windows",
    all(
        target_os = "linux",
        any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
    )
)))]
fn rename_no_replace(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Safe rename is not supported on this platform",
    ))
}

/// Move an entry to another directory on the same filesystem without replacing anything.
/// Cross-filesystem moves fail without altering the source.
pub fn move_into(source: &Path, directory: &Path) -> io::Result<PathBuf> {
    let name = source
        .file_name()
        .ok_or_else(|| invalid("Source has no filename"))?;
    validate_name(name)?;
    let dest = directory.join(name);
    if path_within(source, &dest) && path_within(&dest, source) {
        return Err(invalid("The item is already in the destination folder"));
    }
    rename_no_replace(source, &dest)?;
    Ok(dest)
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Duplicate a regular file in its original directory. Does not follow symbolic
/// links on Linux. The published destination is never overwritten. Partial data
/// stays in a hidden staging file which is removed on normal errors/cancel.
pub fn duplicate_file<F: FnMut(u64)>(
    source: &Path,
    new_name: &OsStr,
    cancel: &AtomicBool,
    progress: F,
) -> io::Result<PathBuf> {
    let parent = source
        .parent()
        .ok_or_else(|| invalid("Source has no parent"))?;
    copy_file(source, parent, new_name, cancel, progress)
}

/// Suggest a non-colliding copy filename in `directory` based on `source`
/// (for example `report copy.txt`, `report copy 2.txt`, …).
pub fn suggest_copy_name(directory: &Path, source: &Path) -> String {
    let file_name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "copy".to_owned());
    let (stem, ext) = match file_name.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < file_name.len() => {
            (&file_name[..dot], Some(&file_name[dot + 1..]))
        }
        _ => (file_name.as_str(), None),
    };
    for attempt in 1..=999u32 {
        let candidate = match (attempt, ext) {
            (1, Some(e)) => format!("{stem} copy.{e}"),
            (1, None) => format!("{stem} copy"),
            (n, Some(e)) => format!("{stem} copy {n}.{e}"),
            (n, None) => format!("{stem} copy {n}"),
        };
        if fs::symlink_metadata(directory.join(&candidate)).is_err() {
            return candidate;
        }
    }
    match ext {
        Some(e) => format!("{stem} copy.{e}"),
        None => format!("{stem} copy"),
    }
}

/// Open a source file for a copy without following a final-component symlink
/// and without blocking on FIFOs or device handles.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
fn open_copy_source(source: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.read(true);
    options.custom_flags(0x800 | crate::linux_flags::O_NOFOLLOW);
    options.open(source)
}

#[cfg(target_os = "windows")]
fn open_copy_source(source: &Path) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileType(handle: *mut std::ffi::c_void) -> u32;
    }
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const FILE_TYPE_DISK: u32 = 0x0001;

    let sym_meta = fs::symlink_metadata(source)?;
    if (sym_meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 || !sym_meta.is_file() {
        return Err(invalid("Only regular files can be copied"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let file = options.open(source)?;
    if unsafe { GetFileType(file.as_raw_handle() as *mut std::ffi::c_void) } != FILE_TYPE_DISK {
        return Err(invalid("Only regular disk files can be copied"));
    }
    let meta = file.metadata()?;
    if (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 || !meta.is_file() {
        return Err(invalid("Only regular files can be copied"));
    }
    Ok(file)
}

#[cfg(not(any(
    target_os = "windows",
    all(
        target_os = "linux",
        any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
    )
)))]
fn open_copy_source(_source: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Safe copy is not supported on this platform",
    ))
}

/// Copy a regular file to a directory without replacing a destination.
pub fn copy_file<F: FnMut(u64)>(
    source: &Path,
    parent: &Path,
    new_name: &OsStr,
    cancel: &AtomicBool,
    mut progress: F,
) -> io::Result<PathBuf> {
    validate_name(new_name)?;
    let destination = parent.join(new_name);
    let mut input = open_copy_source(source)?;
    if !input.metadata()?.is_file() {
        return Err(invalid("Only regular files can be duplicated"));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Copy cancelled"));
    }
    let (temp, mut output) = (0..100)
        .find_map(|_| {
            let path = parent.join(format!(
                ".zfm-copy-{}-{}.tmp",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            match opts.open(&path) {
                Ok(file) => Some(Ok((Temporary(path), file))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or_else(|| {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Cannot allocate a temporary file",
            ))
        })?;
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Copy cancelled"));
        }
        let read = match input.read(&mut buf) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            other => other?,
        };
        if read == 0 {
            break;
        }
        output.write_all(&buf[..read])?;
        total += read as u64;
        progress(total);
    }
    output.flush()?;
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Copy cancelled"));
    }
    drop(output);
    #[cfg(target_os = "windows")]
    {
        // MoveFileExW(flags=0) atomically publishes without replacing existing targets.
        rename_no_replace(&temp.0, &destination)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Atomic, no-replace publication; failure on filesystems without hard links.
        fs::hard_link(&temp.0, &destination)?;
    }
    Ok(destination)
}

/// Copy one regular file or directory tree without overwriting a destination.
/// Directory trees are staged beside the destination and published atomically;
/// an error or cancellation removes the staging tree when possible. Links,
/// reparse points and special files anywhere in the tree are rejected. Neither
/// source nor destination should be writable by an untrusted concurrent actor:
/// directory components are checked by path, not held as no-follow handles.
pub fn copy_item<F: FnMut(u64)>(
    source: &Path,
    parent: &Path,
    new_name: &OsStr,
    cancel: &AtomicBool,
    mut progress: F,
) -> io::Result<PathBuf> {
    validate_name(new_name)?;
    let metadata = fs::symlink_metadata(source)?;
    if is_link_or_reparse(&metadata) {
        return Err(invalid(
            "Copy does not follow symbolic links or reparse points",
        ));
    }
    if metadata.is_file() {
        return copy_file(source, parent, new_name, cancel, progress);
    }
    if !metadata.is_dir() {
        return Err(invalid("Copy supports only regular files and folders"));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Copy cancelled"));
    }
    let source_real = fs::canonicalize(source)?;
    let parent_real = fs::canonicalize(parent)?;
    if path_within(&parent_real, &source_real) {
        return Err(invalid(
            "Cannot copy a folder into itself or one of its descendants",
        ));
    }
    let destination = parent.join(new_name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination already exists",
            ))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    let staging = (0..100)
        .find_map(|_| {
            let path = parent.join(format!(
                ".zfm-copy-tree-{}-{}.tmp",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            #[allow(unused_mut)]
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => Some(Ok(path)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or_else(|| {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Cannot allocate a temporary folder",
            ))
        })?;
    let outcome = (|| -> io::Result<()> {
        const MAX_ENTRIES: usize = 100_000;
        const MAX_DEPTH: usize = 128;
        let mut stack = vec![(source.to_path_buf(), staging.clone(), 0usize)];
        let mut entries = 0usize;
        let mut total = 0u64;
        while let Some((src, dst, depth)) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Copy cancelled"));
            }
            let meta = fs::symlink_metadata(&src)?;
            if is_link_or_reparse(&meta) || !meta.is_dir() {
                return Err(invalid(
                    "A source folder changed or is a link/reparse point",
                ));
            }
            for entry in fs::read_dir(&src)? {
                if cancel.load(Ordering::Relaxed) {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "Copy cancelled"));
                }
                let entry = entry?;
                entries += 1;
                if entries > MAX_ENTRIES {
                    return Err(invalid("Folder exceeds the 100,000 entry copy limit"));
                }
                let name = entry.file_name();
                validate_name(&name)?;
                let child = entry.path();
                let meta = fs::symlink_metadata(&child)?;
                if is_link_or_reparse(&meta) {
                    return Err(invalid("Folder contains a symbolic link or reparse point"));
                }
                if meta.is_dir() {
                    if depth >= MAX_DEPTH {
                        return Err(invalid("Folder exceeds the 128 level copy limit"));
                    }
                    let target = dst.join(name);
                    #[allow(unused_mut)]
                    let mut builder = fs::DirBuilder::new();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::DirBuilderExt;
                        builder.mode(0o700);
                    }
                    builder.create(&target)?;
                    stack.push((child, target, depth + 1));
                } else if meta.is_file() {
                    let mut copied = 0;
                    copy_file(&child, &dst, &name, cancel, |bytes| {
                        copied = bytes;
                        progress(total.saturating_add(bytes));
                    })?;
                    total = total.saturating_add(copied);
                } else {
                    return Err(invalid(
                        "Folder contains a special file; nothing was copied",
                    ));
                }
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Copy cancelled"));
        }
        rename_no_replace(&staging, &destination)?;
        Ok(())
    })();
    match outcome {
        Ok(()) => Ok(destination),
        Err(error) => match fs::remove_dir_all(&staging) {
            Ok(()) => Err(error),
            Err(cleanup) if cleanup.kind() == io::ErrorKind::NotFound => Err(error),
            Err(cleanup) => Err(io::Error::new(
                error.kind(),
                format!(
                    "{error}; could not remove temporary copy {}: {cleanup}",
                    staging.display()
                ),
            )),
        },
    }
}

fn is_link_or_reparse(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return meta.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    false
}

#[cfg(not(windows))]
fn path_within(path: &Path, ancestor: &Path) -> bool {
    path.starts_with(ancestor)
}
#[cfg(windows)]
fn path_within(path: &Path, ancestor: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn CompareStringOrdinal(
            a: *const u16,
            alen: i32,
            b: *const u16,
            blen: i32,
            ignore_case: i32,
        ) -> i32;
    }
    let mut parts = path.components();
    for component in ancestor.components() {
        let Some(part) = parts.next() else {
            return false;
        };
        let a: Vec<u16> = part.as_os_str().encode_wide().collect();
        let b: Vec<u16> = component.as_os_str().encode_wide().collect();
        if unsafe {
            CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1)
        } != 2
        {
            return false;
        }
    }
    true
}

/// Maximum number of top-level items accepted for one clipboard copy.
pub const MAX_COPY_ITEMS: usize = 1024;

/// Copies items in order, stopping at the first failure. Already published items
/// are not rolled back. The failed item and all later items remain untouched
/// except for best-effort staging cleanup performed by `copy_item`.
#[derive(Debug)]
pub struct BatchCopyResult {
    pub completed: usize,
    pub failure: Option<(PathBuf, String)>,
}

impl BatchCopyResult {
    pub fn message(&self, total: usize) -> String {
        match &self.failure {
            Some((path, error)) => format!(
                "Copied {} of {total} items; stopped at {}: {error}. Remaining items were not copied.",
                self.completed, path.display()
            ),
            None => format!("Copied {} items.", self.completed),
        }
    }
}

pub fn copy_items(
    paths: &[PathBuf],
    destination: &Path,
    cancel: &AtomicBool,
) -> io::Result<BatchCopyResult> {
    if paths.is_empty() || paths.len() > MAX_COPY_ITEMS {
        return Err(invalid("Select between 1 and 1024 items to copy"));
    }
    let mut seen = std::collections::HashSet::new();
    if paths
        .iter()
        .any(|path| !path.is_absolute() || !seen.insert(path))
    {
        return Err(invalid("Copy paths must be absolute and unique"));
    }
    let mut completed = 0;
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            return Ok(BatchCopyResult {
                completed,
                failure: Some((path.clone(), "Copy cancelled".into())),
            });
        }
        let name = if path.parent() == Some(destination) {
            std::ffi::OsString::from(suggest_copy_name(destination, path))
        } else {
            match path.file_name() {
                Some(name) => name.to_os_string(),
                None => {
                    return Ok(BatchCopyResult {
                        completed,
                        failure: Some((path.clone(), "Source has no filename".into())),
                    })
                }
            }
        };
        if let Err(error) = copy_item(path, destination, &name, cancel, |_| {}) {
            return Ok(BatchCopyResult {
                completed,
                failure: Some((path.clone(), error.to_string())),
            });
        }
        completed += 1;
    }
    Ok(BatchCopyResult {
        completed,
        failure: None,
    })
}

/// A same-filesystem batch move stops at the first error or cancellation.
/// Earlier moves remain in place; there is no rollback or overwrite.
#[derive(Debug)]
pub struct BatchMoveResult {
    pub completed: usize,
    pub failure: Option<(PathBuf, String)>,
}
impl BatchMoveResult {
    pub fn message(&self, total: usize) -> String {
        match &self.failure {
            Some((path, error)) => format!(
                "Moved {} of {total} items; stopped at {}: {error}. Remaining items were not moved.",
                self.completed, path.display()
            ),
            None => format!("Moved {} items.", self.completed),
        }
    }
}

/// Move up to 1024 absolute, non-overlapping paths without replacement. Moves
/// are atomic per item, not as a batch, and cannot cross filesystems. `cancel`
/// is checked between renames; a blocked filesystem operation cannot be stopped.
/// Lexical overlap checks are not a defense against aliases or concurrent path
/// changes: source and destination directories must be trusted.
pub fn move_items<F: FnMut(usize)>(
    paths: &[PathBuf], destination: &Path, cancel: &AtomicBool, mut progress: F,
) -> io::Result<BatchMoveResult> {
    if paths.is_empty() || paths.len() > MAX_COPY_ITEMS {
        return Err(invalid("Select between 1 and 1024 items to move"));
    }
    let valid_path = |path: &Path| path.is_absolute() && path.file_name().is_some()
        && !path.components().any(|c| matches!(c, Component::ParentDir));
    if !destination.is_absolute() || destination.components().any(|c| matches!(c, Component::ParentDir))
        || paths.iter().any(|p| !valid_path(p)) {
        return Err(invalid("Move paths must be absolute and must not contain parent components"));
    }
    // Prevent moving a selected parent and its selected child in either order,
    // including case-insensitive comparisons on Windows.
    for (i, path) in paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(BatchMoveResult { completed: 0, failure: Some((paths[0].clone(), "Move cancelled".into())) });
        }
        if path_within(destination, path) {
            return Err(invalid("The destination must not be inside a selected item"));
        }
        if paths[..i].iter().any(|other| path_within(path, other) || path_within(other, path)) {
            return Err(invalid("Selected move paths must be distinct and must not contain one another"));
        }
    }
    let mut completed = 0;
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            return Ok(BatchMoveResult { completed, failure: Some((path.clone(), "Move cancelled".into())) });
        }
        if let Err(error) = move_into(path, destination) {
            return Ok(BatchMoveResult { completed, failure: Some((path.clone(), error.to_string())) });
        }
        completed += 1;
        progress(completed);
    }
    Ok(BatchMoveResult { completed, failure: None })
}

/// Permanently remove a file/symlink or an *empty* folder. Never recurse or
/// follow the final symlink. Confirmation is the caller's responsibility.
pub fn remove_one(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(crate::test_support::scratch_dir("zfm-ops"))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn names_reject_paths_and_dot_components() {
        for name in ["", ".", "..", "foo/", "foo/.", "../x", "a/b", "/tmp"] {
            assert!(validate_name(OsStr::new(name)).is_err(), "{name}");
        }
        for name in ["space and ü.txt", ".hidden"] {
            assert!(validate_name(OsStr::new(name)).is_ok());
        }
    }
    #[cfg(windows)]
    #[test]
    fn rejects_windows_streams_reserved_names_and_invalid_characters() {
        for name in [
            "file:stream",
            "NUL",
            "con.txt",
            "COM1.txt",
            "a?b",
            "name.",
            "name ",
            "a\\b",
        ] {
            assert!(validate_name(OsStr::new(name)).is_err(), "{name}");
        }
        for name in ["COM10", "valid.txt", "é"] {
            assert!(validate_name(OsStr::new(name)).is_ok());
        }
    }
    #[test]
    fn creation_does_not_overwrite() {
        let t = Temp::new();
        let file = create_file(&t.0, OsStr::new("file")).unwrap();
        fs::write(&file, b"keep").unwrap();
        assert!(create_file(&t.0, OsStr::new("file")).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"keep");
        assert!(create_directory(&t.0, OsStr::new("dir")).unwrap().is_dir());
        assert!(create_directory(&t.0, OsStr::new("dir")).is_err());
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn rename_files_directories_and_conflicts() {
        let t = Temp::new();
        let file = create_file(&t.0, OsStr::new("one")).unwrap();
        fs::write(&file, b"one").unwrap();
        fs::write(t.0.join("two"), b"two").unwrap();
        assert!(rename_child(&file, OsStr::new("two")).is_err());
        assert_eq!(fs::read(t.0.join("two")).unwrap(), b"two");
        assert_eq!(
            fs::read(rename_child(&file, OsStr::new("new")).unwrap()).unwrap(),
            b"one"
        );
        let dir = create_directory(&t.0, OsStr::new("dir")).unwrap();
        assert!(rename_child(&dir, OsStr::new("new-dir")).unwrap().is_dir());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn duplicate_cancel_conflict_and_symlink() {
        let t = Temp::new();
        let src = t.0.join("source");
        fs::write(&src, vec![42u8; 200_000]).unwrap();
        let cancel = AtomicBool::new(false);
        let dup = duplicate_file(&src, OsStr::new("copy"), &cancel, |_| {}).unwrap();
        assert_eq!(fs::read(&src).unwrap(), fs::read(&dup).unwrap());
        assert!(duplicate_file(&src, OsStr::new("copy"), &cancel, |_| {}).is_err());
        let cancel = AtomicBool::new(false);
        assert_eq!(
            duplicate_file(&src, OsStr::new("cancel"), &cancel, |_| {
                cancel.store(true, Ordering::Relaxed)
            })
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(!t.0.join("cancel").exists());
        assert!(!fs::read_dir(&t.0).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zfm-copy-")));
        let link = t.0.join("link");
        std::os::unix::fs::symlink(&src, &link).unwrap();
        assert!(duplicate_file(
            &link,
            OsStr::new("copied-link"),
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
    }
    #[test]
    fn delete_empty_only_and_symlink_not_target() {
        let t = Temp::new();
        let dir = t.0.join("dir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("child"), b"x").unwrap();
        assert!(remove_one(&dir).is_err());
        assert!(dir.join("child").exists());
        #[cfg(unix)]
        {
            let link = t.0.join("link");
            std::os::unix::fs::symlink(&dir, &link).unwrap();
            remove_one(&link).unwrap();
            assert!(dir.is_dir());
        }
        remove_one(&dir.join("child")).unwrap();
        remove_one(&dir).unwrap();
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn copy_and_move_to_other_folder_without_overwrite() {
        let t = Temp::new();
        let source = t.0.join("file with space.txt");
        let destination = t.0.join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, b"source data").unwrap();
        let copy = copy_file(
            &source,
            &destination,
            source.file_name().unwrap(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(&copy).unwrap(), b"source data");
        assert!(move_into(&source, &destination).is_err());
        assert!(source.exists());
        fs::remove_file(&copy).unwrap();
        assert_eq!(move_into(&source, &destination).unwrap(), copy);
        assert!(!source.exists());
        assert_eq!(fs::read(&copy).unwrap(), b"source data");
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn move_does_not_replace_symlink_and_copies_do_not_follow_symlinks() {
        let t = Temp::new();
        let source = t.0.join("original");
        let dest = t.0.join("out");
        fs::create_dir(&dest).unwrap();
        fs::write(&source, b"original data").unwrap();
        let target = t.0.join("target");
        fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, dest.join("original")).unwrap();
        assert!(move_into(&source, &dest).is_err());
        assert!(copy_file(
            &source,
            &dest,
            OsStr::new("original"),
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        assert_eq!(fs::read(&source).unwrap(), b"original data");
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn copy_streams_larger_files_and_reports_progress() {
        let t = Temp::new();
        let source = t.0.join("large");
        let mut file = fs::File::create(&source).unwrap();
        let chunk = [0x4cu8; 64 * 1024];
        for _ in 0..160 {
            file.write_all(&chunk).unwrap();
        }
        drop(file);
        let mut last = 0;
        let output = duplicate_file(
            &source,
            OsStr::new("large copy"),
            &AtomicBool::new(false),
            |n| {
                assert!(n > last);
                last = n;
            },
        )
        .unwrap();
        assert_eq!(last, 160 * chunk.len() as u64);
        assert_eq!(fs::metadata(output).unwrap().len(), last);
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn copy_tree_nested_empty_and_conflict() {
        let t = Temp::new();
        let src = t.0.join("source");
        fs::create_dir_all(src.join("nested/empty")).unwrap();
        fs::write(src.join("one"), b"abc").unwrap();
        fs::write(src.join("nested/two"), b"defgh").unwrap();
        let cancel = AtomicBool::new(false);
        let mut total = 0;
        let output = copy_item(&src, &t.0, OsStr::new("output"), &cancel, |n| {
            assert!(n > total);
            total = n;
        })
        .unwrap();
        assert_eq!(total, 8);
        assert_eq!(fs::read(output.join("one")).unwrap(), b"abc");
        assert_eq!(fs::read(output.join("nested/two")).unwrap(), b"defgh");
        assert!(output.join("nested/empty").is_dir());
        assert_eq!(
            copy_item(&src, &t.0, OsStr::new("output"), &cancel, |_| {})
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(output.join("one")).unwrap(), b"abc");
        assert!(copy_item(&src, &src, OsStr::new("self"), &cancel, |_| {}).is_err());
        assert!(copy_item(
            &src,
            &src.join("nested"),
            OsStr::new("self"),
            &cancel,
            |_| {}
        )
        .is_err());
        assert!(!src.join("self").exists());
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn copy_tree_cancel_and_publication_conflict_clean_up() {
        let t = Temp::new();
        let src = t.0.join("source");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file"), vec![1; 200_000]).unwrap();
        let cancel = AtomicBool::new(false);
        let err = copy_item(&src, &t.0, OsStr::new("cancelled"), &cancel, |_| {
            cancel.store(true, Ordering::Relaxed)
        })
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!t.0.join("cancelled").exists());
        let cancel = AtomicBool::new(false);
        let dest = t.0.join("raced");
        let mut created = false;
        let err = copy_item(&src, &t.0, OsStr::new("raced"), &cancel, |_| {
            if !created {
                fs::create_dir(&dest).unwrap();
                fs::write(dest.join("keep"), b"keep").unwrap();
                created = true;
            }
        })
        .unwrap_err();
        assert!(matches!(
            err.kind(),
            io::ErrorKind::AlreadyExists | io::ErrorKind::PermissionDenied
        ));
        assert_eq!(fs::read(dest.join("keep")).unwrap(), b"keep");
        assert!(!fs::read_dir(&t.0).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zfm-copy-")));
    }

    #[cfg(unix)]
    #[test]
    fn copy_tree_rejects_links_and_cleans_staging() {
        use std::os::unix::fs::symlink;
        let t = Temp::new();
        let src = t.0.join("source");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file"), b"keep").unwrap();
        symlink(&t.0, src.join("link")).unwrap();
        let cancel = AtomicBool::new(false);
        assert!(copy_item(&src, &t.0, OsStr::new("output"), &cancel, |_| {}).is_err());
        assert!(!t.0.join("output").exists());
        assert!(!fs::read_dir(&t.0).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zfm-copy-")));
        symlink(&src, t.0.join("alias")).unwrap();
        assert!(copy_item(
            &src,
            &t.0.join("alias"),
            OsStr::new("self"),
            &cancel,
            |_| {}
        )
        .is_err());
        assert!(copy_item(
            &t.0.join("alias"),
            &t.0,
            OsStr::new("output"),
            &cancel,
            |_| {}
        )
        .is_err());
        assert_eq!(fs::read(src.join("file")).unwrap(), b"keep");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_tree_rejects_excess_depth_and_special_files() {
        let t = Temp::new();
        let src = t.0.join("source");
        fs::create_dir(&src).unwrap();
        let mut child = src.clone();
        for _ in 0..130 {
            child.push("d");
            fs::create_dir(&child).unwrap();
        }
        let cancel = AtomicBool::new(false);
        assert!(copy_item(&src, &t.0, OsStr::new("deep"), &cancel, |_| {}).is_err());
        assert!(!t.0.join("deep").exists());
        fs::remove_dir_all(&src).unwrap();
        fs::create_dir(&src).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let fifo = std::ffi::CString::new(src.join("fifo").as_os_str().as_bytes()).unwrap();
        extern "C" {
            fn mkfifo(path: *const std::os::raw::c_char, mode: u32) -> i32;
        }
        assert_eq!(unsafe { mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(copy_item(&src, &t.0, OsStr::new("special"), &cancel, |_| {}).is_err());
        assert!(!t.0.join("special").exists());
        assert!(!fs::read_dir(&t.0).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zfm-copy-")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_descendant_check_is_case_insensitive() {
        let t = Temp::new();
        let src = t.0.join("CaseFolder");
        fs::create_dir_all(src.join("Nested")).unwrap();
        assert!(copy_item(
            &src,
            &t.0.join("casefolder/NESTED"),
            OsStr::new("inside"),
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        assert!(!src.join("Nested/inside").exists());
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn batch_copy_success_partial_conflict_and_cancellation() {
        let t = Temp::new();
        let source = t.0.join("src");
        let target = t.0.join("out");
        fs::create_dir_all(source.join("folder/empty")).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("first"), b"first").unwrap();
        fs::write(source.join("folder/nested"), b"nested").unwrap();
        fs::write(source.join("last"), b"last").unwrap();
        let paths = vec![
            source.join("first"),
            source.join("folder"),
            source.join("last"),
        ];
        let cancel = AtomicBool::new(false);
        fs::write(target.join("last"), b"existing").unwrap();
        let result = copy_items(&paths, &target, &cancel).unwrap();
        assert_eq!(result.completed, 2);
        assert_eq!(result.failure.as_ref().unwrap().0, paths[2]);
        assert!(result.message(paths.len()).contains("Copied 2 of 3"));
        assert_eq!(fs::read(target.join("first")).unwrap(), b"first");
        assert_eq!(fs::read(target.join("folder/nested")).unwrap(), b"nested");
        assert!(target.join("folder/empty").is_dir());
        assert_eq!(fs::read(target.join("last")).unwrap(), b"existing");
        let result = copy_items(&paths, &source, &cancel).unwrap();
        assert_eq!(result.completed, 3);
        assert!(result.failure.is_none());
        assert_eq!(fs::read(source.join("first copy")).unwrap(), b"first");
        assert_eq!(
            fs::read(source.join("folder copy/nested")).unwrap(),
            b"nested"
        );
        cancel.store(true, Ordering::Relaxed);
        let result = copy_items(&paths, &source, &cancel).unwrap();
        assert_eq!(result.completed, 0);
        assert!(result.failure.unwrap().1.contains("cancelled"));
        assert!(!source.join("first copy 2").exists());
        assert!(copy_items(&[], &target, &cancel).is_err());
        assert!(copy_items(&[paths[0].clone(), paths[0].clone()], &target, &cancel).is_err());
        assert!(copy_items(
            &vec![paths[0].clone(); MAX_COPY_ITEMS + 1],
            &target,
            &cancel
        )
        .is_err());
    }

    #[test]
    fn batch_move_stops_on_conflict_without_losing_items() {
        let t = Temp::new();
        let source = t.0.join("source"); let target = t.0.join("target");
        fs::create_dir(&source).unwrap(); fs::create_dir(&target).unwrap();
        fs::write(source.join("first"), b"first").unwrap();
        fs::create_dir(source.join("folder")).unwrap();
        fs::write(source.join("folder/nested"), b"nested").unwrap();
        fs::write(source.join("last"), b"last").unwrap();
        fs::write(target.join("last"), b"keep").unwrap();
        let paths = vec![source.join("first"), source.join("folder"), source.join("last")];
        let result = move_items(&paths, &target, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(result.completed, 2);
        assert_eq!(result.failure.as_ref().unwrap().0, paths[2]);
        assert!(result.message(3).contains("Moved 2 of 3"));
        assert_eq!(fs::read(target.join("first")).unwrap(), b"first");
        assert_eq!(fs::read(target.join("folder/nested")).unwrap(), b"nested");
        assert_eq!(fs::read(target.join("last")).unwrap(), b"keep");
        assert_eq!(fs::read(source.join("last")).unwrap(), b"last");
        assert!(!source.join("first").exists());
    }
    #[test]
    fn batch_move_cancel_and_preflight_reject_overlaps() {
        let t = Temp::new(); let source = t.0.join("source"); let target = t.0.join("target");
        fs::create_dir(&source).unwrap(); fs::create_dir(&target).unwrap();
        fs::create_dir_all(source.join("folder/child")).unwrap();
        fs::write(source.join("one"), b"1").unwrap(); fs::write(source.join("two"), b"2").unwrap();
        let folder = source.join("folder"); let child = folder.join("child");
        let cancel = AtomicBool::new(false);
        assert!(move_items(&[], &target, &cancel, |_| {}).is_err());
        assert!(move_items(&[folder.clone(), folder.clone()], &target, &cancel, |_| {}).is_err());
        assert!(move_items(&[folder.clone(), child.clone()], &target, &cancel, |_| {}).is_err());
        assert!(move_items(&[child.clone(), folder.clone()], &target, &cancel, |_| {}).is_err());
        assert!(move_items(&[folder.clone()], &child, &cancel, |_| {}).is_err());
        assert!(move_items(&[PathBuf::from("relative")], &target, &cancel, |_| {}).is_err());
        assert!(move_items(&[folder.clone()], &source.join("../target"), &cancel, |_| {}).is_err());
        assert!(move_items(&vec![folder.clone(); MAX_COPY_ITEMS + 1], &target, &cancel, |_| {}).is_err());
        assert!(folder.is_dir());
        let paths = [source.join("one"), source.join("two")];
        let result = move_items(&paths, &target, &cancel, |_| cancel.store(true, Ordering::Relaxed)).unwrap();
        assert_eq!(result.completed, 1);
        assert!(result.failure.unwrap().1.contains("cancelled"));
        assert!(target.join("one").exists()); assert!(source.join("two").exists());
        let result = move_items(&[source.join("two")], &target, &cancel, |_| {}).unwrap();
        assert_eq!(result.completed, 0); assert!(source.join("two").exists());
    }
    #[test]
    fn batch_move_same_folder_is_a_conflict_and_never_overwrites() {
        let t = Temp::new(); let item = t.0.join("item");
        fs::write(&item, b"keep").unwrap();
        let result = move_items(&[item.clone()], &t.0, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(result.completed, 0); assert!(result.failure.is_some());
        assert_eq!(fs::read(item).unwrap(), b"keep");
    }
    #[cfg(windows)]
    #[test]
    fn batch_move_rejects_case_insensitive_overlaps() {
        let t = Temp::new(); let parent = t.0.join("Folder");
        fs::create_dir(&parent).unwrap();
        let alias = t.0.join("folder").join("child");
        assert!(move_items(&[parent, alias], &t.0, &AtomicBool::new(false), |_| {}).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn batch_move_moves_the_symlink_not_its_target() {
        let t = Temp::new(); let target = t.0.join("target"); fs::create_dir(&target).unwrap();
        let original = t.0.join("original"); fs::write(&original, b"keep").unwrap();
        let link = t.0.join("link"); std::os::unix::fs::symlink(&original, &link).unwrap();
        let result = move_items(&[link.clone()], &target, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(result.completed, 1); assert!(result.failure.is_none());
        assert!(!link.exists()); assert!(fs::symlink_metadata(target.join("link")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(original).unwrap(), b"keep");
    }

    #[test]
    fn suggest_copy_name_avoids_existing_names() {
        let t = Temp::new();
        let src = t.0.join("report.csv");
        fs::write(&src, b"a").unwrap();
        assert_eq!(suggest_copy_name(&t.0, &src), "report copy.csv");
        fs::write(t.0.join("report copy.csv"), b"b").unwrap();
        assert_eq!(suggest_copy_name(&t.0, &src), "report copy 2.csv");
        let hidden = t.0.join(".bashrc");
        fs::write(&hidden, b"c").unwrap();
        assert_eq!(suggest_copy_name(&t.0, &hidden), ".bashrc copy");
    }
}
