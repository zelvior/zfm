//! Windows archive browser. Listing and testing use the bundled engine;
//! single-file extraction streams data to a staged output managed by ZFM.
use super::*;
use std::fs::{File, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use zfm::archive_engine::{engine_path, run_engine_archive_password};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use zfm::archive::{self, Entry, Kind};

#[link(name = "user32")]
extern "system" {
    fn SetTimer(hwnd: H, id: usize, milliseconds: u32, callback: P) -> usize;
    fn KillTimer(hwnd: H, id: usize) -> i32;
    fn EnableWindow(hwnd: H, enable: i32) -> i32;
}
#[link(name = "kernel32")]
extern "system" { fn GetFileType(handle: H) -> u32; }

fn inspect(path: &Path, cancel: &AtomicBool) -> Result<Vec<Entry>, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_attributes() & 0x400 != 0 || !meta.is_file() {
        return Err("Select a regular file, not a reparse point.".into());
    }
    let mut file: File = OpenOptions::new().read(true).custom_flags(0x0020_0000)
        .open(path).map_err(|e| e.to_string())?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    if before.file_attributes() & 0x400 != 0 || !before.is_file()
        || unsafe { GetFileType(file.as_raw_handle() as H) } != 1 {
        return Err("Select a regular disk file, not a reparse point.".into());
    }
    // Prevent seeking through arbitrarily large container files on the UI's
    // behalf. The core has separate limits for entries and expanded data.
    if before.len() > 2 * 1024 * 1024 * 1024 {
        return Err("Archive exceeds the 2 GiB inspection limit.".into());
    }
    let entries = archive::inspect(&mut file, cancel).map_err(|e| e.to_string())?;
    let after = file.metadata().map_err(|e| e.to_string())?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("Archive changed during inspection; retry.".into());
    }
    Ok(entries)
}

fn open_listing(path: &Path, cancel: &AtomicBool, password: Option<&str>) -> Result<Vec<String>, String> {
    if engine_path().is_ok() { return run_engine_archive_password(path, false, cancel, password); }
    if path.extension().and_then(|v| v.to_str()).map_or(false, |v| v.eq_ignore_ascii_case("tar")) {
        return inspect(path, cancel).map(|entries| entries.into_iter().map(|entry| {
            let prefix = if entry.kind == Kind::Directory { "[Folder]" } else { "[File]" };
            format!("{}  {}  ({} bytes)", prefix, entry.path, entry.size)
        }).collect());
    }
    Err("Bundled archive engine is missing; reinstall ZFM. Only TAR headers are available without it.".into())
}

struct Dialog {
    theme: *const Theme,
    title: Vec<u16>,
    list: H,
    status: H,
    close: H,
    test: H,
    extract: H,
    extract_all: H,
    password_button: H,
    password: Option<String>,
    path: PathBuf,
    receiver: Option<Receiver<Result<Vec<String>, String>>>,
    testing: bool,
    extracting: bool,
    cancel: Arc<AtomicBool>,
}
impl Drop for Dialog {
    fn drop(&mut self) { self.cancel.store(true, Ordering::Relaxed); }
}
unsafe fn layout(hwnd: H, state: &Dialog) {
    let mut r: Rect = std::mem::zeroed();
    GetClientRect(hwnd, &mut r);
    let w = r.right.max(200);
    let h = r.bottom.max(130);
    if !state.list.is_null() { MoveWindow(state.list, 10, 10, w - 20, (h - 108).max(1), 1); }
    if !state.status.is_null() { MoveWindow(state.status, 10, h - 90, (w - 155).max(1), 48, 1); }
    if !state.password_button.is_null() { MoveWindow(state.password_button, w - 140, h - 82, 130, 28, 1); }
    if !state.test.is_null() { MoveWindow(state.test, 10, h - 38, 110, 28, 1); }
    if !state.extract.is_null() { MoveWindow(state.extract, 124, h - 38, 130, 28, 1); }
    if !state.extract_all.is_null() { MoveWindow(state.extract_all, 258, h - 38, 100, 28, 1); }
    if !state.close.is_null() { MoveWindow(state.close, w - 95, h - 38, 85, 28, 1); }
}
unsafe extern "system" fn dialog_proc(hwnd: H, message: u32, wp: WParam, lp: LParam) -> isize {
    const WM_INITDIALOG: u32 = 0x0110;
    const WM_TIMER: u32 = 0x0113;
    const WM_CLOSE: u32 = 0x0010;
    if message == WM_INITDIALOG {
        let state = &mut *(lp as *mut Dialog);
        set_data(hwnd, lp);
        SetWindowTextW(hwnd, state.title.as_ptr());
        apply_dark_window_chrome(hwnd);
        let instance = GetModuleHandleW(ptr::null());
        let make = |class: &str, caption: &str, style: u32, id: usize| {
            CreateWindowExW(0, text(class).as_ptr(), text(caption).as_ptr(), WS_CHILD | WS_VISIBLE | style,
                0, 0, 1, 1, hwnd, id as H, instance, ptr::null_mut())
        };
        state.list = make("LISTBOX", "", WS_TABSTOP | WS_VSCROLL | 0x0010_0000 | LBS_HASSTRINGS, 1001);
        state.status = make("STATIC", "Reading archive…", 0, 1002);
        state.test = make("BUTTON", "Test archive", WS_TABSTOP | BS_OWNERDRAW, 1003);
        state.extract = make("BUTTON", "Extract one file…", WS_TABSTOP | BS_OWNERDRAW, 1004);
        state.extract_all = make("BUTTON", "Extract all…", WS_TABSTOP | BS_OWNERDRAW, 1005);
        state.password_button = make("BUTTON", "Password…", WS_TABSTOP | BS_OWNERDRAW, 1006);
        state.close = make("BUTTON", "Close", WS_TABSTOP | BS_OWNERDRAW, 2);
        for control in [state.list, state.status, state.test, state.extract, state.extract_all, state.password_button, state.close] {
            if control.is_null() { EndDialog(hwnd, -1); return 1; }
            SendMessageW(control, WM_SETFONT, (*state.theme).font as usize, 1);
        }
        apply_dark_scrollbar(state.list);
        SendMessageW(hwnd, 0x0401, 2, 0); // DM_SETDEFID: Close
        layout(hwnd, state);
        EnableWindow(state.test, 0);
        EnableWindow(state.extract, 0);
        EnableWindow(state.extract_all, 0);
        EnableWindow(state.password_button, 0);
        if SetTimer(hwnd, 1, 100, ptr::null_mut()) == 0 {
            SetWindowTextW(state.status, text("Could not start archive inspection timer.").as_ptr());
            state.cancel.store(true, Ordering::Relaxed);
        }
        return 1;
    }
    let state = get_data(hwnd) as *mut Dialog;
    if state.is_null() { return 0; }
    let state = &mut *state;
    match message {
        WM_GETMINMAXINFO if lp != 0 => {
            let limits = &mut *(lp as *mut MinMaxInfo);
            limits.min_track_size.x = 520;
            limits.min_track_size.y = 320;
            1
        }
        WM_SIZE => { layout(hwnd, state); 1 }
        WM_TIMER if wp == 1 => {
            let result = match state.receiver.as_ref() { Some(receiver) => receiver.try_recv(), None => return 1 };
            match result {
                Ok(Ok(lines)) => {
                    KillTimer(hwnd, 1);
                    state.receiver = None;
                    EnableWindow(state.test, if engine_path().is_ok() { 1 } else { 0 });
                    EnableWindow(state.extract, if engine_path().is_ok() { 1 } else { 0 });
                    EnableWindow(state.extract_all, if engine_path().is_ok() { 1 } else { 0 });
                    EnableWindow(state.password_button, if engine_path().is_ok() { 1 } else { 0 });
                    if state.extracting {
                        let message = lines.first().map(String::as_str).unwrap_or("Extracted one file without overwriting an existing file.");
                        SetWindowTextW(state.status, text(message).as_ptr());
                    } else if !state.testing {
                        for entry in lines {
                            let line = text(&entry);
                            SendMessageW(state.list, LB_ADDSTRING, 0, line.as_ptr() as isize);
                        }
                        SetWindowTextW(state.status, text("Listing only; contents are not verified. Extract one file using its exact Path value.").as_ptr());
                    } else {
                        SetWindowTextW(state.status, text("Archive test passed. Checksums cannot establish authenticity.").as_ptr());
                    }
                }
                Ok(Err(error)) => {
                    KillTimer(hwnd, 1);
                    state.receiver = None;
                    EnableWindow(state.test, if engine_path().is_ok() { 1 } else { 0 });
                    EnableWindow(state.extract, if engine_path().is_ok() { 1 } else { 0 });
                    EnableWindow(state.extract_all, if engine_path().is_ok() { 1 } else { 0 });
                    EnableWindow(state.password_button, if engine_path().is_ok() { 1 } else { 0 });
                    SetWindowTextW(state.status, text(&format!("Archive operation failed: {error}")).as_ptr());
                }
                Err(TryRecvError::Disconnected) => {
                    KillTimer(hwnd, 1);
                    state.receiver = None;
                    EnableWindow(state.test, if engine_path().is_ok() { 1 } else { 0 });
                    EnableWindow(state.extract, if engine_path().is_ok() { 1 } else { 0 });
                    EnableWindow(state.extract_all, if engine_path().is_ok() { 1 } else { 0 });
                    EnableWindow(state.password_button, if engine_path().is_ok() { 1 } else { 0 });
                    SetWindowTextW(state.status, text("Archive operation did not finish.").as_ptr());
                }
                Err(TryRecvError::Empty) => {}
            }
            1
        }
        WM_COMMAND if wp & 0xffff == 1003 && state.receiver.is_none() => {
            let (sender, receiver) = mpsc::sync_channel(1);
            let path = state.path.clone();
            let cancel = state.cancel.clone();
            state.testing = true;
            state.extracting = false;
            state.receiver = Some(receiver);
            EnableWindow(state.test, 0);
            EnableWindow(state.extract, 0);
            EnableWindow(state.extract_all, 0);
            EnableWindow(state.password_button, 0);
            SetWindowTextW(state.status, text("Testing archive…").as_ptr());
            let password = state.password.clone();
            thread::spawn(move || { let _ = sender.send(run_engine_archive_password(&path, true, &cancel, password.as_deref())); });
            if SetTimer(hwnd, 1, 100, ptr::null_mut()) == 0 {
                state.cancel.store(true, Ordering::Relaxed);
                SetWindowTextW(state.status, text("Could not start archive test timer.").as_ptr());
            }
            1
        }
        WM_COMMAND if wp & 0xffff == 1004 && state.receiver.is_none() => {
            let member = match windows_actions::prompt(hwnd, state.theme, "Exact archive entry path", OsStr::new(""), false) {
                Some(value) => match value.into_string() {
                    Ok(value) => value,
                    Err(_) => { show_error(hwnd, "Entry path must be Unicode."); return 1; }
                },
                None => return 1,
            };
            let proposed = member.rsplit(|c| c == '/' || c == '\\').next().unwrap_or("");
            let name = match windows_actions::prompt(hwnd, state.theme, "Extract file into archive folder as", OsStr::new(proposed), false) {
                Some(value) => value,
                None => return 1,
            };
            if let Err(error) = zfm::operations::validate_name(&name) { show_error(hwnd, &format!("Invalid output name: {error}")); return 1; }
            let path = state.path.clone();
            let output = path.parent().unwrap_or_else(|| Path::new(".")).join(name);
            let cancel = state.cancel.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            state.testing = false;
            state.extracting = true;
            state.receiver = Some(receiver);
            EnableWindow(state.test, 0);
            EnableWindow(state.extract, 0);
            EnableWindow(state.extract_all, 0);
            EnableWindow(state.password_button, 0);
            SetWindowTextW(state.status, text("Extracting one file…").as_ptr());
            let password = state.password.clone();
            thread::spawn(move || {
                let result = zfm::archive_engine::extract_single_file_password(&path, &member, &output, &cancel, password.as_deref())
                    .map(|_| Vec::new()).map_err(|error| error.to_string());
                let _ = sender.send(result);
            });
            if SetTimer(hwnd, 1, 100, ptr::null_mut()) == 0 {
                state.cancel.store(true, Ordering::Relaxed);
                SetWindowTextW(state.status, text("Could not start extraction timer.").as_ptr());
            }
            1
        }
        WM_COMMAND if wp & 0xffff == 1005 && state.receiver.is_none() => {
            let default_destination = state.path.parent().unwrap_or_else(|| Path::new("."));
            let destination = match windows_actions::prompt(hwnd, state.theme, "Extract all: existing destination folder", default_destination.as_os_str(), false) {
                Some(value) => PathBuf::from(value),
                None => return 1,
            };
            let question = format!("Extract all supported entries into {}?\n\nExisting files are not overwritten. Earlier files remain after an error or cancellation. Only use directories that untrusted users cannot modify.", destination.display());
            if !windows_actions::confirm(hwnd, state.theme, "Extract archive", &question) { return 1; }
            let archive = state.path.clone();
            let cancel = state.cancel.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            state.testing = false;
            state.extracting = true;
            state.receiver = Some(receiver);
            EnableWindow(state.test, 0);
            EnableWindow(state.extract, 0);
            EnableWindow(state.extract_all, 0);
            EnableWindow(state.password_button, 0);
            SetWindowTextW(state.status, text("Extracting archive… Close this window to cancel.").as_ptr());
            let password = state.password.clone();
            thread::spawn(move || {
                let result = zfm::archive_engine::extract_all_password(&archive, &destination, &cancel, password.as_deref())
                    .map(|count| vec![format!("Extracted {count} files. Existing files were not overwritten.")])
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            });
            if SetTimer(hwnd, 1, 100, ptr::null_mut()) == 0 {
                state.cancel.store(true, Ordering::Relaxed);
                SetWindowTextW(state.status, text("Could not start extraction timer.").as_ptr());
            }
            1
        }
        WM_COMMAND if wp & 0xffff == 1006 && state.receiver.is_none() => {
            let password = match windows_actions::password_prompt(hwnd, state.theme, "Archive password (leave empty to clear)") {
                Some(p) => p, None => return 1,
            };
            if password.len() > 512 || password.chars().any(char::is_control) {
                show_error(hwnd, "Password must be at most 512 UTF-8 bytes and contain no control characters.");
                return 1;
            }
            state.password = if password.is_empty() { None } else { Some(password) };
            let password = state.password.clone();
            let path = state.path.clone();
            let cancel = state.cancel.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            state.receiver = Some(receiver);
            state.testing = false;
            state.extracting = false;
            SendMessageW(state.list, 0x0184, 0, 0); // LB_RESETCONTENT
            for control in [state.test, state.extract, state.extract_all, state.password_button] { EnableWindow(control, 0); }
            SetWindowTextW(state.status, text("Reading archive…").as_ptr());
            thread::spawn(move || { let _ = sender.send(open_listing(&path, &cancel, password.as_deref())); });
            if SetTimer(hwnd, 1, 100, ptr::null_mut()) == 0 {
                state.cancel.store(true, Ordering::Relaxed);
                SetWindowTextW(state.status, text("Could not start archive inspection timer.").as_ptr());
            }
            1
        }
        WM_COMMAND if wp & 0xffff == 2 => { KillTimer(hwnd, 1); EndDialog(hwnd, 0); 1 }
        WM_CLOSE => { KillTimer(hwnd, 1); EndDialog(hwnd, 0); 1 }
        WM_DRAWITEM if lp != 0 && (*(lp as *const DrawItem)).control_type == ODT_BUTTON => draw_dialog_button(state.theme, &*(lp as *const DrawItem)),
        0x0136 => (*state.theme).window_brush as isize,
        WM_CTLCOLORSTATIC => {
            SetTextColor(wp as H, ui_theme::TEXT_BRIGHT.colorref());
            SetBkColor(wp as H, ui_theme::HEADER_TOP.colorref());
            (*state.theme).window_brush as isize
        }
        WM_CTLCOLORLISTBOX => {
            SetTextColor(wp as H, ui_theme::TEXT_BRIGHT.colorref());
            SetBkColor(wp as H, ui_theme::NAME_EDIT_BG.colorref());
            (*state.theme).name_brush as isize
        }
        _ => 0,
    }
}

pub(super) unsafe fn show(hwnd: H, theme: *const Theme, path: &Path) {
    let (sender, receiver) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let path = path.to_path_buf();
    let title = text(&format!("ZFM Archives — {}", path.display()));
    let worker_path = path.clone();
    thread::spawn(move || { let _ = sender.send(open_listing(&worker_path, &worker_cancel, None)); });
    let mut state = Dialog { theme, title, path, list: ptr::null_mut(), status: ptr::null_mut(), close: ptr::null_mut(), test: ptr::null_mut(), extract: ptr::null_mut(), extract_all: ptr::null_mut(), password_button: ptr::null_mut(), password: None, receiver: Some(receiver), testing: false, extracting: false, cancel };
    let style: u32 = 0x8000_0000 | 0x00c0_0000 | 0x0008_0000 | 0x0004_0000 | 0x0001_0000 | 0x80;
    let template: Vec<u16> = vec![style as u16, (style >> 16) as u16, 0, 0, 0, 0, 0, 520, 340, 0, 0, 0];
    let result = DialogBoxIndirectParamW(GetModuleHandleW(ptr::null()), template.as_ptr(), hwnd, Some(dialog_proc), &mut state as *mut _ as isize);
    if result == -1 { show_error(hwnd, "Could not open the archive browser."); }
}
