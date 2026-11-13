// Modified: settings now persist after Apply.
//! File-list preferences in a native settings dialog.
use super::*;

struct SettingsDialog {
    theme: *const Theme,
    show_hidden: bool,
    ascending: bool,
    hidden_control: H,
    ascending_control: H,
    accepted: bool,
}

unsafe extern "system" fn dialog_proc(hwnd: H, message: u32, wp: WParam, lp: LParam) -> isize {
    const INIT: u32 = 0x0110;
    const CLOSE: u32 = 0x0010;
    const BM_GETCHECK: u32 = 0x00f0;
    const BM_SETCHECK: u32 = 0x00f1;
    if message == INIT {
        let state = &mut *(lp as *mut SettingsDialog);
        set_data(hwnd, lp);
        SetWindowTextW(hwnd, text("ZFM Settings").as_ptr());
        apply_dark_window_chrome(hwnd);
        let instance = GetModuleHandleW(ptr::null());
        let make = |class: &str, caption: &str, style: u32, x: i32, y: i32, w: i32, h: i32, id: usize| {
            CreateWindowExW(0, text(class).as_ptr(), text(caption).as_ptr(), WS_CHILD | WS_VISIBLE | style,
                x, y, w, h, hwnd, id as H, instance, ptr::null_mut())
        };
        state.hidden_control = make("BUTTON", "Show hidden files", WS_TABSTOP | 3, 20, 20, 260, 25, 1001);
        state.ascending_control = make("BUTTON", "Sort ascending", WS_TABSTOP | 3, 20, 55, 260, 25, 1002);
        let note = make("STATIC", "Apply saves settings for future sessions.", 0, 20, 90, 300, 25, 1003);
        let apply = make("BUTTON", "Apply", WS_TABSTOP | 1, 190, 125, 80, 28, 1);
        let cancel = make("BUTTON", "Cancel", WS_TABSTOP, 280, 125, 80, 28, 2);
        for control in [state.hidden_control, state.ascending_control, note, apply, cancel] {
            if control.is_null() { EndDialog(hwnd, -1); return 1; }
            SendMessageW(control, WM_SETFONT, (*state.theme).font as usize, 1);
        }
        SendMessageW(state.hidden_control, BM_SETCHECK, state.show_hidden as usize, 0);
        SendMessageW(state.ascending_control, BM_SETCHECK, state.ascending as usize, 0);
        return 1;
    }
    let state = get_data(hwnd) as *mut SettingsDialog;
    if state.is_null() { return 0; }
    let state = &mut *state;
    match message {
        WM_COMMAND if wp & 0xffff == 1 => {
            state.show_hidden = SendMessageW(state.hidden_control, BM_GETCHECK, 0, 0) == 1;
            state.ascending = SendMessageW(state.ascending_control, BM_GETCHECK, 0, 0) == 1;
            state.accepted = true;
            EndDialog(hwnd, 1);
            1
        }
        WM_COMMAND if wp & 0xffff == 2 => { EndDialog(hwnd, 0); 1 }
        CLOSE => { EndDialog(hwnd, 0); 1 }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            SetTextColor(wp as H, ui_theme::TEXT_BRIGHT.colorref());
            SetBkColor(wp as H, ui_theme::HEADER_TOP.colorref());
            (*state.theme).window_brush as isize
        }
        _ => 0,
    }
}

pub(super) unsafe fn show(hwnd: H, theme: *const Theme, show_hidden: bool, ascending: bool) -> Option<(bool, bool)> {
    let style: u32 = 0x8000_0000 | 0x00c0_0000 | 0x0008_0000 | 0x80;
    let template: Vec<u16> = vec![style as u16, (style >> 16) as u16, 0, 0, 0, 0, 0, 280, 125, 0, 0, 0];
    let mut state = SettingsDialog { theme, show_hidden, ascending, hidden_control: ptr::null_mut(), ascending_control: ptr::null_mut(), accepted: false };
    let result = DialogBoxIndirectParamW(GetModuleHandleW(ptr::null()), template.as_ptr(), hwnd, Some(dialog_proc), &mut state as *mut _ as isize);
    if result == -1 { show_error(hwnd, "Could not open settings."); }
    if state.accepted { Some((state.show_hidden, state.ascending)) } else { None }
}
