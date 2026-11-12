//! Bounded, streaming POSIX ustar archive inspection and Linux extraction.
//! Tar does not carry a checksum for file contents; header checksums are checked.
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const BLOCK: u64 = 512;
pub(crate) const MAX_ENTRIES: usize = 10_000;
pub(crate) const MAX_BYTES: u64 = 1024 * 1024 * 1024;
pub(crate) const MAX_FILE: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub kind: Kind,
    offset: u64,
}

fn bad(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}
fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "Archive operation cancelled")
}
fn check_cancel(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(cancelled())
    } else {
        Ok(())
    }
}
fn octal(bytes: &[u8]) -> io::Result<u64> {
    if bytes.first().map_or(false, |v| v & 0x80 != 0) {
        return Err(bad("Base-256 tar numbers are not supported"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| bad("Invalid tar number"))?
        .trim_matches(|c| c == ' ' || c == '\0');
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8).map_err(|_| bad("Invalid tar number"))
}
fn header_text(bytes: &[u8]) -> io::Result<&str> {
    let end = bytes.iter().position(|&v| v == 0).unwrap_or(bytes.len());
    if bytes[end..].iter().any(|&v| v != 0) {
        return Err(bad("Invalid NUL padding in tar name"));
    }
    std::str::from_utf8(&bytes[..end]).map_err(|_| bad("Tar names must be UTF-8"))
}
/// Normalize harmless './' and reject absolute, drive-like, backslash and '..'
/// paths, even on Linux. Empty root-directory records are handled separately.
pub(crate) fn safe_name(name: &str) -> io::Result<String> {
    if name.starts_with('/')
        || name.contains('\\')
        || name.contains(':')
        || name.chars().any(|c| c.is_control())
    {
        return Err(bad("Archive contains an unsafe path"));
    }
    let mut result = Vec::new();
    for component in name.split('/') {
        match component {
            "" | "." => (),
            ".." => return Err(bad("Archive contains a parent traversal")),
            c => result.push(c),
        }
    }
    Ok(result.join("/"))
}

/// Scan the entire archive and validate all headers before any extraction.
/// Only plain ustar archives with regular files/directories are accepted. GNU
/// long names, PAX extensions, links, devices and compressed streams fail closed.
pub fn inspect(file: &mut File, cancel: &AtomicBool) -> io::Result<Vec<Entry>> {
    file.seek(SeekFrom::Start(0))?;
    let file_len = file.metadata()?.len();
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut headers = 0usize;
    loop {
        check_cancel(cancel)?;
        let mut header = [0u8; 512];
        file.read_exact(&mut header).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                bad("Truncated tar header or missing end markers")
            } else {
                e
            }
        })?;
        if header.iter().all(|b| *b == 0) {
            let mut second = [0u8; 512];
            file.read_exact(&mut second)
                .map_err(|_| bad("Missing second tar end marker"))?;
            if second.iter().any(|b| *b != 0) {
                return Err(bad("Invalid tar end marker"));
            }
            return Ok(entries);
        }
        headers += 1;
        if headers > MAX_ENTRIES {
            return Err(bad("Tar exceeds the 10000 entry limit"));
        }
        let expected = octal(&header[148..156])?;
        let checksum: u64 = header
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    *b as u64
                }
            })
            .sum();
        if checksum != expected {
            return Err(bad("Tar header checksum mismatch"));
        }
        let magic = &header[257..263];
        let prefix = if magic == b"ustar\0" {
            header_text(&header[345..500])?
        } else if magic == b"ustar " {
            ""
        }
        // GNU's prefix area has other meanings.
        else {
            return Err(bad("Only uncompressed ustar tar archives are supported"));
        };
        let name = header_text(&header[..100])?;
        let combined = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        let path = safe_name(&combined)?;
        let size = octal(&header[124..136])?;
        let kind = match header[156] {
            0 | b'0' => Kind::File,
            b'5' => Kind::Directory,
            _ => {
                return Err(bad(
                    "Tar links, special files and extension headers are not supported",
                ))
            }
        };
        if kind == Kind::Directory && size != 0 {
            return Err(bad("Tar directory has nonzero size"));
        }
        if path.is_empty() && kind != Kind::Directory {
            return Err(bad("Empty tar filename"));
        }
        if size > MAX_FILE {
            return Err(bad("Tar entry exceeds the 512 MiB limit"));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| bad("Tar size overflow"))?;
        if total > MAX_BYTES {
            return Err(bad("Tar exceeds the 1 GiB expansion limit"));
        }
        let offset = file.stream_position()?;
        let padded = size
            .checked_add(BLOCK - 1)
            .ok_or_else(|| bad("Tar size overflow"))?
            / BLOCK
            * BLOCK;
        let next = offset
            .checked_add(padded)
            .ok_or_else(|| bad("Tar offset overflow"))?;
        if next > file_len {
            return Err(bad("Truncated tar data"));
        }
        file.seek(SeekFrom::Start(next))?;
        if !path.is_empty() {
            if entries.len() >= MAX_ENTRIES {
                return Err(bad("Tar exceeds the 10000 entry limit"));
            }
            if !seen.insert(path.clone()) {
                return Err(bad("Tar contains duplicate paths"));
            }
            entries.push(Entry {
                path,
                size,
                kind,
                offset,
            });
        }
    }
}

/// Extract all entries (`None`) or exactly the named entries. The destination
/// must exist. Existing files are never overwritten. On error/cancellation,
/// earlier entries remain; a partially written current file is removed.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn extract<F: FnMut(u64)>(
    file: &mut File,
    destination: &Path,
    selected: Option<&[String]>,
    cancel: &AtomicBool,
    mut progress: F,
) -> io::Result<usize> {
    use linux::Directory;
    let entries = inspect(file, cancel)?;
    let wanted = selected.map(|paths| paths.iter().map(String::as_str).collect::<HashSet<_>>());
    if let Some(paths) = &wanted {
        if paths.is_empty() {
            return Err(bad("No archive entries selected"));
        }
        let available: HashSet<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        if paths.iter().any(|path| !available.contains(path)) {
            return Err(bad("An archive entry is no longer present"));
        }
    }
    let root = Directory::open_root(destination)?;
    let mut count = 0;
    let mut written = 0u64;
    let mut buf = [0u8; 64 * 1024];
    for entry in entries.iter().filter(|entry| {
        wanted
            .as_ref()
            .map_or(true, |set| set.contains(entry.path.as_str()))
    }) {
        check_cancel(cancel)?;
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
            file.seek(SeekFrom::Start(entry.offset))?;
            let mut output = directory.create_file(name)?;
            let mut remaining = entry.size;
            let result = (|| -> io::Result<()> {
                while remaining > 0 {
                    check_cancel(cancel)?;
                    let bytes = std::cmp::min(remaining, buf.len() as u64) as usize;
                    file.read_exact(&mut buf[..bytes])?;
                    output.write_all(&buf[..bytes])?;
                    remaining -= bytes as u64;
                    written += bytes as u64;
                    progress(written);
                }
                output.flush()?;
                check_cancel(cancel)
            })();
            drop(output);
            if let Err(error) = result {
                directory.remove_file(name);
                return Err(error);
            }
        }
        count += 1;
    }
    Ok(count)
}

#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))
))]
pub fn extract<F: FnMut(u64)>(
    _: &mut File,
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
    target_os = "linux",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) mod linux {
    use super::*;
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::raw::{c_char, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn open(path: *const c_char, flags: c_int, ...) -> c_int;
        fn openat(dir: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
        fn mkdirat(dir: c_int, path: *const c_char, mode: c_uint) -> c_int;
        fn unlinkat(dir: c_int, path: *const c_char, flags: c_int) -> c_int;
    }
    // Linux constants for x86, x86_64 and aarch64. Directories are opened
    // relative to held directory descriptors; symlinks are never followed.
    const DIRECTORY: c_int = crate::linux_flags::O_DIRECTORY;
    const NOFOLLOW: c_int = crate::linux_flags::O_NOFOLLOW;
    const CLOEXEC: c_int = 0x80000;
    const CREATE: c_int = 0x40;
    const EXCLUSIVE: c_int = 0x80;
    const WRITE: c_int = 1;
    fn cbytes(bytes: &[u8]) -> io::Result<CString> {
        CString::new(bytes).map_err(|_| bad("NUL in path"))
    }
    fn file_from_fd(fd: c_int) -> io::Result<File> {
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }
    pub struct Directory(File);
    impl Directory {
        pub fn open_root(path: &Path) -> io::Result<Self> {
            let path = cbytes(path.as_os_str().as_bytes())?;
            file_from_fd(unsafe { open(path.as_ptr(), DIRECTORY | NOFOLLOW | CLOEXEC) }).map(Self)
        }
        pub fn duplicate(&self) -> io::Result<Self> {
            self.0.try_clone().map(Self)
        }
        pub fn ensure_directory(&self, name: &str) -> io::Result<Self> {
            let name = cbytes(name.as_bytes())?;
            let result = unsafe { mkdirat(self.0.as_raw_fd(), name.as_ptr(), 0o700) };
            if result != 0 {
                let e = io::Error::last_os_error();
                if e.kind() != io::ErrorKind::AlreadyExists {
                    return Err(e);
                }
            }
            file_from_fd(unsafe {
                openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    DIRECTORY | NOFOLLOW | CLOEXEC,
                )
            })
            .map(Self)
        }
        pub fn create_file(&self, name: &str) -> io::Result<File> {
            let name = cbytes(name.as_bytes())?;
            file_from_fd(unsafe {
                openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    WRITE | CREATE | EXCLUSIVE | NOFOLLOW | CLOEXEC,
                    0o600 as c_uint,
                )
            })
        }
        pub fn remove_file(&self, name: &str) {
            if let Ok(name) = cbytes(name.as_bytes()) {
                unsafe {
                    unlinkat(self.0.as_raw_fd(), name.as_ptr(), 0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "zfm-tar-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tar(records: &[(&str, u8, &[u8])]) -> Vec<u8> {
        let mut output = Vec::new();
        for (name, kind, content) in records {
            let mut h = [0u8; 512];
            h[..name.len()].copy_from_slice(name.as_bytes());
            h[100..108].copy_from_slice(b"0000600\0");
            h[108..116].copy_from_slice(b"0000000\0");
            h[116..124].copy_from_slice(b"0000000\0");
            h[124..136].copy_from_slice(format!("{:011o}\0", content.len()).as_bytes());
            h[136..148].copy_from_slice(b"00000000000\0");
            h[148..156].fill(b' ');
            h[156] = *kind;
            h[257..263].copy_from_slice(b"ustar\0");
            h[263..265].copy_from_slice(b"00");
            let sum: usize = h.iter().map(|n| *n as usize).sum();
            h[148..156].copy_from_slice(format!("{:06o}\0 ", sum).as_bytes());
            output.extend_from_slice(&h);
            output.extend_from_slice(content);
            let padded = (content.len() + 511) / 512 * 512;
            output.resize(output.len() + padded - content.len(), 0);
        }
        output.resize(output.len() + 1024, 0);
        output
    }
    fn file_for(t: &Temp, data: &[u8]) -> File {
        let p = t.0.join("input.tar");
        fs::write(&p, data).unwrap();
        File::open(p).unwrap()
    }
    #[test]
    fn inspect_and_detect_corruption() {
        let t = Temp::new();
        let mut file = file_for(
            &t,
            &tar(&[
                ("./", b'5', b""),
                ("./folder/", b'5', b""),
                ("./folder/é.txt", b'0', b"hello"),
            ]),
        );
        let entries = inspect(&mut file, &AtomicBool::new(false)).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].path, "folder/é.txt");
        assert_eq!(entries[1].size, 5);
        let mut bytes = tar(&[("a", b'0', b"1")]);
        bytes[0] = b'b';
        assert!(inspect(&mut file_for(&t, &bytes), &AtomicBool::new(false)).is_err());
        bytes = tar(&[("a", b'0', b"1")]);
        bytes.truncate(600);
        assert!(inspect(&mut file_for(&t, &bytes), &AtomicBool::new(false)).is_err());
    }
    #[test]
    fn reject_traversal_special_files_and_duplicates() {
        let t = Temp::new();
        for (name, kind) in [
            ("../escape", b'0'),
            ("/absolute", b'0'),
            ("x/../../escape", b'0'),
            ("C:/evil", b'0'),
            ("a\\b", b'0'),
            ("link", b'2'),
            ("device", b'3'),
            ("pax", b'x'),
        ] {
            assert!(
                inspect(
                    &mut file_for(&t, &tar(&[(name, kind, b"")])),
                    &AtomicBool::new(false)
                )
                .is_err(),
                "{name}"
            );
        }
        assert!(inspect(
            &mut file_for(&t, &tar(&[("a", b'0', b""), ("./a", b'0', b"")])),
            &AtomicBool::new(false)
        )
        .is_err());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn selected_and_all_extract_without_overwrites() {
        let t = Temp::new();
        let dest = t.0.join("dest");
        fs::create_dir(&dest).unwrap();
        let data = tar(&[("folder/a.txt", b'0', b"a"), ("folder/b.txt", b'0', b"b")]);
        let mut file = file_for(&t, &data);
        assert_eq!(
            extract(
                &mut file,
                &dest,
                Some(&["folder/b.txt".to_owned()]),
                &AtomicBool::new(false),
                |_| {}
            )
            .unwrap(),
            1
        );
        assert!(!dest.join("folder/a.txt").exists());
        assert_eq!(fs::read(dest.join("folder/b.txt")).unwrap(), b"b");
        assert!(extract(&mut file, &dest, None, &AtomicBool::new(false), |_| {}).is_err());
        assert_eq!(fs::read(dest.join("folder/b.txt")).unwrap(), b"b");
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn symlink_destination_and_symlink_parent_are_not_followed() {
        use std::os::unix::fs::symlink;
        let t = Temp::new();
        let dest = t.0.join("dest");
        let outside = t.0.join("outside");
        fs::create_dir(&dest).unwrap();
        fs::create_dir(&outside).unwrap();
        symlink(&outside, dest.join("sub")).unwrap();
        symlink(&outside, t.0.join("root-link")).unwrap();
        let data = tar(&[("sub/escape", b'0', b"bad")]);
        assert!(extract(
            &mut file_for(&t, &data),
            &dest,
            None,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        assert!(extract(
            &mut file_for(&t, &data),
            &t.0.join("root-link"),
            None,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        assert!(!outside.join("escape").exists());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn scan_fails_before_any_output_and_cancellation_removes_partial_file() {
        let t = Temp::new();
        let dest = t.0.join("dest");
        fs::create_dir(&dest).unwrap();
        let data = tar(&[("good", b'0', b"ok"), ("../bad", b'0', b"bad")]);
        assert!(extract(
            &mut file_for(&t, &data),
            &dest,
            None,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        assert!(!dest.join("good").exists());
        let large = vec![3u8; 200_000];
        let data = tar(&[("large", b'0', &large)]);
        let cancel = AtomicBool::new(false);
        assert_eq!(
            extract(&mut file_for(&t, &data), &dest, None, &cancel, |_| cancel
                .store(true, Ordering::Relaxed))
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(!dest.join("large").exists());
    }
    #[test]
    fn enforces_limits_and_cancel() {
        let t = Temp::new();
        let mut data = tar(&[("huge", b'0', b"")]);
        data[124..136].copy_from_slice(format!("{:011o}\0", MAX_FILE + 1).as_bytes());
        data[148..156].fill(b' ');
        let sum: usize = data[..512].iter().map(|n| *n as usize).sum();
        data[148..156].copy_from_slice(format!("{:06o}\0 ", sum).as_bytes());
        assert!(inspect(&mut file_for(&t, &data), &AtomicBool::new(false)).is_err());
        assert_eq!(
            inspect(&mut file_for(&t, &tar(&[])), &AtomicBool::new(true))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
    }
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[test]
    fn interoperates_with_system_tar_if_available() {
        let t = Temp::new();
        let input = t.0.join("source");
        let dest = t.0.join("dest");
        fs::create_dir(&input).unwrap();
        fs::create_dir(&dest).unwrap();
        fs::write(input.join("hello world.txt"), b"Hello, TAR!\n").unwrap();
        let tar_path = t.0.join("generated.tar");
        let command = std::process::Command::new("tar")
            .args(["--format=ustar", "-cf"])
            .arg(&tar_path)
            .arg("-C")
            .arg(&input)
            .arg(".")
            .status();
        let status = match command {
            Ok(status) => status,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return,
            Err(e) => panic!("tar: {e}"),
        };
        assert!(status.success());
        let mut archive = File::open(&tar_path).unwrap();
        assert_eq!(
            extract(&mut archive, &dest, None, &AtomicBool::new(false), |_| {}).unwrap(),
            1
        );
        assert_eq!(
            fs::read(dest.join("hello world.txt")).unwrap(),
            b"Hello, TAR!\n"
        );
    }
}

/// Create an uncompressed ustar archive containing one regular file. Files are
/// streamed to a temporary file and published atomically without replacement.
/// The output filesystem must support hard links. Input links are not followed.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn create_single_file<F: FnMut(u64)>(
    source: &Path,
    output: &Path,
    cancel: &AtomicBool,
    mut progress: F,
) -> io::Result<()> {
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::AtomicU64;
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    struct Temporary(std::path::PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    check_cancel(cancel)?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| bad("Source filename must be UTF-8"))?;
    if safe_name(name)? != name || name.is_empty() || name.len() > 100 {
        return Err(bad(
            "Source name must be a safe UTF-8 filename of at most 100 bytes",
        ));
    }
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(0x800 | crate::linux_flags::O_NOFOLLOW)
        .open(source)?;
    let metadata = input.metadata()?;
    if !metadata.is_file() {
        return Err(bad("Only regular files can be archived"));
    }
    let size = metadata.len();
    if size > MAX_FILE {
        return Err(bad("File exceeds the 512 MiB archive limit"));
    }
    if output.file_name().is_none() {
        return Err(bad("Choose an output filename"));
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (temp, mut out) = (0..100)
        .find_map(|_| {
            let path = parent.join(format!(
                ".zfm-tar-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(file) => Some(Ok((Temporary(path), file))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or_else(|| {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Could not allocate temporary archive",
            ))
        })?;
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[100..108].copy_from_slice(b"0000600\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    header[136..148].copy_from_slice(b"00000000000\0");
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: usize = header.iter().map(|v| *v as usize).sum();
    header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
    out.write_all(&header)?;
    let mut buf = [0u8; 64 * 1024];
    let mut remaining = size;
    let mut copied = 0u64;
    while remaining > 0 {
        check_cancel(cancel)?;
        let count = std::cmp::min(remaining, buf.len() as u64) as usize;
        input.read_exact(&mut buf[..count])?;
        out.write_all(&buf[..count])?;
        remaining -= count as u64;
        copied += count as u64;
        progress(copied);
    }
    let after = input.metadata()?;
    if after.len() != size || after.modified().ok() != metadata.modified().ok() {
        return Err(bad("Source changed during archive creation"));
    }
    let padding = (BLOCK - size % BLOCK) % BLOCK;
    out.write_all(&[0u8; 512][..padding as usize])?;
    out.write_all(&[0u8; 1024])?;
    out.flush()?;
    check_cancel(cancel)?;
    drop(out);
    fs::hard_link(&temp.0, output)?;
    Ok(())
}

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
mod creation_tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[test]
    fn creates_a_real_tar_without_overwriting_and_cleans_up_on_cancel() {
        let dir = std::env::temp_dir().join(format!(
            "zfm-create-tar-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let input = dir.join("data.txt");
        let out = dir.join("archive.tar");
        let dest = dir.join("extract");
        fs::write(&input, vec![42u8; 100_000]).unwrap();
        fs::create_dir(&dest).unwrap();
        create_single_file(&input, &out, &AtomicBool::new(false), |_| {}).unwrap();
        let mut file = File::open(&out).unwrap();
        assert_eq!(
            extract(&mut file, &dest, None, &AtomicBool::new(false), |_| {}).unwrap(),
            1
        );
        assert_eq!(
            fs::read(dest.join("data.txt")).unwrap(),
            fs::read(&input).unwrap()
        );
        assert!(create_single_file(&input, &out, &AtomicBool::new(false), |_| {}).is_err());
        let cancelled = dir.join("cancel.tar");
        let flag = AtomicBool::new(false);
        assert_eq!(
            create_single_file(&input, &cancelled, &flag, |_| flag
                .store(true, Ordering::Relaxed))
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(!cancelled.exists());
        assert!(!fs::read_dir(&dir).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".zfm-tar-")));
        let link = dir.join("link");
        std::os::unix::fs::symlink(&input, &link).unwrap();
        assert!(create_single_file(
            &link,
            &dir.join("link.tar"),
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        if let Ok(status) = std::process::Command::new("tar")
            .arg("-tf")
            .arg(&out)
            .stdout(std::process::Stdio::null())
            .status()
        {
            assert!(status.success());
        }
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))
))]
pub fn create_single_file<F: FnMut(u64)>(
    _: &Path,
    _: &Path,
    _: &AtomicBool,
    _: F,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Safe archive creation is not supported on this architecture",
    ))
}
