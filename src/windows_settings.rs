//! Persistent, native Windows file-manager preferences.
use super::*;
use zfm::settings::Settings;

struct SettingsDialog {
    theme: *const Theme,
    settings: Settings,
    controls: [H; 6],
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
        let headings = [
            make("STATIC", "File list", 0, 20, 12, 320, 22, 1010),
            make("STATIC", "Drives and navigation", 0, 20, 140, 320, 22, 1011),
        ];
        state.controls = [
            make("BUTTON", "Show hidden files", WS_TABSTOP | 3, 30, 35, 340, 23, 1001),
            make("BUTTON", "Sort ascending", WS_TABSTOP | 3, 30, 59, 340, 23, 1002),
            make("BUTTON", "Automatically refresh the current folder", WS_TABSTOP | 3, 30, 83, 340, 23, 1004),
            make("BUTTON", "Use compact view at startup", WS_TABSTOP | 3, 30, 107, 340, 23, 1005),
            make("BUTTON", "Show drive volume names", WS_TABSTOP | 3, 30, 163, 340, 23, 1006),
            make("BUTTON", "Expand Computer at startup", WS_TABSTOP | 3, 30, 187, 340, 23, 1007),
        ];
        let note = make("STATIC", "Drive changes are detected automatically. Apply saves these preferences.", 0, 20, 224, 370, 36, 1003);
        let apply = make("BUTTON", "Apply", WS_TABSTOP | 1, 220, 268, 80, 28, 1);
        let cancel = make("BUTTON", "Cancel", WS_TABSTOP, 310, 268, 80, 28, 2);
        for control in state.controls.into_iter().chain(headings).chain([note, apply, cancel]) {
            if control.is_null() { EndDialog(hwnd, -1); return 1; }
            SendMessageW(control, WM_SETFONT, (*state.theme).font as usize, 1);
        }
        for (control, checked) in state.controls.into_iter().zip([
            state.settings.show_hidden, state.settings.sort_ascending,
            state.settings.windows_live_refresh, state.settings.windows_compact_view,
            state.settings.windows_drive_labels, state.settings.windows_expand_drives,
        ]) { SendMessageW(control, BM_SETCHECK, checked as usize, 0); }
        return 1;
    }
    let state = get_data(hwnd) as *mut SettingsDialog;
    if state.is_null() { return 0; }
    let state = &mut *state;
    match message {
        WM_COMMAND if wp & 0xffff == 1 => {
            let checked = |i: usize| SendMessageW(state.controls[i], BM_GETCHECK, 0, 0) == 1;
            state.settings.show_hidden = checked(0);
            state.settings.sort_ascending = checked(1);
            state.settings.windows_live_refresh = checked(2);
            state.settings.windows_compact_view = checked(3);
            state.settings.windows_drive_labels = checked(4);
            state.settings.windows_expand_drives = checked(5);
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

pub(super) unsafe fn show(hwnd: H, theme: *const Theme, settings: Settings) -> Option<Settings> {
    let style: u32 = 0x8000_0000 | 0x00c0_0000 | 0x0008_0000 | 0x80;
    let template: Vec<u16> = vec![style as u16, (style >> 16) as u16, 0, 0, 0, 0, 0, 315, 245, 0, 0, 0];
    let mut state = SettingsDialog { theme, settings, controls: [ptr::null_mut(); 6], accepted: false };
    let result = DialogBoxIndirectParamW(GetModuleHandleW(ptr::null()), template.as_ptr(), hwnd, Some(dialog_proc), &mut state as *mut _ as isize);
    if result == -1 { show_error(hwnd, "Could not open settings."); }
    if state.accepted { Some(state.settings) } else { None }
}
