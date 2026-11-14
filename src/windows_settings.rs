//! Persistent, native Windows file-manager preferences.
use super::*;
use zfm::settings::Settings;

struct SettingsDialog {
    theme: *const Theme,
    settings: Settings,
    controls: [H; 6],
    accepted: bool,
    headings: [H; 2],
    card_brush: H,
}

// Keep the native auto-checkbox state, keyboard handling and accessibility, but
// replace its check mark with the same switch treatment used in the toolbar.
unsafe extern "system" fn switch_proc(hwnd: H, msg: u32, wp: WParam, lp: LParam, _: usize, data: usize) -> LResult {
    if msg == WM_PAINT {
        let mut paint: PaintStruct = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut paint);
        let mut r: Rect = std::mem::zeroed();
        GetClientRect(hwnd, &mut r);
        let (w, h) = (r.right.max(1), r.bottom.max(1));
        if w <= 4096 && h <= 1024 {
            let mut pixels = vec![Rgb(40, 46, 56).pixel(); (w * h) as usize];
            let checked = SendMessageW(hwnd, 0x00f0, 0, 0) == 1;
            ui_theme::paint_toggle_switch(&mut pixels, w, h, 2, (h - 16) / 2, checked, false);
            let caption = control_value(hwnd).to_string_lossy().into_owned();
            blit_pixels(dc, 0, 0, w, h, &pixels, |mem| {
                draw_text_in(mem, (*(data as *const Theme)).font, ui_theme::TEXT_BRIGHT,
                    BoxRect::new(44, 0, (w - 46).max(1), h), &caption,
                    DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
                if GetFocus() == hwnd {
                    let mut focus = Rect { left: 42, top: 2, right: w - 2, bottom: h - 2 };
                    DrawFocusRect(mem, &mut focus);
                }
            });
        }
        EndPaint(hwnd, &paint);
        return 0;
    }
    let result = DefSubclassProc(hwnd, msg, wp, lp);
    if matches!(msg, 0x00f1 | 0x0007 | 0x0008 | 0x0202 | 0x0101 | 0x000a) {
        InvalidateRect(hwnd, ptr::null(), 0);
    }
    result
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
        let mut rect: Rect = std::mem::zeroed();
        GetClientRect(hwnd, &mut rect);
        let width = rect.right;
        state.card_brush = CreateSolidBrush(Rgb(40, 46, 56).colorref());
        state.headings = [
            make("STATIC", "File list", 0, 36, 28, width - 72, 24, 1010),
            make("STATIC", "Drives and navigation", 0, 36, 190, width - 72, 24, 1011),
        ];
        state.controls = [
            make("BUTTON", "Show hidden files", WS_TABSTOP | 3, 36, 55, width - 72, 24, 1001),
            make("BUTTON", "Sort ascending", WS_TABSTOP | 3, 36, 81, width - 72, 24, 1002),
            make("BUTTON", "Automatically refresh the current folder", WS_TABSTOP | 3, 36, 107, width - 72, 24, 1004),
            make("BUTTON", "Use compact view at startup", WS_TABSTOP | 3, 36, 133, width - 72, 24, 1005),
            make("BUTTON", "Show drive volume names", WS_TABSTOP | 3, 36, 217, width - 72, 24, 1006),
            make("BUTTON", "Expand Computer at startup", WS_TABSTOP | 3, 36, 243, width - 72, 24, 1007),
        ];
        let note = make("STATIC", "Drive changes are detected automatically. Apply saves these preferences.", 1, 20, 300, width - 40, 36, 1003);
        let apply = make("BUTTON", "Apply", WS_TABSTOP | BS_OWNERDRAW, width / 2 - 94, 344, 88, 30, 1);
        let cancel = make("BUTTON", "Cancel", WS_TABSTOP | BS_OWNERDRAW, width / 2 + 6, 344, 88, 30, 2);
        for control in state.controls.into_iter().chain(state.headings).chain([note, apply, cancel]) {
            if control.is_null() { EndDialog(hwnd, -1); return 1; }
            SendMessageW(control, WM_SETFONT, (*state.theme).font as usize, 1);
        }
        for heading in state.headings { SendMessageW(heading, WM_SETFONT, (*state.theme).subtitle_font as usize, 1); }
        for control in state.controls {
            // If subclassing fails, the usable native checkbox remains in place.
            SetWindowSubclass(control, Some(switch_proc), 1, state.theme as usize);
        }
        SendMessageW(hwnd, 0x0401, 1, 0); // DM_SETDEFID: Apply
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
        WM_ERASEBKGND => {
            let mut r: Rect = std::mem::zeroed();
            GetClientRect(hwnd, &mut r);
            let (w, h) = (r.right, r.bottom);
            if w > 0 && h > 0 && w <= 4096 && h <= 4096 {
                let mut pixels = vec![ui_theme::HEADER_TOP.pixel(); (w * h) as usize];
                for card in [BoxRect::new(20, 18, w - 40, 148), BoxRect::new(20, 180, w - 40, 103)] {
                    ui_theme::paint_rounded_box(&mut pixels, w, h, card, 9.0,
                        Rgb(40, 46, 56), Rgb(40, 46, 56), Some(Rgb(30, 35, 43)));
                }
                blit_pixels(wp as H, 0, 0, w, h, &pixels, |_| {});
            }
            1
        }
        WM_DESTROY => { if !state.card_brush.is_null() { DeleteObject(state.card_brush); state.card_brush = ptr::null_mut(); } 0 }
        WM_DRAWITEM if lp != 0 && (*(lp as *const DrawItem)).control_type == ODT_BUTTON => draw_dialog_button(state.theme, &*(lp as *const DrawItem)),
        0x0136 => (*state.theme).window_brush as isize,
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            SetTextColor(wp as H, ui_theme::TEXT_BRIGHT.colorref());
            let in_card = state.headings.contains(&(lp as H));
            SetBkColor(wp as H, if in_card { Rgb(40, 46, 56).colorref() } else { ui_theme::HEADER_TOP.colorref() });
            if in_card && !state.card_brush.is_null() { state.card_brush as isize } else { (*state.theme).window_brush as isize }
        }
        _ => 0,
    }
}

pub(super) unsafe fn show(hwnd: H, theme: *const Theme, settings: Settings) -> Option<Settings> {
    let style: u32 = 0x8000_0000 | 0x00c0_0000 | 0x0008_0000 | 0x80;
    let template: Vec<u16> = vec![style as u16, (style >> 16) as u16, 0, 0, 0, 0, 0, 340, 270, 0, 0, 0];
    let mut state = SettingsDialog { theme, settings, controls: [ptr::null_mut(); 6], accepted: false, headings: [ptr::null_mut(); 2], card_brush: ptr::null_mut() };
    let result = DialogBoxIndirectParamW(GetModuleHandleW(ptr::null()), template.as_ptr(), hwnd, Some(dialog_proc), &mut state as *mut _ as isize);
    if result == -1 { show_error(hwnd, "Could not open settings."); }
    if state.accepted { Some(state.settings) } else { None }
}
