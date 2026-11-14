//! Encoding and validating local file clipboard targets (GNOME and URI list).
#![cfg(unix)]
use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

const MAX_BYTES: usize = 64 * 1024;
#[derive(Debug, PartialEq, Eq)]
pub struct FileClipboard {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn uri(path: &Path) -> io::Result<String> {
    if !path.is_absolute() {
        return Err(invalid("Clipboard path must be absolute"));
    }
    let mut result = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                result.push(*byte as char)
            }
            byte => {
                use std::fmt::Write;
                write!(result, "%{byte:02X}").unwrap();
            }
        }
    }
    if result.len() > MAX_BYTES {
        return Err(invalid("Clipboard path is too long"));
    }
    Ok(result)
}
pub fn encode_gnome(paths: &[PathBuf], cut: bool) -> io::Result<Vec<u8>> {
    encode(paths, if cut { "cut\n" } else { "copy\n" }, "\n")
}
pub fn encode_uri_list(paths: &[PathBuf]) -> io::Result<Vec<u8>> {
    encode(paths, "", "\r\n")
}
fn encode(paths: &[PathBuf], prefix: &str, separator: &str) -> io::Result<Vec<u8>> {
    if paths.is_empty() || paths.len() > crate::operations::MAX_COPY_ITEMS {
        return Err(invalid("Clipboard item count is invalid"));
    }
    let mut result = prefix.to_owned();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        if !seen.insert(path) {
            return Err(invalid("Duplicate clipboard paths"));
        }
        result.push_str(&uri(path)?);
        result.push_str(separator);
        if result.len() > MAX_BYTES {
            return Err(invalid("Clipboard data is too large"));
        }
    }
    Ok(result.into_bytes())
}
fn decode_uri(uri: &[u8]) -> io::Result<PathBuf> {
    let rest = uri
        .strip_prefix(b"file://")
        .ok_or_else(|| invalid("Only local file URIs are supported"))?;
    let path = if rest.starts_with(b"/") {
        rest
    } else if rest.starts_with(b"localhost/") {
        &rest["localhost".len()..]
    } else {
        return Err(invalid("Remote file URIs are not supported"));
    };
    if path
        .iter()
        .any(|b| matches!(b, b'?' | b'#' | b'\\' | 0..=31 | 127))
    {
        return Err(invalid("Invalid characters in clipboard URI"));
    }
    let mut result = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        if path[i] == b'%' {
            if i + 2 >= path.len() {
                return Err(invalid("Invalid URI escape"));
            }
            fn hex(x: u8) -> Option<u8> {
                match x {
                    b'0'..=b'9' => Some(x - b'0'),
                    b'a'..=b'f' => Some(x - b'a' + 10),
                    b'A'..=b'F' => Some(x - b'A' + 10),
                    _ => None,
                }
            }
            let byte = hex(path[i + 1])
                .and_then(|hi| hex(path[i + 2]).map(|lo| hi * 16 + lo))
                .ok_or_else(|| invalid("Invalid URI escape"))?;
            if byte == 0 || byte == b'/' {
                return Err(invalid("Invalid escaped character in clipboard path"));
            }
            result.push(byte);
            i += 3;
        } else {
            result.push(path[i]);
            i += 1;
        }
    }
    let path = PathBuf::from(OsString::from_vec(result));
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(invalid("Clipboard must contain a file or folder path"));
    }
    Ok(path)
}
/// Parse bounded local clipboard paths, rejecting invalid or repeated entries.
pub fn parse(data: &[u8], gnome: bool) -> io::Result<FileClipboard> {
    if data.len() > MAX_BYTES || data.contains(&0) {
        return Err(invalid("Clipboard data is invalid or too large"));
    }
    let mut lines = data
        .split(|b| *b == b'\n')
        .map(|s| s.strip_suffix(b"\r").unwrap_or(s))
        .filter(|s| !s.is_empty());
    let cut = if gnome {
        match lines.next() {
            Some(b"copy") => false,
            Some(b"cut") => true,
            _ => return Err(invalid("Invalid file clipboard action")),
        }
    } else {
        false
    };
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in lines.filter(|line| !line.starts_with(b"#")) {
        if paths.len() == crate::operations::MAX_COPY_ITEMS {
            return Err(invalid("Too many clipboard items"));
        }
        let path = decode_uri(line)?;
        if !seen.insert(path.clone()) {
            return Err(invalid("Duplicate clipboard paths"));
        }
        paths.push(path);
    }
    if paths.is_empty() {
        return Err(invalid("No file in clipboard"));
    }
    Ok(FileClipboard { paths, cut })
}

/// Conservative process-local guard against reusing a source path from a cut
/// payload. It cannot protect other processes, aliases, or survive a restart.
/// A fresh explicit cut inside ZFM permits those selected paths again.
#[derive(Default)]
pub struct CutReplayGuard {
    paths: std::collections::HashSet<PathBuf>,
    bytes: usize,
}
impl CutReplayGuard {
    const MAX_PATHS: usize = 4096;
    const MAX_PATH_BYTES: usize = 1024 * 1024;
    pub fn reserve(&mut self, paths: &[PathBuf]) -> io::Result<()> {
        if paths.iter().any(|p| self.paths.contains(p)) {
            return Err(invalid("A cut source was already used in this process; select and cut it again in ZFM"));
        }
        let unique: std::collections::HashSet<_> = paths.iter().collect();
        if unique.len() != paths.len() { return Err(invalid("Duplicate cut sources")); }
        let added: usize = paths.iter().map(|p| p.as_os_str().as_bytes().len()).sum();
        if self.paths.len() + paths.len() > Self::MAX_PATHS || self.bytes + added > Self::MAX_PATH_BYTES {
            return Err(invalid("Cut replay guard is full; restart ZFM and recut the items"));
        }
        for path in paths { self.paths.insert(path.clone()); }
        self.bytes += added;
        Ok(())
    }
    pub fn recut(&mut self, paths: &[PathBuf]) {
        for path in paths {
            if let Some(stored) = self.paths.take(path) { self.bytes -= stored.as_os_str().as_bytes().len(); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrips_unicode_spaces_and_non_utf8() {
        let p = PathBuf::from(OsString::from_vec(b"/tmp/a \xff%#.txt".to_vec()));
        assert_eq!(
            parse(&encode_gnome(&[p.clone()], true).unwrap(), true).unwrap(),
            FileClipboard {
                paths: vec![p.clone()],
                cut: true
            }
        );
        assert_eq!(
            parse(&encode_uri_list(&[p.clone()]).unwrap(), false).unwrap(),
            FileClipboard {
                paths: vec![p],
                cut: false
            }
        );
    }
    #[test]
    fn multi_copy_roundtrip_and_invalid_lists() {
        let paths = vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b ü")];
        assert_eq!(
            parse(&encode_gnome(&paths, false).unwrap(), true).unwrap(),
            FileClipboard {
                paths: paths.clone(),
                cut: false
            }
        );
        assert_eq!(
            parse(&encode_uri_list(&paths).unwrap(), false).unwrap(),
            FileClipboard {
                paths: paths.clone(),
                cut: false
            }
        );
        assert_eq!(parse(&encode_gnome(&paths, true).unwrap(), true).unwrap(), FileClipboard { paths: paths.clone(), cut: true });
        assert!(parse(b"copy\nfile:///tmp/a\nfile:///tmp/a\n", true).is_err());
        assert!(parse(b"file:///tmp/a\nfile://remote/b\n", false).is_err());
        assert!(encode_uri_list(&[paths[0].clone(), paths[0].clone()]).is_err());
        assert!(encode_gnome(&[], false).is_err());
        let many: Vec<PathBuf> = (0..=crate::operations::MAX_COPY_ITEMS)
            .map(|i| PathBuf::from(format!("/tmp/{i}")))
            .collect();
        assert!(encode_uri_list(&many).is_err());
        let payload = many
            .iter()
            .map(|p| format!("file://{}\n", p.display()))
            .collect::<String>();
        assert!(parse(payload.as_bytes(), false).is_err());
        assert!(parse(&vec![b'a'; MAX_BYTES + 1], false).is_err());
    }
    #[test]
    fn replay_guard_rejects_reordering_and_subsets_until_explicit_recut() {
        let a = PathBuf::from("/tmp/a"); let b = PathBuf::from("/tmp/b");
        let mut guard = CutReplayGuard::default();
        guard.reserve(&[a.clone(), b.clone()]).unwrap();
        assert!(guard.reserve(&[b.clone(), a.clone()]).is_err());
        assert!(guard.reserve(&[a.clone()]).is_err());
        guard.recut(&[a.clone()]);
        assert!(guard.reserve(&[a.clone(), b.clone()]).is_err());
        guard.reserve(&[a]).unwrap();
        assert_eq!(guard.bytes, 12);
    }
    #[test]
    fn replay_guard_fails_closed_at_capacity() {
        let mut guard = CutReplayGuard::default();
        let paths: Vec<_> = (0..CutReplayGuard::MAX_PATHS).map(|n| PathBuf::from(format!("/tmp/{n}"))).collect();
        guard.reserve(&paths).unwrap();
        assert!(guard.reserve(&[PathBuf::from("/tmp/extra")]).is_err());
        guard.recut(&paths[..1]);
        guard.reserve(&[PathBuf::from("/tmp/extra")]).unwrap();
        let long = PathBuf::from(format!("/{}", "x".repeat(CutReplayGuard::MAX_PATH_BYTES)));
        assert!(CutReplayGuard::default().reserve(&[long]).is_err());
    }
    #[test]
    fn rejects_remote_and_invalid() {
        for input in [
            b"file://remote/tmp/x\n".as_slice(),
            b"file:///a%00b",
            b"file:///a%zz",
            b"file:///a?query",
            b"http://example/a",
            b"file:///a/../b",
            b"file:///a%2fb",
        ] {
            assert!(parse(input, false).is_err(), "{input:?}");
        }
        assert!(parse(b"move\nfile:///tmp/x\n", true).is_err());
        assert_eq!(
            parse(b"file://localhost/tmp/a\n", false).unwrap().paths,
            vec![PathBuf::from("/tmp/a")]
        );
    }
}
