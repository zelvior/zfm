//! Explicit, limited Android Debug Bridge commands using an installed adb.
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub enum Job { Devices(PathBuf), Connect(PathBuf, SocketAddr), Push(PathBuf, String, PathBuf, String) }
impl Job {
    pub fn title(&self) -> &'static str { match self { Self::Devices(_) => "Android devices", Self::Connect(..) => "Android wireless connection", Self::Push(..) => "Android file transfer" } }
    pub fn run(self, cancel: &AtomicBool) -> io::Result<String> {
        let program = std::env::var_os("ZFM_ADB").unwrap_or_else(|| "adb".into());
        self.run_with(&program, cancel)
    }
    fn run_with(self, program: &std::ffi::OsStr, cancel: &AtomicBool) -> io::Result<String> {
        let mut cmd = Command::new(program);
        let mut timeout = Duration::from_secs(30);
        let heading = match self {
            Self::Devices(root) => { cmd.current_dir(root).args(["devices", "-l"]); "ADB devices (USB and previously connected wireless devices)" }
            Self::Connect(root, address) => { validate_address(address)?; cmd.current_dir(root).arg("connect").arg(address.to_string()); "ADB wireless connection" }
            Self::Push(root, serial, path, remote) => {
                validate_serial(&serial)?; validate_remote(&remote)?;
                let checked = crate::settings::open_existing(&path)?;
                if checked.metadata()?.len() > 1024 * 1024 * 1024 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "File exceeds the 1 GiB transfer limit")); }
                // adb reopens the source; parent and source replacement cannot be prevented.
                cmd.current_dir(root).arg("-s").arg(serial).arg("push").arg(path).arg(remote);
                timeout = Duration::from_secs(300);
                "ADB push: inspect the result; remote files may have been overwritten or left partial"
            }
        };
        let report = crate::tools::run_command_with_timeout(cmd, cancel, timeout)?;
        Ok(format!("{heading}\n\n{report}\n\nADB may start or reuse its own server; ZFM does not stop it. ADB is not bundled. Device authorization and wireless pairing are controlled by Android and the installed ADB. Legacy TCP debugging may be unencrypted; use only trusted networks."))
    }
}
pub fn validate_serial(serial: &str) -> io::Result<()> {
    if serial.is_empty() || serial.len() > 256 || serial.starts_with('-') || !serial.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-[]".contains(&b)) {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "Use a device serial from the ADB devices report"))
    } else { Ok(()) }
}
/// Restrict Android paths to simple absolute ASCII paths, with no dot segments
/// or shell metacharacters. This is deliberately not an arbitrary shell command.
pub fn validate_remote(path: &str) -> io::Result<()> {
    if path.len() > 1024 || !path.starts_with('/') || path.ends_with('/') || path.split('/').skip(1).any(|part| part.is_empty() || part == "." || part == "..") || !path.bytes().all(|b| b.is_ascii_alphanumeric() || b"/_-.".contains(&b)) {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "Use an absolute Android filename with only letters, digits, /, _, - and ."))
    } else { Ok(()) }
}
fn validate_address(address: SocketAddr) -> io::Result<()> {
    let invalid_ip = match address.ip() {
        IpAddr::V4(ip) => ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast(),
        IpAddr::V6(ip) => ip.is_unspecified() || ip.is_multicast(),
    };
    if invalid_ip || address.port() == 0 { Err(io::Error::new(io::ErrorKind::InvalidInput, "Enter a unicast IP address and nonzero port")) } else { Ok(()) }
}
pub fn parse_address(value: &str) -> io::Result<SocketAddr> {
    let address = value.trim().parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Use IP:port, or [IPv6]:port; hostnames are not supported"))?;
    validate_address(address)?; Ok(address)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test] fn invokes_mock_adb_with_literal_push_arguments() {
        use std::os::unix::fs::PermissionsExt;
        let root = crate::test_support::scratch_dir("mock-adb"); let adb = root.join("adb"); let source = root.join("some file.txt");
        std::fs::write(&adb, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&source, b"hello").unwrap();
        let output = Job::Push(root.clone(), "emulator-5554".into(), source.clone(), "/sdcard/file.txt".into()).run_with(adb.as_os_str(), &AtomicBool::new(false)).unwrap();
        assert!(output.contains(&format!("-s\nemulator-5554\npush\n{}\n/sdcard/file.txt", source.display())));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test] fn accepts_only_literal_unicast_addresses() {
        assert!(parse_address("192.168.1.2:5555").is_ok()); assert!(parse_address("[::1]:5555").is_ok());
        assert!(validate_serial("emulator-5554").is_ok()); assert!(validate_serial("-x").is_err());
        assert!(validate_remote("/sdcard/Download/file.txt").is_ok());
        for value in ["/sdcard/../secret", "/sdcard/", "-x", "/sdcard/*", "/sdcard/a b"] { assert!(validate_remote(value).is_err()); }
        for value in ["example.com:5555", "0.0.0.0:5555", "224.0.0.1:5555", "1.2.3.4:0", "-x"] { assert!(parse_address(value).is_err()); }
    }
}
