// Modified: integrate the native read-only preview and settings dialog.
// Modified: compact reference-inspired file-manager layout and visuals.
//! Native Win32 dark-slate file manager interface.
//!
//! Directory enumeration and file mutations run on background workers; surfaces,
//! vector icons, tabs, sidebar and 4-column rows are rendered offscreen into
//! 32-bit DIB sections with ClearType typography and no external dependencies.

use std::ffi::{c_void, OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use zfm::directory::{self, Entry, Listing};
use zfm::navigation::{Destination, Tabs};

#[path = "windows_actions.rs"]
mod windows_actions;
#[path = "windows_viewer.rs"]
mod windows_viewer;
#[path = "windows_settings.rs"]
mod windows_settings;
use zfm::ui_theme::{
    self, ActionIcon, BoxRect, ItemIcon, Rgb, SortColumn, ViewMode, WindowMetrics,
};

type H = *mut c_void;
type P = *mut c_void;
type WParam = usize;
type LParam = isize;
type LResult = isize;

const WM_CREATE: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_SIZE: u32 = 0x0005;
const WM_PAINT: u32 = 0x000f;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_SETCURSOR: u32 = 0x0020;
const WM_GETMINMAXINFO: u32 = 0x0024;
const WM_DRAWITEM: u32 = 0x002b;
const WM_SETFONT: u32 = 0x0030;
const WM_KEYDOWN: u32 = 0x0100;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_COMMAND: u32 = 0x0111;
const WM_CTLCOLOREDIT: u32 = 0x0133;
const WM_CTLCOLORLISTBOX: u32 = 0x0134;
const WM_CTLCOLORSTATIC: u32 = 0x0138;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_XBUTTONDOWN: u32 = 0x020b;
const WM_CONTEXTMENU: u32 = 0x007b;
const WM_MOUSELEAVE: u32 = 0x02a3;
const WM_RESULT: u32 = 0x8001;
const WM_OPERATION_RESULT: u32 = 0x8002;

const GWLP_USERDATA: i32 = -21;
const WS_OVERLAPPEDWINDOW: u32 = 0x00cf_0000;
const WS_CLIPCHILDREN: u32 = 0x0200_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const ES_AUTOHSCROLL: u32 = 0x0080;
const EM_SETSEL: u32 = 0x00b1;
const EM_SETCUEBANNER: u32 = 0x1501;

const LBS_NOTIFY: u32 = 0x0001;
const LBS_OWNERDRAWFIXED: u32 = 0x0010;
const LBS_HASSTRINGS: u32 = 0x0040;
const LBS_EXTENDEDSEL: u32 = 0x0800;
const LB_ADDSTRING: u32 = 0x0180;
const LB_RESETCONTENT: u32 = 0x0184;
const LB_SETSEL: u32 = 0x0185;
const LB_GETSEL: u32 = 0x0187;
const LB_GETSELCOUNT: u32 = 0x0190;
const LB_GETSELITEMS: u32 = 0x0191;
const LB_SETCARETINDEX: u32 = 0x019e;
const LB_SETITEMHEIGHT: u32 = 0x01a0;
const LB_GETITEMHEIGHT: u32 = 0x01a1;
const LB_GETTOPINDEX: u32 = 0x018e;

const BS_OWNERDRAW: u32 = 0x000b;
const ODT_LISTBOX: u32 = 2;
const ODT_BUTTON: u32 = 4;
const EN_SETFOCUS: usize = 0x0100;
const EN_KILLFOCUS: usize = 0x0200;

const DT_CENTER: u32 = 0x0001;
const DT_RIGHT: u32 = 0x0002;
const DT_VCENTER: u32 = 0x0004;
const DT_SINGLELINE: u32 = 0x0020;
const DT_NOPREFIX: u32 = 0x0800;
const DT_END_ELLIPSIS: u32 = 0x8000;
const SRCCOPY: u32 = 0x00cc_0020;

const BACK: usize = 101;
const FORWARD: usize = 102;
const UP: usize = 103;
const REFRESH: usize = 104;
const GO: usize = 105;
const ADDRESS: usize = 106;
const LIST: usize = 107;
const STATUS: usize = 108;
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
const ROOT: usize = 121;
const COPY: usize = 130;
const CUT: usize = 131;
const PASTE: usize = 132;
const COPY_PATH: usize = 133;
const DUPLICATE: usize = 134;
const TRASH: usize = 135;
const PROPERTIES: usize = 136;
const OPEN_TAB: usize = 137;
const PROMPT_RENAME: usize = 138;
const PROMPT_FILE: usize = 139;
const PROMPT_FOLDER: usize = 140;
const CONVERT: usize = 141;
const PREVIEW: usize = 142;
const SETTINGS: usize = 143;

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
struct PaintStruct {
    hdc: H,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}

#[repr(C)]
struct DrawItem {
    control_type: u32,
    control_id: u32,
    item_id: u32,
    item_action: u32,
    item_state: u32,
    hwnd: H,
    dc: H,
    rect: Rect,
    item_data: usize,
}

#[repr(C)]
struct MinMaxInfo {
    reserved: Point,
    max_size: Point,
    max_position: Point,
    min_track_size: Point,
    max_track_size: Point,
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

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels_per_meter: i32,
    y_pels_per_meter: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 1],
}

#[repr(C)]
struct TrackMouseEvent {
    size: u32,
    flags: u32,
    hwnd_track: H,
    hover_time: u32,
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
    fn SetCursor(cursor: H) -> H;
    fn GetClientRect(hwnd: H, rect: *mut Rect) -> i32;
    fn GetWindowRect(hwnd: H, rect: *mut Rect) -> i32;
    fn ScreenToClient(hwnd: H, point: *mut Point) -> i32;
    fn ClientToScreen(hwnd: H, point: *mut Point) -> i32;
    fn CreatePopupMenu() -> H;
    fn AppendMenuW(menu: H, flags: u32, id: usize, text: *const u16) -> i32;
    fn TrackPopupMenu(
        menu: H,
        flags: u32,
        x: i32,
        y: i32,
        reserved: i32,
        hwnd: H,
        rect: *const Rect,
    ) -> u32;
    fn DestroyMenu(menu: H) -> i32;
    fn MoveWindow(hwnd: H, x: i32, y: i32, w: i32, h: i32, repaint: i32) -> i32;
    fn InvalidateRect(hwnd: H, rect: *const Rect, erase: i32) -> i32;
    fn BeginPaint(hwnd: H, paint: *mut PaintStruct) -> H;
    fn EndPaint(hwnd: H, paint: *const PaintStruct) -> i32;
    fn SendMessageW(hwnd: H, msg: u32, wp: WParam, lp: LParam) -> LResult;
    fn SetWindowTextW(hwnd: H, text: *const u16) -> i32;
    fn GetWindowTextLengthW(hwnd: H) -> i32;
    fn GetWindowTextW(hwnd: H, text: *mut u16, max: i32) -> i32;
    fn MessageBoxW(hwnd: H, text: *const u16, caption: *const u16, kind: u32) -> i32;
    fn GetFocus() -> H;
    fn SetFocus(hwnd: H) -> H;
    fn GetKeyState(key: i32) -> i16;
    fn IsDialogMessageW(hwnd: H, msg: *mut Msg) -> i32;
    fn DrawTextW(dc: H, value: *const u16, count: i32, rect: *mut Rect, format: u32) -> i32;
    fn LoadImageW(instance: H, name: *const u16, kind: u32, cx: i32, cy: i32, load: u32) -> H;
    fn TrackMouseEvent(event: *mut TrackMouseEvent) -> i32;
    fn OpenClipboard(owner: H) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: H) -> H;
    fn GetClipboardData(format: u32) -> H;
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
    fn DialogBoxIndirectParamW(
        instance: H,
        template: *const u16,
        owner: H,
        proc: Option<unsafe extern "system" fn(H, u32, WParam, LParam) -> isize>,
        data: LParam,
    ) -> isize;
    fn EndDialog(dialog: H, result: isize) -> i32;
}

#[link(name = "comctl32")]
extern "system" {
    fn SetWindowSubclass(
        hwnd: H,
        callback: Option<
            unsafe extern "system" fn(H, u32, WParam, LParam, usize, usize) -> LResult,
        >,
        id: usize,
        data: usize,
    ) -> i32;
    fn DefSubclassProc(hwnd: H, msg: u32, wp: WParam, lp: LParam) -> LResult;
}

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
    fn LoadLibraryW(name: *const u16) -> H;
    fn GetProcAddress(module: H, name: *const u8) -> P;
    fn GetLogicalDrives() -> u32;
    fn FileTimeToLocalFileTime(utc: *const u64, local: *mut u64) -> i32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> H;
    fn GlobalLock(memory: H) -> P;
    fn GlobalUnlock(memory: H) -> i32;
    fn GlobalFree(memory: H) -> H;
    fn GlobalSize(memory: H) -> usize;
}

#[link(name = "gdi32")]
extern "system" {
    fn SetTextColor(dc: H, color: u32) -> u32;
    fn SetBkColor(dc: H, color: u32) -> u32;
    fn SetBkMode(dc: H, mode: i32) -> i32;
    fn CreateSolidBrush(color: u32) -> H;
    fn CreateCompatibleDC(dc: H) -> H;
    fn DeleteDC(dc: H) -> i32;
    fn CreateDIBSection(
        dc: H,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut P,
        section: H,
        offset: u32,
    ) -> H;
    fn BitBlt(dest: H, x: i32, y: i32, w: i32, h: i32, src: H, sx: i32, sy: i32, rop: u32) -> i32;
    fn DeleteObject(object: H) -> i32;
    fn SelectObject(dc: H, object: H) -> H;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strikeout: u32,
        charset: u32,
        output_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch_and_family: u32,
        face: *const u16,
    ) -> H;
}

#[link(name = "shell32")]
extern "system" {
    fn DragQueryFileW(drop: H, index: u32, buffer: *mut u16, count: u32) -> u32;
    fn ShellExecuteExW(info: *mut windows_actions::ShellExecuteInfo) -> i32;
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

const IMAGE_ICON: u32 = 1;
const LR_LOADFROMFILE: u32 = 0x0010;

fn companion_icon() -> H {
    let path = match std::env::current_exe() {
        Ok(exe) => match exe.parent() {
            Some(dir) => dir.join("zfm.ico"),
            None => return ptr::null_mut(),
        },
        Err(_) => return ptr::null_mut(),
    };
    unsafe {
        let name = wide(path.as_os_str());
        LoadImageW(
            ptr::null_mut(),
            name.as_ptr(),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE,
        )
    }
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

unsafe fn child_origin(parent: H, child_hwnd: H) -> (i32, i32) {
    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if GetWindowRect(child_hwnd, &mut r) == 0 {
        return (0, 0);
    }
    let mut pt = Point {
        x: r.left,
        y: r.top,
    };
    if ScreenToClient(parent, &mut pt) == 0 {
        return (0, 0);
    }
    (pt.x, pt.y)
}

unsafe fn blit_pixels<F>(
    dest_dc: H,
    dst_x: i32,
    dst_y: i32,
    width: i32,
    height: i32,
    pixels: &[u32],
    overlay: F,
) where
    F: FnOnce(H),
{
    if width <= 0 || height <= 0 {
        return;
    }
    let count = (width * height) as usize;
    if pixels.len() < count {
        return;
    }
    let mem_dc = CreateCompatibleDC(dest_dc);
    if mem_dc.is_null() {
        return;
    }
    let info = BitmapInfo {
        header: BitmapInfoHeader {
            size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            width,
            height: -height,
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: 0,
            x_pels_per_meter: 0,
            y_pels_per_meter: 0,
            clr_used: 0,
            clr_important: 0,
        },
        colors: [0],
    };
    let mut bits: P = ptr::null_mut();
    let dib = CreateDIBSection(mem_dc, &info, 0, &mut bits, ptr::null_mut(), 0);
    if !dib.is_null() && !bits.is_null() {
        ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, count);
        let old = SelectObject(mem_dc, dib);
        overlay(mem_dc);
        BitBlt(dest_dc, dst_x, dst_y, width, height, mem_dc, 0, 0, SRCCOPY);
        SelectObject(mem_dc, old);
        DeleteObject(dib);
    }
    DeleteDC(mem_dc);
}

unsafe fn apply_dark_window_chrome(hwnd: H) {
    let module = LoadLibraryW(text("dwmapi.dll").as_ptr());
    if module.is_null() {
        return;
    }
    let proc = GetProcAddress(module, b"DwmSetWindowAttribute\0".as_ptr());
    if proc.is_null() {
        return;
    }
    let set_attr: unsafe extern "system" fn(H, u32, *const c_void, u32) -> i32 =
        std::mem::transmute(proc);
    let size = std::mem::size_of::<u32>() as u32;
    let dark: u32 = 1;
    let round: u32 = 2;
    let border = ui_theme::BORDER_SUBTLE.colorref();
    let caption = ui_theme::CAPTION_BG.colorref();
    let title_text = ui_theme::TEXT_BRIGHT.colorref();
    let _ = set_attr(hwnd, 20, &dark as *const u32 as *const c_void, size);
    let _ = set_attr(hwnd, 33, &round as *const u32 as *const c_void, size);
    let _ = set_attr(hwnd, 34, &border as *const u32 as *const c_void, size);
    let _ = set_attr(hwnd, 35, &caption as *const u32 as *const c_void, size);
    let _ = set_attr(hwnd, 36, &title_text as *const u32 as *const c_void, size);
}

unsafe fn apply_dark_scrollbar(hwnd: H) {
    let module = LoadLibraryW(text("uxtheme.dll").as_ptr());
    if module.is_null() {
        return;
    }
    let proc = GetProcAddress(module, b"SetWindowTheme\0".as_ptr());
    if proc.is_null() {
        return;
    }
    let set_theme: unsafe extern "system" fn(H, *const u16, *const u16) -> i32 =
        std::mem::transmute(proc);
    if set_theme(hwnd, text("DarkMode_Explorer").as_ptr(), ptr::null()) != 0 {
        let _ = set_theme(hwnd, text("Explorer").as_ptr(), ptr::null());
    }
}

fn format_local_modified(modified: Option<SystemTime>) -> String {
    const WIN_EPOCH_DIFF_SECS: i64 = 11_644_473_600;
    let Some(ts) = modified else {
        return String::new();
    };
    let unix_secs = match ts.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    };
    let utc_ticks = ((unix_secs + WIN_EPOCH_DIFF_SECS).max(0) as u64) * 10_000_000;
    let mut local_ticks: u64 = utc_ticks;
    unsafe {
        let _ = FileTimeToLocalFileTime(&utc_ticks, &mut local_ticks);
    }
    let local_secs = (local_ticks / 10_000_000) as i64 - WIN_EPOCH_DIFF_SECS;
    ui_theme::format_timestamp_us(local_secs)
}

struct Theme {
    window_brush: H,
    address_brush: H,
    filter_brush: H,
    name_brush: H,
    list_brush: H,
    font: H,
    semibold_font: H,
    small_font: H,
    brand_font: H,
    subtitle_font: H,
    hand_cursor: H,
}

impl Theme {
    unsafe fn new() -> Result<Self, String> {
        let font = |height, weight| {
            CreateFontW(
                height,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                text("Segoe UI").as_ptr(),
            )
        };
        let theme = Self {
            window_brush: CreateSolidBrush(ui_theme::HEADER_TOP.colorref()),
            address_brush: CreateSolidBrush(ui_theme::ADDRESS_EDIT_BG.colorref()),
            filter_brush: CreateSolidBrush(ui_theme::FILTER_EDIT_BG.colorref()),
            name_brush: CreateSolidBrush(ui_theme::NAME_EDIT_BG.colorref()),
            list_brush: CreateSolidBrush(ui_theme::ROW_EVEN.colorref()),
            font: font(-13, 400),
            semibold_font: font(-13, 600),
            small_font: font(-12, 400),
            brand_font: font(-25, 700),
            subtitle_font: font(-13, 600),
            hand_cursor: LoadCursorW(ptr::null_mut(), 32649usize as *const u16),
        };
        if [
            theme.window_brush,
            theme.address_brush,
            theme.filter_brush,
            theme.name_brush,
            theme.list_brush,
            theme.font,
            theme.semibold_font,
            theme.small_font,
            theme.brand_font,
            theme.subtitle_font,
        ]
        .iter()
        .any(|h| h.is_null())
        {
            return Err("Could not initialize the interface resources".into());
        }
        Ok(theme)
    }
}

impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            for obj in [
                self.window_brush,
                self.address_brush,
                self.filter_brush,
                self.name_brush,
                self.list_brush,
                self.font,
                self.semibold_font,
                self.small_font,
                self.brand_font,
                self.subtitle_font,
            ] {
                if !obj.is_null() {
                    DeleteObject(obj);
                }
            }
        }
    }
}

#[derive(Clone)]
struct SidebarEntry {
    label: String,
    icon: ItemIcon,
    indent: i32,
    target: Option<PathBuf>,
    is_drives_header: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HotZone {
    None,
    AddressChip,
    Tab(usize),
    Sidebar(usize),
    Column(SortColumn),
    View(ViewMode),
}

struct App {
    theme: *const Theme,
    tabs: Tabs,
    filter: String,
    show_hidden: bool,
    drives_expanded: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    view_mode: ViewMode,
    focused_edit: usize,
    hovered_child: H,
    hovered_row: Option<usize>,
    hot_zone: HotZone,
    tracking_mouse: bool,
    operation_busy: bool,
    notice: Option<String>,
    token: Arc<AtomicU64>,
    address: H,
    filter_control: H,
    name_control: H,
    list: H,
    status: H,
    nav_buttons: [H; 6],
    extra_buttons: [H; 9],
    sidebar_items: Vec<SidebarEntry>,
    entries: Vec<Entry>,
    backdrop_w: i32,
    backdrop_h: i32,
    backdrop: Vec<u32>,
}

struct OperationMessage {
    result: Result<String, String>,
}

struct ResultMessage {
    generation: u64,
    destination: Destination,
    result: std::io::Result<Option<Listing>>,
}

fn build_sidebar_items(drives_expanded: bool) -> Vec<SidebarEntry> {
    let mut items = Vec::new();
    let profile = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    if let Some(base) = profile {
        for (heading, icon, folders) in [
            ("Favorites", ItemIcon::Favorites, vec![
                ("Desktop", "Desktop", ItemIcon::Desktop),
                ("Downloads", "Downloads", ItemIcon::Downloads),
            ]),
            ("Libraries", ItemIcon::Documents, vec![
                ("Documents", "Documents", ItemIcon::Documents),
                ("Music", "Music", ItemIcon::Music),
                ("Pictures", "Pictures", ItemIcon::Folder),
                ("Videos", "Videos", ItemIcon::Videos),
                ("Home", "", ItemIcon::Folder),
            ]),
        ] {
            items.push(SidebarEntry {
                label: heading.to_string(), icon, indent: 0, target: None,
                is_drives_header: false,
            });
            for (label, folder, icon) in folders {
                items.push(SidebarEntry {
                    label: label.to_string(), icon, indent: 1,
                    target: Some(base.join(folder)), is_drives_header: false,
                });
            }
        }
    }
    items.push(SidebarEntry {
        label: "Computer".to_string(),
        icon: ItemIcon::Drives,
        indent: 0,
        target: None,
        is_drives_header: true,
    });
    if drives_expanded {
        let mask = unsafe { GetLogicalDrives() };
        for bit in 0..26u32 {
            if (mask & (1 << bit)) != 0 {
                let letter = (b'A' + bit as u8) as char;
                let root = format!("{letter}:\\");
                items.push(SidebarEntry {
                    label: root.clone(),
                    icon: ItemIcon::DriveDisk,
                    indent: 1,
                    target: Some(PathBuf::from(root)),
                    is_drives_header: false,
                });
            }
        }
    }
    items
}

fn root_of(path: &Path) -> PathBuf {
    let mut components = path.components();
    let mut root = PathBuf::new();
    if let Some(first) = components.next() {
        root.push(first.as_os_str());
        if let Some(second) = components.next() {
            if matches!(second, std::path::Component::RootDir) {
                root.push(second.as_os_str());
            }
        }
    }
    if root.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        root
    }
}

fn tab_title(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn dynamic_tab_rects(metrics: &WindowMetrics, tab_count: usize) -> Vec<BoxRect> {
    if tab_count == 0 {
        return Vec::new();
    }
    let avail = (metrics.dynamic_tabs_right - metrics.dynamic_tabs_left).max(0);
    if avail < 64 {
        return Vec::new();
    }
    let gap = 4;
    let per = ((avail - gap * (tab_count as i32 - 1).max(0)) / tab_count as i32).clamp(64, 136);
    let mut rects = Vec::new();
    let mut x = metrics.dynamic_tabs_left;
    for _ in 0..tab_count {
        if x + 56 > metrics.dynamic_tabs_right {
            break;
        }
        let w = per.min(metrics.dynamic_tabs_right - x);
        rects.push(BoxRect::new(
            x,
            metrics.new_tab.y,
            w,
            metrics.new_tab.height,
        ));
        x += w + gap;
    }
    rects
}

fn sidebar_row_rect(metrics: &WindowMetrics, index: usize) -> Option<BoxRect> {
    let row_h = 22;
    let y = metrics.sidebar.y + 5 + (index as i32) * row_h;
    if y + row_h > metrics.sidebar.bottom() {
        return None;
    }
    Some(BoxRect::new(
        metrics.sidebar.x + 6,
        y,
        (metrics.sidebar.width - 12).max(20),
        row_h - 2,
    ))
}

fn hit_test_custom(app: &App, metrics: &WindowMetrics, x: i32, y: i32) -> HotZone {
    if metrics.address_chip.contains(x, y) {
        return HotZone::AddressChip;
    }
    if metrics.view_details_btn.contains(x, y) {
        return HotZone::View(ViewMode::Details);
    }
    if metrics.view_compact_btn.contains(x, y) {
        return HotZone::View(ViewMode::Compact);
    }
    if metrics.col_name.contains(x, y) {
        return HotZone::Column(SortColumn::Name);
    }
    if metrics.col_modified.contains(x, y) {
        return HotZone::Column(SortColumn::Modified);
    }
    if metrics.col_type.contains(x, y) {
        return HotZone::Column(SortColumn::Kind);
    }
    if metrics.col_size.contains(x, y) {
        return HotZone::Column(SortColumn::Size);
    }
    for (idx, rect) in dynamic_tab_rects(metrics, app.tabs.len())
        .into_iter()
        .enumerate()
    {
        if rect.contains(x, y) {
            return HotZone::Tab(idx);
        }
    }
    if metrics.sidebar.contains(x, y) {
        for idx in 0..app.sidebar_items.len() {
            if let Some(rect) = sidebar_row_rect(metrics, idx) {
                let item = &app.sidebar_items[idx];
                if rect.contains(x, y) && (item.target.is_some() || item.is_drives_header) {
                    return HotZone::Sidebar(idx);
                }
            }
        }
    }
    HotZone::None
}

unsafe fn rebuild_backdrop(hwnd: H, app: &mut App) {
    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut r);
    let w = r.right.max(1);
    let h = r.bottom.max(1);
    let m = WindowMetrics::new(w, h);
    let mut buf = vec![ui_theme::HEADER_TOP.pixel(); (w * h) as usize];

    // 1. Top branding + navigation bar
    ui_theme::fill_vgrad(
        &mut buf,
        w,
        h,
        BoxRect::new(0, 0, w, m.tab_strip_top),
        ui_theme::HEADER_TOP,
        ui_theme::HEADER_BOTTOM,
    );

    // Unified address bar with left breadcrumb chip and angled `>` chevron
    let addr_border = if app.focused_edit == ADDRESS {
        ui_theme::BORDER_FOCUS
    } else {
        ui_theme::BORDER_SUBTLE
    };
    ui_theme::paint_rounded_box(
        &mut buf,
        w,
        h,
        m.address_frame,
        6.0,
        ui_theme::ADDRESS_EDIT_BG,
        ui_theme::ADDRESS_EDIT_BG,
        Some(addr_border),
    );
    let chip_bg = if app.hot_zone == HotZone::AddressChip {
        Rgb(50, 59, 68)
    } else {
        ui_theme::ADDRESS_CHIP_BG
    };
    ui_theme::paint_rounded_box(
        &mut buf,
        w,
        h,
        BoxRect::new(
            m.address_chip.x + 1,
            m.address_chip.y + 1,
            m.address_chip.width - 2,
            m.address_chip.height - 2,
        ),
        5.0,
        chip_bg,
        chip_bg,
        None,
    );
    ui_theme::paint_item_icon(
        &mut buf,
        w,
        h,
        m.address_chip.x + 10,
        m.address_chip.y + (m.address_chip.height - 18) / 2,
        ItemIcon::Folder,
    );
    let chev_x = (m.address_chip.right() - 6) as f32;
    let chev_y = (m.address_chip.y + m.address_chip.height / 2) as f32;
    let half_h = (m.address_chip.height / 2 - 2) as f32;
    ui_theme::draw_segment_aa(
        &mut buf,
        w,
        h,
        chev_x - 5.0,
        chev_y - half_h,
        chev_x + 2.0,
        chev_y,
        1.4,
        ui_theme::BORDER_STRONG,
    );
    ui_theme::draw_segment_aa(
        &mut buf,
        w,
        h,
        chev_x + 2.0,
        chev_y,
        chev_x - 5.0,
        chev_y + half_h,
        1.4,
        ui_theme::BORDER_STRONG,
    );

    // 2. Browser-style tab strip + toolbar deck
    ui_theme::fill_rect(
        &mut buf,
        w,
        h,
        BoxRect::new(0, m.tab_strip_top, w, m.deck_top - m.tab_strip_top),
        ui_theme::TAB_STRIP_BG,
    );
    ui_theme::fill_vgrad(
        &mut buf,
        w,
        h,
        BoxRect::new(0, m.deck_top, w, m.split_top - m.deck_top),
        ui_theme::DECK_TOP,
        ui_theme::DECK_BOTTOM,
    );
    ui_theme::fill_rect(
        &mut buf,
        w,
        h,
        BoxRect::new(0, m.deck_top - 1, w, 1),
        ui_theme::BORDER_STRONG,
    );
    ui_theme::fill_rect(
        &mut buf,
        w,
        h,
        BoxRect::new(0, m.split_top - 1, w, 1),
        ui_theme::BORDER_SUBTLE,
    );

    // Open folder tab chips when multiple tabs exist
    if app.tabs.len() > 1 {
        for (idx, rect) in dynamic_tab_rects(&m, app.tabs.len())
            .into_iter()
            .enumerate()
        {
            let active = idx == app.tabs.active();
            let hovered = app.hot_zone == HotZone::Tab(idx);
            let fill = if active {
                ui_theme::DECK_TOP
            } else if hovered {
                Rgb(38, 44, 52)
            } else {
                ui_theme::TAB_INACTIVE_BG
            };
            ui_theme::paint_top_tab(
                &mut buf,
                w,
                h,
                rect,
                6.5,
                fill,
                fill,
                if active {
                    ui_theme::BORDER_STRONG
                } else {
                    ui_theme::BORDER_SUBTLE
                },
                active,
            );
            if active {
                ui_theme::fill_rect(
                    &mut buf,
                    w,
                    h,
                    BoxRect::new(rect.x + 1, m.deck_top - 1, rect.width - 2, 2),
                    ui_theme::DECK_TOP,
                );
            }
            ui_theme::paint_item_icon(&mut buf, w, h, rect.x + 8, rect.y + (rect.height - 18) / 2, ItemIcon::Folder);
        }
    }

    // 3. Filter & New-name rounded input frames
    let filter_border = if app.focused_edit == FILTER {
        ui_theme::BORDER_FOCUS
    } else {
        ui_theme::BORDER_STRONG
    };
    ui_theme::paint_rounded_box(
        &mut buf,
        w,
        h,
        m.filter_frame,
        6.0,
        ui_theme::FILTER_EDIT_BG,
        ui_theme::FILTER_EDIT_BG,
        Some(filter_border),
    );
    ui_theme::paint_action_icon(
        &mut buf,
        w,
        h,
        (m.filter_frame.right() - 15) as f32,
        (m.filter_frame.y + m.filter_frame.height / 2) as f32,
        ActionIcon::Search,
        ui_theme::TEXT_MUTED,
    );

    let name_border = if app.focused_edit == NAME {
        ui_theme::BORDER_FOCUS
    } else {
        ui_theme::BORDER_SUBTLE
    };
    ui_theme::paint_rounded_box(
        &mut buf,
        w,
        h,
        m.name_frame,
        6.0,
        ui_theme::NAME_EDIT_BG,
        ui_theme::NAME_EDIT_BG,
        Some(name_border),
    );

    // 4. Sidebar & metallic splitter
    ui_theme::fill_rect(&mut buf, w, h, m.sidebar, ui_theme::SIDEBAR_BG);
    ui_theme::fill_rect(
        &mut buf,
        w,
        h,
        BoxRect::new(m.splitter_x, m.split_top, 1, m.sidebar.height),
        ui_theme::SPLITTER_DARK,
    );
    ui_theme::fill_rect(
        &mut buf,
        w,
        h,
        BoxRect::new(m.splitter_x + 1, m.split_top, 2, m.sidebar.height),
        ui_theme::SPLITTER_LIGHT,
    );
    ui_theme::fill_rect(
        &mut buf,
        w,
        h,
        BoxRect::new(m.splitter_x + 3, m.split_top, 1, m.sidebar.height),
        ui_theme::SPLITTER_DARK,
    );

    let current_dir = app.tabs.current().current();
    for (idx, item) in app.sidebar_items.iter().enumerate() {
        let Some(row_rect) = sidebar_row_rect(&m, idx) else {
            break;
        };
        let is_current = item.target.as_deref().map_or(false, |t| t == current_dir);
        let is_hovered = app.hot_zone == HotZone::Sidebar(idx);
        if is_current || is_hovered {
            let fill = if is_current {
                ui_theme::SIDEBAR_ACTIVE
            } else {
                ui_theme::SIDEBAR_HOVER
            };
            ui_theme::paint_rounded_box(&mut buf, w, h, row_rect, 5.0, fill, fill, None);
        }
        let base_x = row_rect.x + 10 + item.indent * 16;
        let cy = (row_rect.y + row_rect.height / 2) as f32;
        if item.is_drives_header {
            ui_theme::paint_chevron(
                &mut buf, w, h, base_x as f32, cy,
                app.drives_expanded, ui_theme::TEXT_MUTED,
            );
        }
        ui_theme::paint_item_icon(&mut buf, w, h, base_x + 10, row_rect.y + (row_rect.height - 18) / 2, item.icon);
    }

    // 5. Column Header Bar
    ui_theme::fill_rect(&mut buf, w, h, m.col_header, ui_theme::COL_HEADER_BG);
    for (col, rect) in [
        (SortColumn::Name, m.col_name),
        (SortColumn::Modified, m.col_modified),
        (SortColumn::Kind, m.col_type),
        (SortColumn::Size, m.col_size),
    ] {
        if app.hot_zone == HotZone::Column(col) {
            ui_theme::fill_rect(
                &mut buf,
                w,
                h,
                BoxRect::new(rect.x, rect.y, rect.width, rect.height - 1),
                ui_theme::COL_HEADER_HOVER,
            );
        }
        if col != SortColumn::Name {
            ui_theme::fill_rect(
                &mut buf,
                w,
                h,
                BoxRect::new(rect.x, rect.y + 7, 1, rect.height - 14),
                ui_theme::BORDER_SUBTLE,
            );
        }
    }
    ui_theme::fill_rect(
        &mut buf,
        w,
        h,
        BoxRect::new(
            m.col_header.x,
            m.col_header.bottom() - 1,
            m.col_header.width,
            1,
        ),
        ui_theme::BORDER_SUBTLE,
    );
    // Fill list area background behind child listbox
    ui_theme::fill_rect(&mut buf, w, h, m.list, ui_theme::ROW_EVEN);

    // 6. Segmented Bottom Status Bar
    ui_theme::fill_rect(&mut buf, w, h, m.status_bar, ui_theme::STATUS_BG);
    ui_theme::fill_rect(
        &mut buf,
        w,
        h,
        BoxRect::new(0, m.status_bar.y, w, 1),
        ui_theme::BORDER_SUBTLE,
    );
    ui_theme::paint_view_icon(
        &mut buf,
        w,
        h,
        m.view_details_btn,
        ViewMode::Details,
        app.view_mode == ViewMode::Details,
        app.hot_zone == HotZone::View(ViewMode::Details),
    );
    ui_theme::paint_view_icon(
        &mut buf,
        w,
        h,
        m.view_compact_btn,
        ViewMode::Compact,
        app.view_mode == ViewMode::Compact,
        app.hot_zone == HotZone::View(ViewMode::Compact),
    );

    app.backdrop_w = w;
    app.backdrop_h = h;
    app.backdrop = buf;
}

fn slice_backdrop(app: &App, ox: i32, oy: i32, width: i32, height: i32) -> Vec<u32> {
    let w = width.max(1);
    let h = height.max(1);
    let mut out = vec![ui_theme::DECK_TOP.pixel(); (w * h) as usize];
    if app.backdrop_w <= 0 || app.backdrop_h <= 0 || app.backdrop.is_empty() {
        return out;
    }
    for y in 0..h {
        let sy = (oy + y).clamp(0, app.backdrop_h - 1);
        for x in 0..w {
            let sx = (ox + x).clamp(0, app.backdrop_w - 1);
            out[(y * w + x) as usize] = app.backdrop[(sy * app.backdrop_w + sx) as usize];
        }
    }
    out
}

unsafe fn draw_text_in(dc: H, font: H, color: Rgb, rect: BoxRect, value: &str, flags: u32) {
    if rect.width <= 0 || rect.height <= 0 || value.is_empty() {
        return;
    }
    let wide_str = text(value);
    let old = SelectObject(dc, font);
    SetBkMode(dc, 1);
    SetTextColor(dc, color.colorref());
    let mut bounds = Rect {
        left: rect.x,
        top: rect.y,
        right: rect.right(),
        bottom: rect.bottom(),
    };
    DrawTextW(dc, wide_str.as_ptr(), -1, &mut bounds, flags);
    SelectObject(dc, old);
}

unsafe fn paint_window(hwnd: H, app: &mut App) {
    let mut ps: PaintStruct = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    if app.backdrop.is_empty() {
        rebuild_backdrop(hwnd, app);
    }
    let w = app.backdrop_w;
    let h = app.backdrop_h;
    let m = WindowMetrics::new(w, h);
    let theme = &*app.theme;
    let current_path = app.tabs.current().current().display().to_string();
    let selected = selected_indices(app);
    let selected_count = selected.len();

    // Sort indicator coordinates in the column header
    let mut pixels = app.backdrop.clone();
    let arrow_col_rect = match app.sort_column {
        SortColumn::Name => (m.col_name, 58),
        SortColumn::Modified => (m.col_modified, 45),
        SortColumn::Kind => (m.col_type, 46),
        SortColumn::Size => (m.col_size, 42),
    };
    ui_theme::paint_sort_arrow(
        &mut pixels,
        w,
        h,
        (arrow_col_rect.0.x + arrow_col_rect.1) as f32,
        (arrow_col_rect.0.y + arrow_col_rect.0.height / 2) as f32,
        app.sort_ascending,
        ui_theme::TEXT_MUTED,
    );

    let status_icon = if selected_count == 1 {
        app.entries.get(selected[0]).map(ui_theme::classify_entry_icon).unwrap_or(ItemIcon::Folder)
    } else {
        ItemIcon::Folder
    };
    // Enlarge the selected-item glyph for the details strip without allocating
    // any native image handles or scaling the rest of the interface.
    let mut status_glyph = vec![ui_theme::STATUS_BG.pixel(); 18 * 18];
    ui_theme::paint_item_icon(&mut status_glyph, 18, 18, 0, 0, status_icon);
    for y in 0..36 {
        for x in 0..36 {
            let px = 10 + x;
            let py = m.status_bar.y + 6 + y;
            if px < w && py < h {
                pixels[(py * w + px) as usize] = status_glyph[((y / 2) * 18 + x / 2) as usize];
            }
        }
    }

    blit_pixels(dc, 0, 0, w, h, &pixels, |mem_dc| {
        // Brand lockup
        draw_text_in(
            mem_dc,
            theme.brand_font,
            ui_theme::TEXT_BRIGHT,
            m.brand_title,
            "ZFM",
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        draw_text_in(
            mem_dc,
            theme.subtitle_font,
            ui_theme::TEXT_MUTED,
            m.brand_subtitle,
            "FILE MANAGER",
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        // The editable path occupies the rest of the address bar; the folder
        // chip itself can be clicked to focus that field.

        // Open tab labels
        if app.tabs.len() > 1 {
            for (idx, rect) in dynamic_tab_rects(&m, app.tabs.len())
                .into_iter()
                .enumerate()
            {
                if let Some(hist) = app.tabs.history(idx) {
                    let title = tab_title(hist.current());
                    let active = idx == app.tabs.active();
                    draw_text_in(
                        mem_dc,
                        if active {
                            theme.semibold_font
                        } else {
                            theme.small_font
                        },
                        if active {
                            ui_theme::TEXT_BRIGHT
                        } else {
                            ui_theme::TEXT_MUTED
                        },
                        BoxRect::new(rect.x + 30, rect.y + 1, rect.width - 36, rect.height - 2),
                        &title,
                        DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                    );
                }
            }
        }

        // Row labels
        draw_text_in(
            mem_dc,
            theme.font,
            ui_theme::TEXT_PRIMARY,
            m.filter_label,
            "Filter:",
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        draw_text_in(
            mem_dc,
            theme.font,
            ui_theme::TEXT_PRIMARY,
            m.name_label,
            "New name:",
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        // Sidebar labels
        for (idx, item) in app.sidebar_items.iter().enumerate() {
            let Some(row_rect) = sidebar_row_rect(&m, idx) else {
                break;
            };
            let base_x = row_rect.x + 10 + item.indent * 16 + 34;
            draw_text_in(
                mem_dc,
                if item.target.is_none() { theme.semibold_font } else { theme.font },
                ui_theme::TEXT_BRIGHT,
                BoxRect::new(
                    base_x,
                    row_rect.y,
                    (row_rect.right() - base_x - 4).max(10),
                    row_rect.height,
                ),
                &item.label,
                DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
            );
        }

        // Column header titles
        for (rect, pad_left, title, right_align) in [
            (m.col_name, 16, "Name", false),
            (m.col_modified, 10, "Date", false),
            (m.col_type, 10, "Type", false),
            (m.col_size, 10, "Size", true),
        ] {
            let flags =
                DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | if right_align { DT_RIGHT } else { 0 };
            draw_text_in(
                mem_dc,
                theme.semibold_font,
                ui_theme::TEXT_PRIMARY,
                BoxRect::new(
                    rect.x + pad_left,
                    rect.y,
                    rect.width - pad_left - 8,
                    rect.height,
                ),
                title,
                flags,
            );
        }

        // Explorer-style details strip: the current selection is prominent,
        // while operational notices and the item count remain visible.
        let sy = m.status_bar.y;
        let selected_entry = if selected_count == 1 {
            app.entries.get(selected[0])
        } else {
            None
        };
        let title = selected_entry
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| tab_title(app.tabs.current().current()));
        let detail = selected_entry
            .map(|entry| format!("{}   ·   {}   ·   {}", ui_theme::entry_type_label(entry),
                format_local_modified(entry.modified), entry.path.display()))
            .unwrap_or_else(|| format!("{} items   ·   {} selected   ·   {}", app.entries.len(), selected_count, current_path));
        draw_text_in(
            mem_dc, theme.semibold_font, ui_theme::TEXT_BRIGHT,
            BoxRect::new(56, sy + 5, (w - 132).max(20), 19), &title,
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        let status_detail = app.notice.as_deref().unwrap_or(&detail);
        draw_text_in(
            mem_dc, theme.small_font, ui_theme::TEXT_PRIMARY,
            BoxRect::new(56, sy + 24, (w - 132).max(20), 18), status_detail,
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
    });
    EndPaint(hwnd, &ps);
}

unsafe fn draw_button(parent: H, app: &App, item: &mut DrawItem) -> LResult {
    let w = (item.rect.right - item.rect.left).max(1);
    let h = (item.rect.bottom - item.rect.top).max(1);
    let (ox, oy) = child_origin(parent, item.hwnd);
    let mut pixels = slice_backdrop(app, ox, oy, w, h);
    let id = item.control_id as usize;
    let pressed = item.item_state & 1 != 0;
    let disabled = item.item_state & 4 != 0;
    let focused = item.item_state & 0x10 != 0;
    let hovered = !disabled && app.hovered_child == item.hwnd;
    let theme = &*app.theme;

    // 1. Show hidden toggle pill
    if id == HIDDEN {
        ui_theme::paint_toggle_switch(
            &mut pixels,
            w,
            h,
            w - 36,
            (h - 16) / 2,
            app.show_hidden,
            hovered,
        );
        blit_pixels(
            item.dc,
            item.rect.left,
            item.rect.top,
            w,
            h,
            &pixels,
            |mem_dc| {
                draw_text_in(
                    mem_dc,
                    theme.font,
                    if hovered {
                        ui_theme::TEXT_BRIGHT
                    } else {
                        ui_theme::TEXT_PRIMARY
                    },
                    BoxRect::new(2, 0, (w - 42).max(10), h),
                    "Show hidden",
                    DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            },
        );
        return 1;
    }

    // 2. Browser-style New tab / Close tab buttons
    if id == NEW_TAB || id == CLOSE_TAB {
        let is_new = id == NEW_TAB;
        let active_look = is_new && app.tabs.len() == 1;
        let fill_top = if pressed {
            ui_theme::BTN_PRESS_TOP
        } else if hovered {
            Rgb(56, 63, 72)
        } else if active_look {
            ui_theme::TAB_ACTIVE_BG
        } else {
            ui_theme::TAB_INACTIVE_BG
        };
        let fill_bot = if active_look && !pressed {
            ui_theme::DECK_TOP
        } else {
            fill_top
        };
        let border = if focused {
            ui_theme::BORDER_FOCUS
        } else if active_look || hovered {
            ui_theme::BORDER_STRONG
        } else {
            ui_theme::BORDER_SUBTLE
        };
        ui_theme::paint_top_tab(
            &mut pixels,
            w,
            h,
            BoxRect::new(0, 0, w, h),
            6.5,
            fill_top,
            fill_bot,
            border,
            active_look,
        );
        let icon = if is_new {
            ActionIcon::NewTab
        } else {
            ActionIcon::CloseTab
        };
        let ink = if active_look || hovered {
            ui_theme::TEXT_BRIGHT
        } else {
            ui_theme::TEXT_MUTED
        };
        ui_theme::paint_action_icon(&mut pixels, w, h, 18.0, (h / 2) as f32, icon, ink);
        let caption = if is_new { "New tab" } else { "Close tab" };
        blit_pixels(
            item.dc,
            item.rect.left,
            item.rect.top,
            w,
            h,
            &pixels,
            |mem_dc| {
                draw_text_in(
                    mem_dc,
                    if active_look {
                        theme.semibold_font
                    } else {
                        theme.font
                    },
                    ink,
                    BoxRect::new(30, 0, w - 34, h),
                    caption,
                    DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            },
        );
        return 1;
    }

    // 3. Standard dark rounded toolbar & navigation buttons
    let (top_c, bot_c) = if disabled {
        (Rgb(38, 42, 48), Rgb(34, 38, 44))
    } else if pressed {
        (ui_theme::BTN_PRESS_TOP, ui_theme::BTN_PRESS_BOTTOM)
    } else if hovered {
        (ui_theme::BTN_HOVER_TOP, ui_theme::BTN_HOVER_BOTTOM)
    } else {
        (ui_theme::BTN_TOP, ui_theme::BTN_BOTTOM)
    };
    let stroke = if focused {
        ui_theme::BORDER_FOCUS
    } else if hovered {
        Rgb(102, 112, 124)
    } else {
        ui_theme::BORDER_STRONG
    };
    ui_theme::paint_rounded_box(
        &mut pixels,
        w,
        h,
        BoxRect::new(0, 0, w, h),
        6.0,
        top_c,
        bot_c,
        Some(stroke),
    );

    let shift = if pressed && !disabled { 1 } else { 0 };
    let ink = if disabled {
        ui_theme::TEXT_DIM
    } else if hovered {
        ui_theme::TEXT_BRIGHT
    } else {
        ui_theme::TEXT_PRIMARY
    };

    let icon_only = match id {
        BACK => Some(ActionIcon::Back),
        FORWARD => Some(ActionIcon::Forward),
        UP => Some(ActionIcon::Up),
        ROOT => Some(ActionIcon::Root),
        REFRESH => Some(ActionIcon::Refresh),
        _ => None,
    };
    if let Some(icon) = icon_only {
        ui_theme::paint_action_icon(
            &mut pixels,
            w,
            h,
            (w as f32) * 0.5,
            (h as f32) * 0.5 + shift as f32,
            icon,
            ink,
        );
        blit_pixels(
            item.dc,
            item.rect.left,
            item.rect.top,
            w,
            h,
            &pixels,
            |_| {},
        );
        return 1;
    }

    let leading_icon = match id {
        GO => Some((ActionIcon::Go, "Go")),
        APPLY_FILTER => None,
        NEW_FILE => Some((ActionIcon::NewFile, "New file")),
        NEW_FOLDER => Some((ActionIcon::NewFolder, "New folder")),
        RENAME => Some((ActionIcon::Rename, "Rename")),
        DELETE => Some((ActionIcon::Delete, "Delete...")),
        _ => None,
    };
    if let Some((icon, caption)) = leading_icon {
        ui_theme::paint_action_icon(
            &mut pixels,
            w,
            h,
            16.0,
            (h as f32) * 0.5 + shift as f32,
            icon,
            ink,
        );
        blit_pixels(
            item.dc,
            item.rect.left,
            item.rect.top,
            w,
            h,
            &pixels,
            |mem_dc| {
                draw_text_in(
                    mem_dc,
                    theme.font,
                    ink,
                    BoxRect::new(26, shift, w - 30, h),
                    caption,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            },
        );
        return 1;
    }

    let raw_label = control_value(item.hwnd).to_string_lossy().into_owned();
    blit_pixels(
        item.dc,
        item.rect.left,
        item.rect.top,
        w,
        h,
        &pixels,
        |mem_dc| {
            draw_text_in(
                mem_dc,
                theme.font,
                ink,
                BoxRect::new(4, shift, w - 8, h),
                &raw_label,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        },
    );
    1
}

unsafe fn draw_list_item(parent: H, app: &App, item: &mut DrawItem) -> LResult {
    if item.item_id == u32::MAX {
        return 1;
    }
    let w = (item.rect.right - item.rect.left).max(1);
    let h = (item.rect.bottom - item.rect.top).max(1);
    let row_index = item.item_id as usize;
    let selected = item.item_state & 1 != 0;
    let base_bg = if !selected && app.hovered_row == Some(row_index) {
        ui_theme::SIDEBAR_HOVER
    } else if row_index % 2 == 0 {
        ui_theme::ROW_EVEN
    } else {
        ui_theme::ROW_ODD
    };
    let mut pixels = vec![base_bg.pixel(); (w * h) as usize];
    ui_theme::fill_rect(
        &mut pixels,
        w,
        h,
        BoxRect::new(0, h - 1, w, 1),
        ui_theme::ROW_LINE,
    );

    if selected {
        ui_theme::paint_rounded_box(
            &mut pixels,
            w,
            h,
            BoxRect::new(6, 1, (w - 12).max(8), (h - 2).max(4)),
            5.0,
            ui_theme::ROW_SELECTED_BG,
            ui_theme::ROW_SELECTED_BG,
            Some(ui_theme::ROW_SELECTED_BORDER),
        );
    }

    let Some(entry) = app.entries.get(row_index) else {
        blit_pixels(
            item.dc,
            item.rect.left,
            item.rect.top,
            w,
            h,
            &pixels,
            |_| {},
        );
        return 1;
    };

    let icon = if entry.is_dir { ItemIcon::Folder } else { ui_theme::classify_entry_icon(entry) };
    ui_theme::paint_item_icon(&mut pixels, w, h, 12, (h - 18) / 2, icon);

    let mut client = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(parent, &mut client);
    let m = WindowMetrics::new(client.right, client.bottom);
    let base_x = m.col_header.x;
    let name_end = (m.col_name.right() - base_x).min(w);
    let mod_x = (m.col_modified.x - base_x).max(44);
    let mod_w = (m.col_modified.width).max(20);
    let type_x = (m.col_type.x - base_x).max(mod_x + 20);
    let type_w = (m.col_type.width).max(20);
    let size_x = (m.col_size.x - base_x).max(type_x + 20);
    let size_w = (m.col_size.width).min((w - size_x).max(0));

    let modified_text = format_local_modified(entry.modified);
    let type_text = ui_theme::entry_type_label(entry);
    let size_text = ui_theme::format_human_size(entry);
    let theme = &*app.theme;

    blit_pixels(
        item.dc,
        item.rect.left,
        item.rect.top,
        w,
        h,
        &pixels,
        |mem_dc| {
            if app.view_mode == ViewMode::Compact {
                draw_text_in(
                    mem_dc,
                    theme.semibold_font,
                    ui_theme::TEXT_BRIGHT,
                    BoxRect::new(44, 2, (w - 56).max(20), h / 2),
                    &entry.name,
                    DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                );
                let sub = if size_text.is_empty() {
                    format!("{type_text}  ·  {modified_text}")
                } else {
                    format!("{type_text}  ·  {size_text}  ·  {modified_text}")
                };
                draw_text_in(
                    mem_dc,
                    theme.small_font,
                    ui_theme::TEXT_MUTED,
                    BoxRect::new(44, h / 2, (w - 56).max(20), h / 2 - 2),
                    &sub,
                    DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                );
            } else {
                draw_text_in(
                    mem_dc,
                    theme.font,
                    ui_theme::TEXT_BRIGHT,
                    BoxRect::new(44, 0, (name_end - 52).max(20), h),
                    &entry.name,
                    DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                );
                draw_text_in(
                    mem_dc,
                    theme.font,
                    ui_theme::TEXT_PRIMARY,
                    BoxRect::new(mod_x + 10, 0, (mod_w - 16).max(10), h),
                    &modified_text,
                    DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                );
                draw_text_in(
                    mem_dc,
                    theme.font,
                    ui_theme::TEXT_MUTED,
                    BoxRect::new(type_x + 10, 0, (type_w - 16).max(10), h),
                    &type_text,
                    DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                );
                if !size_text.is_empty() && size_w > 12 {
                    draw_text_in(
                        mem_dc,
                        theme.font,
                        ui_theme::TEXT_MUTED,
                        BoxRect::new(size_x + 4, 0, size_w - 10, h),
                        &size_text,
                        DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                    );
                }
            }
        },
    );
    1
}

unsafe fn control_value(control: H) -> OsString {
    let length = GetWindowTextLengthW(control).max(0) as usize;
    let mut value = vec![0u16; length + 1];
    let n = GetWindowTextW(control, value.as_mut_ptr(), (length + 1) as i32).max(0) as usize;
    OsString::from_wide(&value[..n])
}

unsafe fn set_status(hwnd: H, app: &mut App, notice: Option<String>) {
    let current = app.tabs.current().current().display().to_string();
    let summary = match &notice {
        Some(msg) => msg.clone(),
        None => format!("{} items  |  {}", app.entries.len(), current),
    };
    app.notice = notice;
    SetWindowTextW(app.status, text(&summary).as_ptr());
    invalidate_status_bar(hwnd);
}

unsafe fn invalidate_status_bar(hwnd: H) {
    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut r);
    let m = WindowMetrics::new(r.right, r.bottom);
    let bar = Rect {
        left: 0,
        top: m.status_bar.y,
        right: r.right,
        bottom: r.bottom,
    };
    InvalidateRect(hwnd, &bar, 0);
}

unsafe fn selected_indices(app: &App) -> Vec<usize> {
    let count = SendMessageW(app.list, LB_GETSELCOUNT, 0, 0);
    if count <= 0 {
        return Vec::new();
    }
    let mut indices = vec![0i32; count as usize];
    let n = SendMessageW(
        app.list,
        LB_GETSELITEMS,
        indices.len(),
        indices.as_mut_ptr() as isize,
    );
    if n < 0 {
        return Vec::new();
    }
    indices.truncate(n as usize);
    indices
        .into_iter()
        .filter(|i| *i >= 0)
        .map(|i| i as usize)
        .filter(|i| *i < app.entries.len())
        .collect()
}
unsafe fn selected_path(hwnd: H, app: &mut App) -> Option<PathBuf> {
    let selected = selected_indices(app);
    if selected.len() != 1 {
        set_status(
            hwnd,
            app,
            Some("Select exactly one item for this action.".into()),
        );
        None
    } else {
        Some(app.entries[selected[0]].path.clone())
    }
}
unsafe fn clear_selection(list: H) {
    SendMessageW(list, LB_SETSEL, 0, -1);
}
unsafe fn select_one(list: H, index: usize) {
    clear_selection(list);
    SendMessageW(list, LB_SETSEL, 1, index as isize);
    SendMessageW(list, LB_SETCARETINDEX, index, 0);
}

unsafe fn populate_listbox(app: &App, preserve_path: Option<&Path>) {
    SendMessageW(app.list, LB_RESETCONTENT, 0, 0);
    let mut select_idx: Option<usize> = None;
    for (idx, entry) in app.entries.iter().enumerate() {
        if preserve_path.map_or(false, |p| p == entry.path) {
            select_idx = Some(idx);
        }
        SendMessageW(
            app.list,
            LB_ADDSTRING,
            0,
            text(&entry.name).as_ptr() as isize,
        );
    }
    if let Some(idx) = select_idx {
        select_one(app.list, idx);
    }
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
        let app = get_data(hwnd) as *mut App;
        if !app.is_null() {
            SendMessageW(control, WM_SETFONT, (*(*app).theme).font as usize, 1);
        }
    }
    control
}

unsafe fn place_box(control: H, rect: BoxRect) {
    if !control.is_null() {
        MoveWindow(
            control,
            rect.x,
            rect.y,
            rect.width.max(1),
            rect.height.max(1),
            1,
        );
    }
}

unsafe fn layout(hwnd: H, app: &mut App) {
    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut r);
    let m = WindowMetrics::new(r.right, r.bottom);

    place_box(app.nav_buttons[0], m.nav_back);
    place_box(app.nav_buttons[1], m.nav_forward);
    place_box(app.nav_buttons[2], m.nav_up);
    place_box(app.nav_buttons[3], m.nav_root);
    place_box(app.nav_buttons[4], m.nav_refresh);
    place_box(app.nav_buttons[5], m.nav_go);
    place_box(app.address, m.address_edit);

    place_box(app.extra_buttons[0], m.new_tab);
    place_box(app.extra_buttons[1], m.close_tab);

    place_box(app.filter_control, m.filter_edit);
    place_box(app.extra_buttons[2], m.search_btn);
    place_box(app.extra_buttons[3], m.clear_btn);
    place_box(app.extra_buttons[4], m.hidden_btn);

    place_box(app.name_control, m.name_edit);
    place_box(app.extra_buttons[5], m.new_file_btn);
    place_box(app.extra_buttons[6], m.new_folder_btn);
    place_box(app.extra_buttons[8], m.rename_btn);
    place_box(app.extra_buttons[7], m.delete_btn);

    place_box(app.list, m.list);

    rebuild_backdrop(hwnd, app);
    InvalidateRect(hwnd, ptr::null(), 0);
    for btn in app.nav_buttons.iter().chain(app.extra_buttons.iter()) {
        if !btn.is_null() {
            InvalidateRect(*btn, ptr::null(), 0);
        }
    }
}

unsafe fn load(hwnd: H, app: &mut App, destination: Destination, preserve_notice: bool) {
    let path = match app.tabs.current().resolve(&destination) {
        Some(path) => path,
        None => return,
    };
    let generation = app.token.fetch_add(1, Ordering::Relaxed) + 1;
    let token = app.token.clone();
    let target = hwnd as usize;
    let filter = app.filter.clone();
    let show_hidden = app.show_hidden;
    if !preserve_notice {
        set_status(hwnd, app, Some(format!("Loading {}…", path.display())));
    }
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

enum Operation {
    NewFile(PathBuf, OsString),
    NewFolder(PathBuf, OsString),
    Delete(PathBuf),
    Rename(PathBuf, OsString),
    Copy(PathBuf, PathBuf, OsString),
    Move(PathBuf, PathBuf),
    CopyItems(Vec<PathBuf>, PathBuf),
    Trash(PathBuf, usize),
    Convert(PathBuf, PathBuf, u8),
}

unsafe fn start_operation(hwnd: H, app: &mut App, operation: Operation) {
    if app.operation_busy {
        set_status(
            hwnd,
            app,
            Some("Another file operation is already running.".into()),
        );
        return;
    }
    app.operation_busy = true;
    set_status(hwnd, app, Some("Working…".into()));
    let target = hwnd as usize;
    thread::spawn(move || {
        let result =
            match operation {
                Operation::NewFile(dir, name) => zfm::operations::create_file(&dir, &name)
                    .map(|p| format!("Created {}", p.display())),
                Operation::NewFolder(dir, name) => zfm::operations::create_directory(&dir, &name)
                    .map(|p| format!("Created {}", p.display())),
                Operation::Rename(path, name) => zfm::operations::rename_child(&path, &name)
                    .map(|p| format!("Renamed to {}", p.display())),
                Operation::Delete(path) => zfm::operations::remove_one(&path)
                    .map(|_| format!("Deleted {}", path.display())),
                Operation::Trash(path, owner) => zfm::trash::trash_with_owner(&path, owner)
                    .map(|_| format!("Moved {} to Recycle Bin", path.display())),
                Operation::Copy(path, dir, name) => {
                    zfm::operations::copy_item(&path, &dir, &name, &AtomicBool::new(false), |_| {})
                        .map(|p| format!("Copied to {}", p.display()))
                }
                Operation::CopyItems(paths, dir) => zfm::operations::copy_items(&paths, &dir, &AtomicBool::new(false))
                    .map(|result| result.message(paths.len())),
                Operation::Move(path, dir) => zfm::operations::move_into(&path, &dir)
                    .map(|p| format!("Moved to {}", p.display())),
                Operation::Convert(input, output, mode) => {
                    let cancel = AtomicBool::new(false);
                    let result = match mode {
                        0 => zfm::converter::csv_to_json(&input, &output, &cancel, |_| {}),
                        1 => zfm::json_csv::json_to_csv(&input, &output, &cancel, |_| {}),
                        2 => zfm::converter::text_to_html(&input, &output, &cancel, |_| {}),
                        3 => zfm::image_convert::convert(&input, &output, true, &cancel, |_| {}),
                        4 => zfm::image_convert::convert(&input, &output, false, &cancel, |_| {}),
                        _ => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Unsupported conversion")),
                    };
                    result.map(|_| format!("Converted to {}", output.display()))
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

pub fn show_startup_error(message: &str) {
    unsafe {
        show_error(ptr::null_mut(), &format!("ZFM could not start: {message}"));
    }
}

unsafe fn show_error(hwnd: H, message: &str) {
    MessageBoxW(hwnd, text(message).as_ptr(), text("ZFM").as_ptr(), 0x10);
}

unsafe fn handle_custom_click(hwnd: H, app: &mut App, x: i32, y: i32) {
    let mut r = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetClientRect(hwnd, &mut r);
    let m = WindowMetrics::new(r.right, r.bottom);
    match hit_test_custom(app, &m, x, y) {
        HotZone::AddressChip => {
            SetFocus(app.address);
            SendMessageW(app.address, EM_SETSEL, 0, -1);
        }
        HotZone::Tab(idx) => {
            if app.tabs.select(idx) {
                rebuild_backdrop(hwnd, app);
                InvalidateRect(hwnd, ptr::null(), 0);
                load(hwnd, app, Destination::Refresh, false);
            }
        }
        HotZone::Sidebar(idx) => {
            if GetKeyState(0x11) < 0 {
                if let Some(path) = app
                    .sidebar_items
                    .get(idx)
                    .and_then(|item| item.target.clone())
                {
                    app.tabs.add(path);
                    rebuild_backdrop(hwnd, app);
                    InvalidateRect(hwnd, ptr::null(), 0);
                    load(hwnd, app, Destination::Refresh, false);
                }
                return;
            }
            if let Some(item) = app.sidebar_items.get(idx).cloned() {
                if item.is_drives_header {
                    app.drives_expanded = !app.drives_expanded;
                    app.sidebar_items = build_sidebar_items(app.drives_expanded);
                    rebuild_backdrop(hwnd, app);
                    InvalidateRect(hwnd, ptr::null(), 0);
                } else if let Some(target) = item.target {
                    load(hwnd, app, Destination::Visit(target), false);
                }
            }
        }
        HotZone::Column(col) => {
            if app.sort_column == col {
                app.sort_ascending = !app.sort_ascending;
            } else {
                app.sort_column = col;
                app.sort_ascending = matches!(col, SortColumn::Name | SortColumn::Kind);
            }
            let selected: std::collections::HashSet<PathBuf> = selected_indices(app)
                .into_iter()
                .map(|i| app.entries[i].path.clone())
                .collect();
            ui_theme::sort_entries(&mut app.entries, app.sort_column, app.sort_ascending);
            populate_listbox(app, None);
            for (i, entry) in app.entries.iter().enumerate() {
                if selected.contains(&entry.path) {
                    SendMessageW(app.list, LB_SETSEL, 1, i as isize);
                }
            }
            InvalidateRect(hwnd, ptr::null(), 0);
            InvalidateRect(app.list, ptr::null(), 0);
        }
        HotZone::View(mode) => {
            if app.view_mode != mode {
                app.view_mode = mode;
                let row_h = match mode {
                    ViewMode::Details => 22,
                    ViewMode::Compact => 38,
                };
                SendMessageW(app.list, LB_SETITEMHEIGHT, 0, row_h);
                rebuild_backdrop(hwnd, app);
                InvalidateRect(hwnd, ptr::null(), 0);
                InvalidateRect(app.list, ptr::null(), 0);
            }
        }
        HotZone::None => {}
    }
}

unsafe fn list_hit(app: &App, x: i32, y: i32) -> Option<usize> {
    let mut rect: Rect = std::mem::zeroed();
    GetClientRect(app.list, &mut rect);
    if x < 0 || y < 0 || x >= rect.right || y >= rect.bottom {
        return None;
    }
    let height = SendMessageW(app.list, LB_GETITEMHEIGHT, 0, 0);
    let top = SendMessageW(app.list, LB_GETTOPINDEX, 0, 0);
    if height <= 0 || top < 0 {
        return None;
    }
    let index = top as usize + y as usize / height as usize;
    if index < app.entries.len() {
        Some(index)
    } else {
        None
    }
}

unsafe extern "system" fn list_proc(
    list: H,
    msg: u32,
    wp: WParam,
    lp: LParam,
    _: usize,
    data: usize,
) -> LResult {
    let parent = data as H;
    let app_ptr = get_data(parent) as *mut App;
    if app_ptr.is_null() {
        return DefSubclassProc(list, msg, wp, lp);
    }
    if msg == WM_RBUTTONDOWN {
        let index = list_hit(&*app_ptr, (lp as i16) as i32, ((lp >> 16) as i16) as i32);
        match index {
            Some(i) if SendMessageW(list, LB_GETSEL, i, 0) <= 0 => select_one(list, i),
            None => clear_selection(list),
            _ => {}
        }
        SetFocus(list);
        invalidate_status_bar(parent);
        return 0;
    }
    if msg == WM_MOUSEMOVE {
        let x = (lp as i16) as i32;
        let y = ((lp >> 16) as i16) as i32;
        let next = list_hit(&*app_ptr, x, y);
        if (*app_ptr).hovered_row != next {
            (*app_ptr).hovered_row = next;
            InvalidateRect(list, ptr::null(), 0);
        }
        let mut tracking = TrackMouseEvent {
            size: std::mem::size_of::<TrackMouseEvent>() as u32,
            flags: 2,
            hwnd_track: list,
            hover_time: 0,
        };
        TrackMouseEvent(&mut tracking);
    } else if msg == WM_MOUSELEAVE {
        (*app_ptr).hovered_row = None;
        InvalidateRect(list, ptr::null(), 0);
    }
    if msg == WM_LBUTTONDOWN || msg == WM_LBUTTONDBLCLK || msg == WM_MBUTTONDOWN {
        let x = (lp as i16) as i32;
        let y = ((lp >> 16) as i16) as i32;
        let index = list_hit(&*app_ptr, x, y);
        if index.is_none() && (msg == WM_LBUTTONDOWN || msg == WM_LBUTTONDBLCLK) {
            clear_selection(list);
            SetFocus(list);
            invalidate_status_bar(parent);
            return 0;
        }
        if msg == WM_LBUTTONDBLCLK && (GetKeyState(0x11) < 0 || GetKeyState(0x12) < 0) {
            if let Some(i) = index {
                select_one(list, i);
                let command = if GetKeyState(0x12) < 0 {
                    PROPERTIES
                } else {
                    OPEN_TAB
                };
                SendMessageW(parent, WM_COMMAND, command, 0);
            }
            return 0;
        }
        if msg == WM_MBUTTONDOWN {
            if let Some(path) = index
                .and_then(|i| (&(*app_ptr).entries).get(i))
                .filter(|e| e.is_dir)
                .map(|e| e.path.clone())
            {
                let app = &mut *app_ptr;
                app.tabs.add(path);
                rebuild_backdrop(parent, app);
                InvalidateRect(parent, ptr::null(), 0);
                load(parent, app, Destination::Refresh, false);
            }
            return 0;
        }
    }
    if msg == WM_XBUTTONDOWN {
        SendMessageW(
            parent,
            WM_COMMAND,
            if (wp >> 16) == 1 { BACK } else { FORWARD },
            0,
        );
        return 1;
    }
    DefSubclassProc(list, msg, wp, lp)
}

unsafe fn list_context(hwnd: H, app: &mut App, lp: LParam) {
    let keyboard = lp == -1;
    let point = if keyboard {
        let mut p = Point { x: 8, y: 8 };
        ClientToScreen(app.list, &mut p);
        p
    } else {
        Point {
            x: (lp as i16) as i32,
            y: ((lp >> 16) as i16) as i32,
        }
    };
    if !keyboard {
        let mut local = Point {
            x: point.x,
            y: point.y,
        };
        ScreenToClient(app.list, &mut local);
        let index = list_hit(app, local.x, local.y);
        match index {
            Some(i) if SendMessageW(app.list, LB_GETSEL, i, 0) <= 0 => select_one(app.list, i),
            None => clear_selection(app.list),
            _ => {}
        }
        invalidate_status_bar(hwnd);
    }
    SetFocus(app.list);
    let selected = !selected_indices(app).is_empty();
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    if selected {
        AppendMenuW(menu, 0, 1, text("Open").as_ptr());
        if selected_indices(app).len() == 1 && app.entries[selected_indices(app)[0]].is_dir {
            AppendMenuW(menu, 0, OPEN_TAB, text("Open in new tab").as_ptr());
        }
        let selected_items = selected_indices(app);
        if selected_items.len() == 1 && app.entries[selected_items[0]].is_file {
            AppendMenuW(menu, 0, PREVIEW, text("Preview in ZFM…").as_ptr());
            let extension = app.entries[selected_items[0]].path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ["csv", "json", "txt", "bmp", "ico"].iter().any(|e| extension.eq_ignore_ascii_case(e)) {
                AppendMenuW(menu, 0, CONVERT, text("Convert file (built-in)…").as_ptr());
            }
        }
        AppendMenuW(menu, 0x800, 0, ptr::null());
        for (id, title) in [
            (CUT, "Cut"),
            (COPY, "Copy"),
            (COPY_PATH, "Copy path"),
            (PASTE, "Paste"),
            (DUPLICATE, "Duplicate…"),
            (PROMPT_RENAME, "Rename…"),
            (TRASH, "Move to Recycle Bin"),
            (DELETE, "Delete permanently…"),
            (PROPERTIES, "Properties"),
        ] {
            AppendMenuW(menu, 0, id, text(title).as_ptr());
        }
    } else {
        for (id, title) in [
            (PROMPT_FILE, "New file…"),
            (PROMPT_FOLDER, "New folder…"),
            (PASTE, "Paste"),
            (REFRESH, "Refresh"),
            (HIDDEN, "Show / hide hidden files"),
            (SETTINGS, "Settings…"),
        ] {
            AppendMenuW(menu, 0, id, text(title).as_ptr());
        }
        let sort = CreatePopupMenu();
        if !sort.is_null() {
            for (id, title) in [
                (201, "Name"),
                (202, "Date Modified"),
                (203, "Type"),
                (204, "Size"),
            ] {
                AppendMenuW(sort, 0, id, text(title).as_ptr());
            }
            AppendMenuW(menu, 0x10, sort as usize, text("Sort by").as_ptr());
        }
        let view = CreatePopupMenu();
        if !view.is_null() {
            AppendMenuW(view, 0, 205, text("Details").as_ptr());
            AppendMenuW(view, 0, 206, text("Compact").as_ptr());
            AppendMenuW(menu, 0x10, view as usize, text("View").as_ptr());
        }
    }
    let id = TrackPopupMenu(
        menu,
        0x0100 | 0x0002,
        point.x,
        point.y,
        0,
        hwnd,
        ptr::null(),
    );
    DestroyMenu(menu);
    if id == 1 {
        SendMessageW(hwnd, WM_COMMAND, LIST | (2 << 16), app.list as isize);
    } else if (201..=206).contains(&id) {
        let mut r: Rect = std::mem::zeroed();
        GetClientRect(hwnd, &mut r);
        let m = WindowMetrics::new(r.right, r.bottom);
        let rect = match id {
            201 => m.col_name,
            202 => m.col_modified,
            203 => m.col_type,
            204 => m.col_size,
            205 => m.view_details_btn,
            _ => m.view_compact_btn,
        };
        handle_custom_click(hwnd, app, rect.x + 1, rect.y + 1);
    } else if id != 0 {
        SendMessageW(hwnd, WM_COMMAND, id as usize, 0);
    }
}

unsafe fn custom_context(hwnd: H, app: &mut App, lp: LParam) {
    if lp == -1 {
        return;
    }
    let point = Point {
        x: (lp as i16) as i32,
        y: ((lp >> 16) as i16) as i32,
    };
    let mut local = Point {
        x: point.x,
        y: point.y,
    };
    ScreenToClient(hwnd, &mut local);
    let mut rect: Rect = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let zone = hit_test_custom(
        app,
        &WindowMetrics::new(rect.right, rect.bottom),
        local.x,
        local.y,
    );
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    match zone {
        HotZone::Tab(_) => {
            AppendMenuW(menu, 0, 1, text("New tab").as_ptr());
            AppendMenuW(menu, 0, 2, text("Duplicate tab").as_ptr());
            AppendMenuW(menu, 0, 3, text("Close tab").as_ptr());
            AppendMenuW(menu, 0, 4, text("Close other tabs").as_ptr());
        }
        HotZone::Sidebar(index)
            if app
                .sidebar_items
                .get(index)
                .and_then(|item| item.target.as_ref())
                .is_some() =>
        {
            AppendMenuW(menu, 0, 5, text("Open").as_ptr());
            AppendMenuW(menu, 0, 6, text("Open in new tab").as_ptr());
        }
        _ => {
            DestroyMenu(menu);
            return;
        }
    }
    let command = TrackPopupMenu(
        menu,
        0x0100 | 0x0002,
        point.x,
        point.y,
        0,
        hwnd,
        ptr::null(),
    );
    DestroyMenu(menu);
    match (zone, command) {
        (HotZone::Tab(_), 1) => {
            let path = app.tabs.current().current().to_path_buf();
            app.tabs.add(path);
        }
        (HotZone::Tab(index), 2) => {
            if let Some(path) = app.tabs.history(index).map(|h| h.current().to_path_buf()) {
                app.tabs.add(path);
            } else {
                return;
            }
        }
        (HotZone::Tab(index), 3) => {
            if !app.tabs.close_at(index) {
                set_status(hwnd, app, Some("At least one tab must remain open.".into()));
                return;
            }
        }
        (HotZone::Tab(index), 4) => {
            if !app.tabs.close_others(index) {
                return;
            }
        }
        (HotZone::Sidebar(index), 5 | 6) => {
            if let Some(path) = app
                .sidebar_items
                .get(index)
                .and_then(|item| item.target.clone())
            {
                if command == 5 {
                    load(hwnd, app, Destination::Visit(path), false);
                    return;
                }
                app.tabs.add(path);
            } else {
                return;
            }
        }
        _ => return,
    }
    rebuild_backdrop(hwnd, app);
    InvalidateRect(hwnd, ptr::null(), 0);
    load(hwnd, app, Destination::Refresh, false);
}

unsafe extern "system" fn window_proc(hwnd: H, msg: u32, wp: WParam, lp: LParam) -> LResult {
    if msg == WM_CREATE {
        let app = &mut *(*(lp as *const CreateStruct)).param.cast::<App>();
        set_data(hwnd, app as *mut App as isize);

        let nav_buttons = [
            child(hwnd, "BUTTON", "Back", WS_TABSTOP | BS_OWNERDRAW, BACK),
            child(hwnd, "BUTTON", "Next", WS_TABSTOP | BS_OWNERDRAW, FORWARD),
            child(hwnd, "BUTTON", "Up", WS_TABSTOP | BS_OWNERDRAW, UP),
            child(hwnd, "BUTTON", "Root", WS_TABSTOP | BS_OWNERDRAW, ROOT),
            child(hwnd, "BUTTON", "Reload", WS_TABSTOP | BS_OWNERDRAW, REFRESH),
            child(hwnd, "BUTTON", "Go", WS_TABSTOP | BS_OWNERDRAW, GO),
        ];
        let address = child(hwnd, "EDIT", "", WS_TABSTOP | ES_AUTOHSCROLL, ADDRESS);
        let list = child(
            hwnd,
            "LISTBOX",
            "",
            WS_TABSTOP
                | WS_VSCROLL
                | LBS_NOTIFY
                | LBS_OWNERDRAWFIXED
                | LBS_HASSTRINGS
                | LBS_EXTENDEDSEL,
            LIST,
        );
        let status = CreateWindowExW(
            0,
            text("STATIC").as_ptr(),
            text("").as_ptr(),
            WS_CHILD,
            0,
            0,
            1,
            1,
            hwnd,
            STATUS as H,
            GetModuleHandleW(ptr::null()),
            ptr::null_mut(),
        );
        let filter_control = child(hwnd, "EDIT", "", WS_TABSTOP | ES_AUTOHSCROLL, FILTER);
        let name_control = child(hwnd, "EDIT", "", WS_TABSTOP | ES_AUTOHSCROLL, NAME);
        let extra_buttons = [
            child(
                hwnd,
                "BUTTON",
                "New tab",
                WS_TABSTOP | BS_OWNERDRAW,
                NEW_TAB,
            ),
            child(
                hwnd,
                "BUTTON",
                "Close tab",
                WS_TABSTOP | BS_OWNERDRAW,
                CLOSE_TAB,
            ),
            child(
                hwnd,
                "BUTTON",
                "Search",
                WS_TABSTOP | BS_OWNERDRAW,
                APPLY_FILTER,
            ),
            child(
                hwnd,
                "BUTTON",
                "Clear",
                WS_TABSTOP | BS_OWNERDRAW,
                CLEAR_FILTER,
            ),
            child(
                hwnd,
                "BUTTON",
                "Show hidden",
                WS_TABSTOP | BS_OWNERDRAW,
                HIDDEN,
            ),
            child(
                hwnd,
                "BUTTON",
                "New file",
                WS_TABSTOP | BS_OWNERDRAW,
                NEW_FILE,
            ),
            child(
                hwnd,
                "BUTTON",
                "New folder",
                WS_TABSTOP | BS_OWNERDRAW,
                NEW_FOLDER,
            ),
            child(
                hwnd,
                "BUTTON",
                "Delete...",
                WS_TABSTOP | BS_OWNERDRAW,
                DELETE,
            ),
            child(hwnd, "BUTTON", "Rename", WS_TABSTOP | BS_OWNERDRAW, RENAME),
        ];

        if address.is_null()
            || list.is_null()
            || status.is_null()
            || filter_control.is_null()
            || name_control.is_null()
            || nav_buttons.iter().any(|b| b.is_null())
            || extra_buttons.iter().any(|b| b.is_null())
        {
            return -1;
        }

        app.nav_buttons = nav_buttons;
        app.address = address;
        app.list = list;
        app.status = status;
        app.filter_control = filter_control;
        app.name_control = name_control;
        app.extra_buttons = extra_buttons;
        SetWindowSubclass(list, Some(list_proc), 1, hwnd as usize);

        SendMessageW(list, LB_SETITEMHEIGHT, 0, 22);
        SendMessageW(
            filter_control,
            EM_SETCUEBANNER,
            1,
            text("Filter").as_ptr() as isize,
        );
        SendMessageW(
            name_control,
            EM_SETCUEBANNER,
            1,
            text("New name").as_ptr() as isize,
        );
        apply_dark_scrollbar(list);
        apply_dark_window_chrome(hwnd);

        let path = wide(app.tabs.current().current().as_os_str());
        SetWindowTextW(address, path.as_ptr());
        layout(hwnd, app);
        load(hwnd, app, Destination::Refresh, false);
        return 0;
    }

    let data = get_data(hwnd);
    if data == 0 {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let app = &mut *(data as *mut App);

    match msg {
        WM_ERASEBKGND => 1,
        WM_GETMINMAXINFO => {
            let info = &mut *(lp as *mut MinMaxInfo);
            info.min_track_size.x = 760;
            info.min_track_size.y = 500;
            0
        }
        WM_PAINT => {
            paint_window(hwnd, app);
            0
        }
        WM_SIZE => {
            layout(hwnd, app);
            0
        }
        WM_SETCURSOR => {
            let target = wp as H;
            let is_btn = app
                .nav_buttons
                .iter()
                .chain(app.extra_buttons.iter())
                .any(|&b| b == target);
            let next_hover = if is_btn { target } else { ptr::null_mut() };
            if app.hovered_child != next_hover {
                let prev = app.hovered_child;
                app.hovered_child = next_hover;
                if !prev.is_null() {
                    InvalidateRect(prev, ptr::null(), 0);
                }
                if !next_hover.is_null() {
                    InvalidateRect(next_hover, ptr::null(), 0);
                }
            }
            let hand = &*app.theme;
            if !hand.hand_cursor.is_null() && (lp & 0xffff) == 1 {
                if is_btn || (target == hwnd && app.hot_zone != HotZone::None) {
                    SetCursor(hand.hand_cursor);
                    return 1;
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_MOUSEMOVE => {
            if !app.tracking_mouse {
                let mut tme = TrackMouseEvent {
                    size: std::mem::size_of::<TrackMouseEvent>() as u32,
                    flags: 2, // TME_LEAVE
                    hwnd_track: hwnd,
                    hover_time: 0,
                };
                if TrackMouseEvent(&mut tme) != 0 {
                    app.tracking_mouse = true;
                }
            }
            let mx = (lp as i16) as i32;
            let my = ((lp >> 16) as i16) as i32;
            let mut r = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetClientRect(hwnd, &mut r);
            let m = WindowMetrics::new(r.right, r.bottom);
            let zone = hit_test_custom(app, &m, mx, my);
            if app.hot_zone != zone {
                app.hot_zone = zone;
                rebuild_backdrop(hwnd, app);
                InvalidateRect(hwnd, ptr::null(), 0);
            }
            0
        }
        WM_MOUSELEAVE => {
            app.tracking_mouse = false;
            if app.hot_zone != HotZone::None {
                app.hot_zone = HotZone::None;
                rebuild_backdrop(hwnd, app);
                InvalidateRect(hwnd, ptr::null(), 0);
            }
            0
        }
        WM_CONTEXTMENU if wp as H == app.list => {
            list_context(hwnd, app, lp);
            0
        }
        WM_CONTEXTMENU if wp as H == hwnd => {
            custom_context(hwnd, app, lp);
            0
        }
        WM_MBUTTONDOWN => {
            let x = (lp as i16) as i32;
            let y = ((lp >> 16) as i16) as i32;
            let mut rect: Rect = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            match hit_test_custom(app, &WindowMetrics::new(rect.right, rect.bottom), x, y) {
                HotZone::Tab(index) => {
                    if app.tabs.close_at(index) {
                        rebuild_backdrop(hwnd, app);
                        InvalidateRect(hwnd, ptr::null(), 0);
                        load(hwnd, app, Destination::Refresh, false);
                    } else {
                        set_status(hwnd, app, Some("At least one tab must remain open.".into()));
                    }
                }
                HotZone::Sidebar(index) => {
                    if let Some(path) = app
                        .sidebar_items
                        .get(index)
                        .and_then(|item| item.target.clone())
                    {
                        app.tabs.add(path);
                        rebuild_backdrop(hwnd, app);
                        InvalidateRect(hwnd, ptr::null(), 0);
                        load(hwnd, app, Destination::Refresh, false);
                    }
                }
                _ => {}
            }
            0
        }
        WM_XBUTTONDOWN => {
            let destination = if (wp >> 16) == 1 {
                Destination::Back
            } else {
                Destination::Forward
            };
            load(hwnd, app, destination, false);
            1
        }
        WM_LBUTTONDBLCLK => {
            let x = (lp as i16) as i32;
            let y = ((lp >> 16) as i16) as i32;
            let mut rect: Rect = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            let metrics = WindowMetrics::new(rect.right, rect.bottom);
            if y >= metrics.tab_strip_top
                && y < metrics.deck_top
                && x >= metrics.dynamic_tabs_left
                && x < metrics.dynamic_tabs_right
                && hit_test_custom(app, &metrics, x, y) == HotZone::None
            {
                let path = app.tabs.current().current().to_path_buf();
                app.tabs.add(path);
                rebuild_backdrop(hwnd, app);
                InvalidateRect(hwnd, ptr::null(), 0);
                load(hwnd, app, Destination::Refresh, false);
            }
            0
        }
        WM_LBUTTONDOWN => {
            let mx = (lp as i16) as i32;
            let my = ((lp >> 16) as i16) as i32;
            let mut rect: Rect = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            if hit_test_custom(app, &WindowMetrics::new(rect.right, rect.bottom), mx, my)
                != HotZone::AddressChip
            {
                SetFocus(app.list);
            }
            handle_custom_click(hwnd, app, mx, my);
            0
        }
        WM_DRAWITEM if lp != 0 => {
            let item = &mut *(lp as *mut DrawItem);
            if item.control_type == ODT_LISTBOX || item.control_id as usize == LIST {
                draw_list_item(hwnd, app, item)
            } else if item.control_type == ODT_BUTTON {
                draw_button(hwnd, app, item)
            } else {
                0
            }
        }
        WM_RESULT => {
            let message = Box::from_raw(lp as *mut ResultMessage);
            if app.token.load(Ordering::Relaxed) != message.generation {
                return 0;
            }
            match message.result {
                Ok(Some(mut listing)) => {
                    app.tabs.current_mut().commit(message.destination);
                    ui_theme::sort_entries(
                        &mut listing.entries,
                        app.sort_column,
                        app.sort_ascending,
                    );
                    let path = wide(app.tabs.current().current().as_os_str());
                    SetWindowTextW(app.address, path.as_ptr());
                    let skipped_note = if listing.skipped > 0 {
                        Some(format!(
                            "{}  ({} entries skipped)",
                            app.tabs.current().current().display(),
                            listing.skipped
                        ))
                    } else {
                        None
                    };
                    app.entries = listing.entries;
                    populate_listbox(app, None);
                    rebuild_backdrop(hwnd, app);
                    set_status(hwnd, app, skipped_note);
                    InvalidateRect(hwnd, ptr::null(), 0);
                    InvalidateRect(app.extra_buttons[0], ptr::null(), 0);
                    InvalidateRect(app.extra_buttons[1], ptr::null(), 0);
                }
                Ok(None) => {}
                Err(e) => {
                    let path = wide(app.tabs.current().current().as_os_str());
                    SetWindowTextW(app.address, path.as_ptr());
                    set_status(hwnd, app, Some(format!("Could not open folder: {e}")));
                }
            }
            0
        }
        WM_OPERATION_RESULT => {
            let message = Box::from_raw(lp as *mut OperationMessage);
            app.operation_busy = false;
            match message.result {
                Ok(status) => {
                    set_status(hwnd, app, Some(status));
                    load(hwnd, app, Destination::Refresh, true);
                }
                Err(error) => {
                    set_status(hwnd, app, Some(format!("File operation failed: {error}")))
                }
            }
            0
        }
        WM_COMMAND => {
            let id = wp & 0xffff;
            let code = (wp >> 16) & 0xffff;

            if matches!(id, ADDRESS | FILTER | NAME)
                && (code == EN_SETFOCUS || code == EN_KILLFOCUS)
            {
                let next_focus = if code == EN_SETFOCUS {
                    id
                } else if app.focused_edit == id {
                    0
                } else {
                    app.focused_edit
                };
                if app.focused_edit != next_focus {
                    app.focused_edit = next_focus;
                    rebuild_backdrop(hwnd, app);
                    InvalidateRect(hwnd, ptr::null(), 0);
                }
                return 0;
            }

            match id {
                BACK if code == 0 => load(hwnd, app, Destination::Back, false),
                FORWARD if code == 0 => load(hwnd, app, Destination::Forward, false),
                REFRESH if code == 0 => load(hwnd, app, Destination::Refresh, false),
                UP if code == 0 => {
                    let parent = app
                        .tabs
                        .current()
                        .current()
                        .parent()
                        .map(|p| p.to_path_buf());
                    if let Some(parent) = parent {
                        load(hwnd, app, Destination::Visit(parent), false);
                    }
                }
                ROOT if code == 0 => {
                    let root = root_of(app.tabs.current().current());
                    load(hwnd, app, Destination::Visit(root), false);
                }
                GO if code == 0 => {
                    let typed = PathBuf::from(control_value(app.address));
                    let path = if typed.is_absolute() {
                        typed
                    } else {
                        app.tabs.current().current().join(typed)
                    };
                    load(hwnd, app, Destination::Visit(path), false);
                }
                NEW_TAB if code == 0 => {
                    let path = app.tabs.current().current().to_path_buf();
                    app.tabs.add(path);
                    rebuild_backdrop(hwnd, app);
                    InvalidateRect(hwnd, ptr::null(), 0);
                    load(hwnd, app, Destination::Refresh, false);
                }
                CLOSE_TAB if code == 0 => {
                    if app.tabs.close() {
                        rebuild_backdrop(hwnd, app);
                        InvalidateRect(hwnd, ptr::null(), 0);
                        load(hwnd, app, Destination::Refresh, false);
                    } else {
                        set_status(hwnd, app, Some("At least one tab must remain open.".into()));
                    }
                }
                APPLY_FILTER if code == 0 => {
                    app.filter = control_value(app.filter_control)
                        .to_string_lossy()
                        .into_owned();
                    load(hwnd, app, Destination::Refresh, false);
                }
                CLEAR_FILTER if code == 0 => {
                    app.filter.clear();
                    SetWindowTextW(app.filter_control, text("").as_ptr());
                    load(hwnd, app, Destination::Refresh, false);
                }
                SETTINGS if code == 0 => {
                    if let Some((hidden, ascending)) = windows_settings::show(hwnd, app.theme, app.show_hidden, app.sort_ascending) {
                        app.show_hidden = hidden;
                        app.sort_ascending = ascending;
                        InvalidateRect(app.extra_buttons[4], ptr::null(), 0);
                        rebuild_backdrop(hwnd, app);
                        InvalidateRect(hwnd, ptr::null(), 0);
                        load(hwnd, app, Destination::Refresh, false);
                    }
                }
                HIDDEN if code == 0 => {
                    app.show_hidden = !app.show_hidden;
                    InvalidateRect(app.extra_buttons[4], ptr::null(), 0);
                    load(hwnd, app, Destination::Refresh, false);
                }
                COPY | CUT if code == 0 => {
                    let indices = selected_indices(app);
                    let message = if indices.is_empty() || indices.len() > zfm::operations::MAX_COPY_ITEMS {
                        "Select between 1 and 1024 items.".to_string()
                    } else if id == CUT && indices.len() != 1 {
                        "Cut supports exactly one item.".to_string()
                    } else if id == COPY && indices.iter().any(|i| !app.entries[*i].is_file && !app.entries[*i].is_dir) {
                        "Copy supports regular files and folders only.".to_string()
                    } else {
                        let paths: Vec<PathBuf> = indices.iter().map(|i| app.entries[*i].path.clone()).collect();
                        if windows_actions::copy_files(hwnd, &paths, id == CUT) {
                            format!("{} item(s) placed on clipboard.", paths.len())
                        } else { "Could not write to clipboard.".to_string() }
                    };
                    set_status(hwnd, app, Some(message));
                }
                COPY_PATH if code == 0 => {
                    let indices = selected_indices(app);
                    let path = if indices.len() == 1 {
                        app.entries[indices[0]].path.clone()
                    } else {
                        app.tabs.current().current().to_path_buf()
                    };
                    let ok = windows_actions::copy_text(hwnd, &path);
                    set_status(
                        hwnd,
                        app,
                        Some(
                            if ok {
                                "Path copied."
                            } else {
                                "Could not write to clipboard."
                            }
                            .into(),
                        ),
                    );
                }
                PASTE if code == 0 => match windows_actions::read_files(hwnd) {
                    Ok((paths, cut)) => {
                        let dir = app.tabs.current().current().to_path_buf();
                        if cut {
                            start_operation(hwnd, app, Operation::Move(paths[0].clone(), dir));
                        } else {
                            start_operation(hwnd, app, Operation::CopyItems(paths, dir));
                        }
                    }
                    Err(message) => set_status(hwnd, app, Some(message.into())),
                },
                PROPERTIES if code == 0 => {
                    if let Some(path) = selected_path(hwnd, app) {
                        windows_actions::properties(hwnd, &path);
                    }
                }
                OPEN_TAB if code == 0 => {
                    let indices = selected_indices(app);
                    if indices.len() == 1 {
                        if let Some(path) = app
                            .entries
                            .get(indices[0])
                            .filter(|e| e.is_dir)
                            .map(|e| e.path.clone())
                        {
                            app.tabs.add(path);
                            rebuild_backdrop(hwnd, app);
                            InvalidateRect(hwnd, ptr::null(), 0);
                            load(hwnd, app, Destination::Refresh, false);
                        }
                    }
                }
                CONVERT if code == 0 => {
                    if let Some(input) = selected_path(hwnd, app) {
                        let ext = input.extension().and_then(|s| s.to_str()).unwrap_or("");
                        let (mode, target_ext, title) = if ext.eq_ignore_ascii_case("csv") {
                            (0, "json", "Convert CSV to JSON")
                        } else if ext.eq_ignore_ascii_case("json") {
                            (1, "csv", "Convert JSON to CSV")
                        } else if ext.eq_ignore_ascii_case("txt") {
                            (2, "html", "Convert text to HTML")
                        } else if ext.eq_ignore_ascii_case("bmp") {
                            (3, "ico", "Convert BMP to ICO")
                        } else if ext.eq_ignore_ascii_case("ico") {
                            (4, "bmp", "Convert ICO to BMP")
                        } else {
                            set_status(hwnd, app, Some("Built-in conversion supports CSV, JSON, TXT, BMP and ICO inputs only.".into()));
                            return 0;
                        };
                        let proposed = input.with_extension(target_ext);
                        if let Some(name) = windows_actions::prompt(hwnd, app.theme, title, proposed.file_name().unwrap_or_default(), false) {
                            match zfm::operations::validate_name(&name) {
                                Ok(()) => {
                                    let output = input.parent().unwrap_or_else(|| Path::new(".")).join(name);
                                    start_operation(hwnd, app, Operation::Convert(input, output, mode));
                                }
                                Err(e) => set_status(hwnd, app, Some(format!("Invalid output name: {e}"))),
                            }
                        }
                    }
                }
                DUPLICATE if code == 0 => {
                    if let Some(path) = selected_path(hwnd, app) {
                        let dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
                        let proposed = zfm::operations::suggest_copy_name(&dir, &path);
                        if let Some(name) = windows_actions::prompt(
                            hwnd,
                            app.theme,
                            "Duplicate",
                            OsStr::new(&proposed),
                            true,
                        ) {
                            start_operation(hwnd, app, Operation::Copy(path, dir, name));
                        }
                    }
                }
                TRASH if code == 0 => {
                    if let Some(path) = selected_path(hwnd, app) {
                        start_operation(hwnd, app, Operation::Trash(path, hwnd as usize));
                    }
                }
                PROMPT_FILE | PROMPT_FOLDER if code == 0 => {
                    let dir = app.tabs.current().current().to_path_buf();
                    let title = if id == PROMPT_FILE {
                        "New file"
                    } else {
                        "New folder"
                    };
                    if let Some(name) =
                        windows_actions::prompt(hwnd, app.theme, title, OsStr::new(""), false)
                    {
                        let operation = if id == PROMPT_FILE {
                            Operation::NewFile(dir, name)
                        } else {
                            Operation::NewFolder(dir, name)
                        };
                        start_operation(hwnd, app, operation);
                    }
                }
                PROMPT_RENAME if code == 0 => {
                    if let Some(path) = selected_path(hwnd, app) {
                        if let Some(name) = windows_actions::prompt(
                            hwnd,
                            app.theme,
                            "Rename",
                            path.file_name().unwrap_or_default(),
                            true,
                        ) {
                            start_operation(hwnd, app, Operation::Rename(path, name));
                        }
                    }
                }
                NEW_FILE | NEW_FOLDER if code == 0 => {
                    let name = control_value(app.name_control);
                    if name.is_empty() {
                        SendMessageW(
                            hwnd,
                            WM_COMMAND,
                            if id == NEW_FILE {
                                PROMPT_FILE
                            } else {
                                PROMPT_FOLDER
                            },
                            0,
                        );
                        return 0;
                    }
                    let dir = app.tabs.current().current().to_path_buf();
                    let operation = if id == NEW_FILE {
                        Operation::NewFile(dir, name)
                    } else {
                        Operation::NewFolder(dir, name)
                    };
                    start_operation(hwnd, app, operation);
                }
                RENAME if code == 0 => {
                    let path = selected_path(hwnd, app);
                    if let Some(path) = path {
                        let name = control_value(app.name_control);
                        if name.is_empty() {
                            if let Some(name) = windows_actions::prompt(
                                hwnd,
                                app.theme,
                                "Rename",
                                path.file_name().unwrap_or_default(),
                                true,
                            ) {
                                start_operation(hwnd, app, Operation::Rename(path, name));
                            }
                        } else {
                            start_operation(hwnd, app, Operation::Rename(path, name));
                        }
                    } else {
                        set_status(
                            hwnd,
                            app,
                            Some("Select an item and enter a new name.".into()),
                        );
                    }
                }
                DELETE if code == 0 => {
                    let path = selected_path(hwnd, app);
                    if let Some(path) = path {
                        let message = format!(
                            "Permanently delete {}? This cannot be undone. Only files or empty folders can be deleted.",
                            path.display()
                        );
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
                        set_status(hwnd, app, Some("Select an item to delete.".into()));
                    }
                }
                LIST if code == 1 => {
                    invalidate_status_bar(hwnd);
                }
                PREVIEW if code == 0 => {
                    if let Some(path) = selected_path(hwnd, app) { windows_viewer::show(hwnd, app.theme, &path); }
                }
                LIST if code == 2 => {
                    let indices = selected_indices(app);
                    let entry = if indices.len() == 1 {
                        app.entries
                            .get(indices[0])
                            .map(|e| (e.path.clone(), e.is_dir))
                    } else {
                        None
                    };
                    if let Some((path, is_dir)) = entry {
                        if is_dir {
                            load(hwnd, app, Destination::Visit(path), false);
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
                _ => {}
            }
            0
        }
        WM_CTLCOLOREDIT => {
            let theme = &*app.theme;
            let ctrl = lp as H;
            SetTextColor(wp as H, ui_theme::TEXT_BRIGHT.colorref());
            if ctrl == app.address {
                SetBkColor(wp as H, ui_theme::ADDRESS_EDIT_BG.colorref());
                theme.address_brush as LResult
            } else if ctrl == app.filter_control {
                SetBkColor(wp as H, ui_theme::FILTER_EDIT_BG.colorref());
                theme.filter_brush as LResult
            } else {
                SetBkColor(wp as H, ui_theme::NAME_EDIT_BG.colorref());
                theme.name_brush as LResult
            }
        }
        WM_CTLCOLORLISTBOX | WM_CTLCOLORSTATIC => {
            let theme = &*app.theme;
            SetTextColor(wp as H, ui_theme::TEXT_BRIGHT.colorref());
            SetBkColor(wp as H, ui_theme::ROW_EVEN.colorref());
            theme.list_brush as LResult
        }
        WM_DESTROY => {
            app.token.fetch_add(1, Ordering::Relaxed);
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
        let theme = Theme::new()?;
        let class = text("ZFMNativeWindow");
        let wc = WndClass {
            style: 0x0008, // CS_DBLCLKS
            proc: Some(window_proc),
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: companion_icon(),
            cursor: LoadCursorW(ptr::null_mut(), 32512usize as *const u16),
            background: theme.window_brush,
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

        let app = Box::into_raw(Box::new(App {
            theme: &theme,
            tabs: Tabs::new(initial),
            filter: String::new(),
            show_hidden: false,
            drives_expanded: true,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            view_mode: ViewMode::Details,
            focused_edit: 0,
            hovered_child: ptr::null_mut(),
            hovered_row: None,
            hot_zone: HotZone::None,
            tracking_mouse: false,
            operation_busy: false,
            notice: None,
            token: Arc::new(AtomicU64::new(0)),
            address: ptr::null_mut(),
            filter_control: ptr::null_mut(),
            name_control: ptr::null_mut(),
            list: ptr::null_mut(),
            status: ptr::null_mut(),
            nav_buttons: [ptr::null_mut(); 6],
            extra_buttons: [ptr::null_mut(); 9],
            sidebar_items: build_sidebar_items(true),
            entries: Vec::new(),
            backdrop_w: 0,
            backdrop_h: 0,
            backdrop: Vec::new(),
        }));

        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            text("ZFM — Zelvior File Manager").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            0x8000_0000u32 as i32,
            0x8000_0000u32 as i32,
            1040,
            640,
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
                let shift = GetKeyState(0x10) < 0;
                if ctrl && !alt && (msg.wparam == 0x09 || msg.wparam == 0x21 || msg.wparam == 0x22)
                {
                    let previous = msg.wparam == 0x21 || (msg.wparam == 0x09 && shift);
                    let state = &mut *app;
                    if if previous {
                        state.tabs.prev_tab()
                    } else {
                        state.tabs.next_tab()
                    } {
                        rebuild_backdrop(window, state);
                        InvalidateRect(window, ptr::null(), 0);
                        load(window, state, Destination::Refresh, false);
                    }
                    continue;
                }
                if (ctrl || alt) && (0x31..=0x39).contains(&msg.wparam) {
                    let state = &mut *app;
                    if state.tabs.select(msg.wparam - 0x31) {
                        rebuild_backdrop(window, state);
                        InvalidateRect(window, ptr::null(), 0);
                        load(window, state, Destination::Refresh, false);
                    }
                    continue;
                }
                if msg.wparam == 0x1b {
                    clear_selection((*app).list);
                    SetFocus((*app).list);
                    invalidate_status_bar(window);
                    continue;
                }
                if focus == (*app).list && (msg.wparam == 0x5d || (msg.wparam == 0x79 && shift)) {
                    SendMessageW(window, WM_CONTEXTMENU, (*app).list as usize, -1);
                    continue;
                }
                let id = match msg.wparam {
                    0x74 if !ctrl && !alt => Some(REFRESH),
                    0x71 if !ctrl && !alt && focus == (*app).list => Some(PROMPT_RENAME),
                    0x43 if ctrl && !alt && shift => Some(COPY_PATH),
                    0x43 if ctrl && !alt && focus == (*app).list => Some(COPY),
                    0x58 if ctrl && !alt && focus == (*app).list => Some(CUT),
                    0x56 if ctrl && !alt && focus == (*app).list => Some(PASTE),
                    0x44 if ctrl && !alt && focus == (*app).list => Some(DUPLICATE),
                    0x4e if ctrl && !alt && shift => Some(PROMPT_FOLDER),
                    0x4e if ctrl && !alt => Some(PROMPT_FILE),
                    0x0d if alt && !ctrl && focus == (*app).list => Some(PROPERTIES),
                    0x2e if focus == (*app).list && !ctrl && !alt && !shift => Some(TRASH),
                    0x52 if ctrl && !alt => Some(REFRESH),
                    0x48 if ctrl && !alt => Some(HIDDEN),
                    0xbc if ctrl && !alt => Some(SETTINGS),
                    0x08 if focus == (*app).list && !ctrl && !alt => Some(UP),
                    0x54 if ctrl && !alt => Some(NEW_TAB),
                    0x57 if ctrl && !alt => Some(CLOSE_TAB),
                    0x25 if alt && !ctrl => Some(BACK),
                    0x27 if alt && !ctrl => Some(FORWARD),
                    0x26 if alt && !ctrl => Some(UP),
                    0x0d if focus == (*app).address => Some(GO),
                    0x0d if focus == (*app).filter_control && !alt => Some(APPLY_FILTER),
                    0x0d if focus == (*app).name_control && !alt => Some(NEW_FILE),
                    0x2e if focus == (*app).list && !ctrl && !alt && shift => Some(DELETE),
                    _ => None,
                };
                if let Some(id) = id {
                    SendMessageW(window, WM_COMMAND, id, 0);
                    continue;
                }
                if (ctrl && !alt && msg.wparam == 0x4c) || (alt && !ctrl && msg.wparam == 0x44) {
                    SetFocus((*app).address);
                    SendMessageW((*app).address, EM_SETSEL, 0, -1);
                    continue;
                }
                if ctrl && !alt && msg.wparam == 0x46 {
                    SetFocus((*app).filter_control);
                    SendMessageW((*app).filter_control, EM_SETSEL, 0, -1);
                    continue;
                }
                if msg.wparam == 0x0d && focus == (*app).list && !alt && !ctrl {
                    SendMessageW(window, WM_COMMAND, LIST | (2 << 16), (*app).list as isize);
                    continue;
                }
            }
            if IsDialogMessageW(window, &mut msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        Ok(())
    }
}
