//! Explicit, loopback-only static sharing. No executable content or directory listing.
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;

const HEADER_LIMIT: usize = 8192;
fn bad(text: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, text) }

#[cfg(unix)]
fn random(bytes: &mut [u8]) -> io::Result<()> { File::open("/dev/urandom")?.read_exact(bytes) }
#[cfg(windows)]
fn random(bytes: &mut [u8]) -> io::Result<()> {
    #[link(name = "advapi32")]
    extern "system" { #[link_name = "SystemFunction036"] fn rtl_gen_random(data: *mut u8, len: u32) -> u8; }
    if unsafe { rtl_gen_random(bytes.as_mut_ptr(), bytes.len() as u32) } == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}
#[cfg(not(any(unix, windows)))]
fn random(_: &mut [u8]) -> io::Result<()> { Err(io::Error::new(io::ErrorKind::Unsupported, "No secure random source")) }

pub struct Server { url: String, stop: Arc<AtomicBool> }
impl Server {
    /// Bind to 127.0.0.1 only, using an OS-selected port and a random URL token.
    pub fn start(root: &Path) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() { return Err(bad("Choose a directory to serve")); }
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let mut token = [0u8; 16]; random(&mut token)?;
        let token: String = token.iter().map(|b| format!("{b:02x}")).collect();
        let url = format!("http://127.0.0.1:{port}/{token}/");
        let stop = Arc::new(AtomicBool::new(false));
        let quit = stop.clone();
        std::thread::Builder::new().name("zfm-local-server".into()).spawn(move || {
            while !quit.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => { let _ = handle(&mut stream, &root, &token, port, &quit); }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(30)),
                    Err(_) => break,
                }
            }
        })?;
        Ok(Self { url, stop })
    }
    pub fn url(&self) -> &str { &self.url }
    pub fn stop(&self) { self.stop.store(true, Ordering::Relaxed); }
}
impl Drop for Server { fn drop(&mut self) { self.stop(); } }
fn decode_path(path: &str) -> io::Result<String> {
    let bytes = path.as_bytes(); let mut out = Vec::new(); let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() { return Err(bad("Bad escape")); }
            let digit = |c: u8| match c { b'0'..=b'9' => Some(c-b'0'), b'a'..=b'f' => Some(c-b'a'+10), b'A'..=b'F' => Some(c-b'A'+10), _ => None };
            let high = digit(bytes[i+1]).ok_or_else(|| bad("Bad escape"))?; let low = digit(bytes[i+2]).ok_or_else(|| bad("Bad escape"))?;
            out.push(high*16+low); i += 3;
        } else { out.push(bytes[i]); i += 1; }
    }
    String::from_utf8(out).map_err(|_| bad("Invalid UTF-8"))
}
fn resolve(root: &Path, url_path: &str) -> io::Result<PathBuf> {
    let path = decode_path(url_path)?;
    if path.contains('\\') || path.contains('\0') { return Err(bad("Invalid path")); }
    let mut resolved = root.to_path_buf();
    for part in path.split('/') {
        if part.is_empty() { continue; }
        let candidate = Path::new(part);
        if part == "." || part == ".." || candidate.components().count() != 1 || !matches!(candidate.components().next(), Some(Component::Normal(_))) { return Err(bad("Invalid path component")); }
        resolved.push(candidate);
        let meta = fs::symlink_metadata(&resolved)?;
        #[cfg(windows)] { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { return Err(bad("Reparse points are not served")); } }
        if meta.file_type().is_symlink() { return Err(bad("Links are not served")); }
    }
    if resolved.is_dir() { resolved.push("index.html"); }
    Ok(resolved)
}
fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8", "css" => "text/css; charset=utf-8", "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json", "txt" => "text/plain; charset=utf-8", "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif", "svg" => "image/svg+xml", "ico" => "image/x-icon", "pdf" => "application/pdf", "wasm" => "application/wasm", _ => "application/octet-stream",
    }
}
fn error(stream: &mut TcpStream, code: &str) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {code}\r\nContent-Length: 0\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n")
}
fn handle(stream: &mut TcpStream, root: &Path, token: &str, port: u16, stop: &AtomicBool) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut header = Vec::new(); let mut chunk = [0u8; 1024];
    loop {
        if stop.load(Ordering::Relaxed) { return Ok(()); }
        let n = stream.read(&mut chunk)?; if n == 0 { return Ok(()); }
        header.extend_from_slice(&chunk[..n]);
        if header.len() > HEADER_LIMIT { return error(stream, "431 Request Header Fields Too Large"); }
        if header.windows(4).any(|w| w == b"\r\n\r\n") { break; }
    }
    let text = match std::str::from_utf8(&header) { Ok(v) => v, Err(_) => return error(stream, "400 Bad Request") };
    let mut lines = text.split("\r\n");
    let mut request = lines.next().unwrap_or("").split_whitespace();
    let method = request.next().unwrap_or(""); let target = request.next().unwrap_or(""); let version = request.next().unwrap_or("");
    if request.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") { return error(stream, "400 Bad Request"); }
    if method != "GET" && method != "HEAD" { return error(stream, "405 Method Not Allowed"); }
    let hosts: Vec<_> = lines.filter_map(|line| line.split_once(':')).filter(|(key, _)| key.eq_ignore_ascii_case("host")).map(|(_, v)| v.trim()).collect();
    if hosts.len() != 1 || !(hosts[0].eq_ignore_ascii_case(&format!("127.0.0.1:{port}")) || hosts[0].eq_ignore_ascii_case(&format!("localhost:{port}"))) { return error(stream, "403 Forbidden"); }
    let target = target.split('?').next().unwrap_or("");
    let prefix = format!("/{token}/");
    let relative = match target.strip_prefix(&prefix) { Some(value) => value, None => return error(stream, "404 Not Found") };
    let path = match resolve(root, relative) { Ok(path) => path, Err(_) => return error(stream, "404 Not Found") };
    let mut file = match crate::settings::open_existing(&path) { Ok(f) => f, Err(_) => return error(stream, "404 Not Found") };
    let size = file.metadata()?.len();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nContent-Type: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\r\n", content_type(&path))?;
    if method == "HEAD" { return Ok(()); }
    let mut remaining = size; let mut buf = [0u8; 32 * 1024];
    while remaining > 0 && !stop.load(Ordering::Relaxed) {
        let limit = (remaining.min(buf.len() as u64)) as usize; let count = file.read(&mut buf[..limit])?;
        if count == 0 { break; } stream.write_all(&buf[..count])?; remaining -= count as u64;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(port: u16, target: &str, host: &str, method: &str) -> String {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(c, "{method} {target} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        let mut text = String::new(); c.read_to_string(&mut text).unwrap(); text
    }
    #[test] fn loopback_token_and_traversal() {
        let root = crate::test_support::scratch_dir("http"); fs::write(root.join("index.html"), "hello").unwrap();
        let server = Server::start(&root).unwrap(); let url = server.url();
        let (host, target) = url.strip_prefix("http://").unwrap().split_once('/').unwrap(); let port: u16 = host.split(':').nth(1).unwrap().parse().unwrap(); let target = format!("/{target}");
        assert!(request(port, &target, host, "GET").ends_with("hello"));
        assert!(!request(port, &target, host, "HEAD").ends_with("hello"));
        assert!(request(port, "/bad/", host, "GET").contains("404 Not Found"));
        assert!(request(port, &format!("{target}%2e%2e/secret"), host, "GET").contains("404 Not Found"));
        assert!(request(port, &target, "evil.example", "GET").contains("403 Forbidden"));
        assert!(request(port, &target, host, "POST").contains("405 Method Not Allowed"));
        #[cfg(unix)] { std::os::unix::fs::symlink("/etc/passwd", root.join("link")).unwrap(); assert!(request(port, &format!("{target}link"), host, "GET").contains("404 Not Found")); }
        server.stop(); fs::remove_dir_all(root).unwrap();
    }
}
