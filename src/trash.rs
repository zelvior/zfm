//! Desktop trash integration with bounded, cancellable batch dispatch.
//! Linux uses GIO without a permanent-delete fallback. Windows delegates to the
//! shell, which may ask the user before permanently deleting an unrecyclable item.
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub struct BatchTrashResult {
    pub completed: usize,
    pub failure: Option<(PathBuf, String)>,
}
impl BatchTrashResult {
    pub fn message(&self, total: usize) -> String {
        match &self.failure {
            Some((path, error)) => format!(
                "Trash request processed {} of {total} items; stopped at {}: {error}. Later items were not attempted; the failed item may be partially processed.",
                self.completed, path.display()
            ),
            None => format!("Trash request processed {} items.", self.completed),
        }
    }
}

/// Send up to 1024 non-overlapping items to the desktop trash one at a time.
/// Stops at the first failure or cancellation; earlier items are not restored.
/// A call already in progress cannot be interrupted by ZFM. On Windows, `owner`
/// must be a valid window for shell warnings. Path checks are lexical and do not
/// protect against aliases or concurrent changes; use trusted directories.
pub fn trash_items(paths: &[PathBuf], owner: usize, cancel: &AtomicBool) -> io::Result<BatchTrashResult> {
    trash_items_with(paths, cancel, |path| {
        #[cfg(target_os = "windows")]
        { trash_with_owner(path, owner) }
        #[cfg(not(target_os = "windows"))]
        { let _ = owner; trash(path) }
    })
}

fn trash_items_with<F: FnMut(&Path) -> io::Result<()>>(
    paths: &[PathBuf], cancel: &AtomicBool, mut trash_one: F,
) -> io::Result<BatchTrashResult> {
    crate::operations::validate_batch_selection(paths)?;
    let mut completed = 0;
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            return Ok(BatchTrashResult { completed, failure: Some((path.clone(), "Trash cancelled".into())) });
        }
        if let Err(error) = trash_one(path) {
            return Ok(BatchTrashResult { completed, failure: Some((path.clone(), error.to_string())) });
        }
        completed += 1;
    }
    Ok(BatchTrashResult { completed, failure: None })
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{CStr, CString};
    use std::io;
    use std::os::raw::{c_char, c_int, c_void};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;
    use std::sync::OnceLock;
    type P = *mut c_void;
    #[repr(C)]
    struct GError {
        domain: u32,
        code: c_int,
        message: *const c_char,
    }
    #[link(name = "dl")]
    extern "C" {
        fn dlopen(name: *const c_char, flags: c_int) -> P;
        fn dlsym(handle: P, symbol: *const c_char) -> P;
    }
    struct Gio {
        create: unsafe extern "C" fn(*const c_char) -> P,
        trash: unsafe extern "C" fn(P, P, *mut *mut GError) -> c_int,
        unref: unsafe extern "C" fn(P),
        free_error: unsafe extern "C" fn(*mut GError),
    }
    static GIO: OnceLock<Result<Gio, String>> = OnceLock::new();
    fn load() -> Result<Gio, String> {
        unsafe {
            let handle = dlopen(b"libgio-2.0.so.0\0".as_ptr() as *const c_char, 2);
            if handle.is_null() {
                return Err("GIO is not available for trash".into());
            }
            // Keep the library loaded for the process lifetime: GIO may start
            // library threads that outlive a call. Initialization happens once.
            macro_rules! symbol {
                ($name:literal, $ty:ty) => {{
                    let ptr = dlsym(handle, concat!($name, "\0").as_ptr() as *const c_char);
                    if ptr.is_null() {
                        return Err(concat!("GIO is missing ", $name).into());
                    }
                    std::mem::transmute::<P, $ty>(ptr)
                }};
            }
            Ok(Gio {
                create: symbol!(
                    "g_file_new_for_path",
                    unsafe extern "C" fn(*const c_char) -> P
                ),
                trash: symbol!(
                    "g_file_trash",
                    unsafe extern "C" fn(P, P, *mut *mut GError) -> c_int
                ),
                unref: symbol!("g_object_unref", unsafe extern "C" fn(P)),
                free_error: symbol!("g_error_free", unsafe extern "C" fn(*mut GError)),
            })
        }
    }
    pub fn trash(path: &Path) -> io::Result<()> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid path"))?;
        let gio = GIO
            .get_or_init(load)
            .as_ref()
            .map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e.clone()))?;
        unsafe {
            let file = (gio.create)(path.as_ptr());
            if file.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Unable to open path for trash",
                ));
            }
            let mut error = ptr::null_mut();
            let result = (gio.trash)(file, ptr::null_mut(), &mut error);
            (gio.unref)(file);
            if result != 0 {
                return Ok(());
            }
            let message = if error.is_null() {
                "Could not move item to trash".to_owned()
            } else {
                let s = if (*error).message.is_null() {
                    "Could not move item to trash".to_owned()
                } else {
                    CStr::from_ptr((*error).message)
                        .to_string_lossy()
                        .into_owned()
                };
                (gio.free_error)(error);
                s
            };
            Err(io::Error::new(io::ErrorKind::Other, message))
        }
    }
}
#[cfg(target_os = "linux")]
pub use linux::trash;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn trash(_: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Trash is not implemented on this platform",
    ))
}

#[cfg(target_os = "windows")]
mod windows {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Path, PathBuf};
    use std::ptr;

    // shellapi.h packs SHFILEOPSTRUCT to one-byte alignment on 32-bit Windows.
    #[cfg_attr(target_pointer_width = "32", repr(C, packed))]
    #[cfg_attr(target_pointer_width = "64", repr(C))]
    struct FileOperation {
        hwnd: *mut c_void,
        function: u32,
        from: *const u16,
        to: *const u16,
        flags: u16,
        aborted: i32,
        mappings: *mut c_void,
        title: *const u16,
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHFileOperationW(op: *mut FileOperation) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
        fn CoUninitialize();
    }

    /// Move to the Recycle Bin. Requires an owner because the shell must be able
    /// to warn the user if the item cannot be recycled; never suppress warnings.
    pub fn trash_with_owner(path: &Path, owner: usize) -> io::Result<()> {
        if owner == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Recycle Bin requires a window to show warnings",
            ));
        }
        use std::os::windows::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Recycle Bin does not accept reparse points in ZFM",
            ));
        }
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        // Only drive-letter paths on fixed drives; reject UNC, device namespaces,
        // removable drives and other media rather than risking silent deletion.
        let mut parts = absolute.components();
        let prefix = match parts.next() {
            Some(Component::Prefix(p)) => match p.kind() {
                std::path::Prefix::Disk(letter) | std::path::Prefix::VerbatimDisk(letter) => letter,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "Recycle Bin requires a local fixed drive",
                    ))
                }
            },
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Recycle Bin requires an absolute drive path",
                ))
            }
        };
        if !matches!(parts.next(), Some(Component::RootDir)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid drive path",
            ));
        }
        let root = [prefix as u16, b':' as u16, b'\\' as u16, 0];
        if unsafe { GetDriveTypeW(root.as_ptr()) } != 3 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Recycle Bin is only supported on fixed drives",
            ));
        }
        // SHFileOperation does not support the verbatim \\?\ prefix.
        let mut normal = PathBuf::from(format!("{}:\\", prefix as char));
        for part in parts {
            normal.push(part.as_os_str());
        }
        let mut from: Vec<u16> = normal.as_os_str().encode_wide().collect();
        if from.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid path"));
        }
        from.extend([0, 0]);
        let mut op = FileOperation {
            hwnd: owner as *mut c_void,
            function: 3,
            from: from.as_ptr(),
            to: ptr::null(),
            flags: 0x0040 | 0x4000,
            aborted: 0,
            mappings: ptr::null_mut(),
            title: ptr::null(),
        };
        // Shell file operations should run in a COM single-threaded apartment.
        let hr = unsafe { CoInitializeEx(ptr::null_mut(), 0x2 | 0x4) };
        if hr < 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Could not initialize the Windows shell ({hr:#x})"),
            ));
        }
        let result = unsafe { SHFileOperationW(&mut op) };
        unsafe {
            CoUninitialize();
        }
        if op.aborted != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Recycle Bin operation cancelled",
            ));
        }
        if result != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Recycle Bin operation failed: {result:#x}"),
            ));
        }
        Ok(())
    }
    pub fn trash(path: &Path) -> io::Result<()> {
        trash_with_owner(path, 0)
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn shell_structure_abi_and_owner_requirement() {
            #[cfg(target_pointer_width = "32")]
            assert_eq!(std::mem::size_of::<FileOperation>(), 30);
            #[cfg(target_pointer_width = "64")]
            assert_eq!(std::mem::size_of::<FileOperation>(), 56);
            assert_eq!(
                trash(Path::new("missing")).unwrap_err().kind(),
                io::ErrorKind::Unsupported
            );
        }
    }
}
#[cfg(target_os = "windows")]
pub use windows::{trash, trash_with_owner};

#[cfg(test)]
mod batch_tests {
    use super::*;
    #[test]
    fn stops_on_failure_and_reports_partial_result() {
        let root = crate::test_support::scratch_dir("trash-batch");
        let paths = vec![root.join("first"), root.join("second"), root.join("third")];
        let mut called = Vec::new();
        let result = trash_items_with(&paths, &AtomicBool::new(false), |path| {
            called.push(path.to_path_buf());
            if path == paths[1] { Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")) } else { Ok(()) }
        }).unwrap();
        assert_eq!(result.completed, 1);
        assert_eq!(result.failure.as_ref().unwrap().0, paths[1]);
        assert_eq!(called, paths[..2]);
        assert!(result.message(3).contains("processed 1 of 3"));
        assert!(result.message(3).contains("partially processed"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancellation_is_checked_between_items() {
        let root = crate::test_support::scratch_dir("trash-batch-cancel");
        let paths = vec![root.join("first"), root.join("second")];
        let cancel = AtomicBool::new(false);
        let result = trash_items_with(&paths, &cancel, |_| { cancel.store(true, Ordering::Relaxed); Ok(()) }).unwrap();
        assert_eq!(result.completed, 1);
        assert_eq!(result.failure.unwrap().0, paths[1]);
        let result = trash_items_with(&paths, &cancel, |_| panic!("cancelled before dispatch")).unwrap();
        assert_eq!(result.completed, 0);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preflight_rejects_duplicates_nested_and_invalid_paths() {
        let root = crate::test_support::scratch_dir("trash-batch-invalid");
        let parent = root.join("folder"); let child = parent.join("child");
        let cancel = AtomicBool::new(false);
        let reject = |paths: &[PathBuf]| trash_items_with(paths, &cancel, |_| panic!("invalid input was dispatched")).is_err();
        assert!(reject(&[]));
        assert!(reject(&[PathBuf::from("relative")]));
        assert!(reject(&[parent.clone(), parent.clone()]));
        assert!(reject(&[parent.clone(), child.clone()]));
        assert!(reject(&[child, parent.clone()]));
        assert!(reject(&[root.join("a/../b")]));
        assert!(reject(&vec![parent; crate::operations::MAX_COPY_ITEMS + 1]));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn rejects_case_insensitive_overlap() {
        let root = crate::test_support::scratch_dir("trash-batch-case");
        let paths = [root.join("Folder"), root.join("folder").join("child")];
        assert!(trash_items_with(&paths, &AtomicBool::new(false), |_| panic!("overlap dispatched")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn moves_files_and_symlinks_to_isolated_trash() {
        if std::env::var_os("ZFM_TRASH_TEST_DIR").is_some() {
            let root = std::path::PathBuf::from(std::env::var_os("ZFM_TRASH_TEST_DIR").unwrap());
            let file = root.join("test-file");
            fs::write(&file, b"data").unwrap();
            trash(&file).unwrap();
            assert!(!file.exists());
            let target = root.join("target");
            fs::write(&target, b"safe").unwrap();
            let link = root.join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            trash(&link).unwrap();
            assert!(fs::symlink_metadata(&link).is_err());
            assert_eq!(fs::read(&target).unwrap(), b"safe");
            assert!(trash(&root.join("not-found")).is_err());
            let first = root.join("batch-first"); let second = root.join("batch-folder"); let last = root.join("batch-last");
            fs::write(&first, b"one").unwrap(); fs::create_dir(&second).unwrap(); fs::write(second.join("inside"), b"two").unwrap(); fs::write(&last, b"three").unwrap();
            let paths = vec![first.clone(), second.clone(), root.join("missing"), last.clone()];
            let result = trash_items(&paths, 0, &AtomicBool::new(false)).unwrap();
            assert_eq!(result.completed, 2); assert_eq!(result.failure.unwrap().0, paths[2]);
            assert!(!first.exists() && !second.exists() && last.exists());
            assert_eq!(fs::read(root.join("data/Trash/files/batch-first")).unwrap(), b"one");
            assert_eq!(fs::read(root.join("data/Trash/files/batch-folder/inside")).unwrap(), b"two");
            return;
        }
        let root = crate::test_support::scratch_dir("zfm-trash");
        let data = root.join("data");
        fs::create_dir(&data).unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "trash::tests::moves_files_and_symlinks_to_isolated_trash",
                "--nocapture",
            ])
            .env("ZFM_TRASH_TEST_DIR", &root)
            .env("XDG_DATA_HOME", &data)
            .status()
            .unwrap();
        let _ = fs::remove_dir_all(&root);
        assert!(result.success());
    }
}
