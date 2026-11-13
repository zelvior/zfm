//! Windows dialogs, shell properties and interoperable single-item clipboard.
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

pub(super) unsafe fn copy_file(hwnd: H, path: &Path, cut: bool) -> bool {
    let value = wide(path.as_os_str());
    let header = DropFiles {
        offset: std::mem::size_of::<DropFiles>() as u32,
        point: Point { x: 0, y: 0 },
        non_client: 0,
        wide: 1,
    };
    let mut bytes = vec![0u8; std::mem::size_of::<DropFiles>() + (value.len() + 1) * 2];
    ptr::copy_nonoverlapping(
        &header as *const _ as *const u8,
        bytes.as_mut_ptr(),
        std::mem::size_of::<DropFiles>(),
    );
    ptr::copy_nonoverlapping(
        value.as_ptr() as *const u8,
        bytes.as_mut_ptr().add(std::mem::size_of::<DropFiles>()),
        value.len() * 2,
    );
    let effect = if cut { 2u32 } else { 1u32 };
    let format = RegisterClipboardFormatW(text("Preferred DropEffect").as_ptr());
    if format == 0 || OpenClipboard(hwnd) == 0 {
        return false;
    }
    let ok = EmptyClipboard() != 0 && set_format(15, &bytes);
    // If the effect cannot be published, fail closed for cut rather than
    // advertising a copy that would leave the user's source behind.
    let ok = ok && set_format(format, &effect.to_ne_bytes());
    if !ok {
        EmptyClipboard();
    }
    CloseClipboard();
    ok
}

pub(super) unsafe fn read_file(hwnd: H) -> Result<(PathBuf, bool), &'static str> {
    if OpenClipboard(hwnd) == 0 {
        return Err("Cannot open clipboard.");
    }
    let drop = GetClipboardData(15);
    let result = if drop.is_null() {
        Err("Clipboard does not contain a file.")
    } else if DragQueryFileW(drop, u32::MAX, ptr::null_mut(), 0) != 1 {
        Err("Paste supports exactly one local item.")
    } else {
        let len = DragQueryFileW(drop, 0, ptr::null_mut(), 0);
        if len == 0 || len > 32767 {
            Err("Invalid clipboard path.")
        } else {
            let mut buf = vec![0u16; len as usize + 1];
            if DragQueryFileW(drop, 0, buf.as_mut_ptr(), len + 1) != len {
                Err("Cannot read clipboard path.")
            } else {
                let path = PathBuf::from(OsString::from_wide(&buf[..len as usize]));
                if !path.is_absolute() {
                    Err("Clipboard path must be absolute.")
                } else {
                    let format = RegisterClipboardFormatW(text("Preferred DropEffect").as_ptr());
                    let effect = if format == 0 {
                        ptr::null_mut()
                    } else {
                        GetClipboardData(format)
                    };
                    let mut cut = false;
                    if !effect.is_null() && GlobalSize(effect) >= 4 {
                        let p = GlobalLock(effect);
                        if !p.is_null() {
                            cut = ptr::read_unaligned(p as *const u32) & 2 != 0;
                            GlobalUnlock(effect);
                        }
                    }
                    Ok((path, cut))
                }
            }
        }
    };
    CloseClipboard();
    result
}

struct Prompt {
    title: Vec<u16>,
    initial: OsString,
    select_stem: bool,
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
            text("Name:").as_ptr(),
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
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
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
    hwnd: H,
    theme: *const Theme,
    title: &str,
    initial: &OsStr,
    stem: bool,
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
