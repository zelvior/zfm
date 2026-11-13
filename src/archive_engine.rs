//! Bounded invocation of the bundled, renamed upstream archive tool.
//! Extraction streams one explicitly named entry into a staged ZFM-managed file;
//! archive paths are not passed directly to the filesystem as destinations.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc};
use std::thread;
use std::time::{Duration, Instant};
#[cfg(target_os = "windows")]
use std::os::windows::{fs::MetadataExt, process::CommandExt};

const MAX_OUTPUT: u64 = 2 * 1024 * 1024;

// The upstream console reads passwords from standard input when prompted.
// Keep secrets out of arguments and environment variables. The small bound
// keeps this one write below the minimum pipe capacity on supported systems.
fn validate_password(password: Option<&str>) -> std::io::Result<()> {
    if let Some(password) = password {
        if password.is_empty() || password.len() > 512 || password.chars().any(char::is_control) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Password must be 1–512 UTF-8 bytes without control characters"));
        }
    }
    Ok(())
}
fn spawn_with_password(command: &mut Command, password: Option<&str>) -> std::io::Result<std::process::Child> {
    use std::io::Write;
    validate_password(password)?;
    command.stdin(if password.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = command.spawn()?;
    if let Some(password) = password {
        let write_result = if let Some(mut input) = child.stdin.take() {
            input.write_all(password.as_bytes()).and_then(|_| input.write_all(b"\n"))
        } else { Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Engine input unavailable")) };
        if let Err(error) = write_result {
            if error.kind() != std::io::ErrorKind::BrokenPipe {
                let _ = child.kill(); let _ = child.wait(); return Err(error);
            }
        }
    }
    Ok(child)
}


pub fn engine_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = executable.parent().ok_or("Could not find ZFM installation folder")?;
    #[cfg(target_os = "windows")]
    let candidates = [parent.join("zfm-archive-engine.exe")];
    #[cfg(target_os = "linux")]
    let candidates = [parent.join("zfm-archive-engine"), PathBuf::from("/usr/lib/zfm/zfm-archive-engine")];
    candidates.into_iter().find(|p| p.is_file())
        .ok_or_else(|| "Bundled archive engine is missing; reinstall ZFM.".into())
}

// No shell, no output extraction, no inherited input and no external PATH
// lookup. Output and time are bounded; closing the dialog terminates the job.
pub fn run_engine(path: &Path, test: bool, cancel: &AtomicBool) -> Result<Vec<String>, String> {
    run_engine_with_timeout(path, test, cancel, Duration::from_secs(120))
}

/// Inspect the inner TAR for compressed TAR names, rather than only the outer
/// compression stream. The expanded TAR is bounded and temporarily staged.
pub fn run_engine_archive_password(path: &Path, test: bool, cancel: &AtomicBool, password: Option<&str>) -> Result<Vec<String>, String> {
    if is_compressed_tar(path) {
        return with_unwrapped_tar(path, &std::env::temp_dir(), cancel, |inner| {
            run_engine_password(inner, test, cancel, Duration::from_secs(120), password)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        }).map_err(|e| e.to_string());
    }
    run_engine_password(path, test, cancel, Duration::from_secs(120), password)
}

pub fn run_engine_with_timeout(path: &Path, test: bool, cancel: &AtomicBool, timeout: Duration) -> Result<Vec<String>, String> {
    run_engine_password(path, test, cancel, timeout, None)
}

pub fn run_engine_password(path: &Path, test: bool, cancel: &AtomicBool, timeout: Duration, password: Option<&str>) -> Result<Vec<String>, String> {
    run_engine_report(path, test, cancel, timeout, password, None)
}

fn run_engine_report(path: &Path, test: bool, cancel: &AtomicBool, timeout: Duration, password: Option<&str>, member: Option<&str>) -> Result<Vec<String>, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("Select a regular file, not a symbolic link.".into());
    }
    #[cfg(target_os = "windows")]
    if meta.file_attributes() & 0x400 != 0 {
        return Err("Select a regular file, not a reparse point.".into());
    }
    if meta.len() > 2 * 1024 * 1024 * 1024 {
        return Err("Archive exceeds the 2 GiB inspection limit.".into());
    }
    let mut command = Command::new(engine_path()?);
    command.arg(if test { "t" } else { "l" });
    if !test { command.args(["-slt", "-ba"]); }
    command.args(["-bd", "-sccUTF-8"]);
    if member.is_some() { command.arg("-spd"); }
    if password.is_none() { command.arg("-p-"); }
    command.arg("--").arg(path);
    if let Some(member) = member { command.arg(member); }
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = spawn_with_password(&mut command, password).map_err(|e| format!("Could not start archive engine: {e}"))?;
    let output = child.stdout.take().ok_or("Could not read archive engine output")?;
    let (send, receive) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = output.take(MAX_OUTPUT + 1).read_to_end(&mut bytes).map(|_| bytes);
        let _ = send.send(result);
    });
    let start = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) || start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Archive operation cancelled or timed out.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(e) => { let _ = child.kill(); let _ = child.wait(); return Err(e.to_string()); }
        }
    };
    let bytes = receive.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "Could not finish reading archive output")?
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_OUTPUT { return Err("Archive listing exceeds the output limit.".into()); }
    if !status.success() { return Err(format!("Archive engine failed ({status}); the archive may be invalid or the password incorrect.")); }
    let output = std::str::from_utf8(&bytes).map_err(|_| "Archive engine produced invalid UTF-8.")?;
    let mut lines = Vec::new();
    for line in output.lines() {
        if line.is_empty() { continue; }
        if lines.len() >= 10_000 { return Err("Archive output exceeds the 10000-line limit.".into()); }
        if line.chars().any(char::is_control) { return Err("Archive output contains control characters and cannot be interpreted safely.".into()); }
        lines.push(line.to_owned());
    }
    Ok(lines)
}

/// An engine can return success while matching no member at all. Before
/// extracting, ask it to list the exact selection and require exactly one
/// matching non-directory entry with the expected size. This also rejects
/// paths that the engine's display normalizes but its selector cannot match.
fn verify_member_selection(archive: &Path, member: &str, size: u64, cancel: &AtomicBool, password: Option<&str>) -> std::io::Result<()> {
    use std::io::{self, ErrorKind};
    let bad = |message: &str| io::Error::new(ErrorKind::InvalidData, message.to_owned());
    let report = run_engine_report(archive, false, cancel, Duration::from_secs(120), password, Some(member))
        .map_err(|e| io::Error::new(ErrorKind::Other, e))?;
    let mut count = 0;
    let mut actual_size = None;
    let mut directory = false;
    for line in report {
        if let Some(name) = line.strip_prefix("Path = ") {
            count += 1;
            #[cfg(windows)]
            let matches = name.replace('\\', "/").eq_ignore_ascii_case(&member.replace('\\', "/"));
            #[cfg(not(windows))]
            let matches = name == member;
            if !matches { return Err(bad("Engine selection did not match the listed entry")); }
        } else if let Some(value) = line.strip_prefix("Size = ") {
            actual_size = value.parse::<u64>().ok();
        } else if line == "Folder = +" { directory = true; }
        if is_link_property(&line) { return Err(bad("Engine selected a linked entry")); }
    }
    if count != 1 || directory || actual_size != Some(size) {
        return Err(bad("Engine selection is missing, ambiguous, not a file, or changed"));
    }
    Ok(())
}


/// Create a 7z, ZIP or TAR archive from one regular file using the bundled
/// engine's standard-input mode. Archive paths are never taken from a tool
/// listing. Output is staged and published with a no-replace hard link.
/// Parent directories must not be writable by untrusted users.
pub fn create_single_file(source: &Path, output: &Path, cancel: &AtomicBool) -> std::io::Result<()> {
    use std::fs::{self, OpenOptions};
    use std::io::{self, ErrorKind};
    use std::sync::atomic::AtomicU64;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let err = |message: &str| io::Error::new(ErrorKind::InvalidInput, message.to_owned());
    let typ = match output.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("7z") => "7z",
        Some("zip") => "zip",
        Some("tar") => "tar",
        _ => return Err(err("Choose a .7z, .zip or .tar archive name")),
    };
    let name = source.file_name().ok_or_else(|| err("Missing input filename"))?;
    crate::operations::validate_name(name)?;
    let name = name.to_str().ok_or_else(|| err("Input filename must be Unicode"))?;
    if name.chars().any(char::is_control) { return Err(err("Input filename contains control characters")); }
    let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).ok_or_else(|| err("Output requires a parent directory"))?;
    let parent_meta = fs::symlink_metadata(parent)?;
    if !parent_meta.is_dir() || parent_meta.file_type().is_symlink() { return Err(err("Output parent must be a regular directory")); }
    #[cfg(target_os = "windows")]
    if parent_meta.file_attributes() & 0x400 != 0 { return Err(err("Output parent must not be a reparse point")); }
    let input_meta = fs::symlink_metadata(source)?;
    if !input_meta.is_file() || input_meta.file_type().is_symlink() { return Err(err("Input must be a regular file")); }
    #[cfg(target_os = "windows")]
    if input_meta.file_attributes() & 0x400 != 0 { return Err(err("Input must not be a reparse point")); }
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(0x800 | crate::linux_flags::O_NOFOLLOW);
    }
    #[cfg(all(target_os = "linux", not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))))]
    return Err(io::Error::new(ErrorKind::Unsupported, "Safe archive access is not supported on this architecture"));
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        opts.custom_flags(0x0020_0000).share_mode(1);
    }
    let file = opts.open(source)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > 512 * 1024 * 1024 { return Err(err("Input must be a regular file no larger than 512 MiB")); }
    #[cfg(target_os = "windows")]
    if meta.file_attributes() & 0x400 != 0 { return Err(err("Input must not be a reparse point")); }
    let engine = engine_path().map_err(|s| io::Error::new(ErrorKind::NotFound, s))?;
    if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, "Cancelled")); }
    let mut stage = None;
    for _ in 0..100 {
        let id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = parent.join(format!(".zfm-archive-{}-{id}", std::process::id()));
        match fs::create_dir(&dir) {
            Ok(()) => { stage = Some(dir); break; }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    let stage = stage.ok_or_else(|| err("Could not create a staging directory"))?;
    let staged = stage.join(format!("archive.{typ}"));
    let result = (|| {
        let mut command = Command::new(engine);
        command.arg("a").arg(format!("-t{typ}")).arg(format!("-si{name}"))
            .args(["-bd", "-y", "--"]).arg(&staged)
            .stdin(Stdio::from(file)).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        command.creation_flags(0x0800_0000);
        let mut child = command.spawn()?;
        let start = Instant::now();
        let status = loop {
            if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120) {
                let _ = child.kill(); let _ = child.wait();
                return Err(io::Error::new(ErrorKind::Interrupted, "Archive creation cancelled or timed out"));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(e) => { let _ = child.kill(); let _ = child.wait(); return Err(e); }
            }
        };
        if !status.success() { return Err(io::Error::new(ErrorKind::Other, format!("Archive engine failed: {status}"))); }
        let result_meta = fs::symlink_metadata(&staged)?;
        if !result_meta.is_file() || result_meta.file_type().is_symlink() || result_meta.len() > 1024 * 1024 * 1024 {
            return Err(err("Invalid or oversized staged archive"));
        }
        #[cfg(target_os = "windows")]
        if result_meta.file_attributes() & 0x400 != 0 { return Err(err("Staged archive is a reparse point")); }
        if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, "Cancelled")); }
        fs::hard_link(&staged, output)?;
        Ok(())
    })();
    let _ = fs::remove_file(&staged);
    let _ = fs::remove_dir(&stage);
    result
}

/// Extract exactly one explicitly named archive item as a regular file. The
/// engine streams bytes to ZFM instead of writing archive paths to disk.
/// The destination must be in a directory not writable by untrusted users.
#[cfg(target_os = "windows")]
pub fn extract_single_file(archive: &Path, member: &str, output: &Path, cancel: &AtomicBool) -> std::io::Result<()> {
    extract_single_file_password(archive, member, output, cancel, None)
}

#[cfg(target_os = "windows")]
pub fn extract_single_file_password(archive: &Path, member: &str, output: &Path, cancel: &AtomicBool, password: Option<&str>) -> std::io::Result<()> {
    if is_compressed_tar(archive) {
        let parent = output.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Output requires a parent directory"))?;
        return with_unwrapped_tar(archive, parent, cancel, |inner| extract_single_file_password(inner, member, output, cancel, password));
    }
    if single_compression_suffix(archive).is_some() {
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Use Extract all for a single compressed stream"));
    }
    use std::fs::{self, OpenOptions};
    use std::io::{self, ErrorKind};
    use std::os::windows::fs::OpenOptionsExt;
    let bad = |message: &str| io::Error::new(ErrorKind::InvalidData, message.to_string());
    if member.is_empty() || member.chars().any(char::is_control) || (member.starts_with('/') || member.starts_with('\\')) || member.contains(':')
        || member.split(['/', '\\']).any(|part| part == ".." || part == "." || part.is_empty()) {
        return Err(bad("Enter a safe, exact archive entry path"));
    }
    let parent = output.parent().ok_or_else(|| bad("Output requires a parent directory"))?;
    crate::operations::validate_name(output.file_name().ok_or_else(|| bad("Missing output filename"))?)?;
    let pm = fs::symlink_metadata(parent)?;
    if !pm.is_dir() || pm.file_attributes() & 0x400 != 0 { return Err(bad("Output parent must not be a reparse point")); }
    let mut opts = OpenOptions::new();
    opts.read(true).custom_flags(0x0020_0000).share_mode(1);
    let file = opts.open(archive)?;
    let before = file.metadata()?;
    if !before.is_file() || before.file_attributes() & 0x400 != 0 || before.len() > 2 * 1024 * 1024 * 1024 {
        return Err(bad("Select a regular archive no larger than 2 GiB"));
    }
    let lines = run_engine_password(archive, false, cancel, Duration::from_secs(120), password).map_err(|s| io::Error::new(ErrorKind::Other, s))?;
    let mut found = 0;
    let mut active = false;
    let mut size = None;
    let mut unsupported = false;
    for line in lines {
        if let Some(name) = line.strip_prefix("Path = ") {
            active = name.replace('\\', "/").eq_ignore_ascii_case(&member.replace('\\', "/"));
            if active { found += 1; }
        } else if active {
            if let Some(value) = line.strip_prefix("Size = ") { size = value.parse::<u64>().ok(); }
            if let Some(value) = line.strip_prefix("Attributes = ") {
                let mut parts = value.split_whitespace();
                let dos = parts.next().unwrap_or("");
                if dos.contains('D') { unsupported = true; }
                if let Some(unix) = parts.next() {
                    if !unix.starts_with('-') { unsupported = true; }
                }
            }
            if let Some(mode) = line.strip_prefix("Mode = ") {
                if !mode.starts_with('-') { unsupported = true; }
            }
            if line == "Folder = +" || is_link_property(&line) { unsupported = true; }
        }
    }
    let size = size.ok_or_else(|| bad("Entry size is missing"))?;
    if found != 1 || unsupported || size > 512 * 1024 * 1024 { return Err(bad("Entry is missing, ambiguous, not a regular file, or exceeds 512 MiB")); }
    extract_verified(archive, member, output, cancel, size, &file, &before, password)
}

// Caller has validated the selected path and listing, and retains an open
// source handle preventing other processes opening it for write or deletion.
#[cfg(target_os = "windows")]
fn extract_verified(archive: &Path, member: &str, output: &Path, cancel: &AtomicBool, size: u64, file: &std::fs::File, before: &std::fs::Metadata, password: Option<&str>) -> std::io::Result<()> {
    use std::fs::{self, OpenOptions};
    use std::io::{self, ErrorKind, Read, Write};
    use std::sync::atomic::AtomicU64;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let bad = |message: &str| io::Error::new(ErrorKind::InvalidData, message.to_string());
    verify_member_selection(archive, member, size, cancel, password)?;
    let parent = output.parent().ok_or_else(|| bad("Output requires a parent directory"))?;
    crate::operations::validate_name(output.file_name().ok_or_else(|| bad("Missing output filename"))?)?;
    let pm = fs::symlink_metadata(parent)?;
    if !pm.is_dir() || pm.file_attributes() & 0x400 != 0 { return Err(bad("Output parent must not be a reparse point")); }
    let mut stage = None;
    for _ in 0..100 {
        let dir = parent.join(format!(".zfm-extract-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        match fs::create_dir(&dir) {
            Ok(()) => { stage = Some(dir); break; }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    let stage = stage.ok_or_else(|| bad("Could not allocate a staging directory"))?;
    let staged = stage.join("data");
    let result = (|| {
        let mut destination = OpenOptions::new().write(true).create_new(true).open(&staged)?;
        let mut command = Command::new(engine_path().map_err(|e| io::Error::new(ErrorKind::NotFound, e))?);
        command.args(["x", "-so", "-bd", "-spd", "-sccUTF-8"]);
        if password.is_none() { command.arg("-p-"); }
        command.arg("--").arg(archive).arg(member)
            .stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(0x0800_0000);
        let mut child = spawn_with_password(&mut command, password)?;
        let source = child.stdout.take().ok_or_else(|| bad("Could not read archive output"))?;
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let result = (|| -> io::Result<u64> {
                let mut source = source.take(size + 1);
                let mut total = 0u64;
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let count = source.read(&mut buffer)?;
                    if count == 0 { break; }
                    total += count as u64;
                    if total > size { return Err(io::Error::new(ErrorKind::InvalidData, "Archive entry exceeds declared size")); }
                    destination.write_all(&buffer[..count])?;
                }
                destination.flush()?;
                Ok(total)
            })();
            let _ = sender.send(result);
        });
        let start = Instant::now();
        let status = loop {
            if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120) {
                let _ = child.kill(); let _ = child.wait();
                let _ = receiver.recv();
                return Err(io::Error::new(ErrorKind::Interrupted, "Extraction cancelled or timed out"));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(e) => { let _ = child.kill(); let _ = child.wait(); let _ = receiver.recv(); return Err(e); }
            }
        };
        let bytes = receiver.recv().map_err(|_| bad("Extraction worker stopped"))??;
        if !status.success() || bytes != size { return Err(bad("Archive extraction failed or size did not match")); }
        let after = file.metadata()?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() { return Err(bad("Archive changed during extraction")); }
        if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, "Cancelled")); }
        fs::hard_link(&staged, output)?;
        Ok(())
    })();
    let _ = fs::remove_file(&staged);
    let _ = fs::remove_dir(&stage);
    result
}

/// Extract a bounded archive tree by streaming individual regular files into
/// ZFM-managed no-overwrite destinations. All entries are inspected before any
/// writes. The destination and its ancestors must be trusted against concurrent
/// renames or reparse-point changes. Partial results remain on error.
#[cfg(target_os = "windows")]
pub fn extract_all(archive: &Path, destination: &Path, cancel: &AtomicBool) -> std::io::Result<usize> {
    extract_all_password(archive, destination, cancel, None)
}

#[cfg(target_os = "windows")]
pub fn extract_all_password(archive: &Path, destination: &Path, cancel: &AtomicBool, password: Option<&str>) -> std::io::Result<usize> {
    if is_compressed_tar(archive) {
        return with_unwrapped_tar(archive, destination, cancel, |inner| extract_all_password(inner, destination, cancel, password));
    }
    if single_compression_suffix(archive).is_some() { return extract_single_stream(archive, destination, cancel); }
    use std::collections::{HashMap, HashSet};
    use std::ffi::OsStr;
    use std::fs::{self, OpenOptions};
    use std::io::{self, ErrorKind};
    use std::os::windows::fs::OpenOptionsExt;
    let bad = |message: &str| io::Error::new(ErrorKind::InvalidData, message.to_owned());
    let mut archive_options = OpenOptions::new();
    archive_options.read(true).custom_flags(0x0020_0000).share_mode(1);
    let source = archive_options.open(archive)?;
    let before = source.metadata()?;
    if !before.is_file() || before.file_attributes() & 0x400 != 0 || before.len() > 2 * 1024 * 1024 * 1024 {
        return Err(bad("Select a regular archive no larger than 2 GiB"));
    }
    if !destination.is_absolute() { return Err(bad("Destination must be an absolute directory path")); }
    let meta = fs::symlink_metadata(destination)?;
    if !meta.is_dir() || meta.file_attributes() & 0x400 != 0 { return Err(bad("Destination must be an existing directory, not a reparse point")); }
    let listing = run_engine_password(archive, false, cancel, Duration::from_secs(120), password).map_err(|e| io::Error::new(ErrorKind::Other, e))?;
    struct Item { original: String, parts: Vec<String>, folder: bool, size: u64 }
    struct Record { name: String, size: Option<u64>, folder: bool, has_type: bool, special: bool }
    let mut records = Vec::new();
    let mut current: Option<Record> = None;
    for line in listing {
        if let Some(name) = line.strip_prefix("Path = ") {
            if let Some(record) = current.take() { records.push(record); }
            current = Some(Record { name: name.to_owned(), size: None, folder: false, has_type: false, special: false });
        } else if let Some(record) = &mut current {
            if let Some(value) = line.strip_prefix("Size = ") { record.size = value.parse().ok(); }
            if let Some(value) = line.strip_prefix("Folder = ") { record.has_type = true; record.folder = value == "+"; }
            if let Some(value) = line.strip_prefix("Attributes = ") {
                record.has_type = true;
                for part in value.split_whitespace() {
                    if part.len() >= 10 && matches!(part.as_bytes()[0], b'-' | b'd' | b'l' | b'p' | b'c' | b'b' | b's') {
                        match part.as_bytes()[0] { b'd' => record.folder = true, b'-' => (), _ => record.special = true }
                    } else if part.contains('D') { record.folder = true; }
                }
            }
            if let Some(mode) = line.strip_prefix("Mode = ") {
                record.has_type = true;
                match mode.as_bytes().first() { Some(b'd') => record.folder = true, Some(b'-') => (), _ => record.special = true }
            }
            if is_link_property(&line) || (line == "Encrypted = +" && password.is_none()) { record.special = true; }
        }
    }
    if let Some(record) = current { records.push(record); }
    if records.len() > 1024 { return Err(bad("Archive exceeds the 1024-entry Windows extraction limit")); }
    let mut items = Vec::new();
    let mut names = HashSet::new();
    let mut kinds = HashMap::new();
    let mut total = 0u64;
    let reserved = reserved_staging_name(archive, destination);
    for record in records {
        if reserved.as_ref().map_or(false, |name| record.name.split(|c| c == '/' || c == '\\').next().map_or(false, |part| part.eq_ignore_ascii_case(name))) {
            return Err(bad("Archive entry conflicts with the temporary TAR directory"));
        }
        if record.special || !record.has_type { return Err(bad("Archive contains encrypted, linked, special or unclassified entries")); }
        let mut raw = record.name.as_str();
        if record.folder { raw = raw.trim_end_matches(|c| c == '/' || c == '\\'); }
        if raw.is_empty() || raw.starts_with('/') || raw.starts_with('\\') || raw.contains(':') || raw.chars().any(char::is_control) {
            return Err(bad("Archive contains an unsafe path"));
        }
        let mut parts = Vec::new();
        for part in raw.split(['/', '\\']) {
            crate::operations::validate_name(OsStr::new(part)).map_err(|_| bad("Archive contains an invalid Windows filename"))?;
            parts.push(part.to_owned());
        }
        if parts.len() > 128 { return Err(bad("Archive path is too deep")); }
        let key = parts.join("/").to_uppercase();
        if !names.insert(key.clone()) { return Err(bad("Archive contains duplicate or case-conflicting paths")); }
        let size = record.size.ok_or_else(|| bad("Archive entry size is missing"))?;
        if record.folder && size != 0 { return Err(bad("Archive folder has nonzero size")); }
        if size > 512 * 1024 * 1024 { return Err(bad("Archive entry exceeds 512 MiB")); }
        total = total.checked_add(size).ok_or_else(|| bad("Archive size overflow"))?;
        if total > 1024 * 1024 * 1024 { return Err(bad("Archive exceeds the 1 GiB expansion limit")); }
        kinds.insert(key, record.folder);
        items.push(Item { original: record.name, parts, folder: record.folder, size });
    }
    for item in &items {
        let mut prefix = String::new();
        for part in item.parts.iter().take(item.parts.len().saturating_sub(1)) {
            if !prefix.is_empty() { prefix.push('/'); }
            prefix.push_str(&part.to_uppercase());
            if kinds.get(&prefix) == Some(&false) { return Err(bad("Archive path has a file as a parent")); }
        }
    }
    // Processing shallow folders first preserves empty directories. Each
    // parent component is checked after creation and before use.
    items.sort_by_key(|item| (item.parts.len(), !item.folder));
    let mut completed = 0usize;
    for item in items {
        let current = source.metadata()?;
        if before.len() != current.len() || before.modified().ok() != current.modified().ok() {
            return Err(bad("Archive changed during extraction; earlier entries may remain"));
        }
        if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, format!("Cancelled after {completed} files; earlier entries remain"))); }
        let mut parent = destination.to_path_buf();
        let directories = if item.folder { item.parts.len() } else { item.parts.len() - 1 };
        for component in item.parts.iter().take(directories) {
            parent.push(component);
            match fs::create_dir(&parent) {
                Ok(()) => (),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => (),
                Err(error) => return Err(io::Error::new(error.kind(), format!("Could not create {} after {completed} files: {error}", parent.display()))),
            }
            let metadata = fs::symlink_metadata(&parent)?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 { return Err(bad("Destination contains a reparse point or non-directory")); }
        }
        if !item.folder {
            let output = parent.join(item.parts.last().unwrap());
            // Paths are validated before any writes; the engine streams data
            // rather than resolving archive paths in the destination tree.
            if let Err(error) = extract_verified(archive, &item.original, &output, cancel, item.size, &source, &before, password) {
                return Err(io::Error::new(error.kind(), format!("Extraction stopped after {completed} files: {error}. Earlier entries remain.")));
            }
            completed += 1;
        }
    }
    Ok(completed)
}

#[derive(Clone, Copy)]
struct CreationFormat { container: &'static str, compression: Option<&'static str> }
fn creation_format(output: &Path) -> Option<CreationFormat> {
    let name = output.file_name()?.to_str()?.to_ascii_lowercase();
    let compression = if name.ends_with(".tar.gz") || name.ends_with(".tgz") { Some("gzip") }
        else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") || name.ends_with(".tbz") { Some("bzip2") }
        else if name.ends_with(".tar.xz") || name.ends_with(".txz") { Some("xz") }
        else { None };
    if compression.is_some() { return Some(CreationFormat { container: "tar", compression }); }
    let container = if name.ends_with(".7z") { "7z" } else if name.ends_with(".zip") { "zip" }
        else if name.ends_with(".tar") { "tar" } else if name.ends_with(".gz") { "gzip" }
        else if name.ends_with(".bz2") { "bzip2" } else if name.ends_with(".xz") { "xz" } else { return None };
    Some(CreationFormat { container, compression: None })
}

/// Whether the output name is supported for archive creation.
pub fn supports_creation(output: &Path, encrypted: bool) -> bool {
    creation_format(output).map_or(false, |format| !encrypted || matches!(format.container, "7z" | "zip"))
}

/// Create an archive from regular files and directory trees using a private
/// staging tree and the bundled engine. No links or special files are copied.
/// Source and destination directories must not be writable by untrusted users.
/// A crash can leave a temporary staging directory beside the output.
pub fn create_items(sources: &[PathBuf], output: &Path, cancel: &AtomicBool) -> std::io::Result<()> {
    create_items_password(sources, output, cancel, None)
}

/// As `create_items`, optionally encrypting 7z or ZIP using a password sent
/// through stdin. 7z headers are encrypted; ZIP uses AES-256.
pub fn create_items_password(sources: &[PathBuf], output: &Path, cancel: &AtomicBool, password: Option<&str>) -> std::io::Result<()> {
    use std::collections::HashSet;
    use std::fs;
    use std::io::{self, ErrorKind};
    use std::sync::atomic::AtomicU64;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let bad = |message: &str| io::Error::new(ErrorKind::InvalidInput, message.to_string());
    if sources.is_empty() || sources.len() > 1024 { return Err(bad("Select between 1 and 1024 items")); }
    let format = creation_format(output).ok_or_else(|| bad("Choose a .7z, .zip, .tar, .tar.gz, .tar.bz2, .tar.xz, .gz, .bz2 or .xz output name"))?;
    let typ = format.container;
    validate_password(password)?;
    if password.is_some() && !matches!(typ, "7z" | "zip") { return Err(bad("Encryption requires .7z or .zip")); }
    if matches!(typ, "gzip" | "bzip2" | "xz") && (sources.len() != 1 || !fs::symlink_metadata(&sources[0])?.is_file()) {
        return Err(bad("Standalone compression requires one regular file"));
    }
    if typ == "zip" && password.map_or(false, |p| !p.is_ascii()) { return Err(bad("ZIP encryption requires an ASCII password in the bundled engine")); }
    let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).ok_or_else(|| bad("Output requires a parent directory"))?;
    crate::operations::validate_name(output.file_name().ok_or_else(|| bad("Missing output filename"))?)?;
    let pm = fs::symlink_metadata(parent)?;
    if !pm.is_dir() || pm.file_type().is_symlink() { return Err(bad("Output parent must be a regular directory")); }
    #[cfg(windows)]
    if pm.file_attributes() & 0x400 != 0 { return Err(bad("Output parent must not be a reparse point")); }
    let parent_real = fs::canonicalize(parent)?;
    let mut names = HashSet::new();
    for source in sources {
        let name = source.file_name().ok_or_else(|| bad("Each input must have a filename"))?;
        crate::operations::validate_name(name)?;
        let key = name.to_string_lossy().to_uppercase();
        if !names.insert(key) { return Err(bad("Input names collide")); }
        let meta = fs::symlink_metadata(source)?;
        if meta.file_type().is_symlink() { return Err(bad("Input must not be a symbolic link")); }
        #[cfg(windows)]
        if meta.file_attributes() & 0x400 != 0 { return Err(bad("Input must not be a reparse point")); }
        if meta.is_dir() {
            let source_real = fs::canonicalize(source)?;
            #[cfg(windows)]
            let inside = {
                let p = parent_real.to_string_lossy().to_uppercase();
                let s = source_real.to_string_lossy().to_uppercase();
                p == s || p.strip_prefix(&s).map_or(false, |tail| tail.starts_with('\\') || tail.starts_with('/'))
            };
            #[cfg(not(windows))]
            let inside = parent_real.starts_with(source_real);
            if inside { return Err(bad("Cannot create an archive inside a selected folder")); }
        }
    }
    fn check_tree(roots: &[PathBuf], cancel: &AtomicBool) -> io::Result<()> {
        let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|p| (p.clone(), 0)).collect();
        let (mut count, mut total) = (0usize, 0u64);
        while let Some((path, depth)) = stack.pop() {
            if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, "Archive creation cancelled")); }
            count += 1;
            if count > 10_000 || depth > 128 { return Err(io::Error::new(ErrorKind::InvalidInput, "Archive input exceeds entry or depth limits")); }
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() { return Err(io::Error::new(ErrorKind::InvalidInput, "Archive input contains a symbolic link")); }
            #[cfg(windows)]
            if meta.file_attributes() & 0x400 != 0 { return Err(io::Error::new(ErrorKind::InvalidInput, "Archive input contains a reparse point")); }
            if meta.is_dir() {
                for child in fs::read_dir(&path)? {
                    let child = child?;
                    crate::operations::validate_name(&child.file_name())?;
                    stack.push((child.path(), depth + 1));
                }
            } else if meta.is_file() {
                if meta.len() > 512 * 1024 * 1024 { return Err(io::Error::new(ErrorKind::InvalidInput, "Input file exceeds 512 MiB")); }
                total = total.checked_add(meta.len()).ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "Input size overflow"))?;
                if total > 1024 * 1024 * 1024 { return Err(io::Error::new(ErrorKind::InvalidInput, "Archive input exceeds 1 GiB")); }
            } else { return Err(io::Error::new(ErrorKind::InvalidInput, "Archive input contains a special file")); }
        }
        Ok(())
    }
    check_tree(sources, cancel)?;
    let mut stage = None;
    for _ in 0..100 {
        let dir = parent.join(format!(".zfm-archive-tree-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
        match builder.create(&dir) {
            Ok(()) => { stage = Some(dir); break; }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let stage = stage.ok_or_else(|| bad("Could not allocate staging directory"))?;
    let outcome = (|| {
        let stage = fs::canonicalize(&stage)?;
        let content = stage.join("content");
        fs::create_dir(&content)?;
        let mut roots = Vec::new();
        for source in sources {
            let name = source.file_name().ok_or_else(|| bad("Missing input filename"))?;
            roots.push(crate::operations::copy_item(source, &content, name, cancel, |_| {})?);
        }
        check_tree(&roots, cancel)?;
        let staged_inner = stage.join(format!("result.{typ}"));
        let staged = match format.compression {
            Some(compression) => stage.join(format!("result.tar.{compression}")),
            None => staged_inner.clone(),
        };
        let engine = engine_path().map_err(|e| io::Error::new(ErrorKind::NotFound, e))?;
        let deadline = Instant::now() + Duration::from_secs(600);
        let run = |command: &mut Command, password: Option<&str>| -> io::Result<()> {
            let mut child = spawn_with_password(command, password)?;
            let status = loop {
                if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    let _ = child.kill(); let _ = child.wait();
                    return Err(io::Error::new(ErrorKind::Interrupted, "Archive creation cancelled or timed out"));
                }
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => thread::sleep(Duration::from_millis(50)),
                    Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); }
                }
            };
            if !status.success() { return Err(io::Error::new(ErrorKind::Other, format!("Archive engine failed: {status}"))); }
            Ok(())
        };
        let mut command = Command::new(&engine);
        // Disable wildcard expansion and prefix names so leading @ cannot be
        // interpreted as a 7-Zip listfile argument (even after --).
        command.arg("a").arg(format!("-t{typ}")).args(["-bd", "-y", "-spd", "-sccUTF-8"]);
        if password.is_some() {
            command.arg("-p").arg(if typ == "7z" { "-mhe=on" } else { "-mem=AES256" });
        }
        command.arg("--").arg(&staged_inner);
        for source in sources { command.arg(Path::new(".").join(source.file_name().unwrap())); }
        command.current_dir(&content).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        run(&mut command, password)?;
        if let Some(compression) = format.compression {
            let intermediate = fs::symlink_metadata(&staged_inner)?;
            if !intermediate.is_file() || intermediate.file_type().is_symlink() || intermediate.len() > 2 * 1024 * 1024 * 1024 {
                return Err(bad("Invalid or oversized intermediate TAR"));
            }
            #[cfg(windows)]
            if intermediate.file_attributes() & 0x400 != 0 { return Err(bad("Intermediate TAR is a reparse point")); }
            let mut command = Command::new(&engine);
            command.arg("a").arg(format!("-t{compression}")).args(["-bd", "-y", "-spd", "-sccUTF-8", "--"])
                .arg(&staged).arg("./result.tar")
                .current_dir(&stage).stdout(Stdio::null()).stderr(Stdio::null());
            #[cfg(windows)]
            command.creation_flags(0x0800_0000);
            run(&mut command, None)?;
        }
        let result = fs::symlink_metadata(&staged)?;
        if !result.is_file() || result.file_type().is_symlink() || result.len() > 2 * 1024 * 1024 * 1024 {
            return Err(bad("Invalid or oversized staged archive"));
        }
        #[cfg(windows)]
        if result.file_attributes() & 0x400 != 0 { return Err(bad("Staged archive is a reparse point")); }
        if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, "Archive creation cancelled")); }
        fs::hard_link(&staged, output)?;
        Ok(())
    })();
    match fs::remove_dir_all(&stage) {
        Ok(()) => outcome,
        Err(error) if error.kind() == ErrorKind::NotFound => outcome,
        Err(error) => match outcome {
            Ok(()) => Err(io::Error::new(error.kind(), format!("Archive created, but staging cleanup failed at {}: {error}", stage.display()))),
            Err(original) => Err(io::Error::new(original.kind(), format!("{original}; staging cleanup failed at {}: {error}", stage.display()))),
        },
    }
}

/// Linux extraction via the bundled engine. It validates the complete listing,
/// then streams each file to a no-follow directory-relative, no-replace output.
/// The archive and destination ancestors must not be changed concurrently by
/// untrusted users. Earlier outputs remain after a failure or cancellation.
#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
pub fn extract_all_linux(archive: &Path, destination: &Path, cancel: &AtomicBool, password: Option<&str>) -> std::io::Result<usize> {
    if is_compressed_tar(archive) {
        return with_unwrapped_tar(archive, destination, cancel, |inner| extract_all_linux(inner, destination, cancel, password));
    }
    if single_compression_suffix(archive).is_some() { return extract_single_stream(archive, destination, cancel); }
    use std::collections::{HashMap, HashSet};
    use std::fs::OpenOptions;
    use std::io::{self, ErrorKind, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use crate::archive::linux::Directory;
    let bad = |s: &str| io::Error::new(ErrorKind::InvalidData, s.to_owned());
    validate_password(password)?;
    if !destination.is_absolute() { return Err(bad("Destination must be an absolute directory path")); }
    let file = OpenOptions::new().read(true).custom_flags(0x800 | crate::linux_flags::O_NOFOLLOW).open(archive)?;
    let before = file.metadata()?;
    if !before.is_file() || before.len() > 2 * 1024 * 1024 * 1024 { return Err(bad("Select a regular archive no larger than 2 GiB")); }
    let unchanged = || -> io::Result<()> {
        let held = file.metadata()?;
        let path = std::fs::symlink_metadata(archive)?;
        if !path.is_file() || path.file_type().is_symlink() || before.dev() != path.dev() || before.ino() != path.ino()
            || before.len() != held.len() || before.modified().ok() != held.modified().ok() || before.len() != path.len() || before.modified().ok() != path.modified().ok() {
            return Err(bad("Archive changed during extraction"));
        }
        Ok(())
    };
    unchanged()?;
    let lines = run_engine_password(archive, false, cancel, Duration::from_secs(120), password).map_err(|e| io::Error::new(ErrorKind::Other, e))?;
    unchanged()?;
    struct Record { name: String, size: Option<u64>, folder: bool, typed: bool, special: bool }
    let mut records = Vec::new();
    let mut current: Option<Record> = None;
    for line in lines {
        if let Some(name) = line.strip_prefix("Path = ") {
            if let Some(record) = current.take() { records.push(record); }
            current = Some(Record { name: name.to_owned(), size: None, folder: false, typed: false, special: false });
        } else if let Some(record) = &mut current {
            if let Some(value) = line.strip_prefix("Size = ") { record.size = value.parse().ok(); }
            if let Some(value) = line.strip_prefix("Folder = ") { record.typed = true; record.folder = value == "+"; }
            if let Some(value) = line.strip_prefix("Attributes = ") {
                record.typed = true;
                for part in value.split_whitespace() {
                    if part.len() >= 10 && matches!(part.as_bytes()[0], b'-' | b'd' | b'l' | b'p' | b'c' | b'b' | b's') {
                        match part.as_bytes()[0] { b'd' => record.folder = true, b'-' => (), _ => record.special = true }
                    } else if part.contains('D') { record.folder = true; }
                }
            }
            if let Some(mode) = line.strip_prefix("Mode = ") {
                record.typed = true;
                match mode.as_bytes().first() { Some(b'd') => record.folder = true, Some(b'-') => (), _ => record.special = true }
            }
            if is_link_property(&line) || (line == "Encrypted = +" && password.is_none()) { record.special = true; }
        }
    }
    if let Some(record) = current { records.push(record); }
    if records.len() > 1024 { return Err(bad("Archive exceeds the 1024-entry engine extraction limit")); }
    struct Item { name: String, path: String, folder: bool, size: u64 }
    let mut items = Vec::new();
    let mut names = HashSet::new();
    let mut kinds = HashMap::new();
    let mut total = 0u64;
    let reserved = reserved_staging_name(archive, destination);
    for record in records {
        if record.special || !record.typed { return Err(bad("Archive contains linked, special, encrypted or unclassified entries")); }
        let path = crate::archive::safe_name(&record.name)?;
        if reserved.as_ref().map_or(false, |name| path.split('/').next() == Some(name.as_str())) {
            return Err(bad("Archive entry conflicts with the temporary TAR directory"));
        }
        if path.is_empty() { return Err(bad("Archive contains an empty entry path")); }
        if path.split('/').count() > 128 || !names.insert(path.clone()) { return Err(bad("Archive contains a duplicate or overly deep path")); }
        let size = record.size.ok_or_else(|| bad("Archive entry size is missing"))?;
        if record.folder && size != 0 { return Err(bad("Archive folder has nonzero size")); }
        if size > 512 * 1024 * 1024 { return Err(bad("Archive entry exceeds 512 MiB")); }
        total = total.checked_add(size).ok_or_else(|| bad("Archive size overflow"))?;
        if total > 1024 * 1024 * 1024 { return Err(bad("Archive exceeds the 1 GiB expansion limit")); }
        kinds.insert(path.clone(), record.folder);
        items.push(Item { name: record.name, path, folder: record.folder, size });
    }
    for item in &items {
        let mut prefix = String::new();
        for part in item.path.split('/').take(item.path.split('/').count() - 1) {
            if !prefix.is_empty() { prefix.push('/'); }
            prefix.push_str(part);
            if kinds.get(&prefix) == Some(&false) { return Err(bad("Archive path has a file as a parent")); }
        }
    }
    items.sort_by_key(|item| (item.path.split('/').count(), !item.folder));
    let root = Directory::open_root(destination)?;
    let mut completed = 0;
    for item in items {
        unchanged()?;
        if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, "Extraction cancelled; earlier entries remain")); }
        let parts: Vec<_> = item.path.split('/').collect();
        let mut dir = root.duplicate()?;
        let parents = if item.folder { parts.len() } else { parts.len() - 1 };
        for part in &parts[..parents] { dir = dir.ensure_directory(part)?; }
        if item.folder { continue; }
        verify_member_selection(archive, &item.name, item.size, cancel, password)?;
        unchanged()?;
        let name = parts[parts.len() - 1];
        let output = dir.create_file(name)?;
        let result = (|| -> io::Result<()> {
            let mut command = Command::new(engine_path().map_err(|e| io::Error::new(ErrorKind::NotFound, e))?);
            command.args(["x", "-so", "-bd", "-spd", "-sccUTF-8"]);
            if password.is_none() { command.arg("-p-"); }
            command.arg("--").arg(archive).arg(&item.name).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut child = spawn_with_password(&mut command, password)?;
            let stdout = match child.stdout.take() { Some(value) => value, None => { let _ = child.kill(); let _ = child.wait(); return Err(bad("Could not read archive output")); } };
            let size = item.size;
            let (sender, receiver) = mpsc::sync_channel(1);
            thread::spawn(move || {
                let result = (|| -> io::Result<u64> {
                    let mut source = stdout.take(size + 1);
                    let mut output = output;
                    let mut total = 0;
                    let mut buffer = [0u8; 64 * 1024];
                    loop {
                        let n = source.read(&mut buffer)?;
                        if n == 0 { break; }
                        total += n as u64;
                        if total > size { return Err(io::Error::new(ErrorKind::InvalidData, "Entry exceeds declared size")); }
                        output.write_all(&buffer[..n])?;
                    }
                    output.flush()?;
                    Ok(total)
                })();
                let _ = sender.send(result);
            });
            let start = Instant::now();
            let status = loop {
                if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120) {
                    let _ = child.kill(); let _ = child.wait(); let _ = receiver.recv();
                    return Err(io::Error::new(ErrorKind::Interrupted, "Extraction cancelled or timed out"));
                }
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => thread::sleep(Duration::from_millis(50)),
                    Err(error) => { let _ = child.kill(); let _ = child.wait(); let _ = receiver.recv(); return Err(error); }
                }
            };
            let count = receiver.recv().map_err(|_| bad("Extraction worker stopped"))??;
            if !status.success() || count != size { return Err(bad("Archive extraction failed or size did not match")); }
            unchanged()?;
            if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, "Extraction cancelled")); }
            Ok(())
        })();
        if let Err(error) = result {
            dir.remove_file(name);
            return Err(io::Error::new(error.kind(), format!("Extraction stopped after {completed} files: {error}. Earlier entries remain.")));
        }
        completed += 1;
    }
    Ok(completed)
}

#[cfg(all(target_os = "linux", not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))))]
pub fn extract_all_linux(_: &Path, _: &Path, _: &AtomicBool, _: Option<&str>) -> std::io::Result<usize> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Safe extraction is not supported on this architecture"))
}

// Some formats emit these properties for every entry with an empty value.
fn is_link_property(line: &str) -> bool {
    ["Symbolic Link = ", "Hard Link = "].iter().any(|prefix| line.strip_prefix(prefix).map_or(false, |value| !value.is_empty()))
}

fn reserved_staging_name(archive: &Path, destination: &Path) -> Option<String> {
    if archive.file_name()?.to_str()? != "payload.tar" { return None; }
    let parent = archive.parent()?;
    if parent.parent()? != destination { return None; }
    let name = parent.file_name()?.to_str()?;
    if !name.starts_with(".zfm-tar-") { return None; }
    Some(name.to_owned())
}

fn is_compressed_tar(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    [".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tbz", ".tar.xz", ".txz"].iter().any(|suffix| name.ends_with(suffix))
}

fn single_compression_suffix(path: &Path) -> Option<usize> {
    if is_compressed_tar(path) { return None; }
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".bz2") { Some(4) }
    else if name.ends_with(".gz") || name.ends_with(".xz") { Some(3) }
    else { None }
}

/// Decompress a single-stream file to the destination without replacing files.
/// The output name is the source filename with its compression suffix removed.
fn extract_single_stream(archive: &Path, destination: &Path, cancel: &AtomicBool) -> std::io::Result<usize> {
    use std::io::{self, ErrorKind};
    let length = single_compression_suffix(archive).ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "Unsupported compression stream"))?;
    let source_name = archive.file_name().and_then(|name| name.to_str()).ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "Compressed filename must be Unicode"))?;
    let name = &source_name[..source_name.len() - length];
    crate::operations::validate_name(std::ffi::OsStr::new(name))?;
    with_unwrapped(archive, destination, cancel, 512 * 1024 * 1024, "payload", |staged| {
        if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, "Extraction cancelled")); }
        std::fs::hard_link(staged, destination.join(name))?;
        Ok(1)
    })
}

/// Unwrap one compressed TAR into a private temporary directory, then let the
/// caller perform the usual archive listing/path validation and extraction.
/// The caller must ensure the input and destination directories are trusted.
fn with_unwrapped_tar<T, F: FnOnce(&Path) -> std::io::Result<T>>(
    archive: &Path, destination: &Path, cancel: &AtomicBool, action: F,
) -> std::io::Result<T> {
    with_unwrapped(archive, destination, cancel, 1088 * 1024 * 1024, "payload.tar", action)
}

fn with_unwrapped<T, F: FnOnce(&Path) -> std::io::Result<T>>(
    archive: &Path, destination: &Path, cancel: &AtomicBool, limit: u64, payload_name: &str, action: F,
) -> std::io::Result<T> {
    use std::fs::{self, OpenOptions};
    use std::io::{self, ErrorKind, Write};
    use std::sync::atomic::AtomicU64;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let bad = |s: &str| io::Error::new(ErrorKind::InvalidData, s.to_owned());
    if !destination.is_absolute() { return Err(bad("Destination must be an absolute directory path")); }
    let meta = fs::symlink_metadata(destination)?;
    if !meta.is_dir() || meta.file_type().is_symlink() { return Err(bad("Destination must be an existing regular directory")); }
    #[cfg(windows)]
    if meta.file_attributes() & 0x400 != 0 { return Err(bad("Destination must not be a reparse point")); }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    { use std::os::unix::fs::OpenOptionsExt; options.custom_flags(0x800 | crate::linux_flags::O_NOFOLLOW); }
    #[cfg(all(target_os = "linux", not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))))]
    return Err(io::Error::new(ErrorKind::Unsupported, "Safe archive access is not supported on this architecture"));
    #[cfg(windows)]
    { use std::os::windows::fs::OpenOptionsExt; options.custom_flags(0x0020_0000).share_mode(1); }
    let source = options.open(archive)?;
    let before = source.metadata()?;
    if !before.is_file() || before.file_type().is_symlink() || before.len() > 2 * 1024 * 1024 * 1024 { return Err(bad("Select a regular archive no larger than 2 GiB")); }
    #[cfg(windows)]
    if before.file_attributes() & 0x400 != 0 { return Err(bad("Archive must not be a reparse point")); }
    #[cfg(target_os = "linux")]
    let unchanged = || -> io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let path = fs::symlink_metadata(archive)?;
        let held = source.metadata()?;
        if !path.is_file() || path.file_type().is_symlink() || path.dev() != before.dev() || path.ino() != before.ino()
            || path.len() != before.len() || held.len() != before.len()
            || path.modified().ok() != before.modified().ok() || held.modified().ok() != before.modified().ok() {
            return Err(bad("Archive changed during decompression"));
        }
        Ok(())
    };
    #[cfg(windows)]
    let unchanged = || -> io::Result<()> {
        let held = source.metadata()?;
        if held.len() != before.len() || held.modified().ok() != before.modified().ok() { return Err(bad("Archive changed during decompression")); }
        Ok(())
    };
    unchanged()?;
    let mut stage = None;
    for _ in 0..100 {
        let path = destination.join(format!(".zfm-tar-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
        match builder.create(&path) {
            Ok(()) => { stage = Some(path); break; }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    let stage = stage.ok_or_else(|| bad("Could not allocate temporary decompression directory"))?;
    let result = (|| -> io::Result<T> {
        let inner = stage.join(payload_name);
        let mut output_options = OpenOptions::new();
        output_options.write(true).create_new(true);
        #[cfg(unix)]
        { use std::os::unix::fs::OpenOptionsExt; output_options.mode(0o600); }
        let mut output = output_options.open(&inner)?;
        let mut command = Command::new(engine_path().map_err(|e| io::Error::new(ErrorKind::NotFound, e))?);
        let filename = archive.file_name().and_then(|name| name.to_str()).unwrap_or("").to_ascii_lowercase();
        let compression = if filename.ends_with(".gz") || filename.ends_with(".tgz") { "gzip" }
            else if filename.ends_with(".bz2") || filename.ends_with(".tbz") || filename.ends_with(".tbz2") { "bzip2" }
            else { "xz" };
        command.arg("x").arg(format!("-t{compression}")).args(["-so", "-bd", "-p-", "--"]).arg(archive)
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command.spawn()?;
        let stdout = match child.stdout.take() { Some(value) => value, None => { let _ = child.kill(); let _ = child.wait(); return Err(bad("Could not read decompressed data")); } };
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let result = (|| -> io::Result<()> {
                let mut input = stdout.take(limit + 1);
                let mut total = 0u64;
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let n = input.read(&mut buffer)?;
                    if n == 0 { break; }
                    total += n as u64;
                    if total > limit { return Err(bad("Decompressed data exceeds the expansion limit")); }
                    output.write_all(&buffer[..n])?;
                }
                output.flush()
            })();
            let _ = sender.send(result);
        });
        let start = Instant::now();
        let status = loop {
            if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120) {
                let _ = child.kill(); let _ = child.wait(); let _ = receiver.recv();
                return Err(io::Error::new(ErrorKind::Interrupted, "Decompression cancelled or timed out"));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(e) => { let _ = child.kill(); let _ = child.wait(); let _ = receiver.recv(); return Err(e); }
            }
        };
        receiver.recv().map_err(|_| bad("Decompression worker stopped"))??;
        if !status.success() { return Err(bad("Archive decompression failed")); }
        unchanged()?;
        if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(ErrorKind::Interrupted, "Extraction cancelled")); }
        action(&inner)
    })();
    match fs::remove_dir_all(&stage) {
        Ok(()) => result,
        Err(e) if e.kind() == ErrorKind::NotFound => result,
        Err(e) => match result {
            Ok(_) => Err(io::Error::new(e.kind(), format!("Extraction completed, but temporary decompression cleanup failed at {}: {e}", stage.display()))),
            Err(original) => Err(io::Error::new(original.kind(), format!("{original}; temporary decompression cleanup failed at {}: {e}", stage.display()))),
        },
    }
}
