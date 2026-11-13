// Modified: expose 7z support through the existing bounded archive manager.
//! GTK archive-manager adapter; all widget calls are made on the GTK main loop.
use super::*;

#[repr(C)]
struct GList {
    data: P,
    next: *mut GList,
    prev: *mut GList,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ArchiveFingerprint {
    dev: u64,
    inode: u64,
    size: u64,
    modified: Option<std::time::SystemTime>,
}
fn archive_fingerprint(file: &std::fs::File) -> std::io::Result<ArchiveFingerprint> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    if !m.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Archive must be a regular file",
        ));
    }
    Ok(ArchiveFingerprint {
        dev: m.dev(),
        inode: m.ino(),
        size: m.len(),
        modified: m.modified().ok(),
    })
}
#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
fn open_archive(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    // Avoid following a symlink or blocking if a FIFO is selected.
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x800 | zfm::linux_flags::O_NOFOLLOW)
        .open(path)
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
fn open_archive(_: &std::path::Path) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Safe archive access is not supported on this architecture",
    ))
}

#[derive(Clone, Copy)]
enum SourceFormat {
    Tar,
    Packed(zfm::packed_archive::Format),
}
#[derive(Clone)]
struct ArchiveItem {
    path: String,
    size: u64,
    kind: zfm::archive::Kind,
}
fn source_format(path: &std::path::Path) -> Option<SourceFormat> {
    if let Some(format) = zfm::packed_archive::format_for_path(path) {
        Some(SourceFormat::Packed(format))
    } else if path
        .extension()
        .and_then(|s| s.to_str())
        .map_or(false, |s| s.eq_ignore_ascii_case("tar"))
    {
        Some(SourceFormat::Tar)
    } else {
        None
    }
}
pub(super) fn supports_archive(path: &std::path::Path) -> bool {
    source_format(path).is_some() || path.extension().and_then(|v| v.to_str()).map_or(false, |ext|
        ["rar", "iso", "cab", "arj", "lzh", "wim", "xz", "gz", "bz2"].iter().any(|v| ext.eq_ignore_ascii_case(v)))
}
pub(super) struct ArchiveManager {
    window: P,
    input: P,
    destination: P,
    list: P,
    status: P,
    progress: P,
    entries: Vec<ArchiveItem>,
    engine_lines: Option<Vec<String>>,
    format: Option<SourceFormat>,
    render_index: usize,
    path: Option<PathBuf>,
    fingerprint: Option<ArchiveFingerprint>,
    busy: bool,
    job: u64,
    pub(super) cancel: Arc<AtomicBool>,
}
enum ArchiveEvent {
    Loaded(Result<(Vec<ArchiveItem>, PathBuf, ArchiveFingerprint, SourceFormat), String>),
    EngineReport(Result<Vec<String>, String>, bool),
    Progress(u64, u64, Arc<AtomicBool>),
    Extracted(Result<usize, String>),
    Created(Result<PathBuf, String>),
    Tested(Result<(usize, bool), String>),
}
struct ArchiveMessage {
    manager: usize,
    job: u64,
    event: ArchiveEvent,
}
struct ArchiveRender {
    manager: usize,
    job: u64,
}
unsafe fn archive_status(manager: &ArchiveManager, text: &str) {
    (api().gtk_label_set_text)(manager.status, c(text).as_ptr());
}
unsafe extern "C" fn archive_update(data: P) -> c_int {
    let msg = Box::from_raw(data as *mut ArchiveMessage);
    if let ArchiveEvent::Progress(_, _, pending) = &msg.event {
        pending.store(false, Ordering::Release);
    }
    let manager = &mut *(msg.manager as *mut ArchiveManager);
    if manager.job != msg.job {
        return 0;
    }
    match msg.event {
        ArchiveEvent::Loaded(Ok((entries, path, fingerprint, format))) => {
            manager.entries = entries;
            manager.path = Some(path);
            manager.fingerprint = Some(fingerprint);
            manager.format = Some(format);
            manager.render_index = 0;
            let render = Box::new(ArchiveRender {
                manager: msg.manager,
                job: msg.job,
            });
            (api().g_idle_add)(archive_render, Box::into_raw(render) as P);
        }
        ArchiveEvent::EngineReport(Ok(lines), testing) => {
            manager.engine_lines = Some(lines);
            manager.render_index = 0;
            if testing { archive_status(manager, "Engine test passed. Checksums do not prove authenticity."); }
            let render = Box::new(ArchiveRender { manager: msg.manager, job: msg.job });
            (api().g_idle_add)(archive_render, Box::into_raw(render) as P);
        }
        ArchiveEvent::EngineReport(Err(error), _) => {
            manager.busy = false;
            archive_status(manager, &format!("Archive engine failed: {error}"));
        }
        ArchiveEvent::Loaded(Err(error)) => {
            manager.busy = false;
            archive_status(manager, &format!("Could not read archive: {error}"));
        }
        ArchiveEvent::Progress(bytes, total, _) if manager.busy => {
            if total > 0 {
                (api().gtk_progress_bar_set_fraction)(
                    manager.progress,
                    (bytes as f64 / total as f64).min(0.99),
                );
            }
            archive_status(manager, &format!("Processed {bytes} bytes…"));
        }
        ArchiveEvent::Progress(..) => (),
        ArchiveEvent::Tested(result) => {
            manager.busy = false;
            match result {
                Ok((count, true)) => {
                    (api().gtk_progress_bar_set_fraction)(manager.progress, 1.0);
                    archive_status(
                        manager,
                        &format!("Checked {count} TAR headers. TAR has no content checksum."),
                    );
                }
                Ok((count, false)) => {
                    (api().gtk_progress_bar_set_fraction)(manager.progress, 1.0);
                    archive_status(
                        manager,
                        &format!("Tested {count} entries. Available format checks passed."),
                    );
                }
                Err(error) => archive_status(manager, &format!("Archive test failed: {error}")),
            }
        }
        ArchiveEvent::Created(result) => {
            manager.busy = false;
            match result {
                Ok(path) => {
                    (api().gtk_progress_bar_set_fraction)(manager.progress, 1.0);
                    archive_status(manager, &format!("Created {}", path.display()));
                }
                Err(error) => {
                    archive_status(manager, &format!("Archive creation stopped: {error}"))
                }
            }
        }
        ArchiveEvent::Extracted(result) => {
            manager.busy = false;
            match result {
                Ok(count) => {
                    (api().gtk_progress_bar_set_fraction)(manager.progress, 1.0);
                    archive_status(
                        manager,
                        &format!("Extracted {count} entries. Do not run untrusted files."),
                    );
                }
                Err(error) => archive_status(
                    manager,
                    &format!("Extraction stopped: {error}. Earlier entries may remain."),
                ),
            }
        }
    }
    0
}
unsafe extern "C" fn archive_render(data: P) -> c_int {
    let msg = &*(data as *mut ArchiveRender);
    let manager = &mut *(msg.manager as *mut ArchiveManager);
    if msg.job != manager.job || manager.cancel.load(Ordering::Relaxed) {
        if msg.job == manager.job {
            manager.busy = false;
            archive_status(
                manager,
                "Archive loading cancelled. Load it again to extract.",
            );
            manager.path = None;
            manager.fingerprint = None;
        }
        drop(Box::from_raw(data as *mut ArchiveRender));
        return 0;
    }
    let length = manager.engine_lines.as_ref().map_or(manager.entries.len(), Vec::len);
    let end = (manager.render_index + 64).min(length);
    if let Some(lines) = &manager.engine_lines {
        for line in &lines[manager.render_index..end] {
            let row = label(line);
            (api().gtk_label_set_xalign)(row, 0.0);
            (api().gtk_list_box_insert)(manager.list, row, -1);
            (api().gtk_widget_show_all)(row);
        }
    } else {
    for entry in &manager.entries[manager.render_index..end] {
        let row = label(&format!(
            "{}    ·    {}",
            entry.path,
            if entry.kind == zfm::archive::Kind::Directory {
                "folder".to_owned()
            } else {
                format!("{} bytes", entry.size)
            }
        ));
        (api().gtk_label_set_xalign)(row, 0.0);
        (api().gtk_list_box_insert)(manager.list, row, -1);
        (api().gtk_widget_show_all)(row);
    }
    }
    manager.render_index = end;
    if end < length {
        1
    } else {
        manager.busy = false;
        if manager.engine_lines.is_some() {
            archive_status(manager, "Engine operation completed. Listing does not verify contents; engine extract-all is available separately.");
        } else {
            archive_status(manager, &format!("{} entries loaded. Use Test to read data and check available checksums.", manager.entries.len()));
        }
        drop(Box::from_raw(data as *mut ArchiveRender));
        0
    }
}
unsafe extern "C" fn archive_load(_: P, data: P) {
    let manager = &mut *(data as *mut ArchiveManager);
    if manager.busy {
        archive_status(manager, "An archive operation is already running.");
        return;
    }
    let path = CStr::from_ptr((api().gtk_entry_get_text)(manager.input))
        .to_string_lossy()
        .into_owned();
    if path.is_empty() {
        archive_status(manager, "Choose a TAR, ZIP, 7z or compressed TAR archive.");
        return;
    }
    let path = PathBuf::from(path);
    let format = match source_format(&path) {
        Some(format) => format,
        None => {
            archive_status(
                manager,
                "Supported: .tar, .zip, .7z, .tar.gz, .tgz, .tar.bz2, .tar.xz",
            );
            return;
        }
    };
    manager.busy = true;
    manager.job += 1;
    manager.cancel = Arc::new(AtomicBool::new(false));
    manager.path = None;
    manager.fingerprint = None;
    manager.format = None;
    manager.entries.clear();
    manager.engine_lines = None;
    (api().gtk_container_foreach)(manager.list, destroy_child, ptr::null_mut());
    (api().gtk_progress_bar_set_fraction)(manager.progress, 0.0);
    archive_status(manager, "Reading archive…");
    let job = manager.job;
    let ptr = data as usize;
    let cancel = manager.cancel.clone();
    thread::spawn(move || {
        let result = (|| -> std::io::Result<_> {
            let mut file = open_archive(&path)?;
            let before = archive_fingerprint(&file)?;
            let entries: Vec<ArchiveItem> = match format {
                SourceFormat::Tar => zfm::archive::inspect(&mut file, &cancel)?
                    .into_iter()
                    .map(|e| ArchiveItem {
                        path: e.path,
                        size: e.size,
                        kind: e.kind,
                    })
                    .collect(),
                SourceFormat::Packed(format) => {
                    zfm::packed_archive::inspect(&mut file, format, &cancel)?
                        .into_iter()
                        .map(|e| ArchiveItem {
                            path: e.path,
                            size: e.size,
                            kind: e.kind,
                        })
                        .collect()
                }
            };
            if archive_fingerprint(&file)? != before {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Archive changed while reading",
                ));
            }
            Ok((entries, path, before, format))
        })()
        .map_err(|e| e.to_string());
        let msg = Box::new(ArchiveMessage {
            manager: ptr,
            job,
            event: ArchiveEvent::Loaded(result),
        });
        unsafe {
            (api().g_idle_add)(archive_update, Box::into_raw(msg) as P);
        }
    });
}
// Modal input uses GTK's hidden-entry mode. Passwords are passed to the engine
// over a pipe, never in the command line or environment.
pub(super) unsafe fn password_prompt(parent: P, title: &str) -> Option<String> {
    let dialog = (api().gtk_dialog_new)();
    (api().gtk_window_set_title)(dialog, c(title).as_ptr());
    (api().gtk_window_set_transient_for)(dialog, parent);
    (api().gtk_window_set_modal)(dialog, 1);
    (api().gtk_window_set_destroy_with_parent)(dialog, 1);
    let area = (api().gtk_dialog_get_content_area)(dialog);
    (api().gtk_box_pack_start)(area, label("Password:"), 0, 0, 8);
    let entry = (api().gtk_entry_new)();
    (api().gtk_entry_set_visibility)(entry, 0);
    (api().gtk_entry_set_activates_default)(entry, 1);
    (api().gtk_box_pack_start)(area, entry, 0, 0, 8);
    (api().gtk_dialog_add_button)(dialog, c("Cancel").as_ptr(), -6);
    (api().gtk_dialog_add_button)(dialog, c("OK").as_ptr(), -3);
    (api().gtk_dialog_set_default_response)(dialog, -3);
    (api().gtk_widget_show_all)(dialog);
    (api().gtk_widget_grab_focus)(entry);
    let response = (api().gtk_dialog_run)(dialog);
    let result = if response == -3 {
        Some(CStr::from_ptr((api().gtk_entry_get_text)(entry)).to_string_lossy().into_owned())
    } else { None };
    if response != -1 { (api().gtk_widget_destroy)(dialog); }
    result
}

unsafe fn run_engine_report(data: P, testing: bool, encrypted: bool) {
    let manager = &*(data as *mut ArchiveManager);
    if manager.busy { archive_status(manager, "An archive operation is already running."); return; }
    let path = CStr::from_ptr((api().gtk_entry_get_text)(manager.input)).to_string_lossy().into_owned();
    if path.is_empty() { archive_status(manager, "Choose an archive first."); return; }
    let window = manager.window;
    let observed_job = manager.job;
    let password = if encrypted {
        match password_prompt(window, "Archive password") { Some(p) => Some(p), None => return }
    } else { None };
    let manager = &mut *(data as *mut ArchiveManager);
    if manager.busy || manager.job != observed_job || CStr::from_ptr((api().gtk_entry_get_text)(manager.input)).to_bytes() != path.as_bytes() {
        archive_status(manager, "Archive changed; review and retry."); return;
    }
    manager.busy = true;
    manager.job += 1;
    manager.cancel = Arc::new(AtomicBool::new(false));
    manager.path = None;
    manager.fingerprint = None;
    manager.format = None;
    manager.entries.clear();
    manager.engine_lines = None;
    (api().gtk_container_foreach)(manager.list, destroy_child, ptr::null_mut());
    (api().gtk_progress_bar_set_fraction)(manager.progress, 0.0);
    archive_status(manager, if testing { "Testing with bundled engine…" } else { "Listing with bundled engine…" });
    let job = manager.job;
    let ptr = data as usize;
    let cancel = manager.cancel.clone();
    thread::spawn(move || {
        let result = zfm::archive_engine::run_engine_archive_password(std::path::Path::new(&path), testing, &cancel, password.as_deref());
        let msg = Box::new(ArchiveMessage { manager: ptr, job, event: ArchiveEvent::EngineReport(result, testing) });
        unsafe { (api().g_idle_add)(archive_update, Box::into_raw(msg) as P); }
    });
}
unsafe extern "C" fn archive_engine_list(_: P, data: P) { run_engine_report(data, false, false); }
unsafe extern "C" fn archive_engine_test(_: P, data: P) { run_engine_report(data, true, false); }
unsafe extern "C" fn archive_engine_list_password(_: P, data: P) { run_engine_report(data, false, true); }
unsafe extern "C" fn archive_engine_test_password(_: P, data: P) { run_engine_report(data, true, true); }

unsafe extern "C" fn archive_engine_extract(_: P, data: P) { engine_extract_all(data, false); }
unsafe extern "C" fn archive_engine_extract_password(_: P, data: P) { engine_extract_all(data, true); }
unsafe fn engine_extract_all(data: P, encrypted: bool) {
    let manager = &*(data as *mut ArchiveManager);
    if manager.busy { archive_status(manager, "An archive operation is already running."); return; }
    let input = CStr::from_ptr((api().gtk_entry_get_text)(manager.input)).to_string_lossy().into_owned();
    let destination = CStr::from_ptr((api().gtk_entry_get_text)(manager.destination)).to_string_lossy().into_owned();
    if input.is_empty() || destination.is_empty() { archive_status(manager, "Choose an archive and destination first."); return; }
    let window = manager.window;
    let observed_job = manager.job;
    let password = if encrypted {
        match password_prompt(window, "Archive password") { Some(p) => Some(p), None => return }
    } else { None };
    if !archive_confirm(window, &destination) { return; }
    let manager = &mut *(data as *mut ArchiveManager);
    if manager.busy || manager.job != observed_job
        || CStr::from_ptr((api().gtk_entry_get_text)(manager.input)).to_bytes() != input.as_bytes()
        || CStr::from_ptr((api().gtk_entry_get_text)(manager.destination)).to_bytes() != destination.as_bytes() {
        archive_status(manager, "Archive or destination changed; review and retry."); return;
    }
    manager.busy = true;
    manager.job += 1;
    manager.cancel = Arc::new(AtomicBool::new(false));
    (api().gtk_progress_bar_set_fraction)(manager.progress, 0.0);
    archive_status(manager, "Extracting with bundled engine… Earlier files remain on failure.");
    let job = manager.job;
    let ptr = data as usize;
    let cancel = manager.cancel.clone();
    thread::spawn(move || {
        let result = zfm::archive_engine::extract_all_linux(std::path::Path::new(&input), std::path::Path::new(&destination), &cancel, password.as_deref()).map_err(|e| e.to_string());
        let msg = Box::new(ArchiveMessage { manager: ptr, job, event: ArchiveEvent::Extracted(result) });
        unsafe { (api().g_idle_add)(archive_update, Box::into_raw(msg) as P); }
    });
}

unsafe extern "C" fn archive_browse(_: P, data: P) {
    let window = (*(data as *mut ArchiveManager)).window;
    let dialog = (api().gtk_file_chooser_native_new)(
        c("Choose archive").as_ptr(),
        window,
        0,
        c("Open").as_ptr(),
        c("Cancel").as_ptr(),
    );
    if (api().gtk_native_dialog_run)(dialog) == -3 {
        let filename = (api().gtk_file_chooser_get_filename)(dialog);
        if !filename.is_null() {
            let path = CStr::from_ptr(filename).to_string_lossy().into_owned();
            (api().g_free)(filename as P);
            let manager = &*(data as *mut ArchiveManager);
            if !manager.busy {
                (api().gtk_entry_set_text)(manager.input, c(&path).as_ptr());
                if let Some(parent) = std::path::Path::new(&path).parent() {
                    (api().gtk_entry_set_text)(
                        manager.destination,
                        c(&parent.to_string_lossy()).as_ptr(),
                    );
                }
                archive_load(ptr::null_mut(), data);
            }
        }
    }
    (api().g_object_unref)(dialog);
}
unsafe extern "C" fn archive_destination(_: P, data: P) {
    let window = (*(data as *mut ArchiveManager)).window;
    if let Some(path) = choose_folder(window) {
        (api().gtk_entry_set_text)(
            (*(data as *mut ArchiveManager)).destination,
            c(&path.to_string_lossy()).as_ptr(),
        );
    }
}
unsafe extern "C" fn archive_select_all(_: P, data: P) {
    (api().gtk_list_box_select_all)((*(data as *mut ArchiveManager)).list);
}
unsafe extern "C" fn archive_select_none(_: P, data: P) {
    (api().gtk_list_box_unselect_all)((*(data as *mut ArchiveManager)).list);
}
unsafe extern "C" fn archive_cancel(_: P, data: P) {
    let manager = &*(data as *mut ArchiveManager);
    if manager.busy {
        manager.cancel.store(true, Ordering::Relaxed);
        archive_status(manager, "Cancellation requested…");
    }
}
unsafe fn choose_archive_file(parent: P, title: &str, save: bool) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let dialog = (api().gtk_file_chooser_native_new)(
        c(title).as_ptr(),
        parent,
        if save { 1 } else { 0 },
        c(if save { "Save" } else { "Open" }).as_ptr(),
        c("Cancel").as_ptr(),
    );
    let result = if (api().gtk_native_dialog_run)(dialog) == -3 {
        let filename = (api().gtk_file_chooser_get_filename)(dialog);
        if filename.is_null() {
            None
        } else {
            let path = PathBuf::from(std::ffi::OsStr::from_bytes(
                CStr::from_ptr(filename).to_bytes(),
            ));
            (api().g_free)(filename as P);
            Some(path)
        }
    } else {
        None
    };
    (api().g_object_unref)(dialog);
    result
}
/// Create a multi-file/folder archive from the file manager's current selection.
pub(super) unsafe extern "C" fn archive_create_selection(_: P, data: P) { create_selection(data, false); }
pub(super) unsafe extern "C" fn archive_create_encrypted_selection(_: P, data: P) { create_selection(data, true); }
unsafe fn create_selection(data: P, encrypted: bool) {
    let app = &*(data as *mut App);
    let manager_ptr = app.archive_manager;
    let manager = &*(manager_ptr as *mut ArchiveManager);
    if manager.busy { set_status(app, "An archive operation is already running."); return; }
    let observed_job = manager.job;
    let window = app.window;
    let rows = selected_rows(app.list);
    if rows.is_empty() || rows.len() > 1024 { set_status(app, "Select between 1 and 1024 files or folders."); return; }
    let mut sources = Vec::new();
    for row in rows {
        let index = (api().gtk_list_box_row_get_index)(row);
        match app.entries.get(index as usize) {
            Some(entry) if entry.is_file || entry.is_dir => sources.push(entry.path.clone()),
            _ => { set_status(app, "Select regular files or folders only."); return; }
        }
    }
    let output = match choose_archive_file(window, "Save archive (.7z, .zip, .tar, .tar.gz, .tar.bz2, .tar.xz, .gz, .bz2 or .xz)", true) {
        Some(path) => path,
        None => return,
    };
    if !zfm::archive_engine::supports_creation(&output, encrypted) {
        set_status(app, if encrypted { "Choose an encrypted .7z or .zip output filename." } else { "Choose .7z, .zip, .tar, compressed TAR, .gz, .bz2 or .xz." });
        return;
    }
    let password = if encrypted {
        let first = match password_prompt(window, "Create encrypted archive: password") { Some(p) => p, None => return };
        let second = match password_prompt(window, "Confirm archive password") { Some(p) => p, None => return };
        if first != second { set_status(app, "Passwords do not match."); return; }
        Some(first)
    } else { None };
    let manager = &mut *(manager_ptr as *mut ArchiveManager);
    if manager.busy || manager.job != observed_job { set_status(&*(data as *mut App), "Archive operation changed; retry."); return; }
    manager.busy = true;
    manager.job += 1;
    manager.cancel = Arc::new(AtomicBool::new(false));
    (api().gtk_progress_bar_set_fraction)(manager.progress, 0.0);
    archive_status(manager, "Creating archive from selection… Use Cancel to stop.");
    (api().gtk_widget_show_all)(manager.window);
    let job = manager.job;
    let manager = manager_ptr as usize;
    let cancel = (*(manager_ptr as *mut ArchiveManager)).cancel.clone();
    thread::spawn(move || {
        let result = zfm::archive_engine::create_items_password(&sources, &output, &cancel, password.as_deref())
            .map(|_| output).map_err(|e| e.to_string());
        let message = Box::new(ArchiveMessage { manager, job, event: ArchiveEvent::Created(result) });
        unsafe { (api().g_idle_add)(archive_update, Box::into_raw(message) as P); }
    });
}

unsafe extern "C" fn archive_create(_: P, data: P) {
    let window = (*(data as *mut ArchiveManager)).window;
    if (*(data as *mut ArchiveManager)).busy {
        archive_status(
            &*(data as *mut ArchiveManager),
            "An archive operation is already running.",
        );
        return;
    }
    let source = match choose_archive_file(window, "Choose a regular file to archive", false) {
        Some(path) => path,
        None => return,
    };
    let output = match choose_archive_file(window, "Save .tar, .zip or .7z archive as", true) {
        Some(path) => path,
        None => return,
    };
    let extension = output.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !["tar", "zip", "7z"].iter().any(|e| extension.eq_ignore_ascii_case(e)) {
        archive_status(&*(data as *mut ArchiveManager), "Choose a .tar, .zip or .7z output filename.");
        return;
    }
    let manager = &mut *(data as *mut ArchiveManager);
    if manager.busy {
        archive_status(manager, "An archive operation is already running.");
        return;
    }
    manager.busy = true;
    manager.job += 1;
    manager.cancel = Arc::new(AtomicBool::new(false));
    (api().gtk_progress_bar_set_fraction)(manager.progress, 0.0);
    archive_status(manager, "Creating archive…");
    let job = manager.job;
    let ptr = data as usize;
    let cancel = manager.cancel.clone();
    thread::spawn(move || {
        let result = zfm::archive_engine::create_single_file(&source, &output, &cancel);
        let result = result.map(|_| output).map_err(|e| e.to_string());
        let msg = Box::new(ArchiveMessage {
            manager: ptr,
            job,
            event: ArchiveEvent::Created(result),
        });
        unsafe {
            (api().g_idle_add)(archive_update, Box::into_raw(msg) as P);
        }
    });
}
unsafe extern "C" fn archive_test(_: P, data: P) {
    let manager = &mut *(data as *mut ArchiveManager);
    if manager.busy {
        archive_status(manager, "An archive operation is already running.");
        return;
    }
    let path_text = CStr::from_ptr((api().gtk_entry_get_text)(manager.input))
        .to_string_lossy()
        .into_owned();
    let path = match &manager.path {
        Some(path) if *path == PathBuf::from(path_text) => path.clone(),
        _ => {
            archive_status(manager, "Load the current archive first.");
            return;
        }
    };
    let fingerprint = match &manager.fingerprint {
        Some(value) => value.clone(),
        None => return,
    };
    let format = match manager.format {
        Some(value) => value,
        None => return,
    };
    let total: u64 = manager.entries.iter().map(|entry| entry.size).sum();
    manager.busy = true;
    manager.job += 1;
    manager.cancel = Arc::new(AtomicBool::new(false));
    (api().gtk_progress_bar_set_fraction)(manager.progress, 0.0);
    archive_status(manager, "Testing archive…");
    let job = manager.job;
    let ptr = data as usize;
    let cancel = manager.cancel.clone();
    thread::spawn(move || {
        let result = (|| -> std::io::Result<(usize, bool)> {
            let mut file = open_archive(&path)?;
            if archive_fingerprint(&file)? != fingerprint {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Archive changed; load it again",
                ));
            }
            match format {
                SourceFormat::Tar => Ok((zfm::archive::inspect(&mut file, &cancel)?.len(), true)),
                SourceFormat::Packed(format) => {
                    let pending = Arc::new(AtomicBool::new(false));
                    let count = zfm::packed_archive::verify(&mut file, format, &cancel, |bytes| {
                        if pending.swap(true, Ordering::AcqRel) {
                            return;
                        }
                        let msg = Box::new(ArchiveMessage {
                            manager: ptr,
                            job,
                            event: ArchiveEvent::Progress(bytes, total, pending.clone()),
                        });
                        unsafe {
                            (api().g_idle_add)(archive_update, Box::into_raw(msg) as P);
                        }
                    })?;
                    Ok((count, false))
                }
            }
        })()
        .map_err(|e| e.to_string());
        let msg = Box::new(ArchiveMessage {
            manager: ptr,
            job,
            event: ArchiveEvent::Tested(result),
        });
        unsafe {
            (api().g_idle_add)(archive_update, Box::into_raw(msg) as P);
        }
    });
}
unsafe extern "C" fn archive_extract_all(_: P, data: P) {
    archive_extract(data, false);
}
unsafe extern "C" fn archive_extract_selected(_: P, data: P) {
    archive_extract(data, true);
}
unsafe fn archive_extract(data: P, selected_only: bool) {
    let manager = &*(data as *mut ArchiveManager);
    if manager.busy {
        archive_status(manager, "An archive operation is already running.");
        return;
    }
    let path_text = CStr::from_ptr((api().gtk_entry_get_text)(manager.input))
        .to_string_lossy()
        .into_owned();
    let destination = CStr::from_ptr((api().gtk_entry_get_text)(manager.destination))
        .to_string_lossy()
        .into_owned();
    let path = match &manager.path {
        Some(path) if *path == PathBuf::from(&path_text) => path.clone(),
        _ => {
            archive_status(manager, "Load the current archive before extracting.");
            return;
        }
    };
    let fingerprint = match &manager.fingerprint {
        Some(v) => v.clone(),
        None => return,
    };
    let format = match manager.format {
        Some(value) => value,
        None => return,
    };
    if destination.is_empty() {
        archive_status(manager, "Choose a destination folder.");
        return;
    }
    let selected = if selected_only {
        let head = (api().gtk_list_box_get_selected_rows)(manager.list) as *mut GList;
        let mut item = head;
        let mut paths = Vec::new();
        while !item.is_null() {
            let index = (api().gtk_list_box_row_get_index)((*item).data);
            if index >= 0 {
                if let Some(entry) = manager.entries.get(index as usize) {
                    paths.push(entry.path.clone());
                }
            }
            item = (*item).next;
        }
        (api().g_list_free)(head as P);
        if paths.is_empty() {
            archive_status(manager, "Select one or more archive entries.");
            return;
        }
        Some(paths)
    } else {
        None
    };
    let selected_set = selected.as_ref().map(|names| {
        names
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>()
    });
    let total: u64 = manager
        .entries
        .iter()
        .filter(|entry| {
            selected_set
                .as_ref()
                .map_or(true, |names| names.contains(entry.path.as_str()))
        })
        .map(|entry| entry.size)
        .sum();
    let window = manager.window;
    let observed_job = manager.job;
    // Do not retain references to manager across the nested confirmation loop.
    if !archive_confirm(window, &destination) {
        return;
    }
    let manager = &mut *(data as *mut ArchiveManager);
    if manager.busy || manager.job != observed_job {
        archive_status(manager, "Archive changed; review it before extracting.");
        return;
    }
    manager.busy = true;
    manager.job += 1;
    manager.cancel = Arc::new(AtomicBool::new(false));
    (api().gtk_progress_bar_set_fraction)(manager.progress, 0.0);
    archive_status(manager, "Extracting…");
    let job = manager.job;
    let ptr = data as usize;
    let cancel = manager.cancel.clone();
    thread::spawn(move || {
        let result = (|| -> std::io::Result<usize> {
            let mut file = open_archive(&path)?;
            if archive_fingerprint(&file)? != fingerprint {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Archive changed; load it again",
                ));
            }
            let pending = Arc::new(AtomicBool::new(false));
            let mut report = |bytes| {
                if pending.swap(true, Ordering::AcqRel) {
                    return;
                }
                let msg = Box::new(ArchiveMessage {
                    manager: ptr,
                    job,
                    event: ArchiveEvent::Progress(bytes, total, pending.clone()),
                });
                unsafe {
                    (api().g_idle_add)(archive_update, Box::into_raw(msg) as P);
                }
            };
            match format {
                SourceFormat::Tar => zfm::archive::extract(
                    &mut file,
                    std::path::Path::new(&destination),
                    selected.as_deref(),
                    &cancel,
                    &mut report,
                ),
                SourceFormat::Packed(format) => zfm::packed_archive::extract(
                    &mut file,
                    format,
                    std::path::Path::new(&destination),
                    selected.as_deref(),
                    &cancel,
                    &mut report,
                ),
            }
        })()
        .map_err(|e| e.to_string());
        let msg = Box::new(ArchiveMessage {
            manager: ptr,
            job,
            event: ArchiveEvent::Extracted(result),
        });
        unsafe {
            (api().g_idle_add)(archive_update, Box::into_raw(msg) as P);
        }
    });
}
unsafe fn archive_confirm(parent: P, destination: &str) -> bool {
    let dialog = (api().gtk_dialog_new)();
    (api().gtk_window_set_title)(dialog, c("Extract archive?").as_ptr());
    (api().gtk_window_set_transient_for)(dialog, parent);
    (api().gtk_window_set_modal)(dialog, 1);
    (api().gtk_window_set_destroy_with_parent)(dialog, 1);
    (api().gtk_dialog_add_button)(dialog, c("Cancel").as_ptr(), -6);
    (api().gtk_dialog_add_button)(dialog, c("Extract").as_ptr(), -3);
    let message = label(&format!("Extract into {destination}?\nExisting files are not overwritten. Earlier files remain if extraction stops.\nArchives may contain dangerous files; never run untrusted content."));
    (api().gtk_widget_set_margin_start)(message, 12);
    (api().gtk_widget_set_margin_end)(message, 12);
    (api().gtk_widget_set_margin_top)(message, 12);
    (api().gtk_widget_set_margin_bottom)(message, 12);
    (api().gtk_box_pack_start)(
        (api().gtk_dialog_get_content_area)(dialog),
        message,
        0,
        0,
        0,
    );
    (api().gtk_widget_show_all)(dialog);
    let response = (api().gtk_dialog_run)(dialog);
    if response != -1 {
        (api().gtk_widget_destroy)(dialog);
    }
    response == -3
}
pub(super) unsafe extern "C" fn show_archive(_: P, data: P) {
    let app = &*(data as *mut App);
    let manager = &*(app.archive_manager as *mut ArchiveManager);
    let row = selected_rows(app.list)
        .first()
        .copied()
        .unwrap_or(ptr::null_mut());
    let mut to_load = false;
    if !row.is_null() && !manager.busy {
        let index = (api().gtk_list_box_row_get_index)(row);
        if index >= 0 {
            if let Some(entry) = app.entries.get(index as usize) {
                if entry.is_file && supports_archive(&entry.path) {
                    (api().gtk_entry_set_text)(
                        manager.input,
                        c(&entry.path.to_string_lossy()).as_ptr(),
                    );
                    if let Some(parent) = entry.path.parent() {
                        (api().gtk_entry_set_text)(
                            manager.destination,
                            c(&parent.to_string_lossy()).as_ptr(),
                        );
                    }
                    to_load = true;
                }
            }
        }
    }
    let window = manager.window;
    let manager_ptr = app.archive_manager;
    (api().gtk_widget_show_all)(window);
    if to_load {
        archive_load(ptr::null_mut(), manager_ptr);
    }
}
pub(super) unsafe fn create_archive_manager(parent: P) -> P {
    let window = (api().gtk_window_new)(0);
    (api().gtk_window_set_title)(window, c("ZFM — Archive manager").as_ptr());
    (api().gtk_window_set_transient_for)(window, parent);
    (api().gtk_window_set_default_size)(window, 700, 520);
    let layout = (api().gtk_box_new)(1, 6);
    (api().gtk_widget_set_margin_start)(layout, 12);
    (api().gtk_widget_set_margin_end)(layout, 12);
    (api().gtk_widget_set_margin_top)(layout, 12);
    (api().gtk_widget_set_margin_bottom)(layout, 12);
    (api().gtk_container_add)(window, layout);
    let title = label(
        "TAR, ZIP, 7z and compressed TAR · no overwrite · selecting a folder does not select its contents",
    );
    (api().gtk_label_set_xalign)(title, 0.0);
    (api().gtk_box_pack_start)(layout, title, 0, 0, 0);
    let input_row = (api().gtk_box_new)(0, 4);
    (api().gtk_box_pack_start)(layout, input_row, 0, 0, 0);
    let input = (api().gtk_entry_new)();
    (api().gtk_entry_set_placeholder_text)(input, c("Archive path").as_ptr());
    (api().gtk_box_pack_start)(input_row, input, 1, 1, 0);
    let engine_row = (api().gtk_box_new)(0, 4);
    (api().gtk_box_pack_start)(layout, engine_row, 0, 0, 0);
    let dest_row = (api().gtk_box_new)(0, 4);
    (api().gtk_box_pack_start)(layout, dest_row, 0, 0, 0);
    let destination = (api().gtk_entry_new)();
    (api().gtk_entry_set_placeholder_text)(destination, c("Destination folder").as_ptr());
    (api().gtk_box_pack_start)(dest_row, destination, 1, 1, 0);
    let scroll = (api().gtk_scrolled_window_new)(ptr::null_mut(), ptr::null_mut());
    (api().gtk_box_pack_start)(layout, scroll, 1, 1, 0);
    let list = (api().gtk_list_box_new)();
    (api().gtk_list_box_set_selection_mode)(list, 3);
    (api().gtk_container_add)(scroll, list);
    let actions = (api().gtk_box_new)(0, 4);
    (api().gtk_box_pack_start)(layout, actions, 0, 0, 0);
    let progress = (api().gtk_progress_bar_new)();
    (api().gtk_box_pack_start)(layout, progress, 0, 0, 0);
    let status = label("Choose an archive and load it.");
    (api().gtk_label_set_xalign)(status, 0.0);
    (api().gtk_box_pack_start)(layout, status, 0, 0, 0);
    let manager = Box::into_raw(Box::new(ArchiveManager {
        window,
        input,
        destination,
        list,
        status,
        progress,
        entries: Vec::new(),
        engine_lines: None,
        format: None,
        render_index: 0,
        path: None,
        fingerprint: None,
        busy: false,
        job: 0,
        cancel: Arc::new(AtomicBool::new(false)),
    })) as P;
    button(input_row, "Browse…", archive_browse as P, manager);
    button(input_row, "Load", archive_load as P, manager);
    button(engine_row, "Engine list", archive_engine_list as P, manager);
    button(engine_row, "Engine test", archive_engine_test as P, manager);
    button(engine_row, "List with password…", archive_engine_list_password as P, manager);
    button(engine_row, "Test with password…", archive_engine_test_password as P, manager);
    button(engine_row, "Engine extract all…", archive_engine_extract as P, manager);
    button(engine_row, "Extract with password…", archive_engine_extract_password as P, manager);
    button(dest_row, "Browse…", archive_destination as P, manager);
    button(actions, "Test archive", archive_test as P, manager);
    button(actions, "Select all", archive_select_all as P, manager);
    button(actions, "Deselect", archive_select_none as P, manager);
    button(
        actions,
        "Create TAR/ZIP/7z from file…",
        archive_create as P,
        manager,
    );
    button(
        actions,
        "Extract selected…",
        archive_extract_selected as P,
        manager,
    );
    button(actions, "Extract all…", archive_extract_all as P, manager);
    button(actions, "Cancel", archive_cancel as P, manager);
    connect(window, "delete-event", converter_hide as P, manager);
    manager
}
