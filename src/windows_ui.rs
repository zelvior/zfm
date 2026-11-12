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
use zfm::navigation::{Destination, History};

type H = *mut c_void;
type WParam = usize;
type LParam = isize;
type LResult = isize;
const WM_CREATE: u32 = 1;
const WM_DESTROY: u32 = 2;
const WM_SIZE: u32 = 5;
const WM_COMMAND: u32 = 0x111;
const WM_SETFONT: u32 = 0x30;
const WM_CTLCOLORSTATIC: u32 = 0x138;
const WM_CTLCOLOREDIT: u32 = 0x133;
const WM_CTLCOLORLISTBOX: u32 = 0x134;
const WM_RESULT: u32 = 0x8001;
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
const BACK: usize = 101;
const FORWARD: usize = 102;
const UP: usize = 103;
const REFRESH: usize = 104;
const GO: usize = 105;
const ADDRESS: usize = 106;
const LIST: usize = 107;
const STATUS: usize = 108;
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
    history: History,
    token: Arc<AtomicU64>,
    address: H,
    list: H,
    status: H,
    buttons: [H; 5],
    entries: Vec<Entry>,
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
    let path = match (*app).history.resolve(&destination) {
        Some(path) => path,
        None => return,
    };
    let generation = (*app).token.fetch_add(1, Ordering::Relaxed) + 1;
    let token = (*app).token.clone();
    let target = hwnd as usize;
    set_status(app, &format!("Loading {}…", path.display()));
    thread::spawn(move || {
        let result = directory::read(&path, false, &token, generation);
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
    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut r);
    let width = r.right.max(320);
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
    MoveWindow(
        list,
        margin,
        top + h + 8,
        width - 2 * margin,
        (height - (top + h + 8) - 32).max(1),
        1,
    );
    MoveWindow(status, margin, height - 25, width - 2 * margin, 20, 1);
}
unsafe fn address_value(app: *mut App) -> PathBuf {
    let address = (*app).address;
    let length = GetWindowTextLengthW(address).max(0) as usize;
    let mut value = vec![0u16; length + 1];
    let n = GetWindowTextW(address, value.as_mut_ptr(), (length + 1) as i32).max(0) as usize;
    PathBuf::from(std::ffi::OsString::from_wide(&value[..n]))
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
        if address.is_null()
            || list.is_null()
            || status.is_null()
            || buttons.iter().any(|b| b.is_null())
        {
            return -1;
        }
        (*app).buttons = buttons;
        (*app).address = address;
        (*app).list = list;
        (*app).status = status;
        let path = wide((*app).history.current().as_os_str());
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
                    (*app).history.commit(message.destination);
                    let path = wide((*app).history.current().as_os_str());
                    let status = format!(
                        "{} items  ·  {}{}",
                        listing.entries.len(),
                        (*app).history.current().display(),
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
                    let path = wide((*app).history.current().as_os_str());
                    let address = (*app).address;
                    SetWindowTextW(address, path.as_ptr());
                    set_status(app, &format!("Could not open folder: {e}"));
                }
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
                    let parent = (*app).history.current().parent().map(|p| p.to_path_buf());
                    if let Some(parent) = parent {
                        load(hwnd, app, Destination::Visit(parent));
                    }
                }
                GO => {
                    let path = address_value(app);
                    let path = if path.is_absolute() {
                        path
                    } else {
                        (*app).history.current().join(path)
                    };
                    load(hwnd, app, Destination::Visit(path));
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
            history: History::new(initial),
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
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
