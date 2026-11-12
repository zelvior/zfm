//! Small Win32 browsing adapter. Directory reads run on workers; there are no
//! browser components or runtime GUI dependencies beyond the OS.
use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::ptr;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::thread;
use zfm::directory::{self, Entry, Listing};
use zfm::navigation::{Destination, Tabs};

type H = *mut c_void;
type WParam = usize;
type LParam = isize;
type LResult = isize;
const WM_CREATE: u32 = 1;
const WM_DESTROY: u32 = 2;
const WM_SIZE: u32 = 5;
const WM_COMMAND: u32 = 0x111;
const WM_KEYDOWN: u32 = 0x100;
const WM_SYSKEYDOWN: u32 = 0x104;
const WM_SETFONT: u32 = 0x30;
const WM_CTLCOLORSTATIC: u32 = 0x138;
const WM_CTLCOLOREDIT: u32 = 0x133;
const WM_CTLCOLORLISTBOX: u32 = 0x134;
const WM_RESULT: u32 = 0x8001;
const WM_OPERATION_RESULT: u32 = 0x8002;
const GWLP_USERDATA: i32 = -21;
const WS_OVERLAPPEDWINDOW: u32 = 0x00cf0000;
const WS_CHILD: u32 = 0x40000000;
const WS_VISIBLE: u32 = 0x10000000;
const WS_BORDER: u32 = 0x00800000;
const WS_VSCROLL: u32 = 0x00200000;
const WS_TABSTOP: u32 = 0x00010000;
const LB_ADDSTRING: u32 = 0x180;
const LB_RESETCONTENT: u32 = 0x184;
const LB_GETCURSEL: u32 = 0x188;
const CB_ADDSTRING: u32 = 0x143;
const CB_RESETCONTENT: u32 = 0x14b;
const CB_SETCURSEL: u32 = 0x14e;
const CB_GETCURSEL: u32 = 0x147;
const BM_GETCHECK: u32 = 0xf0;
const BACK: usize = 101;
const FORWARD: usize = 102;
const UP: usize = 103;
const REFRESH: usize = 104;
const GO: usize = 105;
const ADDRESS: usize = 106;
const LIST: usize = 107;
const STATUS: usize = 108;
const TABS: usize = 109;
const NEW_TAB: usize = 110;
const CLOSE_TAB: usize = 111;
const FILTER: usize = 112;
const APPLY_FILTER: usize = 113;
const CLEAR_FILTER: usize = 114;
const HIDDEN: usize = 115;
const NAME: usize = 116;
const NEW_FILE: usize = 117;
const NEW_FOLDER: usize = 118;
const DELETE: usize = 119;
const RENAME: usize = 120;
#[repr(C)]
struct WndClass {
    style: u32,
    proc: Option<unsafe extern "system" fn(H, u32, WParam, LParam) -> LResult>,
    cls_extra: i32,
    wnd_extra: i32,
    instance: H,
    icon: H,
    cursor: H,
    background: H,
    menu: *const u16,
    class: *const u16,
}
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct Msg {
    hwnd: H,
    message: u32,
    wparam: WParam,
    lparam: LParam,
    time: u32,
    point: Point,
    private: u32,
}
#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct CreateStruct {
    param: *mut c_void,
    instance: H,
    menu: H,
    parent: H,
    cy: i32,
    cx: i32,
    y: i32,
    x: i32,
    style: i32,
    name: *const u16,
    class: *const u16,
    ex_style: u32,
}
#[link(name = "user32")]
extern "system" {
    fn RegisterClassW(c: *const WndClass) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: H,
        menu: H,
        instance: H,
        param: P,
    ) -> H;
    fn DefWindowProcW(hwnd: H, msg: u32, wp: WParam, lp: LParam) -> LResult;
    fn ShowWindow(hwnd: H, show: i32) -> i32;
    fn GetMessageW(msg: *mut Msg, hwnd: H, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> LResult;
    fn PostQuitMessage(code: i32);
    fn PostMessageW(hwnd: H, msg: u32, wp: WParam, lp: LParam) -> i32;
    fn LoadCursorW(instance: H, name: *const u16) -> H;
    fn GetClientRect(hwnd: H, rect: *mut Rect) -> i32;
    fn MoveWindow(hwnd: H, x: i32, y: i32, w: i32, h: i32, repaint: i32) -> i32;
    fn SendMessageW(hwnd: H, msg: u32, wp: WParam, lp: LParam) -> LResult;
    fn SetWindowTextW(hwnd: H, text: *const u16) -> i32;
    fn GetWindowTextLengthW(hwnd: H) -> i32;
    fn GetWindowTextW(hwnd: H, text: *mut u16, max: i32) -> i32;
    fn MessageBoxW(hwnd: H, text: *const u16, caption: *const u16, kind: u32) -> i32;
    fn GetFocus() -> H;
    fn SetFocus(hwnd: H) -> H;
    fn GetKeyState(key: i32) -> i16;
}
type P = *mut c_void;
#[cfg(target_pointer_width = "64")]
#[link(name = "user32")]
extern "system" {
    fn SetWindowLongPtrW(hwnd: H, index: i32, value: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: H, index: i32) -> isize;
}
#[cfg(target_pointer_width = "32")]
#[link(name = "user32")]
extern "system" {
    fn SetWindowLongW(hwnd: H, index: i32, value: i32) -> i32;
    fn GetWindowLongW(hwnd: H, index: i32) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> H;
}
#[link(name = "gdi32")]
extern "system" {
    fn GetStockObject(index: i32) -> H;
    fn SetTextColor(dc: H, color: u32) -> u32;
    fn SetBkColor(dc: H, color: u32) -> u32;
}
#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        hwnd: H,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> H;
}
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}
fn text(s: &str) -> Vec<u16> {
    wide(OsStr::new(s))
}
unsafe fn set_data(hwnd: H, data: isize) {
    #[cfg(target_pointer_width = "64")]
    {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, data);
    }
    #[cfg(target_pointer_width = "32")]
    {
        SetWindowLongW(hwnd, GWLP_USERDATA, data as i32);
    }
}
unsafe fn get_data(hwnd: H) -> isize {
    #[cfg(target_pointer_width = "64")]
    {
        GetWindowLongPtrW(hwnd, GWLP_USERDATA)
    }
    #[cfg(target_pointer_width = "32")]
    {
        GetWindowLongW(hwnd, GWLP_USERDATA) as isize
    }
}
struct App {
    tabs: Tabs,
    tabs_control: H,
    filter_control: H,
    filter: String,
    show_hidden: bool,
    extra_buttons: [H; 9],
    name_control: H,
    filter_label: H,
    name_label: H,
    operation_busy: bool,
    token: Arc<AtomicU64>,
    address: H,
    list: H,
    status: H,
    buttons: [H; 5],
    entries: Vec<Entry>,
}
struct OperationMessage {
    result: Result<String, String>,
}
struct ResultMessage {
    generation: u64,
    destination: Destination,
    result: std::io::Result<Option<Listing>>,
}
unsafe fn child(hwnd: H, class: &str, value: &str, style: u32, id: usize) -> H {
    let control = CreateWindowExW(
        0,
        text(class).as_ptr(),
        text(value).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        1,
        1,
        hwnd,
        id as H,
        GetModuleHandleW(ptr::null()),
        ptr::null_mut(),
    );
    if !control.is_null() {
        SendMessageW(control, WM_SETFONT, GetStockObject(17) as usize, 1);
    }
    control
}
unsafe fn set_status(app: *mut App, value: &str) {
    let status = (*app).status;
    SetWindowTextW(status, text(value).as_ptr());
}
unsafe fn load(hwnd: H, app: *mut App, destination: Destination) {
    let path = match (*app).tabs.current().resolve(&destination) {
        Some(path) => path,
        None => return,
    };
    let generation = (*app).token.fetch_add(1, Ordering::Relaxed) + 1;
    let token = (*app).token.clone();
    let target = hwnd as usize;
    let filter = (*app).filter.clone();
    let show_hidden = (*app).show_hidden;
    set_status(app, &format!("Loading {}…", path.display()));
    thread::spawn(move || {
        let result =
            directory::read_filtered(&path, show_hidden, &filter, false, &token, generation);
        if token.load(Ordering::Relaxed) != generation {
            return;
        }
        let message = Box::new(ResultMessage {
            generation,
            destination,
            result,
        });
        let raw = Box::into_raw(message);
        if unsafe { PostMessageW(target as H, WM_RESULT, 0, raw as isize) } == 0 {
            unsafe {
                drop(Box::from_raw(raw));
            }
        }
    });
}
unsafe fn layout(hwnd: H, app: *mut App) {
    let buttons = (*app).buttons;
    let address = (*app).address;
    let list = (*app).list;
    let status = (*app).status;
    let tabs = (*app).tabs_control;
    let filter = (*app).filter_control;
    let extra = (*app).extra_buttons;
    let name = (*app).name_control;
    let filter_label = (*app).filter_label;
    let name_label = (*app).name_label;
    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut r);
    let width = r.right.max(520);
    let height = r.bottom.max(120);
    let margin = 8;
    let top = 8;
    let h = 30;
    for (i, button) in buttons.iter().take(4).enumerate() {
        MoveWindow(*button, margin + (i as i32) * 42, top, 38, h, 1);
    }
    let x = margin + 4 * 42;
    MoveWindow(buttons[4], width - margin - 42, top, 42, h, 1);
    MoveWindow(address, x, top, (width - margin - 48 - x).max(1), h, 1);
    let second = top + h + 8;
    MoveWindow(tabs, margin, second, (width - 210).max(80), 300, 1);
    MoveWindow(extra[0], width - 194, second, 92, h, 1);
    MoveWindow(extra[1], width - 98, second, 90, h, 1);
    let third = second + h + 8;
    MoveWindow(filter_label, margin, third + 7, 45, 20, 1);
    MoveWindow(filter, margin + 48, third, (width - 336).max(30), h, 1);
    MoveWindow(extra[2], width - 274, third, 76, h, 1);
    MoveWindow(extra[3], width - 194, third, 76, h, 1);
    MoveWindow(extra[4], width - 114, third, 106, h, 1);
    let fourth = third + h + 8;
    MoveWindow(name_label, margin, fourth + 7, 70, 20, 1);
    MoveWindow(name, margin + 72, fourth, (width - 486).max(30), h, 1);
    MoveWindow(extra[5], width - 400, fourth, 92, h, 1);
    MoveWindow(extra[6], width - 304, fourth, 100, h, 1);
    MoveWindow(extra[8], width - 200, fourth, 92, h, 1);
    MoveWindow(extra[7], width - 104, fourth, 96, h, 1);
    let list_top = fourth + h + 8;
    MoveWindow(
        list,
        margin,
        list_top,
        width - 2 * margin,
        (height - list_top - 32).max(1),
        1,
    );
    MoveWindow(status, margin, height - 25, width - 2 * margin, 20, 1);
}
unsafe fn control_value(address: H) -> std::ffi::OsString {
    let length = GetWindowTextLengthW(address).max(0) as usize;
    let mut value = vec![0u16; length + 1];
    let n = GetWindowTextW(address, value.as_mut_ptr(), (length + 1) as i32).max(0) as usize;
    std::ffi::OsString::from_wide(&value[..n])
}
unsafe fn refresh_tabs(app: *mut App) {
    let control = (*app).tabs_control;
    SendMessageW(control, CB_RESETCONTENT, 0, 0);
    let count = (*app).tabs.len();
    for i in 0..count {
        let label = (*app)
            .tabs
            .history(i)
            .unwrap()
            .current()
            .to_string_lossy()
            .into_owned();
        SendMessageW(control, CB_ADDSTRING, 0, text(&label).as_ptr() as isize);
    }
    SendMessageW(control, CB_SETCURSEL, (*app).tabs.active(), 0);
}
enum Operation {
    NewFile(PathBuf, std::ffi::OsString),
    NewFolder(PathBuf, std::ffi::OsString),
    Delete(PathBuf),
    Rename(PathBuf, std::ffi::OsString),
}
unsafe fn start_operation(hwnd: H, app: *mut App, operation: Operation) {
    if (*app).operation_busy {
        set_status(app, "Another file operation is already running.");
        return;
    }
    (*app).operation_busy = true;
    set_status(app, "Working…");
    let target = hwnd as usize;
    thread::spawn(move || {
        let result = match operation {
            Operation::NewFile(dir, name) => zfm::operations::create_file(&dir, &name)
                .map(|p| format!("Created {}", p.display())),
            Operation::NewFolder(dir, name) => zfm::operations::create_directory(&dir, &name)
                .map(|p| format!("Created {}", p.display())),
            Operation::Rename(path, name) => zfm::operations::rename_child(&path, &name)
                .map(|p| format!("Renamed to {}", p.display())),
            Operation::Delete(path) => {
                zfm::operations::remove_one(&path).map(|_| format!("Deleted {}", path.display()))
            }
        }
        .map_err(|e| e.to_string());
        let msg = Box::into_raw(Box::new(OperationMessage { result }));
        if unsafe { PostMessageW(target as H, WM_OPERATION_RESULT, 0, msg as isize) } == 0 {
            unsafe {
                drop(Box::from_raw(msg));
            }
        }
    });
}
/// Surface initialization errors even when launched from Explorer without a console.
pub fn show_startup_error(message: &str) {
    unsafe {
        show_error(ptr::null_mut(), &format!("ZFM could not start: {message}"));
    }
}
unsafe fn show_error(hwnd: H, message: &str) {
    MessageBoxW(hwnd, text(message).as_ptr(), text("ZFM").as_ptr(), 0x10);
}
unsafe extern "system" fn window_proc(hwnd: H, msg: u32, wp: WParam, lp: LParam) -> LResult {
    if msg == WM_CREATE {
        let app = (*(lp as *const CreateStruct)).param as *mut App;
        set_data(hwnd, app as isize);
        let buttons = [
            child(hwnd, "BUTTON", "Back", WS_TABSTOP, BACK),
            child(hwnd, "BUTTON", "Next", WS_TABSTOP, FORWARD),
            child(hwnd, "BUTTON", "Up", WS_TABSTOP, UP),
            child(hwnd, "BUTTON", "Reload", WS_TABSTOP, REFRESH),
            child(hwnd, "BUTTON", "Go", WS_TABSTOP, GO),
        ];
        let address = child(hwnd, "EDIT", "", WS_BORDER | WS_TABSTOP | 0x80, ADDRESS);
        let list = child(
            hwnd,
            "LISTBOX",
            "",
            WS_BORDER | WS_TABSTOP | WS_VSCROLL | 1,
            LIST,
        );
        let status = child(hwnd, "STATIC", "", 0, STATUS);
        let tabs_control = child(hwnd, "COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | 3, TABS);
        let filter_control = child(hwnd, "EDIT", "", WS_BORDER | WS_TABSTOP | 0x80, FILTER);
        let name_control = child(hwnd, "EDIT", "", WS_BORDER | WS_TABSTOP | 0x80, NAME);
        let filter_label = child(hwnd, "STATIC", "Filter:", 0, 0);
        let name_label = child(hwnd, "STATIC", "New name:", 0, 0);
        let extra_buttons = [
            child(hwnd, "BUTTON", "New tab", WS_TABSTOP, NEW_TAB),
            child(hwnd, "BUTTON", "Close tab", WS_TABSTOP, CLOSE_TAB),
            child(hwnd, "BUTTON", "Search", WS_TABSTOP, APPLY_FILTER),
            child(hwnd, "BUTTON", "Clear", WS_TABSTOP, CLEAR_FILTER),
            child(hwnd, "BUTTON", "Show hidden", WS_TABSTOP | 3, HIDDEN),
            child(hwnd, "BUTTON", "New file", WS_TABSTOP, NEW_FILE),
            child(hwnd, "BUTTON", "New folder", WS_TABSTOP, NEW_FOLDER),
            child(hwnd, "BUTTON", "Delete…", WS_TABSTOP, DELETE),
            child(hwnd, "BUTTON", "Rename", WS_TABSTOP, RENAME),
        ];
        if address.is_null()
            || list.is_null()
            || status.is_null()
            || buttons.iter().any(|b| b.is_null())
            || tabs_control.is_null()
            || filter_control.is_null()
            || name_control.is_null()
            || filter_label.is_null()
            || name_label.is_null()
            || extra_buttons.iter().any(|b| b.is_null())
        {
            return -1;
        }
        (*app).buttons = buttons;
        (*app).address = address;
        (*app).list = list;
        (*app).status = status;
        (*app).tabs_control = tabs_control;
        (*app).filter_control = filter_control;
        (*app).name_control = name_control;
        (*app).filter_label = filter_label;
        (*app).name_label = name_label;
        (*app).extra_buttons = extra_buttons;
        refresh_tabs(app);
        let path = wide((*app).tabs.current().current().as_os_str());
        SetWindowTextW(address, path.as_ptr());
        layout(hwnd, app);
        load(hwnd, app, Destination::Refresh);
        return 0;
    }
    let data = get_data(hwnd);
    if data == 0 {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let app = data as *mut App;
    match msg {
        WM_SIZE => {
            layout(hwnd, app);
            0
        }
        WM_RESULT => {
            let message = Box::from_raw(lp as *mut ResultMessage);
            if (*app).token.load(Ordering::Relaxed) != message.generation {
                return 0;
            }
            match message.result {
                Ok(Some(listing)) => {
                    (*app).tabs.current_mut().commit(message.destination);
                    refresh_tabs(app);
                    let path = wide((*app).tabs.current().current().as_os_str());
                    let status = format!(
                        "{} items  ·  {}{}",
                        listing.entries.len(),
                        (*app).tabs.current().current().display(),
                        if listing.skipped > 0 {
                            format!("  ·  {} entries had errors", listing.skipped)
                        } else {
                            String::new()
                        }
                    );
                    (*app).entries = listing.entries;
                    let address = (*app).address;
                    let list = (*app).list;
                    SetWindowTextW(address, path.as_ptr());
                    SendMessageW(list, LB_RESETCONTENT, 0, 0);
                    for index in 0..(*app).entries.len() {
                        let entry = &(&(*app).entries)[index];
                        let line = if entry.is_dir {
                            format!("[Folder]  {}", entry.name)
                        } else {
                            format!("{}     {} bytes", entry.name, entry.size.unwrap_or(0))
                        };
                        SendMessageW(list, LB_ADDSTRING, 0, text(&line).as_ptr() as isize);
                    }
                    set_status(app, &status);
                }
                Ok(None) => (),
                Err(e) => {
                    let path = wide((*app).tabs.current().current().as_os_str());
                    let address = (*app).address;
                    SetWindowTextW(address, path.as_ptr());
                    set_status(app, &format!("Could not open folder: {e}"));
                }
            }
            0
        }
        WM_OPERATION_RESULT => {
            let message = Box::from_raw(lp as *mut OperationMessage);
            (*app).operation_busy = false;
            match message.result {
                Ok(status) => {
                    load(hwnd, app, Destination::Refresh);
                    set_status(app, &status);
                }
                Err(error) => set_status(app, &format!("File operation failed: {error}")),
            }
            0
        }
        WM_COMMAND => {
            let id = wp & 0xffff;
            let code = (wp >> 16) & 0xffff;
            match id {
                BACK => load(hwnd, app, Destination::Back),
                FORWARD => load(hwnd, app, Destination::Forward),
                REFRESH => load(hwnd, app, Destination::Refresh),
                UP => {
                    let parent = (*app)
                        .tabs
                        .current()
                        .current()
                        .parent()
                        .map(|p| p.to_path_buf());
                    if let Some(parent) = parent {
                        load(hwnd, app, Destination::Visit(parent));
                    }
                }
                GO => {
                    let path = PathBuf::from(control_value((*app).address));
                    let path = if path.is_absolute() {
                        path
                    } else {
                        (*app).tabs.current().current().join(path)
                    };
                    load(hwnd, app, Destination::Visit(path));
                }
                NEW_TAB if code == 0 => {
                    let path = (*app).tabs.current().current().to_path_buf();
                    (*app).tabs.add(path);
                    refresh_tabs(app);
                    load(hwnd, app, Destination::Refresh);
                }
                CLOSE_TAB if code == 0 => {
                    if (*app).tabs.close() {
                        refresh_tabs(app);
                        load(hwnd, app, Destination::Refresh);
                    } else {
                        set_status(app, "At least one tab must remain open.");
                    }
                }
                TABS if code == 1 => {
                    let index = SendMessageW((*app).tabs_control, CB_GETCURSEL, 0, 0);
                    if index >= 0 && (*app).tabs.select(index as usize) {
                        load(hwnd, app, Destination::Refresh);
                    }
                }
                APPLY_FILTER if code == 0 => {
                    (*app).filter = control_value((*app).filter_control)
                        .to_string_lossy()
                        .into_owned();
                    load(hwnd, app, Destination::Refresh);
                }
                CLEAR_FILTER if code == 0 => {
                    (*app).filter.clear();
                    SetWindowTextW((*app).filter_control, text("").as_ptr());
                    load(hwnd, app, Destination::Refresh);
                }
                HIDDEN if code == 0 => {
                    (*app).show_hidden =
                        SendMessageW((*app).extra_buttons[4], BM_GETCHECK, 0, 0) == 1;
                    load(hwnd, app, Destination::Refresh);
                }
                NEW_FILE | NEW_FOLDER if code == 0 => {
                    let name = control_value((*app).name_control);
                    let dir = (*app).tabs.current().current().to_path_buf();
                    let operation = if id == NEW_FILE {
                        Operation::NewFile(dir, name)
                    } else {
                        Operation::NewFolder(dir, name)
                    };
                    start_operation(hwnd, app, operation);
                }
                RENAME if code == 0 => {
                    let index = SendMessageW((*app).list, LB_GETCURSEL, 0, 0);
                    let path = if index >= 0 {
                        (&(*app).entries)
                            .get(index as usize)
                            .map(|e| e.path.clone())
                    } else {
                        None
                    };
                    if let Some(path) = path {
                        let name = control_value((*app).name_control);
                        start_operation(hwnd, app, Operation::Rename(path, name));
                    } else {
                        set_status(app, "Select an item and enter a new name.");
                    }
                }
                DELETE if code == 0 => {
                    let index = SendMessageW((*app).list, LB_GETCURSEL, 0, 0);
                    let path = if index >= 0 {
                        (&(*app).entries)
                            .get(index as usize)
                            .map(|e| e.path.clone())
                    } else {
                        None
                    };
                    if let Some(path) = path {
                        let message = format!("Permanently delete {}? This cannot be undone. Only files or empty folders can be deleted.", path.display());
                        if MessageBoxW(
                            hwnd,
                            text(&message).as_ptr(),
                            text("Confirm permanent deletion").as_ptr(),
                            0x134,
                        ) == 6
                        {
                            start_operation(hwnd, app, Operation::Delete(path));
                        }
                    } else {
                        set_status(app, "Select an item to delete.");
                    }
                }
                LIST if code == 2 => {
                    let list = (*app).list;
                    let index = SendMessageW(list, LB_GETCURSEL, 0, 0);
                    let entry = if index >= 0 {
                        (&(*app).entries)
                            .get(index as usize)
                            .map(|e| (e.path.clone(), e.is_dir))
                    } else {
                        None
                    };
                    if let Some((path, is_dir)) = entry {
                        if is_dir {
                            load(hwnd, app, Destination::Visit(path));
                        } else {
                            let result = ShellExecuteW(
                                hwnd,
                                text("open").as_ptr(),
                                wide(path.as_os_str()).as_ptr(),
                                ptr::null(),
                                ptr::null(),
                                1,
                            ) as usize;
                            if result <= 32 {
                                show_error(hwnd, "Windows could not open this file.");
                            }
                        }
                    }
                }
                _ => (),
            };
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            SetTextColor(wp as H, 0xffffff);
            SetBkColor(wp as H, 0);
            GetStockObject(4) as LResult
        }
        WM_DESTROY => {
            (*app).token.fetch_add(1, Ordering::Relaxed);
            set_data(hwnd, 0);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
pub fn run() -> Result<(), String> {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        if instance.is_null() {
            return Err("Cannot get application instance".into());
        }
        let class = text("ZFMNativeWindow");
        let wc = WndClass {
            style: 0,
            proc: Some(window_proc),
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: ptr::null_mut(),
            cursor: LoadCursorW(ptr::null_mut(), 32512usize as *const u16),
            background: GetStockObject(4),
            menu: ptr::null(),
            class: class.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            return Err("Could not register the window class".into());
        }
        let initial = std::env::args_os()
            .nth(1)
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .or_else(|| std::env::current_dir().ok())
            .ok_or("Cannot determine an initial directory")?;
        let initial = if initial.is_absolute() {
            initial
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(initial)
        };
        // The state is retained until process exit; background messages may still be queued.
        let app = Box::into_raw(Box::new(App {
            tabs: Tabs::new(initial),
            tabs_control: ptr::null_mut(),
            filter_control: ptr::null_mut(),
            filter: String::new(),
            show_hidden: false,
            extra_buttons: [ptr::null_mut(); 9],
            name_control: ptr::null_mut(),
            filter_label: ptr::null_mut(),
            name_label: ptr::null_mut(),
            operation_busy: false,
            token: Arc::new(AtomicU64::new(0)),
            address: ptr::null_mut(),
            list: ptr::null_mut(),
            status: ptr::null_mut(),
            buttons: [ptr::null_mut(); 5],
            entries: Vec::new(),
        }));
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            text("ZFM — Zelvior File Manager").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0x80000000u32 as i32,
            0x80000000u32 as i32,
            950,
            650,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            app as P,
        );
        if window.is_null() {
            return Err("Could not create the native window".into());
        }
        ShowWindow(window, 5);
        let mut msg: Msg = std::mem::zeroed();
        loop {
            let status = GetMessageW(&mut msg, ptr::null_mut(), 0, 0);
            if status == 0 {
                break;
            }
            if status < 0 {
                return Err("Windows message loop failed".into());
            }
            if msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN {
                let focus = GetFocus();
                let ctrl = GetKeyState(0x11) < 0;
                let alt = GetKeyState(0x12) < 0;
                let id = match msg.wparam {
                    0x74 if !ctrl && !alt => Some(REFRESH),
                    0x54 if ctrl && !alt => Some(NEW_TAB),
                    0x57 if ctrl && !alt => Some(CLOSE_TAB),
                    0x25 if alt && !ctrl => Some(BACK),
                    0x27 if alt && !ctrl => Some(FORWARD),
                    0x26 if alt && !ctrl => Some(UP),
                    0x0d if focus == (*app).address => Some(GO),
                    0x0d if focus == (*app).filter_control => Some(APPLY_FILTER),
                    0x2e if focus == (*app).list && !ctrl && !alt => Some(DELETE),
                    _ => None,
                };
                if let Some(id) = id {
                    SendMessageW(window, WM_COMMAND, id, 0);
                    continue;
                }
                if ctrl && !alt && msg.wparam == 0x4c {
                    SetFocus((*app).address);
                    continue;
                }
                if ctrl && !alt && msg.wparam == 0x46 {
                    SetFocus((*app).filter_control);
                    continue;
                }
                if msg.wparam == 0x0d && focus == (*app).list && !ctrl && !alt {
                    SendMessageW(window, WM_COMMAND, LIST | (2 << 16), 0);
                    continue;
                }
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
