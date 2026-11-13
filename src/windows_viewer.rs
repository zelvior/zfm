//! Native read-only document and image preview dialog.
use super::*;
use zfm::viewer::{self, Kind};

#[repr(C)]
struct GdiStartup { version: u32, debug: P, suppress_thread: i32, suppress_codecs: i32 }
#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const GdiStartup, output: P) -> i32;
    fn GdiplusShutdown(token: usize);
    fn GdipLoadImageFromStream(stream: H, image: *mut H) -> i32;
    fn GdipDisposeImage(image: H) -> i32;
    fn GdipGetImageWidth(image: H, width: *mut u32) -> i32;
    fn GdipGetImageHeight(image: H, height: *mut u32) -> i32;
    fn GdipCreateFromHDC(dc: H, graphics: *mut H) -> i32;
    fn GdipDeleteGraphics(graphics: H) -> i32;
    fn GdipDrawImageRectI(graphics: H, image: H, x: i32, y: i32, w: i32, h: i32) -> i32;
    fn GdipSetInterpolationMode(graphics: H, mode: i32) -> i32;
}
#[link(name = "ole32")]
extern "system" { fn CreateStreamOnHGlobal(memory: H, delete_on_release: i32, stream: *mut H) -> i32; }
#[link(name = "user32")]
extern "system" { fn FillRect(dc: H, rect: *const Rect, brush: H) -> i32; }

unsafe fn release_stream(stream: H) {
    if !stream.is_null() {
        let vtable = *(stream as *const *const *const c_void);
        let release: unsafe extern "system" fn(H) -> u32 = std::mem::transmute(*vtable.add(2));
        release(stream);
    }
}

struct Image { token: usize, image: H, stream: H, width: u32, height: u32 }
impl Drop for Image {
    fn drop(&mut self) { unsafe { GdipDisposeImage(self.image); release_stream(self.stream); GdiplusShutdown(self.token); } }
}
impl Image {
    unsafe fn load(bytes: &[u8]) -> Result<Self, String> {
        let (header_width, header_height) = viewer::image_header_dimensions(bytes)
            .filter(|&(w, h)| viewer::valid_image_dimensions(w, h))
            .ok_or("Unsupported, invalid or oversized image header.")?;
        let input = GdiStartup { version: 1, debug: ptr::null_mut(), suppress_thread: 0, suppress_codecs: 0 };
        let mut token = 0;
        if GdiplusStartup(&mut token, &input, ptr::null_mut()) != 0 { return Err("Could not initialize image decoder.".into()); }
        let memory = GlobalAlloc(2, bytes.len().max(1)); // GMEM_MOVEABLE
        if memory.is_null() { GdiplusShutdown(token); return Err("Not enough memory for preview.".into()); }
        let raw = GlobalLock(memory);
        if raw.is_null() { GlobalFree(memory); GdiplusShutdown(token); return Err("Could not allocate preview.".into()); }
        ptr::copy_nonoverlapping(bytes.as_ptr(), raw as *mut u8, bytes.len());
        GlobalUnlock(memory);
        let mut stream = ptr::null_mut();
        if CreateStreamOnHGlobal(memory, 1, &mut stream) < 0 {
            GlobalFree(memory); GdiplusShutdown(token); return Err("Could not create image stream.".into());
        }
        let mut image = ptr::null_mut();
        if GdipLoadImageFromStream(stream, &mut image) != 0 || image.is_null() {
            release_stream(stream); GdiplusShutdown(token); return Err("Image could not be decoded.".into());
        }
        let (mut width, mut height) = (0, 0);
        if GdipGetImageWidth(image, &mut width) != 0 || GdipGetImageHeight(image, &mut height) != 0 || !viewer::valid_image_dimensions(width, height) || (width, height) != (header_width, header_height) {
            GdipDisposeImage(image); release_stream(stream); GdiplusShutdown(token);
            return Err("Image dimensions exceed the viewer limit or are invalid.".into());
        }
        Ok(Self { token, image, stream, width, height })
    }
}

struct Preview {
    title: Vec<u16>, text: Option<Vec<u16>>, image: Option<Image>, zoom: i32,
    theme: *const Theme, edit: H, minus: H, fit: H, plus: H, close: H,
}
unsafe fn layout(hwnd: H, state: &Preview) {
    let mut r: Rect = std::mem::zeroed();
    GetClientRect(hwnd, &mut r);
    let w = r.right.max(200); let h = r.bottom.max(100);
    if !state.edit.is_null() { MoveWindow(state.edit, 8, 8, w - 16, (h - 54).max(1), 1); }
    for (control, x, width) in [(state.minus, 8, 45), (state.fit, 57, 52), (state.plus, 113, 45), (state.close, w - 88, 80)] {
        if !control.is_null() { MoveWindow(control, x, h - 38, width, 28, 1); }
    }
}
unsafe extern "system" fn dialog_proc(hwnd: H, msg: u32, wp: WParam, lp: LParam) -> isize {
    const WM_INITDIALOG: u32 = 0x0110;
    const WM_CLOSE: u32 = 0x0010;
    if msg == WM_INITDIALOG {
        let state = &mut *(lp as *mut Preview);
        set_data(hwnd, lp);
        SetWindowTextW(hwnd, state.title.as_ptr());
        apply_dark_window_chrome(hwnd);
        let instance = GetModuleHandleW(ptr::null());
        let create = |class: &str, value: *const u16, style: u32, id: usize| {
            CreateWindowExW(0, text(class).as_ptr(), value, WS_CHILD | WS_VISIBLE | style,
                0, 0, 1, 1, hwnd, id as H, instance, ptr::null_mut())
        };
        if let Some(ref content) = state.text {
            // Multiline read-only edit supports selection, copying and scrolling.
            state.edit = create("EDIT", text("").as_ptr(), WS_TABSTOP | WS_VSCROLL | 0x0010_0000 | 0x0004 | 0x0040 | 0x0080 | 0x0800, 1001);
            if !state.edit.is_null() {
                SendMessageW(state.edit, 0x00c5, 8 * 1024 * 1024, 0); // EM_SETLIMITTEXT
                SetWindowTextW(state.edit, content.as_ptr());
                SendMessageW(state.edit, WM_SETFONT, (*state.theme).font as usize, 1);
            }
        } else {
            state.minus = create("BUTTON", text("−").as_ptr(), WS_TABSTOP, 10);
            state.fit = create("BUTTON", text("Fit").as_ptr(), WS_TABSTOP, 11);
            state.plus = create("BUTTON", text("+").as_ptr(), WS_TABSTOP, 12);
        }
        state.close = create("BUTTON", text("Close").as_ptr(), WS_TABSTOP, 2);
        for b in [state.minus, state.fit, state.plus, state.close] { if !b.is_null() { SendMessageW(b, WM_SETFONT, (*state.theme).font as usize, 1); } }
        layout(hwnd, state);
        return 1;
    }
    let state = get_data(hwnd) as *mut Preview;
    if state.is_null() { return 0; }
    let state = &mut *state;
    match msg {
        WM_SIZE => { layout(hwnd, state); InvalidateRect(hwnd, ptr::null(), 1); 1 }
        WM_COMMAND => match wp & 0xffff {
            2 => { EndDialog(hwnd, 0); 1 }
            10 => { state.zoom = (state.zoom - 25).max(25); InvalidateRect(hwnd, ptr::null(), 1); 1 }
            11 => { state.zoom = 100; InvalidateRect(hwnd, ptr::null(), 1); 1 }
            12 => { state.zoom = (state.zoom + 25).min(200); InvalidateRect(hwnd, ptr::null(), 1); 1 }
            _ => 0,
        },
        WM_CLOSE => { EndDialog(hwnd, 0); 1 }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            SetTextColor(wp as H, ui_theme::TEXT_BRIGHT.colorref());
            SetBkColor(wp as H, ui_theme::NAME_EDIT_BG.colorref());
            (*state.theme).name_brush as isize
        }
        WM_PAINT => {
            let mut ps: PaintStruct = std::mem::zeroed(); let dc = BeginPaint(hwnd, &mut ps);
            let mut r: Rect = std::mem::zeroed(); GetClientRect(hwnd, &mut r);
            FillRect(dc, &r, (*state.theme).window_brush);
            if let Some(ref image) = state.image {
                let avail_w = (r.right - 16).max(1); let avail_h = (r.bottom - 56).max(1);
                let scale = (avail_w as f64 / image.width as f64).min(avail_h as f64 / image.height as f64).min(1.0) * state.zoom as f64 / 100.0;
                let width = (image.width as f64 * scale).max(1.0) as i32;
                let height = (image.height as f64 * scale).max(1.0) as i32;
                let mut graphics = ptr::null_mut();
                if GdipCreateFromHDC(dc, &mut graphics) == 0 && !graphics.is_null() {
                    GdipSetInterpolationMode(graphics, 7); // high-quality bicubic
                    GdipDrawImageRectI(graphics, image.image, (r.right - width) / 2, 8 + (avail_h - height) / 2, width, height);
                    GdipDeleteGraphics(graphics);
                }
            }
            EndPaint(hwnd, &ps); 1
        }
        _ => 0,
    }
}

pub(super) unsafe fn show(hwnd: H, theme: *const Theme, path: &Path) {
    let kind = viewer::kind(path);
    if kind == Kind::Unsupported { show_error(hwnd, "No built-in preview for this format. Use Open to launch the default application."); return; }
    let limit = if kind == Kind::Text { viewer::MAX_TEXT_BYTES } else { viewer::MAX_IMAGE_BYTES };
    let bytes = match viewer::read_bounded(path, limit) { Ok(v) => v, Err(e) => { show_error(hwnd, &format!("Preview failed: {e}")); return; } };
    let mut state = Preview { title: text(&format!("ZFM Preview — {}", path.display())), text: None, image: None, zoom: 100, theme,
        edit: ptr::null_mut(), minus: ptr::null_mut(), fit: ptr::null_mut(), plus: ptr::null_mut(), close: ptr::null_mut() };
    if kind == Kind::Text {
        match viewer::decode_text(&bytes) {
            Ok(value) => state.text = Some(text(&value.replace("\r\n", "\n").replace('\n', "\r\n"))),
            Err(e) => { show_error(hwnd, &format!("Preview failed: {e}")); return; }
        }
    } else {
        match Image::load(&bytes) { Ok(image) => state.image = Some(image), Err(e) => { show_error(hwnd, &e); return; } }
    }
    let style: u32 = 0x8000_0000 | 0x00c0_0000 | 0x0008_0000 | 0x0004_0000 | 0x0001_0000 | 0x80;
    let template: Vec<u16> = vec![style as u16, (style >> 16) as u16, 0, 0, 0, 0, 0, 520, 340, 0, 0, 0];
    let result = DialogBoxIndirectParamW(GetModuleHandleW(ptr::null()), template.as_ptr(), hwnd, Some(dialog_proc), &mut state as *mut _ as isize);
    if result == -1 { show_error(hwnd, "Could not open the preview window."); }
}
