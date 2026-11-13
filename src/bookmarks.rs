//! User-selected folder bookmarks, stored separately from application settings.
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_BOOKMARKS: usize = 64;
const MAX_PATH_BYTES: usize = 4096;
const MAX_FILE_BYTES: u64 = (MAX_BOOKMARKS as u64) * (MAX_PATH_BYTES as u64 * 2 + 1) + 10;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
fn invalid(text: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, text) }

pub fn user_path() -> io::Result<PathBuf> {
    Ok(crate::settings::user_path()?.with_file_name("bookmarks.conf"))
}
fn valid(path: &Path) -> bool {
    path.is_absolute() && path.to_str().map_or(false, |s| !s.is_empty() && s.len() <= MAX_PATH_BYTES && !s.chars().any(char::is_control))
}
fn parse(data: &[u8]) -> io::Result<Vec<PathBuf>> {
    let text = std::str::from_utf8(data).map_err(|_| invalid("Bookmarks are not UTF-8"))?;
    let mut lines = text.lines();
    if lines.next() != Some("version=1") { return Err(invalid("Unsupported bookmark file")); }
    let mut result = Vec::new();
    for line in lines {
        if line.is_empty() || line.len() % 2 != 0 || line.len() > MAX_PATH_BYTES * 2 || result.len() >= MAX_BOOKMARKS {
            return Err(invalid("Invalid bookmark entry"));
        }
        let mut bytes = Vec::with_capacity(line.len() / 2);
        for pair in line.as_bytes().chunks_exact(2) {
            let digit = |ch: u8| match ch { b'0'..=b'9' => Some(ch-b'0'), b'a'..=b'f' => Some(ch-b'a'+10), _ => None };
            let high = digit(pair[0]).ok_or_else(|| invalid("Invalid bookmark encoding"))?;
            let low = digit(pair[1]).ok_or_else(|| invalid("Invalid bookmark encoding"))?;
            bytes.push(high * 16 + low);
        }
        let path = PathBuf::from(String::from_utf8(bytes).map_err(|_| invalid("Invalid bookmark path"))?);
        if !valid(&path) || result.contains(&path) { return Err(invalid("Invalid or duplicate bookmark")); }
        result.push(path);
    }
    Ok(result)
}

/// A missing file means no bookmarks; malformed or unsafe files are errors.
pub fn load(path: &Path) -> io::Result<Vec<PathBuf>> {
    let dir = path.parent().ok_or_else(|| invalid("Invalid bookmark path"))?;
    if !crate::settings::check_directory(dir)? { return Ok(Vec::new()); }
    let file = match crate::settings::open_existing(path) {
        Ok(file) => file, Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()), Err(e) => return Err(e),
    };
    if file.metadata()?.len() > MAX_FILE_BYTES { return Err(invalid("Bookmark file is too large")); }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES { return Err(invalid("Bookmark file is too large")); }
    parse(&bytes)
}

/// Atomically replace a valid bookmark file; paths are UTF-8 and absolute.
pub fn save(path: &Path, bookmarks: &[PathBuf]) -> io::Result<()> {
    if bookmarks.len() > MAX_BOOKMARKS || bookmarks.iter().enumerate().any(|(i, p)| !valid(p) || bookmarks[..i].contains(p)) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Bookmarks must be unique absolute UTF-8 paths, at most 64 entries and 4096 bytes per path, with no control characters"));
    }
    let dir = path.parent().ok_or_else(|| invalid("Invalid bookmark path"))?;
    if !crate::settings::check_directory(dir)? {
        #[cfg(unix)]
        { use std::os::unix::fs::DirBuilderExt; fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?; }
        #[cfg(not(unix))]
        fs::create_dir_all(dir)?;
        if !crate::settings::check_directory(dir)? { return Err(invalid("Bookmark directory is missing")); }
    }
    let _ = load(path)?;
    let mut text = String::from("version=1\n");
    const HEX: &[u8] = b"0123456789abcdef";
    for bookmark in bookmarks {
        for b in bookmark.to_str().unwrap().as_bytes() {
            text.push(HEX[(b >> 4) as usize] as char); text.push(HEX[(b & 15) as usize] as char);
        }
        text.push('\n');
    }
    for _ in 0..100 {
        let temp = dir.join(format!(".bookmarks-{}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        match options.open(&temp) {
            Ok(mut file) => {
                let result = (|| { file.write_all(text.as_bytes())?; file.sync_all()?; drop(file); let _ = load(path)?; crate::settings::replace(&temp, path) })();
                if result.is_err() { let _ = fs::remove_file(&temp); }
                return result;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "Could not allocate bookmark staging file"))
}

pub fn add(file: &Path, folder: &Path) -> io::Result<bool> {
    let mut paths = load(file)?;
    if paths.iter().any(|p| p == folder) { return Ok(false); }
    paths.push(folder.to_path_buf());
    save(file, &paths)?;
    Ok(true)
}
pub fn remove(file: &Path, folder: &Path) -> io::Result<bool> {
    let mut paths = load(file)?;
    let before = paths.len();
    paths.retain(|p| p != folder);
    if before == paths.len() { return Ok(false); }
    save(file, &paths)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_add_remove_and_invalid_files() {
        let root = crate::test_support::scratch_dir("bookmarks");
        let file = root.join("config/zfm/bookmarks.conf");
        let a = root.join("unicode 🌳 and spaces");
        let b = root.join("missing-folder");
        assert!(load(&file).unwrap().is_empty());
        assert!(add(&file, &a).unwrap());
        assert!(!add(&file, &a).unwrap());
        assert!(add(&file, &b).unwrap());
        assert_eq!(load(&file).unwrap(), vec![a.clone(), b.clone()]);
        assert!(remove(&file, &b).unwrap());
        assert!(!remove(&file, &b).unwrap());
        assert_eq!(load(&file).unwrap(), vec![a]);
        fs::write(&file, b"version=99\n").unwrap();
        assert!(load(&file).is_err());
        assert!(save(&file, &[]).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"version=99\n");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_invalid_paths_and_data() {
        let root = crate::test_support::scratch_dir("bad-bookmarks");
        let file = root.join("bookmarks.conf");
        assert!(save(&file, &[PathBuf::from("relative")]).is_err());
        assert!(save(&file, &[root.join("bad\npath")]).is_err());
        for data in ["", "version=1\nxyz\n", "version=1\n00\n", "version=1\n616263\n"] { assert!(parse(data.as_bytes()).is_err()); }
        let many: Vec<_> = (0..=MAX_BOOKMARKS).map(|i| root.join(format!("{i}"))).collect();
        assert!(save(&file, &many).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlinks() {
        use std::os::unix::fs::symlink;
        let root = crate::test_support::scratch_dir("bookmark-link");
        let target = root.join("target");
        let path = root.join("bookmarks.conf");
        fs::write(&target, "version=1\n").unwrap();
        symlink(&target, &path).unwrap();
        assert!(load(&path).is_err());
        assert!(save(&path, &[]).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
