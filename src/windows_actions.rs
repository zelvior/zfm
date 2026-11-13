//! Windows dialogs, shell properties and interoperable file clipboard.
use super::*;

#[repr(C)]
struct DropFiles {
    offset: u32,
    point: Point,
    non_client: i32,
    wide: i32,
}
#[repr(C)]
pub(super) struct ShellExecuteInfo {
    size: u32,
    mask: u32,
    hwnd: H,
    verb: *const u16,
    file: *const u16,
    parameters: *const u16,
    directory: *const u16,
    show: i32,
    instance: H,
    id_list: P,
    class: *const u16,
    class_key: H,
    hot_key: u32,
    icon: H,
    process: H,
}

pub(super) unsafe fn properties(hwnd: H, path: &Path) {
    let verb = text("properties");
    let name = wide(path.as_os_str());
    let mut info: ShellExecuteInfo = std::mem::zeroed();
    info.size = std::mem::size_of::<ShellExecuteInfo>() as u32;
    info.mask = 0x0c; // SEE_MASK_INVOKEIDLIST
    info.hwnd = hwnd;
    info.verb = verb.as_ptr();
    info.file = name.as_ptr();
    info.show = 1;
    if ShellExecuteExW(&mut info) == 0 {
        show_error(hwnd, "Windows could not show properties for this item.");
    }
}

unsafe fn set_format(format: u32, bytes: &[u8]) -> bool {
    let h = GlobalAlloc(0x0002, bytes.len()); // GMEM_MOVEABLE
    if h.is_null() {
        return false;
    }
    let dst = GlobalLock(h);
    if dst.is_null() {
        GlobalFree(h);
        return false;
    }
    ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, bytes.len());
    GlobalUnlock(h);
    if SetClipboardData(format, h).is_null() {
        GlobalFree(h);
        false
    } else {
        true
    }
}

pub(super) unsafe fn copy_text(hwnd: H, path: &Path) -> bool {
    let value = wide(path.as_os_str());
    let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
    if OpenClipboard(hwnd) == 0 {
        return false;
    }
    let ok = EmptyClipboard() != 0 && set_format(13, bytes); // CF_UNICODETEXT
    CloseClipboard();
    ok
}

pub(super) unsafe fn copy_files(hwnd: H, paths: &[PathBuf], cut: bool) -> bool {
    if paths.is_empty()
        || paths.len() > zfm::operations::MAX_COPY_ITEMS
        || (cut && paths.len() != 1)
    {
        return false;
    }
    let mut values = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        if !path.is_absolute() || !seen.insert(path) {
            return false;
        }
        let value = wide(path.as_os_str());
        if value.len() > 32768 || values.len() + value.len() > 524_287 {
            return false;
        }
        values.extend_from_slice(&value);
    }
    values.push(0); // second NUL terminates CF_HDROP's filename list
    let header = DropFiles {
        offset: std::mem::size_of::<DropFiles>() as u32,
        point: Point { x: 0, y: 0 },
        non_client: 0,
        wide: 1,
    };
    let mut bytes = vec![0u8; std::mem::size_of::<DropFiles>() + values.len() * 2];
    ptr::copy_nonoverlapping(
        &header as *const _ as *const u8,
        bytes.as_mut_ptr(),
        std::mem::size_of::<DropFiles>(),
    );
    ptr::copy_nonoverlapping(
        values.as_ptr() as *const u8,
        bytes.as_mut_ptr().add(std::mem::size_of::<DropFiles>()),
        values.len() * 2,
    );
    let effect = if cut { 2u32 } else { 1u32 };
    let format = RegisterClipboardFormatW(text("Preferred DropEffect").as_ptr());
    if format == 0 || OpenClipboard(hwnd) == 0 {
        return false;
    }
    let ok = EmptyClipboard() != 0
        && set_format(15, &bytes)
        && set_format(format, &effect.to_ne_bytes());
    if !ok {
        EmptyClipboard();
    }
    CloseClipboard();
    ok
}

pub(super) unsafe fn read_files(hwnd: H) -> Result<(Vec<PathBuf>, bool), &'static str> {
    if OpenClipboard(hwnd) == 0 {
        return Err("Cannot open clipboard.");
    }
    let result = (|| {
        let drop = GetClipboardData(15);
        if drop.is_null() {
            return Err("Clipboard does not contain a file.");
        }
        let count = DragQueryFileW(drop, u32::MAX, ptr::null_mut(), 0) as usize;
        if count == 0 || count > zfm::operations::MAX_COPY_ITEMS {
            return Err("Clipboard item count is invalid.");
        }
        let mut paths = Vec::with_capacity(count);
        let mut seen = std::collections::HashSet::new();
        let mut units = 0usize;
        for index in 0..count {
            let len = DragQueryFileW(drop, index as u32, ptr::null_mut(), 0);
            units += len as usize + 1;
            if len == 0 || len > 32767 || units > 524_287 {
                return Err("Clipboard paths are invalid or too large.");
            }
            let mut buf = vec![0u16; len as usize + 1];
            if DragQueryFileW(drop, index as u32, buf.as_mut_ptr(), len + 1) != len {
                return Err("Cannot read clipboard path.");
            }
            let path = PathBuf::from(OsString::from_wide(&buf[..len as usize]));
            if !path.is_absolute() || !seen.insert(path.clone()) {
                return Err("Clipboard paths must be absolute and unique.");
            }
            paths.push(path);
        }
        let format = RegisterClipboardFormatW(text("Preferred DropEffect").as_ptr());
        let effect = if format == 0 {
            ptr::null_mut()
        } else {
            GetClipboardData(format)
        };
        let mut cut = false;
        if !effect.is_null() {
            if GlobalSize(effect) < 4 {
                return Err("Invalid clipboard drop effect.");
            }
            let p = GlobalLock(effect);
            if p.is_null() {
                return Err("Cannot read clipboard drop effect.");
            }
            cut = ptr::read_unaligned(p as *const u32) & 2 != 0;
            GlobalUnlock(effect);
        }
        if cut && count != 1 {
            return Err("Cut and paste supports one item only.");
        }
        Ok((paths, cut))
    })();
    CloseClipboard();
    result
}

struct Prompt {
    title: Vec<u16>,
    initial: OsString,
    select_stem: bool,
    password: bool,
    edit: H,
    result: Option<OsString>,
    theme: *const Theme,
}
unsafe extern "system" fn dialog_proc(hwnd: H, msg: u32, wp: WParam, lp: LParam) -> isize {
    const WM_INITDIALOG: u32 = 0x0110;
    const WM_CLOSE: u32 = 0x0010;
    if msg == WM_INITDIALOG {
        let state = &mut *(lp as *mut Prompt);
        set_data(hwnd, lp);
        SetWindowTextW(hwnd, state.title.as_ptr());
        apply_dark_window_chrome(hwnd);
        let instance = GetModuleHandleW(ptr::null());
        let mut rect: Rect = std::mem::zeroed();
        GetClientRect(hwnd, &mut rect);
        let width = rect.right.max(220);
        let caption = CreateWindowExW(
            0,
            text("STATIC").as_ptr(),
            text(if state.password { "Password:" } else { "Name:" }).as_ptr(),
            WS_CHILD | WS_VISIBLE,
            16,
            15,
            width - 32,
            22,
            hwnd,
            ptr::null_mut(),
            instance,
            ptr::null_mut(),
        );
        state.edit = CreateWindowExW(
            0x200,
            text("EDIT").as_ptr(),
            wide(&state.initial).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL | if state.password { 0x20 } else { 0 },
            16,
            40,
            width - 32,
            27,
            hwnd,
            1001usize as H,
            instance,
            ptr::null_mut(),
        );
        let ok = CreateWindowExW(
            0,
            text("BUTTON").as_ptr(),
            text("OK").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | 1,
            width - 190,
            84,
            80,
            28,
            hwnd,
            1usize as H,
            instance,
            ptr::null_mut(),
        );
        let cancel = CreateWindowExW(
            0,
            text("BUTTON").as_ptr(),
            text("Cancel").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            width - 96,
            84,
            80,
            28,
            hwnd,
            2usize as H,
            instance,
            ptr::null_mut(),
        );
        for control in [caption, state.edit, ok, cancel] {
            SendMessageW(control, WM_SETFONT, (*state.theme).font as usize, 1);
        }
        SetFocus(state.edit);
        let end = if state.select_stem {
            Path::new(&state.initial)
                .file_stem()
                .unwrap_or(&state.initial)
                .encode_wide()
                .count() as isize
        } else {
            -1
        };
        SendMessageW(state.edit, EM_SETSEL, 0, end);
        return 0;
    }
    let state = get_data(hwnd) as *mut Prompt;
    if state.is_null() {
        return 0;
    }
    match msg {
        WM_COMMAND => match wp & 0xffff {
            1 => {
                (*state).result = Some(control_value((*state).edit));
                EndDialog(hwnd, 1);
                1
            }
            2 => {
                EndDialog(hwnd, 0);
                1
            }
            _ => 0,
        },
        WM_CLOSE => {
            EndDialog(hwnd, 0);
            1
        }
        0x0136 => (*(*state).theme).window_brush as isize,
        WM_CTLCOLORSTATIC => {
            SetTextColor(wp as H, ui_theme::TEXT_BRIGHT.colorref());
            SetBkColor(wp as H, ui_theme::HEADER_TOP.colorref());
            (*(*state).theme).window_brush as isize
        }
        WM_CTLCOLOREDIT => {
            SetTextColor(wp as H, ui_theme::TEXT_BRIGHT.colorref());
            SetBkColor(wp as H, ui_theme::NAME_EDIT_BG.colorref());
            (*(*state).theme).name_brush as isize
        }
        _ => 0,
    }
}

pub(super) unsafe fn prompt(
    hwnd: H, theme: *const Theme, title: &str, initial: &OsStr, stem: bool,
) -> Option<OsString> {
    prompt_inner(hwnd, theme, title, initial, stem, false)
}

pub(super) unsafe fn password_prompt(hwnd: H, theme: *const Theme, title: &str) -> Option<String> {
    match prompt_inner(hwnd, theme, title, OsStr::new(""), false, true) {
        Some(value) => match value.into_string() {
            Ok(value) => Some(value),
            Err(_) => { show_error(hwnd, "Password must be valid Unicode."); None }
        },
        None => None,
    }
}

unsafe fn prompt_inner(
    hwnd: H, theme: *const Theme, title: &str, initial: &OsStr, stem: bool, password: bool,
) -> Option<OsString> {
    // DLGTEMPLATE header followed by no menu, default class, empty title.
    // Dialog units approximate 400 x 125 pixels with the system font.
    let mut template: Vec<u16> = Vec::new();
    let style = 0x8000_0000u32 | 0x00c0_0000 | 0x0008_0000 | 0x80; // popup, caption, system menu, modal frame
    template.extend([
        style as u16,
        (style >> 16) as u16,
        0,
        0,
        0,
        0,
        0,
        280,
        100,
        0,
        0,
        0,
    ]);
    let mut state = Prompt {
        title: text(title),
        initial: initial.to_os_string(),
        select_stem: stem,
        password,
        edit: ptr::null_mut(),
        result: None,
        theme,
    };
    let result = DialogBoxIndirectParamW(
        GetModuleHandleW(ptr::null()),
        template.as_ptr(),
        hwnd,
        Some(dialog_proc),
        &mut state as *mut _ as isize,
    );
    if result == -1 {
        show_error(hwnd, "Could not open the name dialog.");
    }
    state.result
}
