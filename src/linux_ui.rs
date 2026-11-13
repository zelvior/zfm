// Modified: compact reference-inspired file-manager layout and visuals.
//! Minimal GTK 3 adapter. GTK is loaded at runtime so the filesystem core can
//! be built and tested without GTK headers. All widget calls stay on GTK's
//! main thread; workers only call GLib's thread-safe g_idle_add.
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::path::PathBuf;
use std::ptr;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread;
use zfm::directory::{self, Entry, Listing};
use zfm::navigation::{Destination, Tabs};
use zfm::ui_theme::{self, ItemIcon, SortColumn};

#[path = "archive_ui.rs"]
mod archive_ui;
use archive_ui::{create_archive_manager, show_archive, supports_archive, ArchiveManager};
#[path = "file_clipboard_ui.rs"]
mod file_clipboard_ui;
use file_clipboard_ui::{copy_to_clipboard, cut_to_clipboard, paste_from_clipboard};

type P = *mut c_void;
type Idle = unsafe extern "C" fn(P) -> c_int;
#[link(name = "dl")]
extern "C" {
    fn dlopen(name: *const c_char, flags: c_int) -> P;
    fn dlsym(handle: P, name: *const c_char) -> P;
}

macro_rules! api {
    ($($name:ident: $ty:ty),+ $(,)?) => {
        struct Api { $($name: $ty),+ }
        impl Api {
            unsafe fn load() -> Result<Self, String> {
                let handle = dlopen(c("libgtk-3.so.0").as_ptr(), 2);
                if handle.is_null() { return Err("GTK 3 runtime not found. Install libgtk-3-0.".into()); }
                $(let symbol = dlsym(handle, c(stringify!($name)).as_ptr());
                  if symbol.is_null() { return Err(format!("GTK 3 is missing symbol {}", stringify!($name))); }
                  let $name: $ty = std::mem::transmute(symbol);)+
                Ok(Self { $($name),+ })
            }
        }
    }
}
api! {
    gtk_init_check: unsafe extern "C" fn(P, P) -> c_int,
    gtk_main: unsafe extern "C" fn(), gtk_main_quit: unsafe extern "C" fn(),
    gtk_window_new: unsafe extern "C" fn(c_int) -> P,
    gtk_window_set_title: unsafe extern "C" fn(P, *const c_char),
    gtk_window_get_focus: unsafe extern "C" fn(P) -> P,
    gtk_widget_is_ancestor: unsafe extern "C" fn(P, P) -> c_int,
    gtk_window_set_modal: unsafe extern "C" fn(P, c_int),
    gtk_window_set_destroy_with_parent: unsafe extern "C" fn(P, c_int),
    gtk_dialog_new: unsafe extern "C" fn() -> P,
    gtk_dialog_add_button: unsafe extern "C" fn(P, *const c_char, c_int) -> P,
    gtk_dialog_get_content_area: unsafe extern "C" fn(P) -> P,
    gtk_dialog_run: unsafe extern "C" fn(P) -> c_int,
    gtk_dialog_set_default_response: unsafe extern "C" fn(P, c_int),
    gtk_entry_set_activates_default: unsafe extern "C" fn(P, c_int),
    gtk_editable_select_region: unsafe extern "C" fn(P, c_int, c_int),
    gtk_window_set_default_size: unsafe extern "C" fn(P, c_int, c_int),
    gtk_window_set_transient_for: unsafe extern "C" fn(P, P),
    gtk_file_chooser_native_new: unsafe extern "C" fn(*const c_char, P, c_int, *const c_char, *const c_char) -> P,
    gtk_native_dialog_run: unsafe extern "C" fn(P) -> c_int,
    gtk_file_chooser_get_filename: unsafe extern "C" fn(P) -> *mut c_char,
    gtk_progress_bar_new: unsafe extern "C" fn() -> P,
    gtk_progress_bar_set_fraction: unsafe extern "C" fn(P, f64),
    gtk_widget_hide: unsafe extern "C" fn(P),
    gtk_list_box_get_selected_rows: unsafe extern "C" fn(P) -> P,
    gtk_list_box_get_row_at_y: unsafe extern "C" fn(P, c_int) -> P,
    gtk_list_box_row_is_selected: unsafe extern "C" fn(P) -> c_int,
    gtk_list_box_set_selection_mode: unsafe extern "C" fn(P, c_int),
    gtk_list_box_select_all: unsafe extern "C" fn(P),
    gtk_list_box_unselect_all: unsafe extern "C" fn(P),
    g_list_free: unsafe extern "C" fn(P),
    gtk_box_new: unsafe extern "C" fn(c_int, c_int) -> P,
    gtk_event_box_new: unsafe extern "C" fn() -> P,
    gtk_get_event_widget: unsafe extern "C" fn(P) -> P,
    gtk_box_pack_start: unsafe extern "C" fn(P, P, c_int, c_int, c_uint),
    gtk_button_new_with_label: unsafe extern "C" fn(*const c_char) -> P,
    gtk_button_set_label: unsafe extern "C" fn(P, *const c_char),
    gtk_toggle_button_set_active: unsafe extern "C" fn(P, c_int),
    gtk_check_button_new_with_label: unsafe extern "C" fn(*const c_char) -> P,
    gtk_toggle_button_get_active: unsafe extern "C" fn(P) -> c_int,
    gtk_entry_new: unsafe extern "C" fn() -> P,
    gtk_entry_set_width_chars: unsafe extern "C" fn(P, c_int),
    gtk_image_new_from_icon_name: unsafe extern "C" fn(*const c_char, c_int) -> P,
    gtk_image_set_from_icon_name: unsafe extern "C" fn(P, *const c_char, c_int),
    gtk_combo_box_text_new: unsafe extern "C" fn() -> P,
    gtk_combo_box_text_append_text: unsafe extern "C" fn(P, *const c_char),
    gtk_combo_box_set_active: unsafe extern "C" fn(P, c_int),
    gtk_combo_box_get_active: unsafe extern "C" fn(P) -> c_int,
    gtk_entry_set_text: unsafe extern "C" fn(P, *const c_char),
    gtk_entry_set_placeholder_text: unsafe extern "C" fn(P, *const c_char),
    gtk_entry_get_text: unsafe extern "C" fn(P) -> *const c_char,
    gtk_label_new: unsafe extern "C" fn(*const c_char) -> P,
    gtk_label_set_text: unsafe extern "C" fn(P, *const c_char),
    gtk_label_set_xalign: unsafe extern "C" fn(P, f32),
    gtk_label_set_ellipsize: unsafe extern "C" fn(P, c_int),
    gtk_scrolled_window_new: unsafe extern "C" fn(P, P) -> P,
    gtk_scrolled_window_set_policy: unsafe extern "C" fn(P, c_int, c_int),
    gtk_list_box_new: unsafe extern "C" fn() -> P,
    gtk_list_box_row_new: unsafe extern "C" fn() -> P,
    gtk_list_box_select_row: unsafe extern "C" fn(P, P),
    gtk_menu_new: unsafe extern "C" fn() -> P,
    gtk_menu_item_new_with_label: unsafe extern "C" fn(*const c_char) -> P,
    gtk_menu_shell_append: unsafe extern "C" fn(P, P),
    gtk_menu_item_set_submenu: unsafe extern "C" fn(P, P),
    gtk_separator_menu_item_new: unsafe extern "C" fn() -> P,
    gtk_menu_popup_at_widget: unsafe extern "C" fn(P, P, c_int, c_int, P),
    gtk_menu_popup_at_pointer: unsafe extern "C" fn(P, P),
    gtk_widget_add_events: unsafe extern "C" fn(P, c_int),
    gdk_event_get_button: unsafe extern "C" fn(P, *mut c_uint) -> c_int,
    gdk_event_get_coords: unsafe extern "C" fn(P, *mut f64, *mut f64) -> c_int,
    gdk_event_get_event_type: unsafe extern "C" fn(P) -> c_int,
    gtk_clipboard_set_text: unsafe extern "C" fn(P, *const c_char, c_int),
    gdk_atom_intern: unsafe extern "C" fn(*const c_char, c_int) -> P,
    gtk_clipboard_get: unsafe extern "C" fn(P) -> P,
    gtk_clipboard_set_with_data: unsafe extern "C" fn(P, P, c_uint, P, P, P) -> c_int,
    gtk_clipboard_request_contents: unsafe extern "C" fn(P, P, P, P),
    gtk_selection_data_get_data: unsafe extern "C" fn(P) -> *const u8,
    gtk_selection_data_get_length: unsafe extern "C" fn(P) -> c_int,
    gtk_selection_data_get_target: unsafe extern "C" fn(P) -> P,
    gtk_selection_data_set: unsafe extern "C" fn(P, P, c_int, *const u8, c_int),
    gtk_list_box_set_activate_on_single_click: unsafe extern "C" fn(P, c_int),
    gtk_list_box_insert: unsafe extern "C" fn(P, P, c_int),
    gtk_list_box_row_get_index: unsafe extern "C" fn(P) -> c_int,
    gtk_container_add: unsafe extern "C" fn(P, P),
    gtk_bin_get_child: unsafe extern "C" fn(P) -> P,
    gtk_container_foreach: unsafe extern "C" fn(P, unsafe extern "C" fn(P, P), P),
    gtk_widget_destroy: unsafe extern "C" fn(P),
    gtk_widget_show_all: unsafe extern "C" fn(P),
    gtk_widget_grab_focus: unsafe extern "C" fn(P),
    gtk_widget_set_can_focus: unsafe extern "C" fn(P, c_int),
    gtk_widget_set_tooltip_text: unsafe extern "C" fn(P, *const c_char),
    gdk_event_get_keyval: unsafe extern "C" fn(P, *mut c_uint) -> c_int,
    gdk_event_get_state: unsafe extern "C" fn(P, *mut c_uint) -> c_int,
    gtk_widget_set_size_request: unsafe extern "C" fn(P, c_int, c_int),
    gtk_widget_set_hexpand: unsafe extern "C" fn(P, c_int),
    gtk_widget_set_margin_start: unsafe extern "C" fn(P, c_int),
    gtk_widget_set_margin_end: unsafe extern "C" fn(P, c_int),
    gtk_widget_set_margin_top: unsafe extern "C" fn(P, c_int),
    gtk_widget_set_margin_bottom: unsafe extern "C" fn(P, c_int),
    gtk_css_provider_new: unsafe extern "C" fn() -> P,
    gtk_css_provider_load_from_data: unsafe extern "C" fn(P, *const c_char, isize, P) -> c_int,
    gtk_style_context_add_provider_for_screen: unsafe extern "C" fn(P, P, c_uint),
    gtk_widget_get_style_context: unsafe extern "C" fn(P) -> P,
    gtk_style_context_add_class: unsafe extern "C" fn(P, *const c_char),
    gtk_style_context_remove_class: unsafe extern "C" fn(P, *const c_char),
    gdk_screen_get_default: unsafe extern "C" fn() -> P,
    g_object_unref: unsafe extern "C" fn(P),
    g_signal_connect_data: unsafe extern "C" fn(P, *const c_char, P, P, P, c_int) -> usize,
    g_idle_add: unsafe extern "C" fn(Idle, P) -> c_uint,
    g_date_time_new_from_unix_local: unsafe extern "C" fn(i64) -> P,
    g_date_time_format: unsafe extern "C" fn(P, *const c_char) -> *mut c_char,
    g_date_time_unref: unsafe extern "C" fn(P),
    g_free: unsafe extern "C" fn(P),
}
// Initialized once before starting workers and intentionally retained to process exit.
static mut API: *const Api = ptr::null();
unsafe fn api() -> &'static Api {
    &*API
}
fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "�")).unwrap()
}

struct App {
    tabs: Tabs,
    tab_strip: P,
    generation: Arc<AtomicU64>,
    address: P,
    list: P,
    status: P,
    status_icon: P,
    hidden: P,
    entries: Vec<Entry>,
    render_index: usize,
    skipped: usize,
    error: Option<String>,
    notice: Option<String>,
    converter: P,
    archive_manager: P,
    window: P,
    operation: Option<Arc<AtomicBool>>,
    operation_id: u64,
    search_entry: P,
    case_toggle: P,
    search_query: String,
    search_case: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
    columns: [P; 4],
    sidebar_shortcuts: Vec<(PathBuf, P)>,
}
struct ResultMessage {
    app: usize,
    generation: u64,
    destination: Destination,
    result: std::io::Result<Option<Listing>>,
}
struct RenderMessage {
    app: usize,
    generation: u64,
}
struct OpenMessage {
    app: usize,
    generation: u64,
    error: String,
}
unsafe extern "C" fn open_error(data: P) -> c_int {
    let msg = Box::from_raw(data as *mut OpenMessage);
    let app = &*(msg.app as *mut App);
    if app.generation.load(Ordering::Relaxed) == msg.generation {
        set_status(app, &msg.error);
    }
    0
}

unsafe fn add_class(widget: P, class_name: &str) {
    let ctx = (api().gtk_widget_get_style_context)(widget);
    if !ctx.is_null() {
        (api().gtk_style_context_add_class)(ctx, c(class_name).as_ptr());
    }
}
unsafe fn label(text: &str) -> P {
    (api().gtk_label_new)(c(text).as_ptr())
}
unsafe fn set_status(app: &App, text: &str) {
    (api().gtk_label_set_text)(app.status, c(text).as_ptr());
}
unsafe fn connect(widget: P, signal: &str, callback: P, data: P) {
    (api().g_signal_connect_data)(
        widget,
        c(signal).as_ptr(),
        callback,
        data,
        ptr::null_mut(),
        0,
    );
}
unsafe fn button(parent: P, text: &str, callback: P, app: P) -> P {
    let widget = (api().gtk_button_new_with_label)(c(text).as_ptr());
    (api().gtk_box_pack_start)(parent, widget, 0, 0, 0);
    connect(widget, "clicked", callback, app);
    widget
}
unsafe extern "C" fn destroy_child(widget: P, _: P) {
    (api().gtk_widget_destroy)(widget);
}

unsafe fn navigate(app: &mut App, destination: Destination) {
    let path = match app.tabs.current().resolve(&destination) {
        Some(p) => p,
        None => return,
    };
    if !matches!(destination, Destination::Refresh) {
        app.notice = None;
    }
    let generation = app.generation.fetch_add(1, Ordering::Relaxed) + 1;
    let token = app.generation.clone();
    let hidden = (api().gtk_toggle_button_get_active)(app.hidden) != 0;
    let app_ptr = app as *mut App as usize;
    let query = app.search_query.clone();
    let case_sensitive = app.search_case;
    set_status(app, &format!("Loading {}…", path.display()));
    thread::spawn(move || {
        let result =
            directory::read_filtered(&path, hidden, &query, case_sensitive, &token, generation);
        if token.load(Ordering::Relaxed) != generation {
            return;
        }
        let msg = Box::new(ResultMessage {
            app: app_ptr,
            generation,
            destination,
            result,
        });
        unsafe {
            (api().g_idle_add)(receive, Box::into_raw(msg) as P);
        }
    });
}
unsafe extern "C" fn receive(data: P) -> c_int {
    let msg = Box::from_raw(data as *mut ResultMessage);
    let app = &mut *(msg.app as *mut App);
    if app.generation.load(Ordering::Relaxed) != msg.generation {
        return 0;
    }
    match msg.result {
        Ok(Some(mut listing)) => {
            app.tabs.current_mut().commit(msg.destination);
            for (path, widget) in &app.sidebar_shortcuts {
                let context = (api().gtk_widget_get_style_context)(*widget);
                if path == app.tabs.current().current() {
                    (api().gtk_style_context_add_class)(context, c("zfm-sidebar-active").as_ptr());
                } else {
                    (api().gtk_style_context_remove_class)(context, c("zfm-sidebar-active").as_ptr());
                }
            }
            rebuild_tabs(app);
            app.error = None;
            (api().gtk_entry_set_text)(
                app.address,
                c(&app.tabs.current().current().to_string_lossy()).as_ptr(),
            );
            ui_theme::sort_entries(&mut listing.entries, app.sort_column, app.sort_ascending);
            (api().gtk_container_foreach)(app.list, destroy_child, ptr::null_mut());
            app.entries = listing.entries;
            app.skipped = listing.skipped;
            app.render_index = 0;
            let render = Box::new(RenderMessage {
                app: msg.app,
                generation: msg.generation,
            });
            (api().g_idle_add)(render_batch, Box::into_raw(render) as P);
        }
        Ok(None) => (),
        Err(e) => {
            (api().gtk_entry_set_text)(
                app.address,
                c(&app.tabs.current().current().to_string_lossy()).as_ptr(),
            );
            app.error = Some(format!("Could not open folder: {e}"));
            set_status(app, app.error.as_ref().unwrap());
            // Resume the previous listing if navigation interrupted its rendering.
            if app.render_index < app.entries.len() {
                let render = Box::new(RenderMessage {
                    app: msg.app,
                    generation: msg.generation,
                });
                (api().g_idle_add)(render_batch, Box::into_raw(render) as P);
            }
        }
    }
    0
}
unsafe fn modified(entry: &Entry) -> String {
    let seconds = entry
        .modified
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    if let Some(seconds) = seconds {
        let date = (api().g_date_time_new_from_unix_local)(seconds);
        if !date.is_null() {
            let s = (api().g_date_time_format)(date, c("%m/%d/%Y %I:%M %p").as_ptr());
            let text = if s.is_null() {
                String::new()
            } else {
                let raw = CStr::from_ptr(s).to_string_lossy().into_owned();
                (api().g_free)(s as P);
                raw.replace(" 0", " ")
            };
            (api().g_date_time_unref)(date);
            return text;
        }
    }
    String::new()
}
fn linux_item_icon(entry: &Entry) -> &'static str {
    match ui_theme::classify_entry_icon(entry) {
        ItemIcon::Desktop | ItemIcon::Documents | ItemIcon::Downloads | ItemIcon::Music
        | ItemIcon::Videos | ItemIcon::Contacts | ItemIcon::Favorites | ItemIcon::Links
        | ItemIcon::Folder => "folder-symbolic",
        _ => "text-x-generic-symbolic",
    }
}
unsafe extern "C" fn render_batch(data: P) -> c_int {
    let msg = &*(data as *mut RenderMessage);
    let app = &mut *(msg.app as *mut App);
    if app.generation.load(Ordering::Relaxed) != msg.generation {
        drop(Box::from_raw(data as *mut RenderMessage));
        return 0;
    }
    let end = (app.render_index + 64).min(app.entries.len());
    for (offset, entry) in app.entries[app.render_index..end].iter().enumerate() {
        let row_idx = app.render_index + offset;
        let row = (api().gtk_box_new)(0, 6);
        (api().gtk_widget_set_margin_start)(row, 8);
        (api().gtk_widget_set_margin_end)(row, 8);
        (api().gtk_widget_set_margin_top)(row, 1);
        (api().gtk_widget_set_margin_bottom)(row, 1);
        let kind = (api().gtk_image_new_from_icon_name)(c(linux_item_icon(entry)).as_ptr(), 1);
        add_class(kind, "zfm-file-icon");
        let name = label(&entry.name);
        (api().gtk_label_set_xalign)(name, 0.0);
        (api().gtk_label_set_ellipsize)(name, 3);
        (api().gtk_widget_set_hexpand)(name, 1);
        let date = label(&modified(entry));
        (api().gtk_label_set_xalign)(date, 0.0);
        (api().gtk_widget_set_size_request)(date, 175, -1);
        let file_type = label(&ui_theme::entry_type_label(entry));
        (api().gtk_label_set_xalign)(file_type, 0.0);
        (api().gtk_widget_set_size_request)(file_type, 135, -1);
        let size = label(&ui_theme::format_human_size(entry));
        (api().gtk_label_set_xalign)(size, 1.0);
        (api().gtk_widget_set_size_request)(size, 86, -1);
        for child in [kind, name, date, file_type, size] {
            (api().gtk_box_pack_start)(row, child, 0, 0, 0);
        }
        let list_row = (api().gtk_list_box_row_new)();
        if row_idx % 2 == 1 {
            add_class(list_row, "zfm-row-odd");
        }
        (api().gtk_container_add)(list_row, row);
        (api().gtk_widget_add_events)(list_row, 1 << 8); // GDK_BUTTON_PRESS_MASK
        connect(
            list_row,
            "button-press-event",
            row_context as P,
            msg.app as P,
        );
        (api().gtk_list_box_insert)(app.list, list_row, -1);
        (api().gtk_widget_show_all)(list_row);
    }
    app.render_index = end;
    let skipped = if app.skipped > 0 {
        format!("  |  {} entries skipped", app.skipped)
    } else {
        String::new()
    };
    if let Some(error) = app.error.as_ref() {
        set_status(app, error);
    } else {
        let current = app.tabs.current().current().display().to_string();
        let rows = selected_rows(app.list);
        let selected_path = if rows.len() == 1 {
            let index = (api().gtk_list_box_row_get_index)(rows[0]);
            app.entries
                .get(index as usize)
                .map(|e| e.path.display().to_string())
                .unwrap_or_else(|| current.clone())
        } else {
            current.clone()
        };
        let notice = app
            .notice
            .as_ref()
            .map(|s| format!("  |  {s}"))
            .unwrap_or_default();
        set_status(
            app,
            &format!(
                "{} items   |   Selected: {}   |   {}{}{}   |   Path: {}",
                app.entries.len(),
                rows.len(),
                current,
                skipped,
                notice,
                selected_path
            ),
        );
    }
    if end < app.entries.len() {
        1
    } else {
        drop(Box::from_raw(data as *mut RenderMessage));
        0
    }
}
unsafe extern "C" fn quit(_: P, data: P) {
    let app = &mut *(data as *mut App);
    app.generation.fetch_add(1, Ordering::Relaxed);
    if let Some(cancel) = &app.operation {
        cancel.store(true, Ordering::Relaxed);
    }
    if !app.archive_manager.is_null() {
        (*(app.archive_manager as *mut ArchiveManager))
            .cancel
            .store(true, Ordering::Relaxed);
    }
    if !app.converter.is_null() {
        (*(app.converter as *mut Converter))
            .cancel
            .store(true, Ordering::Relaxed);
    }
    (api().gtk_main_quit)();
}
unsafe extern "C" fn back(_: P, p: P) {
    navigate(&mut *(p as *mut App), Destination::Back);
}
unsafe extern "C" fn forward(_: P, p: P) {
    navigate(&mut *(p as *mut App), Destination::Forward);
}
unsafe extern "C" fn refresh(_: P, p: P) {
    navigate(&mut *(p as *mut App), Destination::Refresh);
}
unsafe extern "C" fn up(_: P, p: P) {
    let app = &mut *(p as *mut App);
    if let Some(parent) = app
        .tabs
        .current()
        .current()
        .parent()
        .map(|p| p.to_path_buf())
    {
        navigate(app, Destination::Visit(parent));
    }
}
unsafe extern "C" fn go_root(_: P, p: P) {
    navigate(
        &mut *(p as *mut App),
        Destination::Visit(PathBuf::from("/")),
    );
}
unsafe extern "C" fn go_address(_: P, p: P) {
    let entry = (*(p as *mut App)).address;
    address(entry, p);
}
unsafe fn apply_linux_sort(p: P, col: SortColumn) {
    let app = &mut *(p as *mut App);
    if app.sort_column == col {
        app.sort_ascending = !app.sort_ascending;
    } else {
        app.sort_column = col;
        app.sort_ascending = matches!(col, SortColumn::Name | SortColumn::Kind);
    }
    for (widget, column, label) in [
        (app.columns[0], SortColumn::Name, "Name"),
        (app.columns[1], SortColumn::Modified, "Date"),
        (app.columns[2], SortColumn::Kind, "Type"),
        (app.columns[3], SortColumn::Size, "Size"),
    ] {
        let suffix = if app.sort_column == column {
            if app.sort_ascending {
                " ▲"
            } else {
                " ▼"
            }
        } else {
            ""
        };
        (api().gtk_button_set_label)(widget, c(&format!("{label}{suffix}")).as_ptr());
    }
    navigate(app, Destination::Refresh);
}
unsafe extern "C" fn sort_by_name(_: P, p: P) {
    apply_linux_sort(p, SortColumn::Name);
}
unsafe extern "C" fn sort_by_modified(_: P, p: P) {
    apply_linux_sort(p, SortColumn::Modified);
}
unsafe extern "C" fn sort_by_type(_: P, p: P) {
    apply_linux_sort(p, SortColumn::Kind);
}
unsafe extern "C" fn sort_by_size(_: P, p: P) {
    apply_linux_sort(p, SortColumn::Size);
}
unsafe extern "C" fn address(entry: P, p: P) {
    let text = CStr::from_ptr((api().gtk_entry_get_text)(entry))
        .to_string_lossy()
        .into_owned();
    let app = &mut *(p as *mut App);
    let path = PathBuf::from(text);
    let path = if path.is_absolute() {
        path
    } else {
        app.tabs.current().current().join(path)
    };
    navigate(app, Destination::Visit(path));
}
unsafe extern "C" fn key_press(_: P, event: P, p: P) -> c_int {
    let mut key = 0;
    let mut state = 0;
    if (api().gdk_event_get_keyval)(event, &mut key) == 0 {
        return 0;
    }
    (api().gdk_event_get_state)(event, &mut state);
    // Do not retain an App reference while callbacks run nested dialogs.
    let modifiers = state & (1 | 4 | 8 | 64);
    match (key, modifiers) {
        (0xffc2, 0) | (0x72, 4) | (0x52, 4) => refresh(ptr::null_mut(), p),
        (0xff51, 8) => back(ptr::null_mut(), p),
        (0xff53, 8) => forward(ptr::null_mut(), p),
        (0xff52, 8) => up(ptr::null_mut(), p),
        (0xff08, 0) if list_has_focus(p) => up(ptr::null_mut(), p),
        (0x6c, 4) | (0x4c, 4) | (0x64, 8) | (0x44, 8) => focus_entry((*(p as *mut App)).address),
        (0x66, 4) | (0x46, 4) => focus_entry((*(p as *mut App)).search_entry),
        (0x63, 5) | (0x43, 5) => copy_path(ptr::null_mut(), p),
        (0x63, 4) | (0x43, 4) if list_has_focus(p) => copy_to_clipboard(ptr::null_mut(), p),
        (0x78, 4) | (0x58, 4) if list_has_focus(p) => cut_to_clipboard(ptr::null_mut(), p),
        (0x76, 4) | (0x56, 4) if list_has_focus(p) => paste_from_clipboard(ptr::null_mut(), p),
        (0x74, 4) | (0x54, 4) => new_tab(ptr::null_mut(), p),
        (0x77, 4) | (0x57, 4) => close_tab(ptr::null_mut(), p),
        (0xff09, 4) | (0xff56, 4) => cycle_tab(p, false),
        (0xfe20, 5) | (0xff09, 5) | (0xff55, 4) => cycle_tab(p, true),
        (0x31..=0x39, 4) | (0x31..=0x39, 8) => {
            if (*(p as *mut App)).tabs.select((key - 0x31) as usize) {
                activate_tab(p);
            }
        }
        (0x6e, 5) | (0x4e, 5) => new_folder(ptr::null_mut(), p),
        (0x6e, 4) | (0x4e, 4) => new_file(ptr::null_mut(), p),
        (0x64, 4) | (0x44, 4) if list_has_focus(p) => duplicate(ptr::null_mut(), p),
        (0x68, 4) | (0x48, 4) => {
            let hidden = (*(p as *mut App)).hidden;
            (api().gtk_toggle_button_set_active)(
                hidden,
                ((api().gtk_toggle_button_get_active)(hidden) == 0) as c_int,
            );
        }
        (0xffbf, 0) if list_has_focus(p) => rename(ptr::null_mut(), p),
        (0xffff, 0) if list_has_focus(p) => move_to_trash(ptr::null_mut(), p),
        (0xffff, 1) if list_has_focus(p) => delete_permanently(ptr::null_mut(), p),
        (0xff0d, 8) if list_has_focus(p) => properties(ptr::null_mut(), p),
        (0xffc7, 1) | (0xff67, 0) if list_has_focus(p) => {
            let list = (*(p as *mut App)).list;
            let row = selected_rows(list).first().copied();
            if let Some(row) = row {
                show_row_menu(row, ptr::null_mut(), p);
            } else {
                show_background_menu(list, ptr::null_mut(), p);
            }
        }
        (0xff1b, 0) => {
            let list = (*(p as *mut App)).list;
            (api().gtk_list_box_unselect_all)(list);
            (api().gtk_widget_grab_focus)(list);
        }
        _ => return 0,
    }
    1
}
#[repr(C)]
struct GList {
    data: P,
    next: *mut GList,
    prev: *mut GList,
}
unsafe fn selected_rows(list: P) -> Vec<P> {
    let head = (api().gtk_list_box_get_selected_rows)(list);
    let mut node = head as *mut GList;
    let mut rows = Vec::new();
    while !node.is_null() {
        rows.push((*node).data);
        node = (*node).next;
    }
    (api().g_list_free)(head);
    rows
}
unsafe extern "C" fn selection_changed(_: P, data: P) {
    let app = &*(data as *mut App);
    let rows = selected_rows(app.list);
    if rows.len() == 1 {
        let i = (api().gtk_list_box_row_get_index)(rows[0]);
        if let Some(entry) = app.entries.get(i as usize) {
            (api().gtk_image_set_from_icon_name)(app.status_icon, c(linux_item_icon(entry)).as_ptr(), 6);
            set_status(app, &format!("{}   Date modified: {}\n{}   ·   {} items   ·   {}",
                entry.name, modified(entry), ui_theme::entry_type_label(entry), app.entries.len(), entry.path.display()));
            return;
        }
    }
    (api().gtk_image_set_from_icon_name)(app.status_icon, c("folder-symbolic").as_ptr(), 6);
    set_status(app, &format!("{}\n{} items   ·   {} selected",
        app.tabs.current().current().display(), app.entries.len(), rows.len()));
}
unsafe fn focus_entry(entry: P) {
    (api().gtk_widget_grab_focus)(entry);
    (api().gtk_editable_select_region)(entry, 0, -1);
}
unsafe fn cycle_tab(data: P, previous: bool) {
    let app = &mut *(data as *mut App);
    if if previous {
        app.tabs.prev_tab()
    } else {
        app.tabs.next_tab()
    } {
        activate_tab(data);
    }
}
unsafe extern "C" fn open_selected(_: P, data: P) {
    let list = (*(data as *mut App)).list;
    let rows = selected_rows(list);
    if rows.len() == 1 {
        row_activated(list, rows[0], data);
    } else {
        set_status(&*(data as *mut App), "Select one item to open.");
    }
}
unsafe extern "C" fn copy_path(_: P, data: P) {
    let app = &*(data as *mut App);
    let rows = selected_rows(app.list);
    let path = if rows.len() == 1 {
        let i = (api().gtk_list_box_row_get_index)(rows[0]);
        app.entries
            .get(i as usize)
            .map(|e| e.path.as_path())
            .unwrap_or(app.tabs.current().current())
    } else {
        app.tabs.current().current()
    };
    let clip = (api().gtk_clipboard_get)((api().gdk_atom_intern)(c("CLIPBOARD").as_ptr(), 0));
    (api().gtk_clipboard_set_text)(clip, c(&path.to_string_lossy()).as_ptr(), -1);
    set_status(app, "Path copied to clipboard.");
}
unsafe extern "C" fn properties(_: P, data: P) {
    let path = match selected_path(data) {
        Some(p) => p,
        None => return,
    };
    let details = match std::fs::symlink_metadata(&path) {
        Ok(m) => format!(
            "{}\n\nType: {}\nSize: {} bytes",
            path.display(),
            if m.file_type().is_symlink() {
                "Symbolic link"
            } else if m.is_dir() {
                "Folder"
            } else {
                "File"
            },
            m.len()
        ),
        Err(e) => format!("{}\n{}", path.display(), e),
    };
    let dialog = (api().gtk_dialog_new)();
    (api().gtk_window_set_title)(dialog, c("Properties").as_ptr());
    (api().gtk_window_set_transient_for)(dialog, (*(data as *mut App)).window);
    (api().gtk_window_set_modal)(dialog, 1);
    (api().gtk_window_set_destroy_with_parent)(dialog, 1);
    (api().gtk_dialog_add_button)(dialog, c("Close").as_ptr(), -7);
    (api().gtk_box_pack_start)(
        (api().gtk_dialog_get_content_area)(dialog),
        label(&details),
        0,
        0,
        12,
    );
    (api().gtk_widget_show_all)(dialog);
    let response = (api().gtk_dialog_run)(dialog);
    if response != -1 {
        (api().gtk_widget_destroy)(dialog);
    }
}
unsafe extern "C" fn list_button(list: P, event: P, data: P) -> c_int {
    let mut button = 0;
    if (api().gdk_event_get_button)(event, &mut button) == 0 {
        return 0;
    }
    if button == 8 {
        back(ptr::null_mut(), data);
        return 1;
    }
    if button == 9 {
        forward(ptr::null_mut(), data);
        return 1;
    }
    let (mut x, mut y) = (0.0, 0.0);
    if (api().gdk_event_get_coords)(event, &mut x, &mut y) == 0 {
        return 0;
    }
    if !(api().gtk_list_box_get_row_at_y)(list, y as c_int).is_null() {
        return 0;
    }
    if button == 1 || button == 3 {
        (api().gtk_list_box_unselect_all)(list);
        (api().gtk_widget_grab_focus)(list);
        if button == 3 {
            show_background_menu(list, event, data);
        }
        return 1;
    }
    0
}
unsafe fn show_background_menu(list: P, event: P, data: P) {
    let menu = (api().gtk_menu_new)();
    menu_action(menu, "New file…", new_file as P, data);
    menu_action(menu, "New folder…", new_folder as P, data);
    menu_action(menu, "Paste", paste_from_clipboard as P, data);
    menu_action(menu, "Refresh", refresh as P, data);
    menu_action(
        menu,
        "Show / hide hidden files",
        toggle_hidden_menu as P,
        data,
    );
    let item = (api().gtk_menu_item_new_with_label)(c("Sort by").as_ptr());
    let submenu = (api().gtk_menu_new)();
    for (name, cb) in [
        ("Name", sort_by_name as P),
        ("Date Modified", sort_by_modified as P),
        ("Type", sort_by_type as P),
        ("Size", sort_by_size as P),
    ] {
        menu_action(submenu, name, cb, data);
    }
    (api().gtk_menu_item_set_submenu)(item, submenu);
    (api().gtk_menu_shell_append)(menu, item);
    connect(menu, "selection-done", destroy_child as P, ptr::null_mut());
    (api().gtk_widget_show_all)(menu);
    if event.is_null() {
        (api().gtk_menu_popup_at_widget)(menu, list, 6, 2, ptr::null_mut());
    } else {
        (api().gtk_menu_popup_at_pointer)(menu, event);
    }
}
unsafe extern "C" fn toggle_hidden_menu(_: P, data: P) {
    let hidden = (*(data as *mut App)).hidden;
    (api().gtk_toggle_button_set_active)(
        hidden,
        ((api().gtk_toggle_button_get_active)(hidden) == 0) as c_int,
    );
}
unsafe fn list_has_focus(data: P) -> bool {
    let window = (*(data as *mut App)).window;
    let list = (*(data as *mut App)).list;
    let focus = (api().gtk_window_get_focus)(window);
    !focus.is_null() && (focus == list || (api().gtk_widget_is_ancestor)(focus, list) != 0)
}
unsafe extern "C" fn row_activated(_: P, row: P, p: P) {
    let app = &mut *(p as *mut App);
    let index = (api().gtk_list_box_row_get_index)(row);
    if index < 0 {
        return;
    }
    if let Some(entry) = app.entries.get(index as usize) {
        let path = entry.path.clone();
        if entry.is_dir {
            navigate(app, Destination::Visit(path));
        } else {
            match std::process::Command::new("xdg-open").arg(&path).spawn() {
                Ok(mut child) => {
                    let app_ptr = app as *mut App as usize;
                    let generation = app.generation.clone();
                    let expected = generation.load(Ordering::Relaxed);
                    thread::spawn(move || {
                        let error = match child.wait() {
                            Ok(status) if status.success() => return,
                            Ok(status) => {
                                format!("Could not open file (xdg-open exited with {status})")
                            }
                            Err(e) => format!("Could not open file: {e}"),
                        };
                        if generation.load(Ordering::Relaxed) == expected {
                            let msg = Box::new(OpenMessage {
                                app: app_ptr,
                                generation: expected,
                                error,
                            });
                            unsafe {
                                (api().g_idle_add)(open_error, Box::into_raw(msg) as P);
                            }
                        }
                    });
                }
                Err(e) => set_status(app, &format!("Could not open file: {e}")),
            }
        }
    }
}
unsafe extern "C" fn row_context(row: P, event: P, data: P) -> c_int {
    let mut mouse_button = 0;
    if (api().gdk_event_get_button)(event, &mut mouse_button) == 0 {
        return 0;
    }
    let mut state = 0;
    (api().gdk_event_get_state)(event, &mut state);
    if mouse_button == 1 && state & 8 != 0 && (api().gdk_event_get_event_type)(event) == 5 {
        let list = (*(data as *mut App)).list;
        (api().gtk_list_box_unselect_all)(list);
        (api().gtk_list_box_select_row)(list, row);
        properties(ptr::null_mut(), data);
        return 1;
    }
    if mouse_button == 2
        || (mouse_button == 1 && state & 4 != 0 && (api().gdk_event_get_event_type)(event) == 5)
    {
        let app = &mut *(data as *mut App);
        let index = (api().gtk_list_box_row_get_index)(row);
        if let Some(path) = app
            .entries
            .get(index as usize)
            .filter(|e| e.is_dir)
            .map(|e| e.path.clone())
        {
            app.tabs.add(path);
            activate_tab(data);
            return 1;
        }
    }
    if mouse_button != 3 {
        return 0;
    }
    show_row_menu(row, event, data);
    1
}
unsafe fn show_row_menu(row: P, event: P, data: P) {
    let app = &*(data as *mut App);
    let index = (api().gtk_list_box_row_get_index)(row);
    if index < 0 {
        return;
    }
    let entry = match app.entries.get(index as usize) {
        Some(entry) => entry,
        None => return,
    };
    let is_directory = entry.is_dir;
    let can_duplicate = entry.is_file;
    let can_copy = entry.is_file || entry.is_dir;
    let extension = entry
        .path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let can_convert = can_duplicate
        && ["csv", "json", "txt", "bmp", "ico"].iter().any(|ext| extension.eq_ignore_ascii_case(ext));
    let is_json = extension.eq_ignore_ascii_case("json");
    let can_archive = can_duplicate && supports_archive(&entry.path);
    let list = app.list;
    if (api().gtk_list_box_row_is_selected)(row) == 0 {
        (api().gtk_list_box_unselect_all)(list);
        (api().gtk_list_box_select_row)(list, row);
    }
    (api().gtk_widget_grab_focus)(list);
    let menu = (api().gtk_menu_new)();
    menu_action(menu, "Open", open_selected as P, data);
    if is_directory {
        menu_action(menu, "Open folder in new tab", open_folder_tab as P, data);
    }
    if can_convert {
        menu_action(
            menu,
            if is_json {
                "Convert JSON to CSV…"
            } else if extension.eq_ignore_ascii_case("txt") {
                "Convert text to HTML…"
            } else if extension.eq_ignore_ascii_case("bmp") {
                "Convert BMP to ICO…"
            } else if extension.eq_ignore_ascii_case("ico") {
                "Convert ICO to BMP…"
            } else {
                "Convert CSV to JSON…"
            },
            show_converter as P,
            data,
        );
    }
    if can_archive {
        menu_action(menu, "Open archive…", show_archive as P, data);
    }
    if can_copy {
        menu_action(menu, "Copy", copy_to_clipboard as P, data);
    }
    menu_action(menu, "Cut", cut_to_clipboard as P, data);
    menu_action(menu, "Copy path", copy_path as P, data);
    menu_action(
        menu,
        "Paste into current folder",
        paste_from_clipboard as P,
        data,
    );
    menu_action(menu, "Rename…", rename as P, data);
    menu_action(menu, "Move to…", move_to as P, data);
    if can_copy {
        menu_action(menu, "Copy to…", copy_to as P, data);
        menu_action(menu, "Duplicate…", duplicate as P, data);
    }
    menu_action(menu, "Move to trash", move_to_trash as P, data);
    menu_action(menu, "Delete permanently…", delete_permanently as P, data);
    (api().gtk_menu_shell_append)(menu, (api().gtk_separator_menu_item_new)());
    menu_action(menu, "Properties", properties as P, data);
    connect(menu, "selection-done", destroy_child as P, ptr::null_mut());
    (api().gtk_widget_show_all)(menu);
    if event.is_null() {
        (api().gtk_menu_popup_at_widget)(menu, row, 6, 2, ptr::null_mut());
    } else {
        (api().gtk_menu_popup_at_pointer)(menu, event);
    }
}
unsafe fn menu_action(menu: P, text: &str, callback: P, data: P) {
    let item = (api().gtk_menu_item_new_with_label)(c(text).as_ptr());
    (api().gtk_menu_shell_append)(menu, item);
    connect(item, "activate", callback, data);
}
unsafe extern "C" fn organize_menu(widget: P, data: P) {
    let menu = (api().gtk_menu_new)();
    for (title, callback) in [
        ("New file…", new_file as P), ("New folder…", new_folder as P),
        ("Rename…", rename as P), ("Move to trash", move_to_trash as P),
        ("Delete permanently…", delete_permanently as P),
        ("Paste", paste_from_clipboard as P), ("Copy to…", copy_to as P),
        ("Move to…", move_to as P), ("Duplicate…", duplicate as P),
        ("Cancel copy", cancel_operation as P),
    ] {
        menu_action(menu, title, callback, data);
    }
    connect(menu, "selection-done", destroy_child as P, ptr::null_mut());
    (api().gtk_widget_show_all)(menu);
    (api().gtk_menu_popup_at_widget)(menu, widget, 6, 2, ptr::null_mut());
}

struct Shortcut {
    app: usize,
    path: PathBuf,
}
unsafe extern "C" fn shortcut(_: P, p: P) {
    let shortcut = &*(p as *const Shortcut);
    let app = &mut *(shortcut.app as *mut App);
    navigate(app, Destination::Visit(shortcut.path.clone()));
}
unsafe extern "C" fn sidebar_new_tab(_: P, p: P) {
    let shortcut = &*(p as *const Shortcut);
    let app_ptr = shortcut.app as P;
    let path = shortcut.path.clone();
    (*(app_ptr as *mut App)).tabs.add(path);
    activate_tab(app_ptr);
}
unsafe extern "C" fn sidebar_button(_: P, event: P, data: P) -> c_int {
    let mut button = 0;
    if (api().gdk_event_get_button)(event, &mut button) == 0 {
        return 0;
    }
    let mut state = 0;
    (api().gdk_event_get_state)(event, &mut state);
    if button == 2 || (button == 1 && state & 4 != 0) {
        sidebar_new_tab(ptr::null_mut(), data);
        return 1;
    }
    if button != 3 {
        return 0;
    }
    let menu = (api().gtk_menu_new)();
    menu_action(menu, "Open", shortcut as P, data);
    menu_action(menu, "Open in new tab", sidebar_new_tab as P, data);
    connect(menu, "selection-done", destroy_child as P, ptr::null_mut());
    (api().gtk_widget_show_all)(menu);
    (api().gtk_menu_popup_at_pointer)(menu, event);
    1
}

pub fn run() -> Result<(), String> {
    unsafe {
        API = Box::into_raw(Box::new(Api::load()?));
        if (api().gtk_init_check)(ptr::null_mut(), ptr::null_mut()) == 0 {
            return Err("could not connect to a graphical display".into());
        }
        let css = (api().gtk_css_provider_new)();
        // Keep the compact desktop theme separate from widget construction.
        let style = c(include_str!("linux-theme.css"));
        (api().gtk_css_provider_load_from_data)(css, style.as_ptr(), -1, ptr::null_mut());
        (api().gtk_style_context_add_provider_for_screen)(
            (api().gdk_screen_get_default)(),
            css,
            600,
        );
        (api().g_object_unref)(css);

        let window = (api().gtk_window_new)(0);
        (api().gtk_window_set_title)(window, c("ZFM — Zelvior File Manager").as_ptr());
        (api().gtk_window_set_default_size)(window, 1040, 640);
        let outer = (api().gtk_box_new)(1, 0);
        (api().gtk_container_add)(window, outer);

        // Navigation/address/search and a compact command/tab strip.
        let top = (api().gtk_box_new)(0, 4);
        add_class(top, "zfm-nav");
        (api().gtk_box_pack_start)(outer, top, 0, 0, 0);
        let tabs_row = (api().gtk_box_new)(0, 4);
        add_class(tabs_row, "zfm-tabs");
        (api().gtk_box_pack_start)(outer, tabs_row, 0, 0, 0);
        let tab_strip = (api().gtk_box_new)(0, 4);

        // Split workspace: left sidebar + right table pane
        let content = (api().gtk_box_new)(0, 0);
        (api().gtk_box_pack_start)(outer, content, 1, 1, 0);
        let sidebar_scroll = (api().gtk_scrolled_window_new)(ptr::null_mut(), ptr::null_mut());
        (api().gtk_widget_set_size_request)(sidebar_scroll, 150, -1);
        (api().gtk_box_pack_start)(content, sidebar_scroll, 0, 0, 0);
        let sidebar = (api().gtk_box_new)(1, 0);
        add_class(sidebar, "zfm-sidebar");
        (api().gtk_container_add)(sidebar_scroll, sidebar);

        let right_pane = (api().gtk_box_new)(1, 0);
        (api().gtk_box_pack_start)(content, right_pane, 1, 1, 0);
        let col_bar = (api().gtk_box_new)(0, 8);
        add_class(col_bar, "zfm-col-bar");
        (api().gtk_box_pack_start)(right_pane, col_bar, 0, 0, 0);

        let scroll = (api().gtk_scrolled_window_new)(ptr::null_mut(), ptr::null_mut());
        (api().gtk_box_pack_start)(right_pane, scroll, 1, 1, 0);
        let list = (api().gtk_list_box_new)();
        (api().gtk_widget_set_can_focus)(list, 1);
        (api().gtk_list_box_set_activate_on_single_click)(list, 0);
        (api().gtk_list_box_set_selection_mode)(list, 3); // GTK_SELECTION_MULTIPLE
        (api().gtk_container_add)(scroll, list);

        let status_box = (api().gtk_box_new)(0, 0);
        add_class(status_box, "zfm-status");
        let status_icon = (api().gtk_image_new_from_icon_name)(c("folder-symbolic").as_ptr(), 6);
        add_class(status_icon, "zfm-file-icon");
        (api().gtk_box_pack_start)(status_box, status_icon, 0, 0, 8);
        let status = label("Ready");
        (api().gtk_label_set_xalign)(status, 0.0);
        (api().gtk_label_set_ellipsize)(status, 3);
        (api().gtk_box_pack_start)(status_box, status, 1, 1, 0);
        (api().gtk_box_pack_start)(outer, status_box, 0, 0, 0);

        let initial = std::env::args_os()
            .nth(1)
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let initial = if initial.is_absolute() {
            initial
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .join(initial)
        };
        let app = Box::into_raw(Box::new(App {
            tabs: Tabs::new(initial),
            tab_strip: ptr::null_mut(),
            generation: Arc::new(AtomicU64::new(0)),
            address: ptr::null_mut(),
            list,
            status,
            status_icon,
            hidden: ptr::null_mut(),
            entries: Vec::new(),
            render_index: 0,
            skipped: 0,
            error: None,
            notice: None,
            converter: ptr::null_mut(),
            archive_manager: ptr::null_mut(),
            window,
            operation: None,
            operation_id: 0,
            search_entry: ptr::null_mut(),
            case_toggle: ptr::null_mut(),
            search_query: String::new(),
            search_case: false,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            columns: [ptr::null_mut(); 4],
            sidebar_shortcuts: Vec::new(),
        }));
        let p = app as P;

        // Frequently used actions are visible; the rest remain in Organize.
        button(tabs_row, "Organize ▾", organize_menu as P, p);
        button(tabs_row, "New folder", new_folder as P, p);
        button(tabs_row, "New file", new_file as P, p);
        let new_tab_btn = button(tabs_row, "+ New tab", new_tab as P, p);
        add_class(new_tab_btn, "zfm-tab-btn");
        let close_tab_btn = button(tabs_row, "× Close tab", close_tab as P, p);
        add_class(close_tab_btn, "zfm-tab-btn");
        let case_toggle = (api().gtk_check_button_new_with_label)(c("Case").as_ptr());
        (*app).case_toggle = case_toggle;
        (api().gtk_box_pack_start)(tabs_row, case_toggle, 0, 0, 0);
        let hidden = (api().gtk_check_button_new_with_label)(c("Hidden").as_ptr());
        (*app).hidden = hidden;
        (api().gtk_box_pack_start)(tabs_row, hidden, 0, 0, 0);
        connect(hidden, "toggled", refresh as P, p);
        (*app).tab_strip = tab_strip;
        let tab_background = (api().gtk_event_box_new)();
        (api().gtk_container_add)(tab_background, tab_strip);
        // Many tabs must not force the main window wider than the screen.
        let tab_scroll = (api().gtk_scrolled_window_new)(ptr::null_mut(), ptr::null_mut());
        (api().gtk_scrolled_window_set_policy)(tab_scroll, 1, 2); // automatic / never
        (api().gtk_widget_set_size_request)(tab_scroll, 72, -1);
        (api().gtk_container_add)(tab_scroll, tab_background);
        (api().gtk_box_pack_start)(tabs_row, tab_scroll, 1, 1, 0);
        (api().gtk_widget_add_events)(tab_background, 1 << 8);
        connect(tab_background, "button-press-event", tab_background_button as P, p);
        rebuild_tabs(&*app);

        // Navigation and search share the top line, as in the reference.
        for (caption, callback, tooltip) in [
            ("←", back as P, "Back"), ("→", forward as P, "Forward"),
            ("↑", up as P, "Parent folder"), ("⌂", go_root as P, "Root"),
            ("↻", refresh as P, "Refresh"),
        ] {
            let nav = button(top, caption, callback, p);
            (api().gtk_widget_set_tooltip_text)(nav, c(tooltip).as_ptr());
        }
        let entry = (api().gtk_entry_new)();
        (*app).address = entry;
        (api().gtk_entry_set_width_chars)(entry, 12);
        (api().gtk_box_pack_start)(top, entry, 1, 1, 0);
        connect(entry, "activate", address as P, p);
        button(top, "Go", go_address as P, p);
        let search_entry = (api().gtk_entry_new)();
        (*app).search_entry = search_entry;
        (api().gtk_entry_set_width_chars)(search_entry, 12);
        (api().gtk_entry_set_placeholder_text)(search_entry, c("Search this folder").as_ptr());
        (api().gtk_box_pack_start)(top, search_entry, 0, 0, 0);
        connect(search_entry, "activate", search_apply as P, p);
        button(top, "Search", search_apply as P, p);
        button(top, "Clear", search_clear as P, p);

        // Sortable column header bar
        let col_name = button(col_bar, "Name ▲", sort_by_name as P, p);
        (api().gtk_widget_set_hexpand)(col_name, 1);
        let col_mod = button(col_bar, "Date", sort_by_modified as P, p);
        (api().gtk_widget_set_size_request)(col_mod, 175, -1);
        let col_type = button(col_bar, "Type", sort_by_type as P, p);
        (api().gtk_widget_set_size_request)(col_type, 135, -1);
        let col_size = button(col_bar, "Size", sort_by_size as P, p);
        (api().gtk_widget_set_size_request)(col_size, 86, -1);

        for col in [col_name, col_mod, col_type, col_size] {
            let child = (api().gtk_bin_get_child)(col);
            if !child.is_null() { (api().gtk_label_set_xalign)(child, 0.0); }
        }
        (*app).columns = [col_name, col_mod, col_type, col_size];
        connect(list, "row-activated", row_activated as P, p);
        connect(list, "selected-rows-changed", selection_changed as P, p);
        (api().gtk_widget_add_events)(list, 1 << 8);
        connect(list, "button-press-event", list_button as P, p);
        connect(window, "destroy", quit as P, p);
        connect(window, "key-press-event", key_press as P, p);
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            // Headings are presentation only; every shortcut remains a real path.
            for (heading, shortcuts) in [
                ("★  Favorites", vec![
                    ("  ▣  Desktop", home.join("Desktop")),
                    ("  ↓  Downloads", home.join("Downloads")),
                ]),
                ("▣  Libraries", vec![
                    ("  ▤  Documents", home.join("Documents")),
                    ("  ♫  Music", home.join("Music")),
                    ("  ▧  Pictures", home.join("Pictures")),
                    ("  ▷  Videos", home.join("Videos")),
                    ("  ▰  Home", home.clone()),
                ]),
                ("▣  Computer", vec![("  ▰  Filesystem", PathBuf::from("/"))]),
            ] {
                let group = label(heading);
                add_class(group, "zfm-sidebar-heading");
                (api().gtk_label_set_xalign)(group, 0.0);
                (api().gtk_box_pack_start)(sidebar, group, 0, 0, 0);
                for (name, path) in shortcuts {
                    let data = Box::into_raw(Box::new(Shortcut {
                        app: app as usize, path: path.clone(),
                    })) as P;
                    let b = button(sidebar, name, shortcut as P, data);
                    (*app).sidebar_shortcuts.push((path, b));
                    (api().gtk_widget_add_events)(b, 1 << 8);
                    connect(b, "button-press-event", sidebar_button as P, data);
                    let child_lbl = (api().gtk_bin_get_child)(b);
                    if !child_lbl.is_null() { (api().gtk_label_set_xalign)(child_lbl, 0.0); }
                }
            }
        }
        for (caption, cb) in [
            ("⚙  Converter", show_converter as P),
            ("▣  Archives", show_archive as P),
        ] {
            let b = button(sidebar, caption, cb, p);
            let child_lbl = (api().gtk_bin_get_child)(b);
            if !child_lbl.is_null() { (api().gtk_label_set_xalign)(child_lbl, 0.0); }
        }
        (*app).converter = create_converter(window);
        (*app).archive_manager = create_archive_manager(window);
        (api().gtk_widget_show_all)(window);
        navigate(&mut *app, Destination::Refresh);
        (api().gtk_main)();
        // App and shortcut data remain valid for pending worker callbacks until process exit.
    }
    Ok(())
}

// The converter window and state live until process exit. Background threads
// communicate with it only through GLib's main-loop idle queue.
struct Converter {
    window: P,
    mode: P,
    input: P,
    output: P,
    status: P,
    progress: P,
    running: bool,
    job: u64,
    cancel: Arc<AtomicBool>,
    last_output: Option<PathBuf>,
}
struct ConvertUpdate {
    converter: usize,
    job: u64,
    bytes: u64,
    total: u64,
    result: Option<Result<u64, String>>,
    output: PathBuf,
    pending: Option<Arc<AtomicBool>>,
}
unsafe extern "C" fn converter_update(data: P) -> c_int {
    let update = Box::from_raw(data as *mut ConvertUpdate);
    if let Some(pending) = &update.pending {
        pending.store(false, Ordering::Release);
    }
    let converter = &mut *(update.converter as *mut Converter);
    if converter.job != update.job || (!converter.running && update.result.is_none()) {
        return 0;
    }
    if let Some(result) = update.result {
        converter.running = false;
        match result {
            Ok(units) => {
                converter.last_output = Some(update.output);
                (api().gtk_progress_bar_set_fraction)(converter.progress, 1.0);
                (api().gtk_label_set_text)(
                    converter.status,
                    c(&format!("Done: {units} rows or bytes converted.")).as_ptr(),
                );
            }
            Err(error) => (api().gtk_label_set_text)(
                converter.status,
                c(&format!("Conversion failed: {error}")).as_ptr(),
            ),
        }
    } else {
        if update.total > 0 {
            (api().gtk_progress_bar_set_fraction)(
                converter.progress,
                (update.bytes as f64 / update.total as f64).min(0.99),
            );
        }
        (api().gtk_label_set_text)(
            converter.status,
            c(&format!("Converting: {} bytes read", update.bytes)).as_ptr(),
        );
    }
    0
}
unsafe extern "C" fn converter_start(_: P, data: P) {
    let converter = &mut *(data as *mut Converter);
    if converter.running {
        (api().gtk_label_set_text)(
            converter.status,
            c("A conversion is already running.").as_ptr(),
        );
        return;
    }
    let input = CStr::from_ptr((api().gtk_entry_get_text)(converter.input))
        .to_string_lossy()
        .into_owned();
    let output = CStr::from_ptr((api().gtk_entry_get_text)(converter.output))
        .to_string_lossy()
        .into_owned();
    if input.trim().is_empty() || output.trim().is_empty() {
        (api().gtk_label_set_text)(
            converter.status,
            c("Choose both an input and an output file.").as_ptr(),
        );
        return;
    }
    let input = PathBuf::from(input);
    let output = PathBuf::from(output);
    let mode = (api().gtk_combo_box_get_active)(converter.mode);
    converter.running = true;
    converter.job += 1;
    converter.last_output = None;
    converter.cancel = Arc::new(AtomicBool::new(false));
    (api().gtk_progress_bar_set_fraction)(converter.progress, 0.0);
    (api().gtk_label_set_text)(converter.status, c("Starting conversion…").as_ptr());
    let cancel = converter.cancel.clone();
    let job = converter.job;
    let ptr = data as usize;
    thread::spawn(move || {
        let total = std::fs::metadata(&input).map(|m| m.len()).unwrap_or(0);
        let pending = Arc::new(AtomicBool::new(false));
        let mut report = |bytes| {
            if pending.swap(true, Ordering::AcqRel) {
                return;
            }
            let msg = Box::new(ConvertUpdate {
                converter: ptr,
                job,
                bytes,
                total,
                result: None,
                output: PathBuf::new(),
                pending: Some(pending.clone()),
            });
            unsafe {
                (api().g_idle_add)(converter_update, Box::into_raw(msg) as P);
            }
        };
        let result = match mode {
            1 => zfm::json_csv::json_to_csv(&input, &output, &cancel, &mut report),
            2 => zfm::converter::text_to_html(&input, &output, &cancel, &mut report),
            3 => zfm::image_convert::convert(&input, &output, true, &cancel, &mut report),
            4 => zfm::image_convert::convert(&input, &output, false, &cancel, &mut report),
            _ => zfm::converter::csv_to_json(&input, &output, &cancel, &mut report),
        }
        .map_err(|e| e.to_string());
        let msg = Box::new(ConvertUpdate {
            converter: ptr,
            job,
            bytes: 0,
            total,
            result: Some(result),
            output,
            pending: None,
        });
        unsafe {
            (api().g_idle_add)(converter_update, Box::into_raw(msg) as P);
        }
    });
}
unsafe extern "C" fn converter_cancel(_: P, data: P) {
    let converter = &mut *(data as *mut Converter);
    if converter.running {
        converter.cancel.store(true, Ordering::Relaxed);
        (api().gtk_label_set_text)(converter.status, c("Cancelling…").as_ptr());
    }
}
unsafe extern "C" fn converter_hide(window: P, _: P, _: P) -> c_int {
    (api().gtk_widget_hide)(window);
    1
}
unsafe extern "C" fn converter_browse_input(_: P, data: P) {
    converter_browse(data, false);
}
unsafe extern "C" fn converter_browse_output(_: P, data: P) {
    converter_browse(data, true);
}
unsafe fn converter_browse(data: P, save: bool) {
    // gtk_native_dialog_run enters a nested event loop. Retain only raw widget
    // pointers across it, not a Rust reference to state callbacks may mutate.
    let window = (*(data as *mut Converter)).window;
    let dialog = (api().gtk_file_chooser_native_new)(
        c(if save {
            "Save output as"
        } else {
            "Choose input file"
        })
        .as_ptr(),
        window,
        if save { 1 } else { 0 },
        c(if save { "Select" } else { "Open" }).as_ptr(),
        c("Cancel").as_ptr(),
    );
    if (api().gtk_native_dialog_run)(dialog) == -3 {
        // GTK_RESPONSE_ACCEPT
        let filename = (api().gtk_file_chooser_get_filename)(dialog);
        if !filename.is_null() {
            let name = CStr::from_ptr(filename).to_string_lossy();
            (api().gtk_entry_set_text)(
                if save {
                    (*(data as *mut Converter)).output
                } else {
                    (*(data as *mut Converter)).input
                },
                c(&name).as_ptr(),
            );
            (api().g_free)(filename as P);
        }
    }
    (api().g_object_unref)(dialog);
}
unsafe extern "C" fn converter_open_folder(_: P, data: P) {
    let converter = &*(data as *mut Converter);
    if let Some(path) = converter.last_output.as_ref() {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        match std::process::Command::new("xdg-open").arg(parent).spawn() {
            Ok(mut child) => {
                thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => (api().gtk_label_set_text)(
                converter.status,
                c(&format!("Could not open folder: {e}")).as_ptr(),
            ),
        }
    } else {
        (api().gtk_label_set_text)(converter.status, c("Convert a file first.").as_ptr());
    }
}
unsafe extern "C" fn show_converter(_: P, data: P) {
    let app = &mut *(data as *mut App);
    let converter = &mut *(app.converter as *mut Converter);
    let row = selected_rows(app.list)
        .first()
        .copied()
        .unwrap_or(ptr::null_mut());
    if !row.is_null() && !converter.running {
        let i = (api().gtk_list_box_row_get_index)(row);
        if i >= 0 {
            if let Some(entry) = app.entries.get(i as usize) {
                if entry.is_file {
                    let extension = entry
                        .path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    if ["csv", "json", "txt", "bmp", "ico"].iter().any(|e| extension.eq_ignore_ascii_case(e)) {
                        let (mode, output_extension) = if extension.eq_ignore_ascii_case("json") { (1, "csv") }
                            else if extension.eq_ignore_ascii_case("txt") { (2, "html") }
                            else if extension.eq_ignore_ascii_case("bmp") { (3, "ico") }
                            else if extension.eq_ignore_ascii_case("ico") { (4, "bmp") }
                            else { (0, "json") };
                        (api().gtk_combo_box_set_active)(converter.mode, mode);
                        (api().gtk_entry_set_text)(
                            converter.input,
                            c(&entry.path.to_string_lossy()).as_ptr(),
                        );
                        (api().gtk_entry_set_text)(
                            converter.output,
                            c(&entry
                                .path
                                .with_extension(output_extension)
                                .to_string_lossy())
                            .as_ptr(),
                        );
                    }
                }
            }
        }
    }
    (api().gtk_widget_show_all)(converter.window);
}
unsafe fn create_converter(parent: P) -> P {
    let window = (api().gtk_window_new)(0);
    (api().gtk_window_set_title)(window, c("ZFM — File converter").as_ptr());
    (api().gtk_window_set_transient_for)(window, parent);
    (api().gtk_window_set_default_size)(window, 600, 240);
    let layout = (api().gtk_box_new)(1, 8);
    (api().gtk_widget_set_margin_start)(layout, 12);
    (api().gtk_widget_set_margin_end)(layout, 12);
    (api().gtk_widget_set_margin_top)(layout, 12);
    (api().gtk_widget_set_margin_bottom)(layout, 12);
    (api().gtk_container_add)(window, layout);
    let caption = label("Built-in offline converter · CSV/JSON values are strings · images: opaque 24-bit");
    (api().gtk_label_set_xalign)(caption, 0.0);
    (api().gtk_box_pack_start)(layout, caption, 0, 0, 0);
    let mode = (api().gtk_combo_box_text_new)();
    (api().gtk_combo_box_text_append_text)(mode, c("CSV → JSON").as_ptr());
    (api().gtk_combo_box_text_append_text)(mode, c("JSON → CSV (string-valued objects)").as_ptr());
    (api().gtk_combo_box_text_append_text)(mode, c("Plain text → HTML (escaped)").as_ptr());
    (api().gtk_combo_box_text_append_text)(mode, c("24-bit BMP → ICO (up to 256 × 256)").as_ptr());
    (api().gtk_combo_box_text_append_text)(mode, c("Opaque 24-bit ICO → BMP").as_ptr());
    (api().gtk_combo_box_set_active)(mode, 0);
    (api().gtk_box_pack_start)(layout, mode, 0, 0, 0);
    let input_row = (api().gtk_box_new)(0, 4);
    (api().gtk_box_pack_start)(layout, input_row, 0, 0, 0);
    (api().gtk_box_pack_start)(input_row, label("Input"), 0, 0, 0);
    let input = (api().gtk_entry_new)();
    (api().gtk_box_pack_start)(input_row, input, 1, 1, 0);
    let output_row = (api().gtk_box_new)(0, 4);
    (api().gtk_box_pack_start)(layout, output_row, 0, 0, 0);
    (api().gtk_box_pack_start)(output_row, label("Output"), 0, 0, 0);
    let output = (api().gtk_entry_new)();
    (api().gtk_box_pack_start)(output_row, output, 1, 1, 0);
    let progress = (api().gtk_progress_bar_new)();
    (api().gtk_box_pack_start)(layout, progress, 0, 0, 0);
    let status = label("Output must not already exist.");
    (api().gtk_label_set_xalign)(status, 0.0);
    (api().gtk_box_pack_start)(layout, status, 0, 0, 0);
    let actions = (api().gtk_box_new)(0, 4);
    (api().gtk_box_pack_start)(layout, actions, 0, 0, 0);
    let converter = Box::into_raw(Box::new(Converter {
        window,
        mode,
        input,
        output,
        status,
        progress,
        running: false,
        job: 0,
        cancel: Arc::new(AtomicBool::new(false)),
        last_output: None,
    })) as P;
    button(input_row, "Browse…", converter_browse_input as P, converter);
    button(
        output_row,
        "Browse…",
        converter_browse_output as P,
        converter,
    );
    button(actions, "Convert", converter_start as P, converter);
    button(actions, "Cancel", converter_cancel as P, converter);
    button(
        actions,
        "Open output folder",
        converter_open_folder as P,
        converter,
    );
    connect(window, "delete-event", converter_hide as P, converter);
    converter
}

/// GTK dialogs run a nested main loop: never keep Rust references into App
/// across gtk_dialog_run, because another callback may run during the dialog.
unsafe fn prompt(parent: P, title: &str, message: &str, initial: Option<&str>) -> Option<String> {
    let dialog = (api().gtk_dialog_new)();
    (api().gtk_window_set_title)(dialog, c(title).as_ptr());
    (api().gtk_window_set_transient_for)(dialog, parent);
    (api().gtk_window_set_modal)(dialog, 1);
    (api().gtk_window_set_destroy_with_parent)(dialog, 1);
    (api().gtk_dialog_add_button)(dialog, c("Cancel").as_ptr(), -6);
    (api().gtk_dialog_add_button)(
        dialog,
        c(if initial.is_some() {
            "OK"
        } else {
            "Delete permanently"
        })
        .as_ptr(),
        -3,
    );
    let area = (api().gtk_dialog_get_content_area)(dialog);
    let description = label(message);
    (api().gtk_label_set_xalign)(description, 0.0);
    (api().gtk_widget_set_margin_start)(description, 12);
    (api().gtk_widget_set_margin_end)(description, 12);
    (api().gtk_widget_set_margin_top)(description, 12);
    (api().gtk_widget_set_margin_bottom)(description, 12);
    (api().gtk_box_pack_start)(area, description, 0, 0, 0);
    let entry = if let Some(text) = initial {
        let entry = (api().gtk_entry_new)();
        (api().gtk_entry_set_text)(entry, c(text).as_ptr());
        (api().gtk_box_pack_start)(area, entry, 0, 0, 8);
        entry
    } else {
        ptr::null_mut()
    };
    (api().gtk_dialog_set_default_response)(dialog, -3);
    (api().gtk_widget_show_all)(dialog);
    if !entry.is_null() {
        (api().gtk_entry_set_activates_default)(entry, 1);
        (api().gtk_widget_grab_focus)(entry);
        if title == "Rename" {
            let name = initial.unwrap_or("");
            let stem = std::path::Path::new(name)
                .file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or(name);
            (api().gtk_editable_select_region)(entry, 0, stem.chars().count() as c_int);
        }
    }
    let response = (api().gtk_dialog_run)(dialog);
    let result = if response == -3 {
        Some(if entry.is_null() {
            String::new()
        } else {
            CStr::from_ptr((api().gtk_entry_get_text)(entry))
                .to_string_lossy()
                .into_owned()
        })
    } else {
        None
    };
    if response != -1 {
        (api().gtk_widget_destroy)(dialog);
    }
    result
}

fn initial_copy_name(path: &std::path::Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    match path.extension().and_then(|s| s.to_str()) {
        Some(extension) if !extension.is_empty() => format!("{stem} copy.{extension}"),
        _ => format!("{stem} copy"),
    }
}
unsafe fn selected_path(data: P) -> Option<PathBuf> {
    let app = &*(data as *mut App);
    let rows = selected_rows(app.list);
    if rows.len() > 1 {
        set_status(app, "This action supports one selected item at a time.");
        return None;
    }
    let row = rows.first().copied().unwrap_or(ptr::null_mut());
    if row.is_null() {
        set_status(app, "Select an item first.");
        return None;
    }
    let index = (api().gtk_list_box_row_get_index)(row);
    if index < 0 {
        return None;
    }
    app.entries
        .get(index as usize)
        .map(|entry| entry.path.clone())
}

enum Operation {
    CreateFile(PathBuf, String),
    CreateDirectory(PathBuf, String),
    Rename(PathBuf, String),
    Duplicate(PathBuf, String),
    CopyTo(PathBuf, PathBuf),
    CopyItems(Vec<PathBuf>, PathBuf),
    MoveTo(PathBuf, PathBuf),
    Trash(PathBuf),
    Delete(PathBuf),
}
struct OperationResult {
    app: usize,
    id: u64,
    directory: PathBuf,
    generation: u64,
    result: Result<String, String>,
}
unsafe extern "C" fn operation_result(data: P) -> c_int {
    let message = Box::from_raw(data as *mut OperationResult);
    let app = &mut *(message.app as *mut App);
    if app.operation_id != message.id {
        return 0;
    }
    app.operation = None;
    match message.result {
        Ok(result) => {
            if app.tabs.current().current() == message.directory
                && app.generation.load(Ordering::Relaxed) == message.generation
            {
                app.notice = Some(result);
                navigate(app, Destination::Refresh);
            } else {
                set_status(app, &result);
            }
        }
        Err(error) => {
            app.error = Some(format!("Operation failed: {error}"));
            set_status(app, app.error.as_ref().unwrap());
        }
    }
    0
}
unsafe fn start_operation(data: P, directory: PathBuf, operation: Operation) {
    let app = &mut *(data as *mut App);
    if app.operation.is_some() {
        set_status(app, "An operation is already running.");
        return;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    app.operation = Some(cancel.clone());
    app.operation_id += 1;
    let id = app.operation_id;
    let generation = app.generation.load(Ordering::Relaxed);
    let app_ptr = data as usize;
    set_status(app, "Working…");
    thread::spawn(move || {
        use std::ffi::OsStr;
        use zfm::operations;
        let result = match operation {
            Operation::CreateFile(dir, name) => operations::create_file(&dir, OsStr::new(&name))
                .map(|p| format!("Created {}", p.display())),
            Operation::CreateDirectory(dir, name) => {
                operations::create_directory(&dir, OsStr::new(&name))
                    .map(|p| format!("Created {}", p.display()))
            }
            Operation::Rename(path, name) => operations::rename_child(&path, OsStr::new(&name))
                .map(|p| format!("Renamed to {}", p.display())),
            Operation::Duplicate(path, name) => {
                let dir = path.parent().unwrap_or_else(|| std::path::Path::new("/"));
                operations::copy_item(&path, dir, OsStr::new(&name), &cancel, |_| {})
                    .map(|p| format!("Created {}", p.display()))
            }
            Operation::CopyTo(path, destination) => {
                let name = path.file_name().unwrap_or_default();
                operations::copy_item(&path, &destination, name, &cancel, |_| {})
                    .map(|p| format!("Copied to {}", p.display()))
            }
            Operation::CopyItems(paths, destination) => {
                operations::copy_items(&paths, &destination, &cancel).map(|result| result.message(paths.len()))
            }
            Operation::MoveTo(path, destination) => operations::move_into(&path, &destination)
                .map(|p| format!("Moved to {}", p.display())),
            Operation::Trash(path) => {
                zfm::trash::trash(&path).map(|_| format!("Moved {} to trash", path.display()))
            }
            Operation::Delete(path) => {
                operations::remove_one(&path).map(|_| format!("Deleted {}", path.display()))
            }
        }
        .map_err(|e| e.to_string());
        let message = Box::new(OperationResult {
            app: app_ptr,
            id,
            directory,
            generation,
            result,
        });
        unsafe {
            (api().g_idle_add)(operation_result, Box::into_raw(message) as P);
        }
    });
}
unsafe extern "C" fn search_apply(_: P, data: P) {
    let app = &mut *(data as *mut App);
    app.search_query = CStr::from_ptr((api().gtk_entry_get_text)(app.search_entry))
        .to_string_lossy()
        .into_owned();
    app.search_case = (api().gtk_toggle_button_get_active)(app.case_toggle) != 0;
    navigate(app, Destination::Refresh);
}
unsafe extern "C" fn search_clear(_: P, data: P) {
    let entry = (*(data as *mut App)).search_entry;
    (api().gtk_entry_set_text)(entry, c("").as_ptr());
    search_apply(ptr::null_mut(), data);
}
unsafe extern "C" fn cancel_operation(_: P, data: P) {
    let app = &*(data as *mut App);
    if let Some(cancel) = &app.operation {
        cancel.store(true, Ordering::Relaxed);
        set_status(app, "Cancellation requested (applies only to copying).");
    } else {
        set_status(app, "No operation is running.");
    }
}
unsafe extern "C" fn new_file(_: P, data: P) {
    let parent = (*(data as *mut App)).window;
    let dir = (*(data as *mut App)).tabs.current().current().to_path_buf();
    if let Some(name) = prompt(parent, "New file", "Enter a file name:", Some("")) {
        start_operation(data, dir.clone(), Operation::CreateFile(dir, name));
    }
}
unsafe extern "C" fn new_folder(_: P, data: P) {
    let parent = (*(data as *mut App)).window;
    let dir = (*(data as *mut App)).tabs.current().current().to_path_buf();
    if let Some(name) = prompt(parent, "New folder", "Enter a folder name:", Some("")) {
        start_operation(data, dir.clone(), Operation::CreateDirectory(dir, name));
    }
}
unsafe extern "C" fn rename(_: P, data: P) {
    let path = match selected_path(data) {
        Some(p) => p,
        None => return,
    };
    let parent = (*(data as *mut App)).window;
    let dir = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"))
        .to_path_buf();
    let old_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if let Some(name) = prompt(
        parent,
        "Rename",
        "Enter a new name (existing items are never replaced):",
        Some(&old_name),
    ) {
        start_operation(data, dir, Operation::Rename(path, name));
    }
}
unsafe extern "C" fn duplicate(_: P, data: P) {
    let path = match selected_path(data) {
        Some(p) => p,
        None => return,
    };
    let parent = (*(data as *mut App)).window;
    let dir = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"))
        .to_path_buf();
    if let Some(name) = prompt(
        parent,
        "Duplicate",
        "Enter a name for the copy (links and special files are not supported):",
        Some(&initial_copy_name(&path)),
    ) {
        start_operation(data, dir, Operation::Duplicate(path, name));
    }
}
unsafe fn choose_folder(parent: P) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let dialog = (api().gtk_file_chooser_native_new)(
        c("Select destination folder").as_ptr(),
        parent,
        2,
        c("Select").as_ptr(),
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
unsafe extern "C" fn copy_to(_: P, data: P) {
    let path = match selected_path(data) {
        Some(p) => p,
        None => return,
    };
    let parent = (*(data as *mut App)).window;
    let source_dir = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"))
        .to_path_buf();
    if let Some(destination) = choose_folder(parent) {
        start_operation(data, source_dir, Operation::CopyTo(path, destination));
    }
}
unsafe extern "C" fn move_to(_: P, data: P) {
    let path = match selected_path(data) {
        Some(p) => p,
        None => return,
    };
    let parent = (*(data as *mut App)).window;
    let source_dir = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"))
        .to_path_buf();
    if let Some(destination) = choose_folder(parent) {
        start_operation(data, source_dir, Operation::MoveTo(path, destination));
    }
}
unsafe extern "C" fn move_to_trash(_: P, data: P) {
    let path = match selected_path(data) {
        Some(p) => p,
        None => return,
    };
    let dir = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"))
        .to_path_buf();
    start_operation(data, dir, Operation::Trash(path));
}
unsafe extern "C" fn delete_permanently(_: P, data: P) {
    let path = match selected_path(data) {
        Some(p) => p,
        None => return,
    };
    let parent = (*(data as *mut App)).window;
    let dir = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"))
        .to_path_buf();
    if prompt(
        parent,
        "Permanently delete?",
        &format!(
            "Permanently delete {}?\nThis cannot be undone. Only empty folders can be deleted.",
            path.display()
        ),
        None,
    )
    .is_some()
    {
        start_operation(data, dir, Operation::Delete(path));
    }
}

struct TabButtonData {
    app: usize,
    index: usize,
}
unsafe extern "C" fn free_tab_data(data: P, _: P) {
    drop(Box::from_raw(data as *mut TabButtonData));
}
unsafe extern "C" fn tab_clicked(_: P, data: P) {
    let data = &*(data as *const TabButtonData);
    let app_ptr = data.app as P;
    let index = data.index;
    if (*(app_ptr as *mut App)).tabs.select(index) {
        activate_tab(app_ptr);
    }
}
unsafe extern "C" fn tab_menu_new(_: P, data: P) {
    new_tab(ptr::null_mut(), (*(data as *const TabButtonData)).app as P);
}
unsafe extern "C" fn tab_menu_duplicate(_: P, data: P) {
    let tab = &*(data as *const TabButtonData);
    let app_ptr = tab.app as P;
    let path = (*(app_ptr as *mut App))
        .tabs
        .history(tab.index)
        .map(|h| h.current().to_path_buf());
    if let Some(path) = path {
        (*(app_ptr as *mut App)).tabs.add(path);
        activate_tab(app_ptr);
    }
}
unsafe extern "C" fn tab_menu_close(_: P, data: P) {
    let tab = &*(data as *const TabButtonData);
    let app_ptr = tab.app as P;
    let index = tab.index;
    if (*(app_ptr as *mut App)).tabs.close_at(index) {
        activate_tab(app_ptr);
    } else {
        set_status(&*(app_ptr as *mut App), "The last tab cannot be closed.");
    }
}
unsafe extern "C" fn tab_menu_close_others(_: P, data: P) {
    let tab = &*(data as *const TabButtonData);
    let app_ptr = tab.app as P;
    let index = tab.index;
    if (*(app_ptr as *mut App)).tabs.close_others(index) {
        activate_tab(app_ptr);
    }
}
unsafe extern "C" fn tab_button(_: P, event: P, data: P) -> c_int {
    let mut mouse = 0;
    if (api().gdk_event_get_button)(event, &mut mouse) == 0 {
        return 0;
    }
    if mouse == 2 {
        tab_menu_close(ptr::null_mut(), data);
        return 1;
    }
    if mouse != 3 {
        return 0;
    }
    let tab = &*(data as *const TabButtonData);
    let (app, index) = (tab.app, tab.index);
    let menu = (api().gtk_menu_new)();
    for (name, cb) in [
        ("New tab", tab_menu_new as P),
        ("Duplicate tab", tab_menu_duplicate as P),
        ("Close tab", tab_menu_close as P),
        ("Close other tabs", tab_menu_close_others as P),
    ] {
        let item = (api().gtk_menu_item_new_with_label)(c(name).as_ptr());
        (api().gtk_menu_shell_append)(menu, item);
        let action = Box::into_raw(Box::new(TabButtonData { app, index })) as P;
        (api().g_signal_connect_data)(
            item,
            c("activate").as_ptr(),
            cb,
            action,
            free_tab_data as P,
            0,
        );
    }
    connect(menu, "selection-done", destroy_child as P, ptr::null_mut());
    (api().gtk_widget_show_all)(menu);
    (api().gtk_menu_popup_at_pointer)(menu, event);
    1
}
unsafe extern "C" fn tab_background_button(widget: P, event: P, data: P) -> c_int {
    let event_widget = (api().gtk_get_event_widget)(event);
    let strip = (*(data as *mut App)).tab_strip;
    if event_widget != widget && event_widget != strip {
        return 0;
    }
    let mut button = 0;
    if (api().gdk_event_get_button)(event, &mut button) != 0
        && button == 1
        && (api().gdk_event_get_event_type)(event) == 5
    {
        new_tab(ptr::null_mut(), data);
        return 1;
    }
    0
}
unsafe fn rebuild_tabs(app: &App) {
    let data = app as *const App as usize;
    let strip = app.tab_strip;
    if strip.is_null() {
        return;
    }
    (api().gtk_container_foreach)(strip, destroy_child, ptr::null_mut());
    for index in 0..app.tabs.len() {
        let path = app.tabs.history(index).unwrap().current();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| path.display().to_string());
        let widget = (api().gtk_button_new_with_label)(c(&name).as_ptr());
        add_class(widget, "zfm-tab-btn");
        (api().gtk_widget_set_tooltip_text)(widget, c(&path.to_string_lossy()).as_ptr());
        if index == app.tabs.active() {
            add_class(widget, "zfm-tab-active");
        }
        let callback_data = Box::into_raw(Box::new(TabButtonData { app: data, index })) as P;
        (api().g_signal_connect_data)(
            widget,
            c("clicked").as_ptr(),
            tab_clicked as P,
            callback_data,
            free_tab_data as P,
            0,
        );
        (api().gtk_widget_add_events)(widget, 1 << 8);
        connect(widget, "button-press-event", tab_button as P, callback_data);
        (api().gtk_box_pack_start)(strip, widget, 0, 0, 0);
        (api().gtk_widget_show_all)(widget);
    }
}
unsafe fn activate_tab(data: P) {
    let app = &mut *(data as *mut App);
    // Cancel any work rendering the old tab before discarding its visible rows.
    app.generation.fetch_add(1, Ordering::Relaxed);
    (api().gtk_container_foreach)(app.list, destroy_child, ptr::null_mut());
    app.entries.clear();
    app.render_index = 0;
    app.error = None;
    app.notice = None;
    (api().gtk_entry_set_text)(
        app.address,
        c(&app.tabs.current().current().to_string_lossy()).as_ptr(),
    );
    rebuild_tabs(app);
    navigate(app, Destination::Refresh);
}
unsafe extern "C" fn new_tab(_: P, data: P) {
    let app = &mut *(data as *mut App);
    let path = app.tabs.current().current().to_path_buf();
    app.tabs.add(path);
    activate_tab(data);
}
unsafe extern "C" fn close_tab(_: P, data: P) {
    if (*(data as *mut App)).tabs.close() {
        activate_tab(data);
    } else {
        set_status(&*(data as *mut App), "The last tab cannot be closed.");
    }
}
unsafe extern "C" fn open_folder_tab(_: P, data: P) {
    let path = match selected_path(data) {
        Some(path) => path,
        None => return,
    };
    let app = &mut *(data as *mut App);
    app.tabs.add(path);
    activate_tab(data);
}
