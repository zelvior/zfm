//! Optional Linux archive mounts via user-installed FUSE tools. No embedded driver.
use std::fs;
use std::io::{self, Read};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const MOUNT_TYPE: &str = "fuse.zfm-archivemount";
const MAX_MOUNTINFO: u64 = 8 * 1024 * 1024;
#[link(name = "c")]
extern "C" { fn geteuid() -> u32; }
fn invalid(text: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, text) }

#[derive(Clone, Debug)]
struct Mount { point: PathBuf, kind: String }
fn unescape(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut result = Vec::with_capacity(bytes.len()); let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if i + 3 >= bytes.len() || !bytes[i+1..i+4].iter().all(|b| (b'0'..=b'7').contains(b)) { return Err(invalid("Invalid mountinfo escape")); }
            let value = (bytes[i+1] - b'0') as u16 * 64 + (bytes[i+2] - b'0') as u16 * 8 + (bytes[i+3] - b'0') as u16;
            if value > 255 || value == 0 { return Err(invalid("Invalid mountinfo escape")); }
            result.push(value as u8); i += 4;
        } else { result.push(bytes[i]); i += 1; }
    }
    Ok(result)
}
fn parse_mountinfo(data: &[u8]) -> io::Result<Vec<Mount>> {
    let mut result = Vec::new();
    for line in data.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
        let sep = line.windows(3).position(|v| v == b" - ").ok_or_else(|| invalid("Invalid mountinfo record"))?;
        let fields: Vec<_> = line[..sep].split(|b| *b == b' ').collect();
        let kind = line[sep + 3..].split(|b| *b == b' ').next().unwrap_or(&[]);
        if fields.len() < 6 || kind.is_empty() { return Err(invalid("Invalid mountinfo record")); }
        let point = std::ffi::OsString::from_vec(unescape(fields[4])?);
        result.push(Mount { point: PathBuf::from(point), kind: String::from_utf8_lossy(kind).into_owned() });
    }
    Ok(result)
}
fn mounts() -> io::Result<Vec<Mount>> {
    let mut data = Vec::new(); fs::File::open("/proc/self/mountinfo")?.take(MAX_MOUNTINFO + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_MOUNTINFO { return Err(invalid("System mount table exceeds limit")); }
    parse_mountinfo(&data)
}
fn real_directory(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() { return Err(invalid("Mountpoint must be an absolute directory")); }
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() { return Err(invalid("Mountpoint must be a real directory, not a link")); }
    Ok(fs::canonicalize(path)?)
}
fn validate_mountpoint(path: &Path, table: &[Mount]) -> io::Result<PathBuf> {
    let point = real_directory(path)?;
    if table.iter().any(|mount| mount.point == point) { return Err(invalid("Directory is already a mountpoint")); }
    let metadata = fs::metadata(&point)?;
    if metadata.uid() != unsafe { geteuid() } { return Err(invalid("Mountpoint must be owned by the current user")); }
    if fs::read_dir(&point)?.next().transpose()?.is_some() { return Err(invalid("Mountpoint directory must be empty")); }
    Ok(point)
}
fn validate_unmount(path: &Path, table: &[Mount]) -> io::Result<PathBuf> {
    let point = real_directory(path)?;
    let matching: Vec<_> = table.iter().filter(|m| m.point == point).collect();
    if matching.len() != 1 || matching[0].kind != MOUNT_TYPE { return Err(invalid("Not a uniquely mounted ZFM archive FUSE mountpoint")); }
    Ok(point)
}

fn mount_command(program: &std::ffi::OsStr, archive: &Path, point: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir("/").args(["-o", "readonly,ro,nosuid,nodev,noexec,fsname=zfm-archivemount,subtype=zfm-archivemount"]).arg(archive).arg(point);
    cmd
}
fn unmount_command(program: &std::ffi::OsStr, point: &Path) -> Command {
    let mut cmd = Command::new(program); cmd.current_dir("/").arg("-u").arg(point); cmd
}

pub enum Job { Mount { archive: PathBuf, point: PathBuf }, Unmount(PathBuf) }
impl Job {
    pub fn title(&self) -> &'static str { "Linux archive mount" }
    pub fn run(self, cancel: &AtomicBool) -> io::Result<String> {
        match self {
            Self::Mount { archive, point } => {
                if !archive.is_absolute() { return Err(invalid("Archive must be an absolute path")); }
                let _checked = crate::settings::open_existing(&archive)?;
                if !crate::archive_actions::can_extract(&archive) { return Err(invalid("Choose a supported archive filename")); }
                let point = validate_mountpoint(&point, &mounts()?)?;
                let executable = std::env::var_os("ZFM_ARCHIVEMOUNT").unwrap_or_else(|| "archivemount".into());
                let command = mount_command(&executable, &archive, &point);
                let output = crate::tools::run_command_with_timeout(command, cancel, Duration::from_secs(60))?;
                let state = match mounts() {
                    Ok(table) if table.iter().any(|m| m.point == point && m.kind == MOUNT_TYPE) => "The expected FUSE mount appears in this process's mount table.",
                    Ok(_) => "The expected mount was not found in this process's mount table; inspect the command result.",
                    Err(_) => "Could not re-read the mount table; inspect the system before retrying.",
                };
                Ok(format!("Archive: {:?}\nMountpoint: {:?}\n{state}\n\n{output}\n\nExternal FUSE processes and mounts may outlive ZFM, a timeout or cancellation. Dismount the folder explicitly when finished.", archive, point))
            }
            Self::Unmount(point) => {
                let point = validate_unmount(&point, &mounts()?)?;
                let executable = std::env::var_os("ZFM_FUSERMOUNT").unwrap_or_else(|| "fusermount".into());
                let command = unmount_command(&executable, &point);
                let output = crate::tools::run_command_with_timeout(command, cancel, Duration::from_secs(60))?;
                let state = match mounts() {
                    Ok(table) if !table.iter().any(|m| m.point == point) => "The mountpoint is no longer mounted in this process's mount table.",
                    Ok(_) => "A mount is still present at that path; inspect the command result.",
                    Err(_) => "Could not re-read the mount table; inspect the system before retrying.",
                };
                Ok(format!("Dismount: {:?}\n{state}\n\n{output}\n\nThe external process may finish after the command returns.", point))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    #[test] fn parses_mounts_and_rejects_other_filesystems() {
        let mounts = parse_mountinfo(b"26 22 0:37 / /tmp/a\\040b rw,nosuid - fuse.zfm-archivemount zfm-archivemount rw\n27 22 0:38 / /tmp/other rw - ext4 /dev/sda rw\n").unwrap();
        assert_eq!(mounts[0].point.as_os_str().as_bytes(), b"/tmp/a b");
        assert_eq!(mounts[0].kind, MOUNT_TYPE); assert_eq!(mounts[1].kind, "ext4");
        assert!(parse_mountinfo(b"bad\n").is_err()); assert!(unescape(b"foo\\400").is_err());
    }
    #[test] fn commands_do_not_interpolate_paths() {
        let archive = Path::new("/tmp/a ; touch sentinel.zip"); let point = Path::new("/tmp/mount space");
        let cmd = mount_command(std::ffi::OsStr::new("archivemount"), archive, point);
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args[0], "-o"); assert!(args[1].to_string_lossy().contains("readonly,ro,nosuid,nodev,noexec"));
        assert_eq!(args[2], archive.as_os_str()); assert_eq!(args[3], point.as_os_str());
        let unmount = unmount_command(std::ffi::OsStr::new("fusermount"), point);
        assert_eq!(unmount.get_args().collect::<Vec<_>>(), vec![std::ffi::OsStr::new("-u"), point.as_os_str()]);
    }
    #[test] fn requires_empty_owned_directory_and_no_mount() {
        let root = crate::test_support::scratch_dir("mount-dir");
        let table = Vec::new(); assert!(validate_mountpoint(&root, &table).is_ok());
        let owned = vec![Mount { point: fs::canonicalize(&root).unwrap(), kind: MOUNT_TYPE.into() }];
        assert!(validate_mountpoint(&root, &owned).is_err()); assert!(validate_unmount(&root, &owned).is_ok());
        let mut stacked = owned.clone(); stacked.push(owned[0].clone()); assert!(validate_unmount(&root, &stacked).is_err());
        let foreign = vec![Mount { point: owned[0].point.clone(), kind: "fuse.other".into() }]; assert!(validate_unmount(&root, &foreign).is_err());
        fs::write(root.join("file"), b"x").unwrap(); assert!(validate_mountpoint(&root, &table).is_err());
        assert!(validate_unmount(&root, &table).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
