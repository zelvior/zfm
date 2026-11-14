//! Small local Markdown/code scratchpad utilities; no execution or web content.
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const MAX_BYTES: usize = 16 * 1024;
/// A deliberately limited plain-text Markdown preview, not an HTML renderer.
/// No links, markup, scripts or remote resources are executed or fetched.
pub fn preview(input: &str) -> String {
    let mut out = String::from("Plain-text Markdown preview (limited syntax)\n\n");
    let mut fence = false;
    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") { fence = !fence; out.push_str(if fence { "[code]\n" } else { "[/code]\n" }); continue; }
        if !fence {
            let hashes = trimmed.bytes().take_while(|c| *c == b'#').count();
            if (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ') {
                out.push_str(&trimmed[hashes + 1..]); out.push('\n'); continue;
            }
            if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
                out.push_str("• "); out.push_str(item); out.push('\n'); continue;
            }
        }
        out.push_str(line); out.push('\n');
    }
    out.push_str("\nPreview is plain text: inline styling, links, tables and HTML are not rendered.");
    out
}
/// Save a new UTF-8 file. Never intentionally replaces an existing target;
/// unsuccessful writes attempt to remove the newly created incomplete file.
pub fn save_new(directory: &Path, name: &OsStr, content: &str) -> io::Result<PathBuf> {
    crate::operations::validate_name(name)?;
    if !directory.is_absolute() || !directory.is_dir() || content.len() > MAX_BYTES || content.contains('\0') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid folder or scratchpad text (maximum 16 KiB)"));
    }
    let path = directory.join(name); let mut options = OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut output = options.open(&path)?;
    let result = output.write_all(content.as_bytes()).and_then(|_| output.sync_all());
    drop(output);
    if let Err(error) = result { let _ = fs::remove_file(&path); return Err(error); }
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn preview_and_safe_new_save() {
        let root = crate::test_support::scratch_dir("scratch");
        let text = "# Heading\n- item\n```rust\n# not a heading\n```\n";
        let preview = preview(text); assert!(preview.contains("Heading\n• item\n[code]\n# not a heading"));
        let file = save_new(&root, OsStr::new("note.md"), text).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), text);
        assert!(save_new(&root, OsStr::new("note.md"), "replace").is_err());
        assert_eq!(fs::read_to_string(file).unwrap(), text);
        assert!(save_new(&root, OsStr::new("../bad"), "x").is_err());
        assert!(save_new(&root, OsStr::new("large"), &"x".repeat(MAX_BYTES + 1)).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
