//! PDFium-backed read-only raster preview. Libraries are loaded only from the
//! installation, not from the process search path. PDFium is not a sandbox;
//! only preview documents from trusted sources.
use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::ptr;
use std::sync::{Mutex, OnceLock};

pub const MAX_PDF_BYTES: u64 = 64 * 1024 * 1024;
pub const PASSWORD_REQUIRED: &str = "PDF password required or incorrect.";
type P = *mut c_void;
struct Api {
    load: unsafe extern "C" fn(*const c_void, usize, *const i8) -> P,
    last_error: unsafe extern "C" fn() -> std::os::raw::c_ulong,
    close: unsafe extern "C" fn(P),
    count: unsafe extern "C" fn(P) -> i32,
    load_page: unsafe extern "C" fn(P, i32) -> P,
    close_page: unsafe extern "C" fn(P),
    width: unsafe extern "C" fn(P) -> f32,
    height: unsafe extern "C" fn(P) -> f32,
    bitmap: unsafe extern "C" fn(i32, i32, i32, P, i32) -> P,
    bitmap_fill: unsafe extern "C" fn(P, i32, i32, i32, i32, std::os::raw::c_ulong) -> i32,
    bitmap_destroy: unsafe extern "C" fn(P),
    render: unsafe extern "C" fn(P, P, i32, i32, i32, i32, i32, i32),
}
static API: OnceLock<Result<Api, String>> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

fn library_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = exe.parent().ok_or("Could not locate ZFM installation")?;
    #[cfg(target_os = "windows")]
    let candidates = [parent.join("pdfium.dll")];
    #[cfg(target_os = "linux")]
    let candidates = [parent.join("libpdfium.so"), PathBuf::from("/usr/lib/zfm/libpdfium.so")];
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| "Bundled PDF renderer is missing. Reinstall ZFM.".to_string())
}
#[cfg(target_os = "linux")]
unsafe fn open_library(path: &std::path::Path) -> P {
    use std::os::unix::ffi::OsStrExt;
    #[link(name = "dl")]
    extern "C" { fn dlopen(path: *const i8, flags: i32) -> P; }
    match CString::new(path.as_os_str().as_bytes()) { Ok(path) => dlopen(path.as_ptr(), 2), Err(_) => ptr::null_mut() }
}
#[cfg(target_os = "windows")]
unsafe fn open_library(path: &std::path::Path) -> P {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" { fn LoadLibraryExW(path: *const u16, file: P, flags: u32) -> P; }
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    LoadLibraryExW(name.as_ptr(), ptr::null_mut(), 0x8) // absolute path, LOAD_WITH_ALTERED_SEARCH_PATH
}
#[cfg(target_os = "linux")]
unsafe fn symbol(handle: P, name: *const i8) -> P {
    #[link(name = "dl")]
    extern "C" { fn dlsym(handle: P, name: *const i8) -> P; }
    dlsym(handle, name)
}
#[cfg(target_os = "windows")]
unsafe fn symbol(handle: P, name: *const i8) -> P {
    #[link(name = "kernel32")]
    extern "system" { fn GetProcAddress(handle: P, name: *const i8) -> P; }
    GetProcAddress(handle, name)
}
fn load_api() -> Result<Api, String> {
    unsafe {
        let handle = open_library(&library_path()?);
        if handle.is_null() { return Err("The bundled PDF renderer could not be loaded on this system.".into()); }
        // Keep the library loaded for the process lifetime; PDFium is shared and
        // all calls below are serialized by LOCK.
        macro_rules! get {
            ($name:literal, $ty:ty) => {{
                let p = symbol(handle, concat!($name, "\0").as_ptr() as *const i8);
                if p.is_null() { return Err(concat!("PDF renderer lacks ", $name).into()); }
                std::mem::transmute::<P, $ty>(p)
            }};
        }
        let init = get!("FPDF_InitLibrary", unsafe extern "C" fn());
        let api = Api {
            load: get!("FPDF_LoadMemDocument64", unsafe extern "C" fn(*const c_void, usize, *const i8) -> P),
            last_error: get!("FPDF_GetLastError", unsafe extern "C" fn() -> std::os::raw::c_ulong),
            close: get!("FPDF_CloseDocument", unsafe extern "C" fn(P)),
            count: get!("FPDF_GetPageCount", unsafe extern "C" fn(P) -> i32),
            load_page: get!("FPDF_LoadPage", unsafe extern "C" fn(P, i32) -> P),
            close_page: get!("FPDF_ClosePage", unsafe extern "C" fn(P)),
            width: get!("FPDF_GetPageWidthF", unsafe extern "C" fn(P) -> f32),
            height: get!("FPDF_GetPageHeightF", unsafe extern "C" fn(P) -> f32),
            bitmap: get!("FPDFBitmap_CreateEx", unsafe extern "C" fn(i32, i32, i32, P, i32) -> P),
            bitmap_fill: get!("FPDFBitmap_FillRect", unsafe extern "C" fn(P, i32, i32, i32, i32, std::os::raw::c_ulong) -> i32),
            bitmap_destroy: get!("FPDFBitmap_Destroy", unsafe extern "C" fn(P)),
            render: get!("FPDF_RenderPageBitmap", unsafe extern "C" fn(P, P, i32, i32, i32, i32, i32, i32)),
        };
        init();
        Ok(api)
    }
}

/// Render one zero-based page into a top-down 32-bit BMP; returns page count.
/// The input must remain unchanged during this call.
pub fn render_page(bytes: &[u8], page_index: usize) -> Result<(Vec<u8>, usize), String> {
    render_page_password(bytes, page_index, None)
}

/// Render with an optional PDF password. Passwords are held in process memory.
pub fn render_page_password(bytes: &[u8], page_index: usize, password: Option<&str>) -> Result<(Vec<u8>, usize), String> {
    let password = match password {
        Some(value) if value.len() <= 512 => Some(CString::new(value).map_err(|_| "PDF password contains a NUL character.")?),
        Some(_) => return Err("PDF password exceeds 512 UTF-8 bytes.".into()),
        None => None,
    };
    if bytes.is_empty() || bytes.len() as u64 > MAX_PDF_BYTES { return Err("PDF exceeds the preview size limit or is empty.".into()); }
    let _lock = LOCK.lock().map_err(|_| "PDF renderer lock failed")?;
    let api = API.get_or_init(load_api).as_ref().map_err(|e| e.clone())?;
    unsafe {
        let password_ptr = password.as_ref().map_or(ptr::null(), |p| p.as_ptr());
        let doc = (api.load)(bytes.as_ptr() as *const c_void, bytes.len(), password_ptr);
        if doc.is_null() {
            return Err(if (api.last_error)() == 4 { PASSWORD_REQUIRED } else { "Could not open PDF; it may be damaged or unsupported." }.into());
        }
        let result = (|| {
            let count = (api.count)(doc);
            if count <= 0 || count > 10_000 || page_index >= count as usize { return Err("Invalid or excessive PDF page count.".into()); }
            let page = (api.load_page)(doc, page_index as i32);
            if page.is_null() { return Err("Could not open this PDF page.".into()); }
            let result = (|| {
                let (w, h) = ((api.width)(page) as f64, (api.height)(page) as f64);
                if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 { return Err("Invalid PDF page dimensions.".into()); }
                let scale = (1400.0 / w).min(1800.0 / h);
                let (width, height) = ((w * scale).round().max(1.0) as i32, (h * scale).round().max(1.0) as i32);
                let pixel_bytes = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)).ok_or("PDF page too large")?;
                let mut bmp = vec![0u8; 54 + pixel_bytes];
                bmp[..2].copy_from_slice(b"BM");
                bmp[2..6].copy_from_slice(&((54 + pixel_bytes) as u32).to_le_bytes());
                bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
                bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
                bmp[18..22].copy_from_slice(&width.to_le_bytes());
                bmp[22..26].copy_from_slice(&(-height).to_le_bytes());
                bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
                bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
                bmp[34..38].copy_from_slice(&(pixel_bytes as u32).to_le_bytes());
                // PDFium requires its external pixel buffer to be 4-byte aligned;
                // the BMP header ends at byte 54, which is not aligned.
                let mut pixels = vec![0u32; pixel_bytes / 4];
                let bitmap = (api.bitmap)(width, height, 4, pixels.as_mut_ptr() as P, width * 4);
                if bitmap.is_null() { return Err("Could not allocate PDF page surface.".into()); }
                (api.bitmap_fill)(bitmap, 0, 0, width, height, 0xffff_ffff);
                (api.render)(bitmap, page, 0, 0, width, height, 0, 1); // annotations, no JavaScript
                (api.bitmap_destroy)(bitmap);
                let pixels = std::slice::from_raw_parts(pixels.as_ptr() as *const u8, pixel_bytes);
                bmp[54..].copy_from_slice(pixels);
                Ok((bmp, count as usize))
            })();
            (api.close_page)(page);
            result
        })();
        (api.close)(doc);
        result
    }
}

#[cfg(test)]
mod tests {
    /// Run when the bundled PDFium library is next to the test executable (or
    /// at the Linux installation path). This covers external buffer alignment.
    #[test]
    fn render_single_page_if_pdfium_is_installed() {
        if super::library_path().is_err() { return; }
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
        ];
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
        for offset in offsets { pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()); }
        pdf.extend_from_slice(format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        let (bmp, pages) = super::render_page(&pdf, 0).expect("render a valid PDF");
        assert_eq!(pages, 1);
        assert!(bmp.starts_with(b"BM"));
    }
}
