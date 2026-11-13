//! GTK file clipboard using GNOME's copied-files target and text/uri-list.
use super::*;
use zfm::file_clipboard;

#[repr(C)]
struct TargetEntry {
    target: *mut c_char,
    flags: c_uint,
    info: c_uint,
}
struct ClipboardData {
    gnome: Vec<u8>,
    uri: Vec<u8>,
}
struct PasteRequest {
    app: usize,
    destination: PathBuf,
    generation: u64,
    gnome: bool,
}
unsafe fn atom(name: &str) -> P {
    (api().gdk_atom_intern)(c(name).as_ptr(), 0)
}
unsafe fn clipboard() -> P {
    (api().gtk_clipboard_get)(atom("CLIPBOARD"))
}
unsafe extern "C" fn clipboard_get(_: P, selection: P, info: c_uint, data: P) {
    let content = &*(data as *const ClipboardData);
    let bytes = if info == 0 {
        &content.gnome
    } else {
        &content.uri
    };
    (api().gtk_selection_data_set)(
        selection,
        (api().gtk_selection_data_get_target)(selection),
        8,
        bytes.as_ptr(),
        bytes.len() as c_int,
    );
}
unsafe extern "C" fn clipboard_clear(_: P, data: P) {
    drop(Box::from_raw(data as *mut ClipboardData));
}
unsafe fn set_file(data: P, cut: bool) {
    let app = &*(data as *mut App);
    let rows = selected_rows(app.list);
    if rows.is_empty() || rows.len() > zfm::operations::MAX_COPY_ITEMS || (cut && rows.len() != 1) {
        set_status(
            app,
            if cut {
                "Cut supports exactly one item."
            } else {
                "Select between 1 and 1024 items to copy."
            },
        );
        return;
    }
    let mut paths = Vec::with_capacity(rows.len());
    for row in rows {
        let index = (api().gtk_list_box_row_get_index)(row);
        let entry = if index < 0 {
            None
        } else {
            app.entries.get(index as usize)
        };
        match entry {
            Some(entry) if cut || entry.is_file || entry.is_dir => paths.push(entry.path.clone()),
            _ => {
                set_status(
                    app,
                    "Copy supports regular files and folders; links are not copied.",
                );
                return;
            }
        }
    }
    let gnome = match file_clipboard::encode_gnome(&paths, cut) {
        Ok(v) => v,
        Err(e) => {
            set_status(app, &e.to_string());
            return;
        }
    };
    let uri = match file_clipboard::encode_uri_list(&paths) {
        Ok(v) => v,
        Err(e) => {
            set_status(app, &e.to_string());
            return;
        }
    };
    let gnome_name = c("x-special/gnome-copied-files");
    let uri_name = c("text/uri-list");
    let targets = [
        TargetEntry {
            target: gnome_name.as_ptr() as *mut c_char,
            flags: 0,
            info: 0,
        },
        TargetEntry {
            target: uri_name.as_ptr() as *mut c_char,
            flags: 0,
            info: 1,
        },
    ];
    let content = Box::into_raw(Box::new(ClipboardData { gnome, uri })) as P;
    let ok = (api().gtk_clipboard_set_with_data)(
        clipboard(),
        targets.as_ptr() as P,
        targets.len() as c_uint,
        clipboard_get as P,
        clipboard_clear as P,
        content,
    );
    if ok == 0 {
        drop(Box::from_raw(content as *mut ClipboardData));
        set_status(&*(data as *mut App), "Could not set the file clipboard.");
    } else {
        set_status(
            &*(data as *mut App),
            if cut {
                "Cut: choose a destination and paste."
            } else {
                "Copied: choose a destination and paste."
            },
        );
    }
}
pub(super) unsafe extern "C" fn copy_to_clipboard(_: P, data: P) {
    set_file(data, false);
}
pub(super) unsafe extern "C" fn cut_to_clipboard(_: P, data: P) {
    set_file(data, true);
}
pub(super) unsafe extern "C" fn paste_from_clipboard(_: P, data: P) {
    let app = &*(data as *mut App);
    let request = Box::new(PasteRequest {
        app: data as usize,
        destination: app.tabs.current().current().to_path_buf(),
        generation: app.generation.load(Ordering::Relaxed),
        gnome: true,
    });
    // This can invoke the callback synchronously. No App reference survives it.
    (api().gtk_clipboard_request_contents)(
        clipboard(),
        atom("x-special/gnome-copied-files"),
        received as P,
        Box::into_raw(request) as P,
    );
}
unsafe extern "C" fn received(clip: P, selection: P, data: P) {
    let mut request = Box::from_raw(data as *mut PasteRequest);
    let length = (api().gtk_selection_data_get_length)(selection);
    if length < 0 && request.gnome {
        request.gnome = false;
        (api().gtk_clipboard_request_contents)(
            clip,
            atom("text/uri-list"),
            received as P,
            Box::into_raw(request) as P,
        );
        return;
    }
    let app = &*(request.app as *mut App);
    if app.generation.load(Ordering::Relaxed) != request.generation
        || app.tabs.current().current() != request.destination
    {
        set_status(app, "Paste cancelled because the destination changed.");
        return;
    }
    let bytes = (api().gtk_selection_data_get_data)(selection);
    if length < 0 || length > 64 * 1024 || bytes.is_null() {
        set_status(
            app,
            "The clipboard does not contain a supported local file.",
        );
        return;
    }
    let bytes = std::slice::from_raw_parts(bytes, length as usize);
    let item = match file_clipboard::parse(bytes, request.gnome) {
        Ok(item) => item,
        Err(e) => {
            set_status(app, &format!("Cannot paste: {e}"));
            return;
        }
    };
    let operation = if item.cut {
        Operation::MoveTo(item.paths[0].clone(), request.destination.clone())
    } else {
        Operation::CopyItems(item.paths, request.destination.clone())
    };
    start_operation(request.app as P, request.destination, operation);
}
