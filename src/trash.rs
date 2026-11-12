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

#[cfg(not(target_os = "linux"))]
pub fn trash(_: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Trash is not implemented on this platform",
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
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
        let root = std::env::temp_dir().join(format!(
            "zfm-trash-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
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
