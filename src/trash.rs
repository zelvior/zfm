//! Linux trash integration using GIO. There is no permanent-delete fallback.
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
        let result = unsafe { SHFileOperationW(&mut op) };
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
}
#[cfg(target_os = "windows")]
pub use windows::{trash, trash_with_owner};

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
