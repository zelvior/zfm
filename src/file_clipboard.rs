//! Encoding and validating local file clipboard targets (GNOME and URI list).
#![cfg(unix)]
use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

const MAX_BYTES: usize = 64 * 1024;
#[derive(Debug, PartialEq, Eq)]
pub struct FileClipboard {
    pub path: PathBuf,
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
pub fn encode_gnome(path: &Path, cut: bool) -> io::Result<Vec<u8>> {
    Ok(format!("{}\n{}\n", if cut { "cut" } else { "copy" }, uri(path)?).into_bytes())
}
pub fn encode_uri_list(path: &Path) -> io::Result<Vec<u8>> {
    Ok(format!("{}\r\n", uri(path)?).into_bytes())
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
/// Accept one local file at a time; reject multi-file clipboard payloads rather
/// than silently operating on only part of them.
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
    let mut uris = lines.filter(|line| !line.starts_with(b"#"));
    let path = decode_uri(uris.next().ok_or_else(|| invalid("No file in clipboard"))?)?;
    if uris.next().is_some() {
        return Err(invalid("Only one clipboard item is supported"));
    }
    Ok(FileClipboard { path, cut })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrips_unicode_spaces_and_non_utf8() {
        let p = PathBuf::from(OsString::from_vec(b"/tmp/a \xff%#.txt".to_vec()));
        assert_eq!(
            parse(&encode_gnome(&p, true).unwrap(), true).unwrap(),
            FileClipboard {
                path: p.clone(),
                cut: true
            }
        );
        assert_eq!(
            parse(&encode_uri_list(&p).unwrap(), false).unwrap(),
            FileClipboard {
                path: p,
                cut: false
            }
        );
    }
    #[test]
    fn rejects_remote_invalid_and_multiple() {
        for input in [
            b"file://remote/tmp/x\n".as_slice(),
            b"file:///a%00b",
            b"file:///a%zz",
            b"file:///a?query",
            b"file:///a\nfile:///b\n",
            b"http://example/a",
            b"file:///a/../b",
            b"file:///a%2fb",
        ] {
            assert!(parse(input, false).is_err(), "{input:?}");
        }
        assert!(parse(b"move\nfile:///tmp/x\n", true).is_err());
        assert_eq!(
            parse(b"file://localhost/tmp/a\n", false).unwrap().path,
            Path::new("/tmp/a")
        );
    }
}
