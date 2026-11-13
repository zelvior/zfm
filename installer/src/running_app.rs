//! Identify and politely close only ZFM windows from the installation being upgraded.
//! Windows 7 supports QueryFullProcessImageNameW and PROCESS_QUERY_LIMITED_INFORMATION.
use crate::win32::{self, Handle, Hwnd, LParam};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(
        callback: Option<unsafe extern "system" fn(Hwnd, LParam) -> i32>,
        data: LParam,
    ) -> i32;
    fn GetClassNameW(hwnd: Hwnd, class: *mut u16, max: i32) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
}
#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> Handle;
    fn QueryFullProcessImageNameW(
        process: Handle,
        flags: u32,
        path: *mut u16,
        length: *mut u32,
    ) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn CompareStringOrdinal(
        left: *const u16,
        left_len: i32,
        right: *const u16,
        right_len: i32,
        ignore_case: i32,
    ) -> i32;
}

struct Process {
    id: u32,
    handle: Handle,
}
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            win32::CloseHandle(self.handle);
        }
    }
}
struct Windows {
    target: PathBuf,
    windows: Vec<(Hwnd, u32)>,
    processes: Vec<Process>,
}

fn same_file(left: &Path, right: &Path) -> bool {
    let (left, right) = match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(l), Ok(r)) => (l, r),
        _ => (left.to_path_buf(), right.to_path_buf()),
    };
    let left = win32::wide(left.as_os_str());
    let right = win32::wide(right.as_os_str());
    unsafe { CompareStringOrdinal(left.as_ptr(), -1, right.as_ptr(), -1, 1) == 2 }
}

unsafe extern "system" fn inspect_window(hwnd: Hwnd, data: LParam) -> i32 {
    let found = &mut *(data as *mut Windows);
    let mut class = [0u16; 64];
    let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
    if n <= 0 || String::from_utf16_lossy(&class[..n as usize]) != "ZFMNativeWindow" {
        return 1;
    }
    let mut id = 0;
    GetWindowThreadProcessId(hwnd, &mut id);
    if id == 0 {
        return 1;
    }
    // Keep a process handle open through posting and waiting, preventing PID reuse.
    let process = OpenProcess(0x0010_0000 | 0x1000, 0, id);
    if process.is_null() {
        return 1;
    }
    let mut buffer = vec![0u16; 32768];
    let mut len = buffer.len() as u32;
    if QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len) == 0 {
        win32::CloseHandle(process);
        return 1;
    }
    let path = PathBuf::from(OsString::from_wide(&buffer[..len as usize]));
    if !same_file(&path, &found.target) {
        win32::CloseHandle(process);
        return 1;
    }
    found.windows.push((hwnd, id));
    if found.processes.iter().any(|p| p.id == id) {
        win32::CloseHandle(process);
    } else {
        found.processes.push(Process {
            id,
            handle: process,
        });
    }
    1
}

fn find(target: &Path) -> Windows {
    let mut windows = Windows {
        target: target.to_path_buf(),
        windows: Vec::new(),
        processes: Vec::new(),
    };
    unsafe {
        EnumWindows(Some(inspect_window), &mut windows as *mut _ as isize);
    }
    windows
}

pub fn is_running(target: &Path) -> bool {
    !find(target).windows.is_empty()
}

/// Sends WM_CLOSE, never TerminateProcess; a process can refuse to close.
/// Wait on the opened process handles (not PIDs), bounded by one timeout.
pub fn close_and_wait(target: &Path) -> Result<(), String> {
    let found = find(target);
    for (hwnd, id) in &found.windows {
        let mut actual = 0;
        unsafe {
            GetWindowThreadProcessId(*hwnd, &mut actual);
            if actual == *id {
                win32::PostMessageW(*hwnd, 0x0010, 0, 0);
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    for process in &found.processes {
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u32::MAX as u128) as u32;
        let result = unsafe { WaitForSingleObject(process.handle, remaining) };
        if result != 0 {
            return Err("ZFM did not close in time. Close it yourself, then retry. No process was force-closed.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compares_windows_paths_case_insensitively() {
        assert!(same_file(
            Path::new(r"C:\Users\Name\ZFM.exe"),
            Path::new(r"c:\users\name\zfm.EXE")
        ));
        assert!(!same_file(
            Path::new(r"C:\one\zfm.exe"),
            Path::new(r"C:\two\zfm.exe")
        ));
        assert!(!is_running(Path::new(r"C:\no-such-folder\zfm.exe")));
    }
}
