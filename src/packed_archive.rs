//! ZIP and compressed TAR reader using the system libarchive. Extraction is
//! performed by ZFM through no-follow directory descriptors, never by libarchive.
#![cfg(target_os = "linux")]
use crate::archive::{self, Kind};
use std::collections::HashSet;
use std::ffi::CStr;
use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};

type P = *mut c_void;
#[link(name = "dl")]
extern "C" {
    fn dlopen(name: *const c_char, flags: c_int) -> P;
    fn dlsym(handle: P, symbol: *const c_char) -> P;
}
struct Api {
    read_new: unsafe extern "C" fn() -> P,
    write_new: unsafe extern "C" fn() -> P,
    write_format_zip: unsafe extern "C" fn(P) -> c_int,
    write_open_fd: unsafe extern "C" fn(P, c_int) -> c_int,
    write_header: unsafe extern "C" fn(P, P) -> c_int,
    write_data: unsafe extern "C" fn(P, P, usize) -> isize,
    write_close: unsafe extern "C" fn(P) -> c_int,
    write_free: unsafe extern "C" fn(P) -> c_int,
    entry_new: unsafe extern "C" fn() -> P,
    entry_free: unsafe extern "C" fn(P),
    entry_set_path: unsafe extern "C" fn(P, *const c_char),
    entry_set_size: unsafe extern "C" fn(P, i64),
    entry_set_type: unsafe extern "C" fn(P, u32),
    entry_set_perm: unsafe extern "C" fn(P, u32),
    support_zip: unsafe extern "C" fn(P) -> c_int,
    support_tar: unsafe extern "C" fn(P) -> c_int,
    filter_none: unsafe extern "C" fn(P) -> c_int,
    filter_gzip: unsafe extern "C" fn(P) -> c_int,
    filter_bzip2: unsafe extern "C" fn(P) -> c_int,
    filter_xz: unsafe extern "C" fn(P) -> c_int,
    open_fd: unsafe extern "C" fn(P, c_int, usize) -> c_int,
    next: unsafe extern "C" fn(P, *mut P) -> c_int,
    skip: unsafe extern "C" fn(P) -> c_int,
    data: unsafe extern "C" fn(P, P, usize) -> isize,
    free: unsafe extern "C" fn(P) -> c_int,
    error: unsafe extern "C" fn(P) -> *const c_char,
    filter_code: unsafe extern "C" fn(P, c_int) -> c_int,
    pathname: unsafe extern "C" fn(P) -> *const c_char,
    filetype: unsafe extern "C" fn(P) -> u32,
    size: unsafe extern "C" fn(P) -> i64,
    size_is_set: unsafe extern "C" fn(P) -> c_int,
    symlink: unsafe extern "C" fn(P) -> *const c_char,
    hardlink: unsafe extern "C" fn(P) -> *const c_char,
    encrypted: unsafe extern "C" fn(P) -> c_int,
}
static API: OnceLock<Result<Api, String>> = OnceLock::new();
fn load_api() -> Result<Api, String> {
    unsafe {
        let handle = dlopen(b"libarchive.so.13\0".as_ptr() as *const c_char, 2);
        if handle.is_null() {
            return Err("libarchive.so.13 not found; install the libarchive13 runtime".into());
        }
        // Keep the library loaded for the process lifetime.
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let address = dlsym(handle, concat!($name, "\0").as_ptr() as *const c_char);
                if address.is_null() {
                    return Err(concat!("libarchive lacks ", $name).into());
                }
                std::mem::transmute::<P, $ty>(address)
            }};
        }
        Ok(Api {
            read_new: symbol!("archive_read_new", unsafe extern "C" fn() -> P),
            write_new: symbol!("archive_write_new", unsafe extern "C" fn() -> P),
            write_format_zip: symbol!(
                "archive_write_set_format_zip",
                unsafe extern "C" fn(P) -> c_int
            ),
            write_open_fd: symbol!(
                "archive_write_open_fd",
                unsafe extern "C" fn(P, c_int) -> c_int
            ),
            write_header: symbol!("archive_write_header", unsafe extern "C" fn(P, P) -> c_int),
            write_data: symbol!(
                "archive_write_data",
                unsafe extern "C" fn(P, P, usize) -> isize
            ),
            write_close: symbol!("archive_write_close", unsafe extern "C" fn(P) -> c_int),
            write_free: symbol!("archive_write_free", unsafe extern "C" fn(P) -> c_int),
            entry_new: symbol!("archive_entry_new", unsafe extern "C" fn() -> P),
            entry_free: symbol!("archive_entry_free", unsafe extern "C" fn(P)),
            entry_set_path: symbol!(
                "archive_entry_set_pathname",
                unsafe extern "C" fn(P, *const c_char)
            ),
            entry_set_size: symbol!("archive_entry_set_size", unsafe extern "C" fn(P, i64)),
            entry_set_type: symbol!("archive_entry_set_filetype", unsafe extern "C" fn(P, u32)),
            entry_set_perm: symbol!("archive_entry_set_perm", unsafe extern "C" fn(P, u32)),
            support_zip: symbol!(
                "archive_read_support_format_zip",
                unsafe extern "C" fn(P) -> c_int
            ),
            support_tar: symbol!(
                "archive_read_support_format_tar",
                unsafe extern "C" fn(P) -> c_int
            ),
            filter_none: symbol!(
                "archive_read_support_filter_none",
                unsafe extern "C" fn(P) -> c_int
            ),
            filter_gzip: symbol!(
                "archive_read_support_filter_gzip",
                unsafe extern "C" fn(P) -> c_int
            ),
            filter_bzip2: symbol!(
                "archive_read_support_filter_bzip2",
                unsafe extern "C" fn(P) -> c_int
            ),
            filter_xz: symbol!(
                "archive_read_support_filter_xz",
                unsafe extern "C" fn(P) -> c_int
            ),
            open_fd: symbol!(
                "archive_read_open_fd",
                unsafe extern "C" fn(P, c_int, usize) -> c_int
            ),
            next: symbol!(
                "archive_read_next_header",
                unsafe extern "C" fn(P, *mut P) -> c_int
            ),
            skip: symbol!("archive_read_data_skip", unsafe extern "C" fn(P) -> c_int),
            data: symbol!(
                "archive_read_data",
                unsafe extern "C" fn(P, P, usize) -> isize
            ),
            free: symbol!("archive_read_free", unsafe extern "C" fn(P) -> c_int),
            error: symbol!(
                "archive_error_string",
                unsafe extern "C" fn(P) -> *const c_char
            ),
            filter_code: symbol!(
                "archive_filter_code",
                unsafe extern "C" fn(P, c_int) -> c_int
            ),
            pathname: symbol!(
                "archive_entry_pathname",
                unsafe extern "C" fn(P) -> *const c_char
            ),
            filetype: symbol!("archive_entry_filetype", unsafe extern "C" fn(P) -> u32),
            size: symbol!("archive_entry_size", unsafe extern "C" fn(P) -> i64),
            size_is_set: symbol!(
                "archive_entry_size_is_set",
                unsafe extern "C" fn(P) -> c_int
            ),
            symlink: symbol!(
                "archive_entry_symlink",
                unsafe extern "C" fn(P) -> *const c_char
            ),
            hardlink: symbol!(
                "archive_entry_hardlink",
                unsafe extern "C" fn(P) -> *const c_char
            ),
            encrypted: symbol!(
                "archive_entry_is_encrypted",
                unsafe extern "C" fn(P) -> c_int
            ),
        })
    }
}
fn api() -> io::Result<&'static Api> {
    API.get_or_init(load_api)
        .as_ref()
        .map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e.clone()))
}
fn bad(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}
fn check(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Archive operation cancelled",
        ))
    } else {
        Ok(())
    }
}
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Format {
    Zip,
    TarGzip,
    TarBzip2,
    TarXz,
}
pub fn format_for_path(path: &Path) -> Option<Format> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".zip") {
        Some(Format::Zip)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(Format::TarGzip)
    } else if name.ends_with(".tar.bz2") {
        Some(Format::TarBzip2)
    } else if name.ends_with(".tar.xz") {
        Some(Format::TarXz)
    } else {
        None
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub kind: Kind,
}
struct Reader {
    api: &'static Api,
    handle: P,
    expected_filter: c_int,
}
impl Drop for Reader {
    fn drop(&mut self) {
        unsafe {
            (self.api.free)(self.handle);
        }
    }
}
impl Reader {
    fn error(&self, context: &str) -> io::Error {
        unsafe {
            let error = (self.api.error)(self.handle);
            let detail = if error.is_null() {
                "unknown error".into()
            } else {
                CStr::from_ptr(error).to_string_lossy().into_owned()
            };
            bad(&format!("{context}: {detail}"))
        }
    }
    fn new(file: &mut File, format: Format) -> io::Result<Self> {
        let api = api()?;
        file.seek(SeekFrom::Start(0))?;
        let handle = unsafe { (api.read_new)() };
        if handle.is_null() {
            return Err(bad("Could not initialize archive reader"));
        }
        let expected_filter = match format {
            Format::Zip => 0,
            Format::TarGzip => 1,
            Format::TarBzip2 => 2,
            Format::TarXz => 6,
        };
        let reader = Self {
            api,
            handle,
            expected_filter,
        };
        unsafe {
            if (api.filter_none)(handle) != 0 {
                return Err(reader.error("Filter unavailable"));
            }
            let filter = match format {
                Format::Zip => 0,
                Format::TarGzip => (api.filter_gzip)(handle),
                Format::TarBzip2 => (api.filter_bzip2)(handle),
                Format::TarXz => (api.filter_xz)(handle),
            };
            if filter != 0 {
                return Err(reader.error("Built-in compression filter unavailable"));
            }
            let format_result = if format == Format::Zip {
                (api.support_zip)(handle)
            } else {
                (api.support_tar)(handle)
            };
            if format_result != 0 {
                return Err(reader.error("Archive format unavailable"));
            }
            if (api.open_fd)(handle, file.as_raw_fd(), 64 * 1024) != 0 {
                return Err(reader.error("Cannot open archive"));
            }
        }
        Ok(reader)
    }
    fn next(&mut self) -> io::Result<Option<Entry>> {
        let mut entry = ptr::null_mut();
        let result = unsafe { (self.api.next)(self.handle, &mut entry) };
        if result == 1 {
            return Ok(None);
        } // ARCHIVE_EOF
        if result != 0 || entry.is_null() {
            return Err(self.error("Invalid archive header"));
        }
        if unsafe { (self.api.filter_code)(self.handle, 0) } != self.expected_filter {
            return Err(bad("Archive compression does not match its extension"));
        }
        unsafe {
            if (self.api.encrypted)(entry) != 0 {
                return Err(bad("Encrypted entries are not supported"));
            }
            if !(self.api.symlink)(entry).is_null() || !(self.api.hardlink)(entry).is_null() {
                return Err(bad("Archive links are not supported"));
            }
            let name = (self.api.pathname)(entry);
            if name.is_null() {
                return Err(bad("Archive entry has no filename"));
            }
            let name = CStr::from_ptr(name)
                .to_str()
                .map_err(|_| bad("Archive paths must be UTF-8"))?;
            let path = archive::safe_name(name)?;
            let kind = match (self.api.filetype)(entry) {
                0o100000 => Kind::File,
                0o040000 => Kind::Directory,
                _ => return Err(bad("Archive contains a link or special file")),
            };
            if (self.api.size_is_set)(entry) == 0 {
                return Err(bad("Archive entry size is unknown"));
            }
            let size = (self.api.size)(entry);
            if size < 0 || size as u64 > archive::MAX_FILE {
                return Err(bad("Archive entry exceeds the 512 MiB limit"));
            }
            if kind == Kind::Directory && size != 0 {
                return Err(bad("Directory has nonzero size"));
            }
            if path.is_empty() && kind != Kind::Directory {
                return Err(bad("Empty file path"));
            }
            Ok(Some(Entry {
                path,
                size: size as u64,
                kind,
            }))
        }
    }
    fn skip(&self) -> io::Result<()> {
        let status = unsafe { (self.api.skip)(self.handle) };
        if status != 0 {
            Err(self.error("Cannot skip archive data"))
        } else {
            Ok(())
        }
    }
    fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let n = unsafe { (self.api.data)(self.handle, buffer.as_mut_ptr() as P, buffer.len()) };
        if n < 0 {
            Err(self.error("Corrupt archive data"))
        } else {
            Ok(n as usize)
        }
    }
}

/// Inspect metadata without writing any files. CRCs are verified only when data is read.
pub fn inspect(file: &mut File, format: Format, cancel: &AtomicBool) -> io::Result<Vec<Entry>> {
    check(cancel)?;
    let mut reader = Reader::new(file, format)?;
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut headers = 0usize;
    loop {
        check(cancel)?;
        let entry = match reader.next()? {
            Some(e) => e,
            None => return Ok(entries),
        };
        headers += 1;
        if headers > archive::MAX_ENTRIES {
            return Err(bad("Archive exceeds the 10000 entry limit"));
        }
        total = total
            .checked_add(entry.size)
            .ok_or_else(|| bad("Archive size overflow"))?;
        if total > archive::MAX_BYTES {
            return Err(bad("Archive exceeds the 1 GiB expansion limit"));
        }
        if !entry.path.is_empty() {
            if !seen.insert(entry.path.clone()) {
                return Err(bad("Archive contains duplicate paths"));
            }
            entries.push(entry);
        }
        reader.skip()?;
    }
}

fn read_checked<F: FnMut(u64)>(
    reader: &Reader,
    entry: &Entry,
    cancel: &AtomicBool,
    mut sink: Option<&mut File>,
    completed: &mut u64,
    progress: &mut F,
) -> io::Result<()> {
    let mut buf = [0u8; 64 * 1024];
    let mut count = 0u64;
    loop {
        check(cancel)?;
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .ok_or_else(|| bad("Archive size overflow"))?;
        if count > entry.size {
            return Err(bad("Entry expands beyond declared size"));
        }
        if let Some(file) = sink.as_deref_mut() {
            file.write_all(&buf[..n])?;
        }
        *completed += n as u64;
        progress(*completed);
    }
    if count != entry.size {
        return Err(bad("Archive entry is truncated"));
    }
    if let Some(file) = sink {
        file.flush()?;
    }
    check(cancel)
}

/// Read all entry data and ask libarchive to validate available checksums.
/// TAR itself does not contain content checksums.
pub fn verify<F: FnMut(u64)>(
    file: &mut File,
    format: Format,
    cancel: &AtomicBool,
    mut progress: F,
) -> io::Result<usize> {
    let plan = inspect(file, format, cancel)?;
    let mut reader = Reader::new(file, format)?;
    let mut index = 0;
    let mut completed = 0;
    while let Some(entry) = reader.next()? {
        check(cancel)?;
        if entry.path.is_empty() {
            reader.skip()?;
            continue;
        }
        if plan.get(index) != Some(&entry) {
            return Err(bad("Archive changed during verification"));
        }
        read_checked(&reader, &entry, cancel, None, &mut completed, &mut progress)?;
        index += 1;
    }
    if index != plan.len() {
        return Err(bad("Archive changed during verification"));
    }
    Ok(index)
}

/// Inspect every header, then stream selected entries to the destination.
/// No overwrite, links or special files. Earlier completed entries remain on failure.
#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
pub fn extract<F: FnMut(u64)>(
    file: &mut File,
    format: Format,
    destination: &Path,
    selected: Option<&[String]>,
    cancel: &AtomicBool,
    mut progress: F,
) -> io::Result<usize> {
    use crate::archive::linux::Directory;
    let plan = inspect(file, format, cancel)?;
    let selected = selected.map(|names| names.iter().map(String::as_str).collect::<HashSet<_>>());
    if let Some(set) = &selected {
        if set.is_empty() {
            return Err(bad("No archive entries selected"));
        }
        let available: HashSet<&str> = plan.iter().map(|e| e.path.as_str()).collect();
        if set.iter().any(|name| !available.contains(name)) {
            return Err(bad("An archive entry is no longer present"));
        }
    }
    let root = Directory::open_root(destination)?;
    let mut reader = Reader::new(file, format)?;
    let mut index = 0;
    let mut extracted = 0;
    let mut completed = 0;
    while let Some(entry) = reader.next()? {
        check(cancel)?;
        if entry.path.is_empty() {
            reader.skip()?;
            continue;
        }
        if plan.get(index) != Some(&entry) {
            return Err(bad("Archive changed during extraction"));
        }
        index += 1;
        if selected
            .as_ref()
            .map_or(false, |set| !set.contains(entry.path.as_str()))
        {
            reader.skip()?;
            continue;
        }
        let parts: Vec<&str> = entry.path.split('/').collect();
        let mut directory = root.duplicate()?;
        let parents = if entry.kind == Kind::Directory {
            parts.len()
        } else {
            parts.len() - 1
        };
        for part in &parts[..parents] {
            directory = directory.ensure_directory(part)?;
        }
        if entry.kind == Kind::File {
            let name = parts[parts.len() - 1];
            let mut output = directory.create_file(name)?;
            let result = read_checked(
                &reader,
                &entry,
                cancel,
                Some(&mut output),
                &mut completed,
                &mut progress,
            );
            drop(output);
            if let Err(error) = result {
                directory.remove_file(name);
                return Err(error);
            }
        } else {
            reader.skip()?;
        }
        extracted += 1;
    }
    if index != plan.len() {
        return Err(bad("Archive changed during extraction"));
    }
    Ok(extracted)
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
pub fn extract<F: FnMut(u64)>(
    _: &mut File,
    _: Format,
    _: &Path,
    _: Option<&[String]>,
    _: &AtomicBool,
    _: F,
) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Safe extraction is not supported on this architecture",
    ))
}

#[cfg(all(
    test,
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(crate::test_support::scratch_dir("zfm-packed"))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixtures(t: &Temp) -> bool {
        if let Err(error) = api() {
            assert!(
                std::env::var_os("ZFM_REQUIRE_ARCHIVE_TESTS").is_none(),
                "libarchive is required for these tests: {error}"
            );
            eprintln!("Skipping packed-archive integration tests: libarchive unavailable");
            return false;
        }
        let script = r#"
import sys,zipfile,tarfile,io,os
p=sys.argv[1]
with zipfile.ZipFile(os.path.join(p,'data.zip'),'w',zipfile.ZIP_DEFLATED) as z:
 z.writestr('dir/one.txt',b'hello'); z.writestr('dir/two.txt',b'world')
with zipfile.ZipFile(os.path.join(p,'traversal.zip'),'w') as z:
 z.writestr('good.txt',b'hello'); z.writestr('../escape.txt',b'bad')
with zipfile.ZipFile(os.path.join(p,'link.zip'),'w') as z:
 i=zipfile.ZipInfo('link'); i.create_system=3; i.external_attr=(0o120777 << 16); z.writestr(i,'../escape')
with zipfile.ZipFile(os.path.join(p,'badcrc.zip'),'w',zipfile.ZIP_STORED) as z:
 z.writestr('file.txt',b'bad crc payload')
f=os.path.join(p,'badcrc.zip'); data=bytearray(open(f,'rb').read()); at=data.index(b'bad crc payload'); data[at]^=1; open(f,'wb').write(data)
for suffix,mode in [('tar.gz','w:gz'),('tar.bz2','w:bz2'),('tar.xz','w:xz')]:
 with tarfile.open(os.path.join(p,'data.'+suffix),mode) as a:
  b=b'compressed archive'; i=tarfile.TarInfo('nested/file.txt'); i.size=len(b); a.addfile(i,io.BytesIO(b))
"#;
        let status = std::process::Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(&t.0)
            .status();
        match status {
            Ok(s) => {
                assert!(s.success());
                true
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                assert!(
                    std::env::var_os("ZFM_REQUIRE_ARCHIVE_TESTS").is_none(),
                    "python3 is required for these tests"
                );
                eprintln!("Skipping packed-archive tests: python3 unavailable");
                false
            }
            Err(e) => panic!("{e}"),
        }
    }
    #[test]
    fn zip_and_compressed_tar_round_trip() {
        let t = Temp::new();
        if !fixtures(&t) {
            return;
        }
        for (file, format) in [
            ("data.zip", Format::Zip),
            ("data.tar.gz", Format::TarGzip),
            ("data.tar.bz2", Format::TarBzip2),
            ("data.tar.xz", Format::TarXz),
        ] {
            let mut archive = File::open(t.0.join(file)).unwrap();
            let dest = t.0.join(format!("out-{file}"));
            fs::create_dir(&dest).unwrap();
            let entries = inspect(&mut archive, format, &AtomicBool::new(false)).unwrap();
            assert!(!entries.is_empty());
            assert_eq!(
                verify(&mut archive, format, &AtomicBool::new(false), |_| {}).unwrap(),
                entries.len()
            );
            assert_eq!(
                extract(
                    &mut archive,
                    format,
                    &dest,
                    None,
                    &AtomicBool::new(false),
                    |_| {}
                )
                .unwrap(),
                entries.len()
            );
            let path = if format == Format::Zip {
                "dir/one.txt"
            } else {
                "nested/file.txt"
            };
            assert!(dest.join(path).exists());
        }
    }
    #[test]
    fn rejects_traversal_links_and_crc_corruption() {
        let t = Temp::new();
        if !fixtures(&t) {
            return;
        }
        for filename in ["traversal.zip", "link.zip"] {
            let mut file = File::open(t.0.join(filename)).unwrap();
            assert!(
                inspect(&mut file, Format::Zip, &AtomicBool::new(false)).is_err(),
                "{filename}"
            );
        }
        let mut file = File::open(t.0.join("badcrc.zip")).unwrap();
        assert!(verify(&mut file, Format::Zip, &AtomicBool::new(false), |_| {}).is_err());
        let dest = t.0.join("dest");
        fs::create_dir(&dest).unwrap();
        assert!(extract(
            &mut file,
            Format::Zip,
            &dest,
            None,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        assert!(!dest.join("file.txt").exists());
    }
    #[test]
    fn creates_zip_accepted_by_python_and_refuses_overwrite() {
        let t = Temp::new();
        if !fixtures(&t) {
            return;
        }
        let input = t.0.join("input.txt");
        fs::write(&input, vec![42u8; 100_000]).unwrap();
        let output = t.0.join("created.zip");
        create_single_file(&input, &output, &AtomicBool::new(false), |_| {}).unwrap();
        let status = std::process::Command::new("python3").arg("-c").arg("import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None and z.read('input.txt') == b'\x2a'*100000").arg(&output).status().unwrap();
        assert!(status.success());
        let mut file = File::open(&output).unwrap();
        assert_eq!(
            verify(&mut file, Format::Zip, &AtomicBool::new(false), |_| {}).unwrap(),
            1
        );
        assert!(create_single_file(&input, &output, &AtomicBool::new(false), |_| {}).is_err());
        let cancel = AtomicBool::new(false);
        let stopped = t.0.join("stopped.zip");
        assert!(create_single_file(&input, &stopped, &cancel, |_| cancel
            .store(true, Ordering::Relaxed))
        .is_err());
        assert!(!stopped.exists());
    }
    #[test]
    fn selection_conflict_symlinks_and_cancellation() {
        let t = Temp::new();
        if !fixtures(&t) {
            return;
        }
        let mut file = File::open(t.0.join("data.zip")).unwrap();
        let dest = t.0.join("dest");
        fs::create_dir(&dest).unwrap();
        assert_eq!(
            extract(
                &mut file,
                Format::Zip,
                &dest,
                Some(&["dir/two.txt".into()]),
                &AtomicBool::new(false),
                |_| {}
            )
            .unwrap(),
            1
        );
        assert!(!dest.join("dir/one.txt").exists());
        assert_eq!(fs::read(dest.join("dir/two.txt")).unwrap(), b"world");
        assert!(extract(
            &mut file,
            Format::Zip,
            &dest,
            None,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        let other = t.0.join("other");
        fs::create_dir(&other).unwrap();
        let outside = t.0.join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, other.join("dir")).unwrap();
        assert!(extract(
            &mut file,
            Format::Zip,
            &other,
            None,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        assert!(!outside.join("one.txt").exists());
        let cancelled = t.0.join("cancelled");
        fs::create_dir(&cancelled).unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            extract(&mut file, Format::Zip, &cancelled, None, &flag, |_| flag
                .store(true, Ordering::Relaxed))
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(!cancelled.join("dir/one.txt").exists());
    }
}

/// Stream one regular file to a new ZIP, refusing to replace existing output.
#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
pub fn create_single_file<F: FnMut(u64)>(
    source: &Path,
    output: &Path,
    cancel: &AtomicBool,
    mut progress: F,
) -> io::Result<()> {
    use std::fs::{self, OpenOptions};
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::AtomicU64;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    struct Writer {
        api: &'static Api,
        handle: P,
    }
    impl Drop for Writer {
        fn drop(&mut self) {
            unsafe {
                (self.api.write_free)(self.handle);
            }
        }
    }
    struct Header {
        api: &'static Api,
        handle: P,
    }
    impl Drop for Header {
        fn drop(&mut self) {
            unsafe {
                (self.api.entry_free)(self.handle);
            }
        }
    }
    check(cancel)?;
    let name = source
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| bad("Source name must be UTF-8"))?;
    if archive::safe_name(name)? != name || name.is_empty() || name.len() > 100 {
        return Err(bad(
            "Source name must be a safe UTF-8 filename of at most 100 bytes",
        ));
    }
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(0x800 | crate::linux_flags::O_NOFOLLOW)
        .open(source)?;
    let before = input.metadata()?;
    if !before.is_file() || before.len() > archive::MAX_FILE {
        return Err(bad("Only regular files up to 512 MiB are supported"));
    }
    if output.file_name().is_none() {
        return Err(bad("Choose an output filename"));
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (temporary, out) = (0..100)
        .find_map(|_| {
            let path = parent.join(format!(
                ".zfm-zip-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(file) => Some(Ok((Temp(path), file))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or_else(|| {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Cannot allocate temporary ZIP",
            ))
        })?;
    let api = api()?;
    let handle = unsafe { (api.write_new)() };
    if handle.is_null() {
        return Err(bad("Cannot initialize ZIP writer"));
    }
    let writer = Writer { api, handle };
    let error = |context: &str| {
        let ptr = unsafe { (api.error)(handle) };
        let detail = if ptr.is_null() {
            "unknown error".into()
        } else {
            unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        };
        bad(&format!("{context}: {detail}"))
    };
    if unsafe { (api.write_format_zip)(handle) } != 0
        || unsafe { (api.write_open_fd)(handle, out.as_raw_fd()) } != 0
    {
        return Err(error("Cannot create ZIP"));
    }
    let entry = unsafe { (api.entry_new)() };
    if entry.is_null() {
        return Err(bad("Cannot create ZIP entry"));
    }
    let header = Header { api, handle: entry };
    let name = std::ffi::CString::new(name).map_err(|_| bad("Invalid filename"))?;
    unsafe {
        (api.entry_set_path)(entry, name.as_ptr());
        (api.entry_set_size)(entry, before.len() as i64);
        (api.entry_set_type)(entry, 0o100000);
        (api.entry_set_perm)(entry, 0o600);
    }
    if unsafe { (api.write_header)(handle, entry) } != 0 {
        return Err(error("Cannot write ZIP header"));
    }
    drop(header);
    let mut buf = [0u8; 64 * 1024];
    let mut remaining = before.len();
    let mut copied = 0u64;
    while remaining > 0 {
        check(cancel)?;
        let n = std::cmp::min(remaining, buf.len() as u64) as usize;
        input.read_exact(&mut buf[..n])?;
        let mut written = 0;
        while written < n {
            check(cancel)?;
            let count =
                unsafe { (api.write_data)(handle, buf[written..n].as_ptr() as P, n - written) };
            if count <= 0 {
                return Err(error("Cannot write ZIP data"));
            }
            written += count as usize;
        }
        remaining -= n as u64;
        copied += n as u64;
        progress(copied);
    }
    let after = input.metadata()?;
    if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
        return Err(bad("Source changed during archive creation"));
    }
    check(cancel)?;
    if unsafe { (api.write_close)(handle) } != 0 {
        return Err(error("Cannot finish ZIP"));
    }
    drop(writer);
    drop(out);
    check(cancel)?;
    fs::hard_link(&temporary.0, output)?;
    Ok(())
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
pub fn create_single_file<F: FnMut(u64)>(
    _: &Path,
    _: &Path,
    _: &AtomicBool,
    _: F,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Safe ZIP creation is not supported on this architecture",
    ))
}
