//! The small set of Win32 calls the setup wizard needs.
//!
//! The declarations mirror `src/windows_ui.rs`: only user32, gdi32, kernel32,
//! advapi32, shell32 and ole32 entry points are declared, so the installer has
//! no runtime dependencies and no C toolchain beyond the linker.

#![allow(dead_code)]
#![allow(non_snake_case)]

use std::ffi::{c_void, OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr;

pub type Handle = *mut c_void;
pub type Hwnd = *mut c_void;
pub type Hdc = *mut c_void;
pub type Hicon = *mut c_void;
pub type Hfont = *mut c_void;
pub type Hbrush = *mut c_void;
pub type Hpen = *mut c_void;
pub type Hgdiobj = *mut c_void;
pub type Hkey = *mut c_void;
pub type Hinstance = *mut c_void;
pub type WParam = usize;
pub type LParam = isize;
pub type LResult = isize;

#[repr(C)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub struct Msg {
    pub hwnd: Hwnd,
    pub message: u32,
    pub wparam: WParam,
    pub lparam: LParam,
    pub time: u32,
    pub point: Point,
    pub private: u32,
}

#[repr(C)]
pub struct DrawItem {
    pub control_type: u32,
    pub control_id: u32,
    pub item_id: u32,
    pub item_action: u32,
    pub item_state: u32,
    pub hwnd: Hwnd,
    pub dc: Hdc,
    pub rect: Rect,
    pub item_data: usize,
}

#[repr(C)]
pub struct PaintStruct {
    pub dc: Hdc,
    pub erase: i32,
    pub paint: Rect,
    pub restore: i32,
    pub inc_update: i32,
    pub reserved: [u8; 32],
}

#[repr(C)]
pub struct BitmapInfoHeader {
    pub size: u32,
    pub width: i32,
    pub height: i32,
    pub planes: u16,
    pub bit_count: u16,
    pub compression: u32,
    pub size_image: u32,
    pub x_pels_per_meter: i32,
    pub y_pels_per_meter: i32,
    pub clr_used: u32,
    pub clr_important: u32,
}

#[repr(C)]
pub struct BitmapInfo {
    pub header: BitmapInfoHeader,
    pub colors: [u32; 1],
}

#[repr(C)]
pub struct WndClass {
    pub style: u32,
    pub proc: Option<unsafe extern "system" fn(Hwnd, u32, WParam, LParam) -> LResult>,
    pub class_extra: i32,
    pub window_extra: i32,
    pub instance: Hinstance,
    pub icon: Hicon,
    pub cursor: Handle,
    pub background: Hbrush,
    pub menu: *const u16,
    pub class: *const u16,
}

#[repr(C)]
pub struct BrowseInfo {
    owner: Hwnd,
    root: *mut c_void,
    display: *mut u16,
    title: *const u16,
    flags: u32,
    callback: *mut c_void,
    parameter: LParam,
    image: i32,
}

#[repr(C)]
pub struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const CLSID_SHELL_LINK: Guid = Guid {
    data1: 0x0002_1401,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const IID_ISHELL_LINK_W: Guid = Guid {
    data1: 0x0002_14f9,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const IID_IPERSIST_FILE: Guid = Guid {
    data1: 0x0000_010b,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const COINIT_APARTMENTTHREADED: u32 = 0x2;
const COINIT_DISABLE_OLE1DDE: u32 = 0x4;
const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;

/// HKEY_CURRENT_USER. The installer only writes per-user settings.
pub const HKEY_CURRENT_USER: Hkey = 0x8000_0001usize as Hkey;
const KEY_WRITE: u32 = 0x0002_0006;
const KEY_READ: u32 = 0x0002_0019;
const REG_SZ: u32 = 1;
const REG_DWORD: u32 = 4;
pub const ERROR_FILE_NOT_FOUND: u32 = 2;

pub const MB_OK: u32 = 0x0000_0000;
pub const MB_YESNO: u32 = 0x0000_0004;
pub const MB_ICONERROR: u32 = 0x0000_0010;
pub const MB_ICONQUESTION: u32 = 0x0000_0020;
pub const MB_SETFOREGROUND: u32 = 0x0001_0000;
pub const IDYES: i32 = 6;

pub const SW_HIDE: i32 = 0;
pub const SW_SHOW: i32 = 5;
pub const ICON_BIG: WParam = 1;
pub const ICON_SMALL: WParam = 0;
pub const WM_SETICON: u32 = 0x0080;

pub const NULL_BRUSH: i32 = 5;
pub const NULL_PEN: i32 = 8;
pub const DT_CENTER: u32 = 0x0000_0001;
pub const DT_VCENTER: u32 = 0x0000_0004;
pub const DT_WORDBREAK: u32 = 0x0000_0010;
pub const DT_SINGLELINE: u32 = 0x0000_0020;
pub const DT_NOPREFIX: u32 = 0x0000_0800;
pub const DT_EDITCONTROL: u32 = 0x0000_2000;
const SRCCOPY: u32 = 0x00cc_0020;

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassW(class: *const WndClass) -> u16;
    pub fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: Handle,
        instance: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    pub fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: WParam, lparam: LParam) -> LResult;
    pub fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
    pub fn UpdateWindow(hwnd: Hwnd) -> i32;
    pub fn GetMessageW(message: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(message: *const Msg) -> i32;
    pub fn DispatchMessageW(message: *const Msg) -> LResult;
    pub fn PostQuitMessage(code: i32);
    pub fn PostMessageW(hwnd: Hwnd, message: u32, wparam: WParam, lparam: LParam) -> i32;
    pub fn SendMessageW(hwnd: Hwnd, message: u32, wparam: WParam, lparam: LParam) -> LResult;
    pub fn DestroyWindow(hwnd: Hwnd) -> i32;
    pub fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
    pub fn GetWindowTextLengthW(hwnd: Hwnd) -> i32;
    pub fn GetWindowTextW(hwnd: Hwnd, buffer: *mut u16, max: i32) -> i32;
    pub fn MoveWindow(hwnd: Hwnd, x: i32, y: i32, width: i32, height: i32, repaint: i32) -> i32;
    pub fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    pub fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    pub fn ScreenToClient(hwnd: Hwnd, point: *mut Point) -> i32;
    pub fn MessageBoxW(hwnd: Hwnd, text: *const u16, caption: *const u16, kind: u32) -> i32;
    pub fn SetFocus(hwnd: Hwnd) -> Hwnd;
    pub fn GetFocus() -> Hwnd;
    pub fn EnableWindow(hwnd: Hwnd, enable: i32) -> i32;
    pub fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
    pub fn BeginPaint(hwnd: Hwnd, paint: *mut PaintStruct) -> Hdc;
    pub fn EndPaint(hwnd: Hwnd, paint: *const PaintStruct) -> i32;
    pub fn FillRect(dc: Hdc, rect: *const Rect, brush: Hbrush) -> i32;
    pub fn FrameRect(dc: Hdc, rect: *const Rect, brush: Hbrush) -> i32;
    pub fn DrawTextW(
        dc: Hdc,
        text: *const u16,
        count: i32,
        rect: *mut Rect,
        format: u32,
    ) -> i32;
    pub fn LoadCursorW(instance: Hinstance, name: *const u16) -> Handle;
    pub fn SetCursor(cursor: Handle) -> Handle;
    pub fn IsDialogMessageW(hwnd: Hwnd, message: *mut Msg) -> i32;
    pub fn CreateIconFromResourceEx(
        bits: *mut u8,
        size: u32,
        icon: i32,
        version: u32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> Hicon;
    pub fn DestroyIcon(icon: Hicon) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateSolidBrush(color: u32) -> Hbrush;
    pub fn CreateCompatibleDC(dc: Hdc) -> Hdc;
    pub fn DeleteDC(dc: Hdc) -> i32;
    pub fn CreateDIBSection(
        dc: Hdc,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: Handle,
        offset: u32,
    ) -> Handle;
    pub fn BitBlt(
        dest: Hdc,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        src: Hdc,
        src_x: i32,
        src_y: i32,
        rop: u32,
    ) -> i32;
    pub fn CreateFontW(
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
    ) -> Hfont;
    pub fn CreatePen(style: i32, width: i32, color: u32) -> Hpen;
    pub fn SelectObject(dc: Hdc, object: Hgdiobj) -> Hgdiobj;
    pub fn DeleteObject(object: Hgdiobj) -> i32;
    pub fn GetStockObject(index: i32) -> Hgdiobj;
    pub fn SetTextColor(dc: Hdc, color: u32) -> u32;
    pub fn SetBkColor(dc: Hdc, color: u32) -> u32;
    pub fn SetBkMode(dc: Hdc, mode: i32) -> i32;
    pub fn RoundRect(
        dc: Hdc,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        width: i32,
        height: i32,
    ) -> i32;
    pub fn Polygon(dc: Hdc, points: *mut Point, count: i32) -> i32;
    pub fn Rectangle(dc: Hdc, left: i32, top: i32, right: i32, bottom: i32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> Hinstance;
    pub fn LoadLibraryW(name: *const u16) -> Hinstance;
    pub fn GetProcAddress(module: Hinstance, name: *const u8) -> *mut c_void;
    pub fn GetEnvironmentVariableW(name: *const u16, buffer: *mut u16, size: u32) -> u32;
    pub fn GetTempPathW(size: u32, buffer: *mut u16) -> u32;
    pub fn GetModuleFileNameW(module: Hinstance, buffer: *mut u16, size: u32) -> u32;
    pub fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    pub fn CloseHandle(handle: Handle) -> i32;
    pub fn Sleep(milliseconds: u32);
}

#[link(name = "advapi32")]
extern "system" {
    pub fn RegCreateKeyExW(
        key: Hkey,
        sub_key: *const u16,
        reserved: u32,
        class: *mut u16,
        options: u32,
        desired: u32,
        attributes: *mut c_void,
        result: *mut Hkey,
        disposition: *mut u32,
    ) -> u32;
    pub fn RegOpenKeyExW(
        key: Hkey,
        sub_key: *const u16,
        options: u32,
        desired: u32,
        result: *mut Hkey,
    ) -> u32;
    pub fn RegSetValueExW(
        key: Hkey,
        name: *const u16,
        reserved: u32,
        kind: u32,
        data: *const u8,
        size: u32,
    ) -> u32;
    pub fn RegQueryValueExW(
        key: Hkey,
        name: *const u16,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> u32;
    pub fn RegDeleteKeyW(key: Hkey, sub_key: *const u16) -> u32;
    pub fn RegCloseKey(key: Hkey) -> u32;
}

#[link(name = "shell32")]
extern "system" {
    pub fn SHGetFolderPathW(
        hwnd: Hwnd,
        folder: i32,
        token: Handle,
        flags: u32,
        path: *mut u16,
    ) -> i32;
    pub fn SHBrowseForFolderW(info: *mut BrowseInfo) -> *mut c_void;
    pub fn SHGetPathFromIDListW(item: *mut c_void, path: *mut u16) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    pub fn CoInitializeEx(reserved: *mut c_void, model: u32) -> i32;
    pub fn CoUninitialize();
    pub fn CoCreateInstance(
        class: *const Guid,
        outer: *mut c_void,
        context: u32,
        interface: *const Guid,
        result: *mut *mut c_void,
    ) -> i32;
    pub fn CoTaskMemFree(memory: *mut c_void);
}

pub fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

pub fn text(value: &str) -> Vec<u16> {
    wide(OsStr::new(value))
}

pub fn os_from_wide(buffer: &[u16]) -> OsString {
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    OsString::from_wide(&buffer[..length])
}

pub fn set_text(hwnd: Hwnd, value: &str) {
    unsafe {
        SetWindowTextW(hwnd, text(value).as_ptr());
    }
}

pub fn window_text(hwnd: Hwnd) -> String {
    unsafe {
        let length = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), (length + 1) as i32).max(0) as usize;
        String::from_utf16_lossy(&buffer[..copied])
    }
}

pub fn show(hwnd: Hwnd, visible: bool) {
    unsafe {
        ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
    }
}

pub fn message(owner: Hwnd, body: &str, title: &str, kind: u32) {
    unsafe {
        MessageBoxW(
            owner,
            text(body).as_ptr(),
            text(title).as_ptr(),
            kind | MB_SETFOREGROUND,
        );
    }
}

pub fn confirm(owner: Hwnd, body: &str, title: &str) -> bool {
    unsafe {
        MessageBoxW(
            owner,
            text(body).as_ptr(),
            text(title).as_ptr(),
            MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND,
        ) == IDYES
    }
}

pub fn environment(name: &str) -> Option<PathBuf> {
    let name = text(name);
    let mut buffer = vec![0u16; 260];
    loop {
        let length = unsafe {
            GetEnvironmentVariableW(name.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32)
        };
        if length == 0 {
            return None;
        }
        if (length as usize) < buffer.len() {
            return Some(PathBuf::from(os_from_wide(&buffer[..length as usize])));
        }
        buffer.resize(length as usize + 1, 0);
    }
}

pub fn temporary_folder() -> Option<PathBuf> {
    let mut buffer = vec![0u16; 260];
    loop {
        let length = unsafe { GetTempPathW(buffer.len() as u32, buffer.as_mut_ptr()) };
        if length == 0 {
            return None;
        }
        if (length as usize) < buffer.len() {
            return Some(PathBuf::from(os_from_wide(&buffer[..length as usize])));
        }
        buffer.resize(length as usize + 1, 0);
    }
}

pub fn programs_folder() -> Option<PathBuf> {
    // CSIDL_PROGRAMS == 2 for the current user.
    let mut buffer = [0u16; 260];
    let result = unsafe {
        SHGetFolderPathW(ptr::null_mut(), 2, ptr::null_mut(), 0, buffer.as_mut_ptr())
    };
    if result != 0 {
        return None;
    }
    Some(PathBuf::from(os_from_wide(&buffer)))
}

pub fn browse_for_folder(owner: Hwnd, title: &str) -> Option<PathBuf> {
    let _com = ComGuard::new().ok()?;
    let title = text(title);
    let mut display = [0u16; 260];
    let mut path = [0u16; 260];
    unsafe {
        let mut info = BrowseInfo {
            owner,
            root: ptr::null_mut(),
            display: display.as_mut_ptr(),
            title: title.as_ptr(),
            // BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE
            flags: 0x0000_0001 | 0x0000_0040,
            callback: ptr::null_mut(),
            parameter: 0,
            image: 0,
        };
        let item = SHBrowseForFolderW(&mut info);
        if item.is_null() {
            return None;
        }
        let converted = SHGetPathFromIDListW(item, path.as_mut_ptr());
        CoTaskMemFree(item);
        if converted == 0 {
            return None;
        }
    }
    Some(PathBuf::from(os_from_wide(&path)))
}

pub fn icon_from_png(png: &[u8]) -> Hicon {
    unsafe {
        CreateIconFromResourceEx(
            png.as_ptr() as *mut u8,
            png.len() as u32,
            1,
            0x0003_0000,
            0,
            0,
            0,
        )
    }
}

pub fn set_icon(hwnd: Hwnd, icon: Hicon) {
    if icon.is_null() {
        return;
    }
    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_BIG, icon as LParam);
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL, icon as LParam);
    }
}

pub fn child_origin(parent: Hwnd, child: Hwnd) -> (i32, i32) {
    let mut rect = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        if GetWindowRect(child, &mut rect) == 0 {
            return (0, 0);
        }
        let mut pt = Point {
            x: rect.left,
            y: rect.top,
        };
        if ScreenToClient(parent, &mut pt) == 0 {
            return (0, 0);
        }
        (pt.x, pt.y)
    }
}

/// Blit a top-down 32-bit `0x00RRGGBB` pixel buffer onto `dest_dc`, running
/// `overlay` on the memory DC beforehand so ClearType text composites cleanly
/// onto the anti-aliased pixels without flicker.
pub fn blit_pixels<F>(
    dest_dc: Hdc,
    dst_x: i32,
    dst_y: i32,
    width: i32,
    height: i32,
    pixels: &[u32],
    overlay: F,
) where
    F: FnOnce(Hdc),
{
    if width <= 0 || height <= 0 {
        return;
    }
    let count = (width * height) as usize;
    if pixels.len() < count {
        return;
    }
    unsafe {
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
        let mut bits: *mut c_void = ptr::null_mut();
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
}

/// Best-effort Windows 11 DWM rounded corner and slate-blue caption tinting;
/// dynamically loaded so Windows 7 SP1 continues to run without `dwmapi`
/// symbols it does not export.
pub fn apply_window_chrome(
    hwnd: Hwnd,
    caption_colorref: u32,
    border_colorref: u32,
    text_colorref: u32,
) {
    unsafe {
        let module = LoadLibraryW(text("dwmapi.dll").as_ptr());
        if module.is_null() {
            return;
        }
        let proc = GetProcAddress(module, b"DwmSetWindowAttribute\0".as_ptr());
        if proc.is_null() {
            return;
        }
        let set_attr: unsafe extern "system" fn(Hwnd, u32, *const c_void, u32) -> i32 =
            std::mem::transmute(proc);
        let round: u32 = 2; // DWMWCP_ROUND
        let size = std::mem::size_of::<u32>() as u32;
        let _ = set_attr(hwnd, 33, &round as *const u32 as *const c_void, size);
        let _ = set_attr(
            hwnd,
            34,
            &border_colorref as *const u32 as *const c_void,
            size,
        );
        let _ = set_attr(
            hwnd,
            35,
            &caption_colorref as *const u32 as *const c_void,
            size,
        );
        let _ = set_attr(
            hwnd,
            36,
            &text_colorref as *const u32 as *const c_void,
            size,
        );
    }
}

/// Apply the modern Explorer scrollbar theme to a scrollable control when
/// `uxtheme.dll` is present.
pub fn apply_explorer_scrollbar(hwnd: Hwnd) {
    unsafe {
        let module = LoadLibraryW(text("uxtheme.dll").as_ptr());
        if module.is_null() {
            return;
        }
        let proc = GetProcAddress(module, b"SetWindowTheme\0".as_ptr());
        if proc.is_null() {
            return;
        }
        let set_theme: unsafe extern "system" fn(Hwnd, *const u16, *const u16) -> i32 =
            std::mem::transmute(proc);
        let _ = set_theme(hwnd, text("Explorer").as_ptr(), ptr::null());
    }
}

pub fn open_key(root: Hkey, sub_key: &str) -> Option<Hkey> {
    let name = text(sub_key);
    let mut key: Hkey = ptr::null_mut();
    let result = unsafe { RegOpenKeyExW(root, name.as_ptr(), 0, KEY_READ, &mut key) };
    if result == 0 {
        Some(key)
    } else {
        None
    }
}

pub fn create_key(root: Hkey, sub_key: &str) -> Result<Hkey, String> {
    let name = text(sub_key);
    let mut key: Hkey = ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            root,
            name.as_ptr(),
            0,
            ptr::null_mut(),
            0,
            KEY_WRITE,
            ptr::null_mut(),
            &mut key,
            ptr::null_mut(),
        )
    };
    if result == 0 {
        Ok(key)
    } else {
        Err(format!(
            "Could not create the registry key {sub_key} (error {result})."
        ))
    }
}

pub fn close_key(key: Hkey) {
    unsafe {
        RegCloseKey(key);
    }
}

pub fn set_string(key: Hkey, name: &str, value: &str) -> Result<(), String> {
    let wide_name = text(name);
    let data = text(value);
    let bytes = (data.len() * 2) as u32;
    let result = unsafe {
        RegSetValueExW(
            key,
            wide_name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr() as *const u8,
            bytes,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "Could not write the registry value {name} (error {result})."
        ))
    }
}

pub fn set_dword(key: Hkey, name: &str, value: u32) -> Result<(), String> {
    let wide_name = text(name);
    let data = value.to_le_bytes();
    let result = unsafe {
        RegSetValueExW(key, wide_name.as_ptr(), 0, REG_DWORD, data.as_ptr(), 4)
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "Could not write the registry value {name} (error {result})."
        ))
    }
}

pub fn get_string(root: Hkey, sub_key: &str, name: &str) -> Option<String> {
    let key = open_key(root, sub_key)?;
    let value_name = text(name);
    let mut size: u32 = 0;
    let status = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        )
    };
    let mut result = None;
    if status == 0 && size >= 2 {
        let mut buffer = vec![0u8; size as usize + 2];
        let mut size = buffer.len() as u32;
        let status = unsafe {
            RegQueryValueExW(
                key,
                value_name.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                buffer.as_mut_ptr(),
                &mut size,
            )
        };
        if status == 0 {
            let units: Vec<u16> = buffer[..size as usize]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            result = Some(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string());
        }
    }
    close_key(key);
    result
}

pub fn delete_key(root: Hkey, sub_key: &str) -> u32 {
    let name = text(sub_key);
    unsafe { RegDeleteKeyW(root, name.as_ptr()) }
}

/// Move a running uninstaller copy away so Windows deletes it at the next
/// restart; a running executable cannot delete its own file.
pub fn delete_on_reboot(path: &Path) {
    let name = wide(path.as_os_str());
    unsafe {
        MoveFileExW(name.as_ptr(), ptr::null(), 0x0000_0004);
    }
}

/// Per-thread COM apartment guard.
///
/// Shell links (`CLSID_ShellLink`) and the modern folder picker require COM to
/// be initialized on the calling thread; initializing COM on the wizard UI
/// thread does not cover the background installation worker thread, which
/// otherwise fails with `CO_E_NOTINITIALIZED` (`0x800401f0` / `-2147221008`).
pub struct ComGuard {
    uninitialize: bool,
}

impl ComGuard {
    pub fn new() -> Result<Self, i32> {
        let status = unsafe {
            CoInitializeEx(
                ptr::null_mut(),
                COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
            )
        };
        if status >= 0 {
            Ok(Self { uninitialize: true })
        } else if status == RPC_E_CHANGED_MODE {
            Ok(Self {
                uninitialize: false,
            })
        } else {
            Err(status)
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

pub fn format_hresult(code: i32) -> String {
    format!("0x{:08x}", code as u32)
}

/// Create a Windows shell shortcut. Returns an error instead of falling back to
/// a plain file, so a broken Start Menu entry is reported by setup.
pub fn create_shortcut(
    link: &Path,
    target: &Path,
    icon: &Path,
    description: &str,
    working_directory: &Path,
) -> Result<(), String> {
    let _com = ComGuard::new().map_err(|code| {
        format!(
            "Could not initialize COM for the shortcut {} (error {}).",
            link.display(),
            format_hresult(code)
        )
    })?;
    unsafe {
        let mut shell_link: *mut c_void = ptr::null_mut();
        let created = CoCreateInstance(
            &CLSID_SHELL_LINK,
            ptr::null_mut(),
            1, // CLSCTX_INPROC_SERVER
            &IID_ISHELL_LINK_W,
            &mut shell_link,
        );
        if created != 0 || shell_link.is_null() {
            return Err(format!(
                "Could not create the shortcut {} (error {}).",
                link.display(),
                format_hresult(created)
            ));
        }
        // The object starts with a pointer to its vtable; index that table.
        let table = *(shell_link as *const *const *const c_void);
        let set_path: unsafe extern "system" fn(*mut c_void, *const u16) -> i32 =
            std::mem::transmute(*table.add(20));
        let set_description: unsafe extern "system" fn(*mut c_void, *const u16) -> i32 =
            std::mem::transmute(*table.add(7));
        let set_working_directory: unsafe extern "system" fn(*mut c_void, *const u16) -> i32 =
            std::mem::transmute(*table.add(9));
        let set_icon_location: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32 =
            std::mem::transmute(*table.add(17));
        let query_interface: unsafe extern "system" fn(
            *mut c_void,
            *const Guid,
            *mut *mut c_void,
        ) -> i32 = std::mem::transmute(*table.add(0));
        let release: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*table.add(2));

        let target_name = wide(target.as_os_str());
        let working_name = wide(working_directory.as_os_str());
        let icon_name = wide(icon.as_os_str());
        let description = text(description);
        set_path(shell_link, target_name.as_ptr());
        set_working_directory(shell_link, working_name.as_ptr());
        set_icon_location(shell_link, icon_name.as_ptr(), 0);
        set_description(shell_link, description.as_ptr());

        let mut persist: *mut c_void = ptr::null_mut();
        let queried = query_interface(shell_link, &IID_IPERSIST_FILE, &mut persist);
        if queried != 0 || persist.is_null() {
            release(shell_link);
            return Err(format!(
                "Could not save the shortcut {} (error {}).",
                link.display(),
                format_hresult(queried)
            ));
        }
        let persist_table = *(persist as *const *const *const c_void);
        let save: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32 =
            std::mem::transmute(*persist_table.add(6));
        let release_persist: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*persist_table.add(2));
        let link_name = wide(link.as_os_str());
        let saved = save(persist, link_name.as_ptr(), 1);
        release_persist(persist);
        release(shell_link);
        if saved != 0 {
            return Err(format!(
                "Could not save the shortcut {} (error {}).",
                link.display(),
                format_hresult(saved)
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zfm-win32-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_shortcuts_on_a_fresh_worker_thread() {
        let dir = scratch();
        let target = dir.join("zfm.exe");
        let icon = dir.join("zfm.ico");
        fs::write(&target, b"placeholder").unwrap();
        fs::write(&icon, crate::payload::ICON).unwrap();

        let work = dir.clone();
        let result = thread::spawn(move || {
            let first = work.join("ZFM File Manager.lnk");
            let second = work.join("Uninstall ZFM.lnk");
            create_shortcut(
                &first,
                &work.join("zfm.exe"),
                &work.join("zfm.ico"),
                "Browse and manage files",
                &work,
            )?;
            create_shortcut(
                &second,
                &work.join("zfm.exe"),
                &work.join("zfm.ico"),
                "Remove ZFM File Manager",
                &work,
            )?;
            Ok::<_, String>((first.is_file(), second.is_file()))
        })
        .join()
        .unwrap();

        let (first_exists, second_exists) = result.expect("worker thread shortcut creation");
        assert!(first_exists);
        assert!(second_exists);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_com_guards_keep_thread_usable() {
        let dir = scratch();
        let target = dir.join("zfm.exe");
        let icon = dir.join("zfm.ico");
        fs::write(&target, b"placeholder").unwrap();
        fs::write(&icon, crate::payload::ICON).unwrap();

        let _outer = ComGuard::new().expect("outer COM guard");
        let link = dir.join("Nested.lnk");
        create_shortcut(&link, &target, &icon, "Nested COM check", &dir)
            .expect("shortcut inside outer COM guard");
        let second = dir.join("AfterInnerDrop.lnk");
        create_shortcut(&second, &target, &icon, "Second shortcut", &dir)
            .expect("outer COM guard remains active after inner guard drops");
        assert!(link.is_file());
        assert!(second.is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}
