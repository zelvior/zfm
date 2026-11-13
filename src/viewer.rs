//! Bounded, read-only input handling for the built-in viewer.
//!
//! File formats are deliberately allowlisted; other documents should be opened
//! with a suitable external application, never interpreted as executable code.
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

pub const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_IMAGE_PIXELS: u64 = 40_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind { Text, Image, Pdf, Office, Unsupported }

pub fn kind(path: &Path) -> Kind {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "md" | "markdown" | "log" | "csv" | "tsv" | "json" | "xml" | "html" | "htm" | "css" | "js" | "ts" | "rs" | "py" | "c" | "h" | "cpp" | "toml" | "yaml" | "yml" | "ini" | "conf" | "sh" | "ps1" => Kind::Text,
        "png" | "jpg" | "jpeg" | "gif" | "bmp" => Kind::Image,
        "pdf" => Kind::Pdf,
        "doc" | "docx" | "ppt" | "pptx" | "xls" | "xlsx" | "odt" | "odp" | "ods" => Kind::Office,
        _ => Kind::Unsupported,
    }
}

fn open_regular(path: &Path) -> io::Result<File> {
    #[cfg(all(target_os = "linux", not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))))]
    return Err(io::Error::new(io::ErrorKind::Unsupported, "Safe preview is unavailable on this architecture"));
    // Avoid waiting on a FIFO and reject a final-component symlink on Linux.
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x800 | crate::linux_flags::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open the reparse point itself rather than following it.
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { // FILE_ATTRIBUTE_REPARSE_POINT
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Viewer rejects reparse points"));
        }
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Viewer requires a regular file"));
    }
    Ok(file)
}

pub fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = open_regular(path)?;
    if file.metadata()?.len() > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "File exceeds the viewer size limit"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "File exceeds the viewer size limit"));
    }
    Ok(bytes)
}

pub fn decode_text(bytes: &[u8]) -> Result<String, &'static str> {
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let le = bytes[0] == 0xff;
        let tail = &bytes[2..];
        if tail.len() % 2 != 0 { return Err("Invalid UTF-16 text"); }
        let words: Vec<u16> = tail.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0],c[1]]) } else { u16::from_be_bytes([c[0],c[1]]) }).collect();
        String::from_utf16(&words).map_err(|_| "Invalid UTF-16 text")?
    } else {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        std::str::from_utf8(bytes).map_err(|_| "Text must be UTF-8 or BOM-marked UTF-16")?.to_owned()
    };
    if text.contains('\0') { return Err("Binary data cannot be displayed as text"); }
    Ok(text)
}

/// Inspect a conservative set of raster headers before invoking native decoders.
/// This is a resource guard, not validation or a sandbox for untrusted images.
pub fn image_header_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    fn be32(b: &[u8]) -> Option<u32> { Some(u32::from_be_bytes(b.get(..4)?.try_into().ok()?)) }
    fn le16(b: &[u8]) -> Option<u32> { Some(u16::from_le_bytes(b.get(..2)?.try_into().ok()?) as u32) }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.get(12..16)? == b"IHDR" {
        return Some((be32(bytes.get(16..)?)?, be32(bytes.get(20..)?)?));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some((le16(bytes.get(6..)?)?, le16(bytes.get(8..)?)?));
    }
    if bytes.starts_with(b"BM") {
        let dib = u32::from_le_bytes(bytes.get(14..18)?.try_into().ok()?);
        if dib == 12 { return Some((le16(bytes.get(18..)?)?, le16(bytes.get(20..)?)?)); }
        if dib < 40 { return None; }
        let width = i32::from_le_bytes(bytes.get(18..22)?.try_into().ok()?);
        let height = i32::from_le_bytes(bytes.get(22..26)?.try_into().ok()?);
        return Some((u32::try_from(width).ok()?, height.checked_abs()? as u32));
    }
    if !bytes.starts_with(&[0xff, 0xd8]) { return None; }
    let mut pos = 2usize;
    while pos < bytes.len() {
        if *bytes.get(pos)? != 0xff { return None; }
        while *bytes.get(pos)? == 0xff { pos += 1; }
        let marker = *bytes.get(pos)?; pos += 1;
        if marker == 0xd9 || marker == 0xda { return None; }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) { continue; }
        let len = u16::from_be_bytes(bytes.get(pos..pos.checked_add(2)?)?.try_into().ok()?) as usize;
        if len < 2 || pos.checked_add(len)? > bytes.len() { return None; }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if len < 8 { return None; }
            let h = u16::from_be_bytes(bytes.get(pos+3..pos+5)?.try_into().ok()?) as u32;
            let w = u16::from_be_bytes(bytes.get(pos+5..pos+7)?.try_into().ok()?) as u32;
            return Some((w, h));
        }
        pos += len;
    }
    None
}

pub fn valid_image_dimensions(width: u32, height: u32) -> bool {
    width > 0 && height > 0 && (width as u64) * (height as u64) <= MAX_IMAGE_PIXELS
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_only_supported_types() {
        assert_eq!(kind(Path::new("photo.JPEG")), Kind::Image);
        assert_eq!(kind(Path::new("notes.MD")), Kind::Text);
        assert_eq!(kind(Path::new("letter.pdf")), Kind::Pdf);
    }
    #[test]
    fn decodes_text_without_silently_accepting_binary() {
        assert_eq!(decode_text(b"\xef\xbb\xbfhello").unwrap(), "hello");
        assert_eq!(decode_text(&[0xff,0xfe,0x41,0,0xac,0x20]).unwrap(), "A€");
        assert_eq!(decode_text(&[0xfe,0xff,0,0x41]).unwrap(), "A");
        assert!(decode_text(b"a\0b").is_err());
        assert!(decode_text(&[0xff, 0xfe, 0x41]).is_err());
        assert!(decode_text(&[0xff]).is_err());
    }
    #[test]
    fn checks_image_headers_before_decoding() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x02\x00\x00\x00\x03";
        assert_eq!(image_header_dimensions(png), Some((2, 3)));
        assert_eq!(image_header_dimensions(b"GIF89a\x02\x00\x03\x00"), Some((2, 3)));
        assert_eq!(image_header_dimensions(&[0xff,0xd8,0xff,0xc0,0,8,8,0,3,0,2,1]), Some((2,3)));
        assert_eq!(image_header_dimensions(&png[..19]), None);
        assert_eq!(image_header_dimensions(b"not an image"), None);
    }
    #[test]
    fn bounds_pixels_and_file_reads() {
        assert!(!valid_image_dimensions(0, 1));
        assert!(!valid_image_dimensions(u32::MAX, u32::MAX));
        assert!(valid_image_dimensions(4000, 3000));
        let dir = crate::test_support::scratch_dir("viewer");
        let path = dir.join("a.txt");
        std::fs::write(&path, b"hello").unwrap();
        assert_eq!(read_bounded(&path, 5).unwrap(), b"hello");
        assert!(read_bounded(&path, 4).is_err());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(&path, dir.join("link.txt")).unwrap();
            assert!(read_bounded(&dir.join("link.txt"), 5).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
