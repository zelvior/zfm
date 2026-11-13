//! Optional bundled Office-layout renderer bridge. The pinned runtime is
//! executed out of process, not discovered from PATH. It is not a sandbox.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const MAX_INPUT: u64 = 64 * 1024 * 1024;
const MAX_PDF: u64 = 64 * 1024 * 1024;

/// Locate an explicitly installed ZFM runtime, never the system Node binary.
pub fn runtime_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = executable.parent().ok_or("Could not locate ZFM installation")?;
    #[cfg(windows)]
    let paths = [parent.join("office-runtime")];
    #[cfg(target_os = "linux")]
    let paths = [parent.join("office-runtime"), PathBuf::from("/usr/lib/zfm/office-runtime")];
    #[cfg(windows)]
    let node = "node.exe";
    #[cfg(target_os = "linux")]
    let node = "node";
    paths.into_iter().find(|p| p.join(node).is_file() && p.join("office-renderer.mjs").is_file()
        && p.join("node_modules/@deepseek-ai/libreoffice-kit-wasm/package.json").is_file())
        .ok_or_else(|| "Bundled Office layout renderer is unavailable.".to_owned())
}

/// Convert a bounded document to PDF using the packaged LibreOffice WASM
/// runtime. Cancellation terminates the process; a forced exit may leave
/// private temporary files. The input and runtime must be trusted.
pub fn render(path: &Path, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp"].contains(&ext.as_str()) {
        return Err("Unsupported Office layout format.".into());
    }
    let runtime = runtime_path()?;
    let bytes = crate::viewer::read_bounded(path, MAX_INPUT).map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) { return Err("Document rendering cancelled.".into()); }
    #[cfg(windows)]
    let node = "node.exe";
    #[cfg(target_os = "linux")]
    let node = "node";
    let mut command = Command::new(runtime.join(node));
    command.arg(runtime.join("office-renderer.mjs")).arg(&ext).current_dir(&runtime)
        .env_remove("NODE_OPTIONS").env_remove("NODE_PATH").env_remove("NODE_EXTRA_CA_CERTS")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x0800_0000); }
    let mut child = command.spawn().map_err(|e| format!("Could not start Office renderer: {e}"))?;
    let stdin = match child.stdin.take() { Some(s) => s, None => { let _ = child.kill(); let _ = child.wait(); return Err("Office renderer input unavailable.".into()); } };
    let stdout = match child.stdout.take() { Some(s) => s, None => { let _ = child.kill(); let _ = child.wait(); return Err("Office renderer output unavailable.".into()); } };
    let (write_tx, write_rx) = mpsc::sync_channel(1);
    thread::spawn(move || { let mut stdin = stdin; let _ = write_tx.send(stdin.write_all(&bytes)); });
    let (read_tx, read_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout.take(MAX_PDF + 1).read_to_end(&mut output).map(|_| output);
        let _ = read_tx.send(result);
    });
    let start = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(100) {
            let _ = child.kill(); let _ = child.wait();
            let _ = write_rx.recv_timeout(Duration::from_secs(2));
            let _ = read_rx.recv_timeout(Duration::from_secs(2));
            return Err("Document rendering cancelled or timed out.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(e) => { let _ = child.kill(); let _ = child.wait(); return Err(e.to_string()); }
        }
    };
    write_rx.recv_timeout(Duration::from_secs(2)).map_err(|_| "Office input worker did not finish.")?
        .map_err(|e| format!("Office input failed: {e}"))?;
    let output = read_rx.recv_timeout(Duration::from_secs(2)).map_err(|_| "Office output worker did not finish.")?
        .map_err(|e| format!("Office output failed: {e}"))?;
    if !status.success() { return Err(format!("Office renderer failed ({status}).")); }
    if output.len() as u64 > MAX_PDF || !output.starts_with(b"%PDF-") { return Err("Office renderer produced invalid or oversized PDF output.".into()); }
    Ok(output)
}

/// Select a packaged layout renderer if installed. The old XML extractor is a
/// limited fallback for formats it understands, and labels its output as text.
pub enum Preview { LayoutPdf(Vec<u8>), Text(String) }
pub fn preview(path: &Path, cancel: &AtomicBool) -> Result<Preview, String> {
    if runtime_path().is_ok() {
        return render(path, cancel).map(Preview::LayoutPdf);
    }
    match path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "docx" | "pptx" | "xlsx" | "odt" => crate::office::preview(path, cancel).map(Preview::Text),
        _ => Err("The bundled Office layout renderer is unavailable for this document.".into()),
    }
}
