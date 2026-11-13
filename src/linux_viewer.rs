//! Read-only GTK previews and package-manager removal. GTK calls stay on the UI thread.
use super::*;
use zfm::viewer::{self, Kind};

struct ImageInfo { width: i32, height: i32, rejected: bool }
unsafe extern "C" fn image_size(loader: P, width: c_int, height: c_int, data: P) {
    let info = &mut *(data as *mut ImageInfo);
    info.width = width;
    info.height = height;
    if width <= 0 || height <= 0 || !viewer::valid_image_dimensions(width as u32, height as u32) {
        info.rejected = true;
        (api().gdk_pixbuf_loader_set_size)(loader, 1, 1);
    } else {
        // Bound the decoded surface even when the source is within the pixel limit.
        let scale = (1600.0 / width as f64).min(1200.0 / height as f64).min(1.0);
        (api().gdk_pixbuf_loader_set_size)(loader, (width as f64 * scale).max(1.0) as i32,
            (height as f64 * scale).max(1.0) as i32);
    }
}

unsafe fn decode_image(bytes: &[u8]) -> Result<(P, i32, i32), String> {
    let loader = (api().gdk_pixbuf_loader_new)();
    if loader.is_null() { return Err("Could not create an image decoder.".into()); }
    let mut info = ImageInfo { width: 0, height: 0, rejected: false };
    connect(loader, "size-prepared", image_size as P, &mut info as *mut _ as P);
    let mut ok = true;
    // Feed incrementally so an oversized image is rejected as soon as its
    // header is known, before handing the remaining bytes to the decoder.
    for chunk in bytes.chunks(256) {
        if (api().gdk_pixbuf_loader_write)(loader, chunk.as_ptr(), chunk.len(), ptr::null_mut()) == 0 || info.rejected {
            ok = false;
            break;
        }
    }
    if (api().gdk_pixbuf_loader_close)(loader, ptr::null_mut()) == 0 { ok = false; }
    let image = if ok && info.width > 0 {
        let image = (api().gdk_pixbuf_loader_get_pixbuf)(loader);
        if !image.is_null() { (api().g_object_ref)(image); }
        image
    } else { ptr::null_mut() };
    (api().g_object_unref)(loader);
    if image.is_null() {
        Err(if info.rejected { "Image dimensions exceed the viewer limit." } else { "The image could not be decoded." }.into())
    } else { Ok((image, info.width, info.height)) }
}

struct OfficePreviewResult { app: usize, alive: Arc<AtomicBool>, cancel: Arc<AtomicBool>, title: String, result: Result<zfm::office_render::Preview, String> }
unsafe extern "C" fn office_preview_ready(data: P) -> c_int {
    let result = Box::from_raw(data as *mut OfficePreviewResult);
    if !result.alive.load(Ordering::Relaxed) { return 0; }
    let app = &mut *(result.app as *mut App);
    app.office_cancels.retain(|cancel| !Arc::ptr_eq(cancel, &result.cancel));
    let content = match result.result {
        Ok(zfm::office_render::Preview::LayoutPdf(bytes)) => {
            show_visual(app, &result.title, Kind::Pdf, bytes);
            return 0;
        }
        Ok(zfm::office_render::Preview::Text(content)) => content,
        Err(error) => { set_status(app, &format!("Document preview failed: {error}")); return 0; }
    };
    let dialog = (api().gtk_dialog_new)();
    (api().gtk_window_set_title)(dialog, c(&format!("ZFM text-only preview — {}", result.title)).as_ptr());
    (api().gtk_window_set_transient_for)(dialog, app.window);
    (api().gtk_window_set_modal)(dialog, 1);
    (api().gtk_window_set_destroy_with_parent)(dialog, 1);
    (api().gtk_window_set_default_size)(dialog, 900, 650);
    let scroll = (api().gtk_scrolled_window_new)(ptr::null_mut(), ptr::null_mut());
    (api().gtk_box_pack_start)((api().gtk_dialog_get_content_area)(dialog), scroll, 1, 1, 0);
    let view = (api().gtk_text_view_new)();
    (api().gtk_text_view_set_editable)(view, 0);
    (api().gtk_text_view_set_cursor_visible)(view, 0);
    (api().gtk_text_buffer_set_text)((api().gtk_text_view_get_buffer)(view), c(&content).as_ptr(), -1);
    (api().gtk_container_add)(scroll, view);
    (api().gtk_dialog_add_button)(dialog, c("Close").as_ptr(), -7);
    (api().gtk_widget_show_all)(dialog);
    let response = (api().gtk_dialog_run)(dialog);
    if response != -1 { (api().gtk_widget_destroy)(dialog); }
    0
}

pub(super) unsafe extern "C" fn preview_selected(_: P, data: P) {
    let app = &mut *(data as *mut App);
    let rows = selected_rows(app.list);
    if rows.len() != 1 { set_status(app, "Select one file to preview."); return; }
    let index = (api().gtk_list_box_row_get_index)(rows[0]);
    let entry = match app.entries.get(index as usize) { Some(e) if e.is_file => e, _ => return };
    let kind = viewer::kind(&entry.path);
    if kind == Kind::Office {
        if app.office_cancels.len() >= 4 { set_status(app, "Too many document previews are already running."); return; }
        set_status(app, "Preparing document preview…");
        let path = entry.path.clone();
        let title = entry.name.clone();
        let alive = app.alive.clone();
        let app_ptr = data as usize;
        let cancel = Arc::new(AtomicBool::new(false));
        app.office_cancels.push(cancel.clone());
        thread::spawn(move || {
            let result = zfm::office_render::preview(&path, &cancel);
            let message = Box::new(OfficePreviewResult { app: app_ptr, alive, cancel, title, result });
            unsafe { (api().g_idle_add)(office_preview_ready, Box::into_raw(message) as P); }
        });
        return;
    }
    if kind == Kind::Unsupported {
        set_status(app, "No built-in preview for this format. Use Open to choose the system's default app.");
        return;
    }
    let limit = match kind { Kind::Text => viewer::MAX_TEXT_BYTES, Kind::Image => viewer::MAX_IMAGE_BYTES, Kind::Pdf => zfm::pdf::MAX_PDF_BYTES, Kind::Office | Kind::Unsupported => unreachable!() };
    let bytes = match viewer::read_bounded(&entry.path, limit) {
        Ok(bytes) => bytes,
        Err(e) => { set_status(app, &format!("Preview failed: {e}")); return; }
    };
    let title = entry.name.clone();
    show_visual(app, &title, kind, bytes);
}

unsafe fn show_visual(app: &App, title: &str, kind: Kind, bytes: Vec<u8>) {
    let dialog = (api().gtk_dialog_new)();
    (api().gtk_window_set_title)(dialog, c(&format!("ZFM Preview — {title}")).as_ptr());
    (api().gtk_window_set_transient_for)(dialog, app.window);
    (api().gtk_window_set_modal)(dialog, 1);
    (api().gtk_window_set_destroy_with_parent)(dialog, 1);
    (api().gtk_window_set_default_size)(dialog, 900, 650);
    let content = (api().gtk_dialog_get_content_area)(dialog);
    let scroll = (api().gtk_scrolled_window_new)(ptr::null_mut(), ptr::null_mut());
    (api().gtk_box_pack_start)(content, scroll, 1, 1, 0);
    let mut pixbuf = ptr::null_mut();
    let mut image_widget = ptr::null_mut();
    let mut page_index = 0usize;
    let mut page_count = 0usize;
    let mut pdf_password: Option<String> = None;
    let info_label = label("");
    (api().gtk_label_set_xalign)(info_label, 0.0);
    (api().gtk_box_pack_start)(content, info_label, 0, 0, 6);
    if kind == Kind::Text {
        let text = match viewer::decode_text(&bytes) {
            Ok(text) => text,
            Err(e) => { (api().gtk_widget_destroy)(dialog); set_status(app, &format!("Preview failed: {e}")); return; }
        };
        let view = (api().gtk_text_view_new)();
        (api().gtk_text_view_set_editable)(view, 0);
        (api().gtk_text_view_set_cursor_visible)(view, 0);
        (api().gtk_text_buffer_set_text)((api().gtk_text_view_get_buffer)(view), c(&text).as_ptr(), -1);
        (api().gtk_container_add)(scroll, view);
        (api().gtk_label_set_text)(info_label, c(&format!("Read-only text · {} bytes", bytes.len())).as_ptr());
    } else {
        let rendered;
        let image_bytes = if kind == Kind::Pdf {
            let mut result = zfm::pdf::render_page_password(&bytes, 0, None);
            for _ in 0..3 {
                if !matches!(&result, Err(error) if error == zfm::pdf::PASSWORD_REQUIRED) { break; }
                let password = match super::archive_ui::password_prompt(app.window, "PDF password") {
                    Some(value) => value,
                    None => { (api().gtk_widget_destroy)(dialog); return; }
                };
                result = zfm::pdf::render_page_password(&bytes, 0, Some(&password));
                pdf_password = Some(password);
            }
            match result {
                Ok((page, count)) => { rendered = page; page_count = count; &rendered[..] }
                Err(error) => { (api().gtk_widget_destroy)(dialog); set_status(app, &error); return; }
            }
        } else { &bytes[..] };
        let (image, width, height) = match decode_image(image_bytes) {
            Ok(result) => result,
            Err(error) => { (api().gtk_widget_destroy)(dialog); set_status(app, &error); return; }
        };
        pixbuf = image;
        image_widget = (api().gtk_image_new_from_pixbuf)(pixbuf);
        (api().gtk_container_add)(scroll, image_widget);
        let info = if kind == Kind::Pdf { format!("Page 1 / {page_count} · PDF preview") } else { format!("{width} × {height} pixels · scaled preview") };
        (api().gtk_label_set_text)(info_label, c(&info).as_ptr());
        if kind == Kind::Pdf {
            (api().gtk_dialog_add_button)(dialog, c("Previous page").as_ptr(), 13);
            (api().gtk_dialog_add_button)(dialog, c("Next page").as_ptr(), 14);
        }
        (api().gtk_dialog_add_button)(dialog, c("−").as_ptr(), 10);
        (api().gtk_dialog_add_button)(dialog, c("Fit").as_ptr(), 11);
        (api().gtk_dialog_add_button)(dialog, c("+").as_ptr(), 12);
    }
    (api().gtk_dialog_add_button)(dialog, c("Close").as_ptr(), -7);
    (api().gtk_widget_show_all)(dialog);
    let mut zoom = 100i32;
    loop {
        let response = (api().gtk_dialog_run)(dialog);
        if pixbuf.is_null() || ![10, 11, 12, 13, 14].contains(&response) {
            if response != -1 { (api().gtk_widget_destroy)(dialog); }
            break;
        }
        if kind == Kind::Pdf && (response == 13 || response == 14) {
            let next = if response == 13 { page_index.saturating_sub(1) } else { (page_index + 1).min(page_count - 1) };
            if next != page_index {
                match zfm::pdf::render_page_password(&bytes, next, pdf_password.as_deref()).and_then(|(bmp, _)| decode_image(&bmp)) {
                    Ok((new_pixbuf, _, _)) => {
                        (api().g_object_unref)(pixbuf);
                        pixbuf = new_pixbuf;
                        page_index = next;
                        (api().gtk_label_set_text)(info_label, c(&format!("Page {} / {page_count} · PDF preview", page_index + 1)).as_ptr());
                    }
                    Err(error) => (api().gtk_label_set_text)(info_label, c(&format!("Page failed: {error}")).as_ptr()),
                }
            }
        } else {
            zoom = match response { 10 => (zoom - 25).max(25), 11 => 100, 12 => (zoom + 25).min(200), _ => zoom };
        }
        let width = ((api().gdk_pixbuf_get_width)(pixbuf) * zoom / 100).max(1);
        let height = ((api().gdk_pixbuf_get_height)(pixbuf) * zoom / 100).max(1);
        let scaled = (api().gdk_pixbuf_scale_simple)(pixbuf, width, height, 2);
        if !scaled.is_null() {
            (api().gtk_image_set_from_pixbuf)(image_widget, scaled);
            (api().g_object_unref)(scaled);
        }
    }
    if !pixbuf.is_null() { (api().g_object_unref)(pixbuf); }
}

struct RemovalResult { app: usize, alive: Arc<AtomicBool>, message: String }
unsafe extern "C" fn removal_done(data: P) -> c_int {
    let result = Box::from_raw(data as *mut RemovalResult);
    if result.alive.load(Ordering::Relaxed) { set_status(&*(result.app as *mut App), &result.message); }
    0
}

pub(super) unsafe extern "C" fn show_uninstaller(_: P, data: P) {
    let app = &*(data as *mut App);
    let dialog = (api().gtk_dialog_new)();
    (api().gtk_window_set_title)(dialog, c("Uninstall ZFM").as_ptr());
    (api().gtk_window_set_transient_for)(dialog, app.window);
    (api().gtk_window_set_modal)(dialog, 1);
    (api().gtk_window_set_destroy_with_parent)(dialog, 1);
    let text = label("Remove the installed Debian package ‘zfm’?\n\nYour personal files are not removed. Authentication is required.\nPackage manager dependency checks remain in effect.\nIf this was not installed as a Debian package, use your installation method instead.");
    (api().gtk_box_pack_start)((api().gtk_dialog_get_content_area)(dialog), text, 1, 1, 16);
    (api().gtk_dialog_add_button)(dialog, c("Cancel").as_ptr(), -6);
    (api().gtk_dialog_add_button)(dialog, c("Uninstall").as_ptr(), -5);
    (api().gtk_widget_show_all)(dialog);
    let response = (api().gtk_dialog_run)(dialog);
    if response != -1 { (api().gtk_widget_destroy)(dialog); }
    if response != -5 { return; }
    set_status(app, "Checking package and requesting authentication…");
    let app_ptr = data as usize;
    let alive = app.alive.clone();
    thread::spawn(move || {
        let output = std::process::Command::new("/usr/bin/dpkg-query")
            .args(["-W", "-f=${db:Status-Status}", "zfm"]).output();
        let message = match output {
            Ok(out) if out.status.success() && out.stdout == b"installed" => {
                match std::process::Command::new("/usr/bin/pkexec").args(["/usr/bin/dpkg", "--remove", "zfm"]).status() {
                    Ok(status) if status.success() => "ZFM package removed. Close this running window to finish.".to_string(),
                    Ok(_) => "Removal failed or authentication was cancelled. Try your package manager or sudo apt remove zfm.".to_string(),
                    Err(e) => format!("Could not start authentication: {e}. Use your package manager or sudo apt remove zfm."),
                }
            }
            _ => "No installed Debian zfm package was found. Use your installation method to remove ZFM.".to_string(),
        };
        let result = Box::new(RemovalResult { app: app_ptr, alive, message });
        unsafe { (api().g_idle_add)(removal_done, Box::into_raw(result) as P); }
    });
}
