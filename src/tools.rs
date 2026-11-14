//! Explicitly invoked external interpreter and terminal integration.
//! These are not sandboxed; the selected program has the user's privileges.
use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc};
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 128 * 1024;
const TIME_LIMIT: Duration = Duration::from_secs(30);
fn invalid(s: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, s) }
fn validate(program: &OsStr, directory: &Path) -> io::Result<()> {
    if program.is_empty() || !directory.is_absolute() || !directory.is_dir() { return Err(invalid("Choose an executable and an existing absolute working folder")); }
    Ok(())
}
/// Launch a user-specified terminal executable with zero or one literal argument.
/// No shell parses the argument; executable lookup follows the OS PATH rules.
pub fn terminal(program: &OsStr, argument: &OsStr, directory: &Path) -> io::Result<()> {
    validate(program, directory)?;
    let mut cmd = Command::new(program); cmd.current_dir(directory);
    if !argument.is_empty() { cmd.arg(argument); }
    #[cfg(windows)] { use std::os::windows::process::CommandExt; cmd.creation_flags(0x0000_0010); }
    let mut child = cmd.spawn()?;
    // Reap the terminal after it exits; dropping Child alone leaves a zombie on Unix.
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(())
}
fn collect<R: Read + Send + 'static>(mut reader: R) -> mpsc::Receiver<(Vec<u8>, bool)> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut data = Vec::new(); let mut truncated = false; let mut buf = [0; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break; }
            let keep = n.min(OUTPUT_LIMIT.saturating_sub(data.len()));
            data.extend_from_slice(&buf[..keep]); if keep < n { truncated = true; }
        }
        let _ = tx.send((data, truncated));
    });
    rx
}
/// Run a code snippet using an installed interpreter. `flag` is a single literal
/// argument (for example -c, -e or -Command). No language is bundled. The
/// output capture is bounded; timeout/cancellation only kills the direct child,
/// not any descendants. This is not a sandbox.
pub fn snippet(program: &OsStr, flag: &OsStr, code: &str, directory: &Path, cancel: &AtomicBool) -> io::Result<String> {
    validate(program, directory)?;
    if code.is_empty() || code.len() > 16 * 1024 { return Err(invalid("Snippet must contain 1–16384 bytes")); }
    if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled")); }
    execute(program, flag, OsStr::new(code), directory, cancel)
}

/// Run a selected script file with an explicitly chosen installed interpreter.
pub fn script_file(program: &OsStr, flag: &OsStr, path: &Path, directory: &Path, cancel: &AtomicBool) -> io::Result<String> {
    validate(program, directory)?;
    let _checked = crate::settings::open_existing(path)?;
    // The interpreter reopens the path; use only trusted directories because
    // a concurrent replacement between validation and execution is possible.
    execute(program, flag, path.as_os_str(), directory, cancel)
}
fn execute(program: &OsStr, flag: &OsStr, argument: &OsStr, directory: &Path, cancel: &AtomicBool) -> io::Result<String> {
    if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled")); }
    let mut cmd = Command::new(program);
    cmd.current_dir(directory).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if !flag.is_empty() { cmd.arg(flag); }
    cmd.arg(argument);
    #[cfg(windows)] { use std::os::windows::process::CommandExt; cmd.creation_flags(0x0800_0000); }
    let mut child = cmd.spawn()?;
    let stdout = collect(child.stdout.take().ok_or_else(|| invalid("Could not capture stdout"))?);
    let stderr = collect(child.stderr.take().ok_or_else(|| invalid("Could not capture stderr"))?);
    let started = Instant::now();
    let (status, reason) = loop {
        if let Some(status) = child.try_wait()? { break (status, None); }
        if cancel.load(Ordering::Relaxed) || started.elapsed() >= TIME_LIMIT {
            let reason = if cancel.load(Ordering::Relaxed) { "Cancelled" } else { "Timed out after 30 seconds" };
            child.kill()?; break (child.wait()?, Some(reason));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let read = |rx: mpsc::Receiver<(Vec<u8>, bool)>| match rx.recv_timeout(Duration::from_millis(500)) {
        Ok((bytes, truncated)) => format!("{}{}", String::from_utf8_lossy(&bytes), if truncated { "\n[output truncated]" } else { "" }),
        Err(_) => "[output still open by a descendant or unavailable]".to_string(),
    };
    Ok(format!("{}\nExit status: {}\n\nStandard output:\n{}\n\nStandard error:\n{}\n\nThis process was not sandboxed. Descendant processes may still be running.", reason.unwrap_or("Process finished"), status, read(stdout), read(stderr)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test] fn windows_shell_captures_output() {
        let root = crate::test_support::scratch_dir("snippet-win");
        let report = snippet(OsStr::new("cmd.exe"), OsStr::new("/C"), "echo zfm-snippet-test", &root, &AtomicBool::new(false)).unwrap();
        assert!(report.contains("zfm-snippet-test"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test] fn runs_captures_and_cancels() {
        let root = crate::test_support::scratch_dir("snippet");
        let report = snippet(OsStr::new("/bin/sh"), OsStr::new("-c"), "printf ok; printf err >&2", &root, &AtomicBool::new(false)).unwrap();
        assert!(report.contains("ok") && report.contains("err"));
        assert_eq!(snippet(OsStr::new("/bin/sh"), OsStr::new("-c"), "true", &root, &AtomicBool::new(true)).unwrap_err().kind(), io::ErrorKind::Interrupted);
        std::fs::remove_dir_all(root).unwrap();
    }
}
