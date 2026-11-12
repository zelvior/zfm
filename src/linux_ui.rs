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
use zfm::navigation::{Destination, History};

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
    gtk_window_set_modal: unsafe extern "C" fn(P, c_int),
    gtk_window_set_destroy_with_parent: unsafe extern "C" fn(P, c_int),
    gtk_dialog_new: unsafe extern "C" fn() -> P,
    gtk_dialog_add_button: unsafe extern "C" fn(P, *const c_char, c_int) -> P,
    gtk_dialog_get_content_area: unsafe extern "C" fn(P) -> P,
    gtk_dialog_run: unsafe extern "C" fn(P) -> c_int,
    gtk_window_set_default_size: unsafe extern "C" fn(P, c_int, c_int),
    gtk_window_set_transient_for: unsafe extern "C" fn(P, P),
    gtk_file_chooser_native_new: unsafe extern "C" fn(*const c_char, P, c_int, *const c_char, *const c_char) -> P,
    gtk_native_dialog_run: unsafe extern "C" fn(P) -> c_int,
    gtk_file_chooser_get_filename: unsafe extern "C" fn(P) -> *mut c_char,
    gtk_progress_bar_new: unsafe extern "C" fn() -> P,
    gtk_progress_bar_set_fraction: unsafe extern "C" fn(P, f64),
    gtk_widget_hide: unsafe extern "C" fn(P),
    gtk_list_box_get_selected_row: unsafe extern "C" fn(P) -> P,
    gtk_box_new: unsafe extern "C" fn(c_int, c_int) -> P,
    gtk_box_pack_start: unsafe extern "C" fn(P, P, c_int, c_int, c_uint),
    gtk_button_new_with_label: unsafe extern "C" fn(*const c_char) -> P,
    gtk_check_button_new_with_label: unsafe extern "C" fn(*const c_char) -> P,
    gtk_toggle_button_get_active: unsafe extern "C" fn(P) -> c_int,
    gtk_entry_new: unsafe extern "C" fn() -> P,
    gtk_entry_set_text: unsafe extern "C" fn(P, *const c_char),
    gtk_entry_set_placeholder_text: unsafe extern "C" fn(P, *const c_char),
    gtk_entry_get_text: unsafe extern "C" fn(P) -> *const c_char,
    gtk_label_new: unsafe extern "C" fn(*const c_char) -> P,
    gtk_label_set_text: unsafe extern "C" fn(P, *const c_char),
    gtk_label_set_xalign: unsafe extern "C" fn(P, f32),
    gtk_label_set_ellipsize: unsafe extern "C" fn(P, c_int),
    gtk_scrolled_window_new: unsafe extern "C" fn(P, P) -> P,
    gtk_list_box_new: unsafe extern "C" fn() -> P,
    gtk_list_box_row_new: unsafe extern "C" fn() -> P,
    gtk_list_box_select_row: unsafe extern "C" fn(P, P),
    gtk_menu_new: unsafe extern "C" fn() -> P,
    gtk_menu_item_new_with_label: unsafe extern "C" fn(*const c_char) -> P,
    gtk_menu_shell_append: unsafe extern "C" fn(P, P),
    gtk_menu_popup_at_pointer: unsafe extern "C" fn(P, P),
    gtk_widget_add_events: unsafe extern "C" fn(P, c_int),
    gdk_event_get_button: unsafe extern "C" fn(P, *mut c_uint) -> c_int,
    gtk_list_box_set_activate_on_single_click: unsafe extern "C" fn(P, c_int),
    gtk_list_box_insert: unsafe extern "C" fn(P, P, c_int),
    gtk_list_box_row_get_index: unsafe extern "C" fn(P) -> c_int,
    gtk_container_add: unsafe extern "C" fn(P, P),
    gtk_container_foreach: unsafe extern "C" fn(P, unsafe extern "C" fn(P, P), P),
    gtk_widget_destroy: unsafe extern "C" fn(P),
    gtk_widget_show_all: unsafe extern "C" fn(P),
    gtk_widget_grab_focus: unsafe extern "C" fn(P),
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
    history: History,
    generation: Arc<AtomicU64>,
    address: P,
    list: P,
    status: P,
    hidden: P,
    entries: Vec<Entry>,
    render_index: usize,
    skipped: usize,
    error: Option<String>,
    notice: Option<String>,
    converter: P,
    window: P,
    operation: Option<Arc<AtomicBool>>,
    operation_id: u64,
    search_entry: P,
    case_toggle: P,
    search_query: String,
    search_case: bool,
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
    let path = match app.history.resolve(&destination) {
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
        Ok(Some(listing)) => {
            app.history.commit(msg.destination);
            app.error = None;
            (api().gtk_entry_set_text)(
                app.address,
                c(&app.history.current().to_string_lossy()).as_ptr(),
            );
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
                c(&app.history.current().to_string_lossy()).as_ptr(),
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
            let s = (api().g_date_time_format)(date, c("%Y-%m-%d %H:%M").as_ptr());
            let text = if s.is_null() {
                String::new()
            } else {
                let text = CStr::from_ptr(s).to_string_lossy().into_owned();
                (api().g_free)(s as P);
                text
            };
            (api().g_date_time_unref)(date);
            return text;
        }
    }
    "—".into()
}
unsafe extern "C" fn render_batch(data: P) -> c_int {
    let msg = &*(data as *mut RenderMessage);
    let app = &mut *(msg.app as *mut App);
    if app.generation.load(Ordering::Relaxed) != msg.generation {
        drop(Box::from_raw(data as *mut RenderMessage));
        return 0;
    }
    let end = (app.render_index + 64).min(app.entries.len());
    for entry in &app.entries[app.render_index..end] {
        let row = (api().gtk_box_new)(0, 12);
        (api().gtk_widget_set_margin_start)(row, 10);
        (api().gtk_widget_set_margin_end)(row, 10);
        (api().gtk_widget_set_margin_top)(row, 6);
        (api().gtk_widget_set_margin_bottom)(row, 6);
        let kind = label(if entry.is_dir { "▸" } else { "·" });
        let name = label(&entry.name);
        (api().gtk_label_set_xalign)(name, 0.0);
        (api().gtk_label_set_ellipsize)(name, 3);
        (api().gtk_widget_set_hexpand)(name, 1);
        let size = label(&if entry.is_dir {
            "Folder".into()
        } else {
            entry
                .size
                .map(|n| format!("{n} B"))
                .unwrap_or_else(|| "—".into())
        });
        (api().gtk_widget_set_size_request)(size, 95, -1);
        let date = label(&modified(entry));
        (api().gtk_widget_set_size_request)(date, 140, -1);
        for child in [kind, name, size, date] {
            (api().gtk_box_pack_start)(row, child, 0, 0, 0);
        }
        let list_row = (api().gtk_list_box_row_new)();
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
        format!("  ·  {} entries had errors", app.skipped)
    } else {
        String::new()
    };
    if let Some(error) = app.error.as_ref() {
        set_status(app, error);
    } else {
        let notice = app
            .notice
            .as_ref()
            .map(|s| format!("  ·  {s}"))
            .unwrap_or_default();
        set_status(
            app,
            &format!(
                "{} items  ·  {}{}{}",
                app.entries.len(),
                app.history.current().display(),
                skipped,
                notice
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
    if let Some(parent) = app.history.current().parent().map(|p| p.to_path_buf()) {
        navigate(app, Destination::Visit(parent));
    }
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
        app.history.current().join(path)
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
        (0xffc2, 0) => navigate(&mut *(p as *mut App), Destination::Refresh),
        (0xff51, 8) => navigate(&mut *(p as *mut App), Destination::Back),
        (0xff53, 8) => navigate(&mut *(p as *mut App), Destination::Forward),
        (0xff52, 8) => up(ptr::null_mut(), p),
        (0x6c, 4) | (0x4c, 4) => (api().gtk_widget_grab_focus)((*(p as *mut App)).address),
        (0x66, 4) | (0x46, 4) => (api().gtk_widget_grab_focus)((*(p as *mut App)).search_entry),
        (0xffbf, 0) => rename(ptr::null_mut(), p), // F2
        (0xffff, 1) => delete_permanently(ptr::null_mut(), p), // Shift+Delete
        _ => return 0,
    }
    1
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
    if (api().gdk_event_get_button)(event, &mut mouse_button) == 0 || mouse_button != 3 {
        return 0;
    }
    let app = &*(data as *mut App);
    let index = (api().gtk_list_box_row_get_index)(row);
    if index < 0 {
        return 0;
    }
    let entry = match app.entries.get(index as usize) {
        Some(entry) => entry,
        None => return 0,
    };
    let can_duplicate = entry.is_file;
    let can_convert = can_duplicate
        && entry
            .path
            .extension()
            .and_then(|s| s.to_str())
            .map_or(false, |s| s.eq_ignore_ascii_case("csv"));
    let list = app.list;
    (api().gtk_list_box_select_row)(list, row);
    let menu = (api().gtk_menu_new)();
    if can_convert {
        menu_action(menu, "Convert CSV to JSON…", show_converter as P, data);
    }
    menu_action(menu, "Rename…", rename as P, data);
    menu_action(menu, "Move to…", move_to as P, data);
    if can_duplicate {
        menu_action(menu, "Copy to…", copy_to as P, data);
        menu_action(menu, "Duplicate file…", duplicate as P, data);
    }
    menu_action(menu, "Delete permanently…", delete_permanently as P, data);
    connect(menu, "selection-done", destroy_child as P, ptr::null_mut());
    (api().gtk_widget_show_all)(menu);
    (api().gtk_menu_popup_at_pointer)(menu, event);
    1
}
unsafe fn menu_action(menu: P, text: &str, callback: P, data: P) {
    let item = (api().gtk_menu_item_new_with_label)(c(text).as_ptr());
    (api().gtk_menu_shell_append)(menu, item);
    connect(item, "activate", callback, data);
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

pub fn run() -> Result<(), String> {
    unsafe {
        API = Box::into_raw(Box::new(Api::load()?));
        if (api().gtk_init_check)(ptr::null_mut(), ptr::null_mut()) == 0 {
            return Err("could not connect to a graphical display".into());
        }
        let css = (api().gtk_css_provider_new)();
        let style = c("* { background-image: none; } window, box, scrolledwindow, list, row { background-color: #080808; color: #f3f3f3; } button, entry { background: #161616; color: #fff; border: 1px solid #444; border-radius: 0; padding: 7px; box-shadow: none; } button:hover { background: #333; } row:selected, row:selected box, row:selected label { background: #eee; color: #080808; } label { color: inherit; } ");
        (api().gtk_css_provider_load_from_data)(css, style.as_ptr(), -1, ptr::null_mut());
        (api().gtk_style_context_add_provider_for_screen)(
            (api().gdk_screen_get_default)(),
            css,
            600,
        );
        (api().g_object_unref)(css);
        let window = (api().gtk_window_new)(0);
        (api().gtk_window_set_title)(window, c("ZFM — Zelvior File Manager").as_ptr());
        (api().gtk_window_set_default_size)(window, 1000, 650);
        let outer = (api().gtk_box_new)(1, 0);
        (api().gtk_container_add)(window, outer);
        let top = (api().gtk_box_new)(0, 4);
        (api().gtk_box_pack_start)(outer, top, 0, 0, 6);
        let search = (api().gtk_box_new)(0, 4);
        (api().gtk_box_pack_start)(outer, search, 0, 0, 4);
        let operations = (api().gtk_box_new)(0, 4);
        (api().gtk_box_pack_start)(outer, operations, 0, 0, 4);
        let content = (api().gtk_box_new)(0, 0);
        (api().gtk_box_pack_start)(outer, content, 1, 1, 0);
        let sidebar = (api().gtk_box_new)(1, 3);
        (api().gtk_widget_set_size_request)(sidebar, 160, -1);
        (api().gtk_box_pack_start)(content, sidebar, 0, 0, 8);
        let scroll = (api().gtk_scrolled_window_new)(ptr::null_mut(), ptr::null_mut());
        (api().gtk_box_pack_start)(content, scroll, 1, 1, 0);
        let list = (api().gtk_list_box_new)();
        (api().gtk_list_box_set_activate_on_single_click)(list, 0);
        (api().gtk_container_add)(scroll, list);
        let status = label("Ready");
        (api().gtk_label_set_xalign)(status, 0.0);
        (api().gtk_widget_set_margin_start)(status, 10);
        (api().gtk_widget_set_margin_top)(status, 8);
        (api().gtk_widget_set_margin_bottom)(status, 8);
        (api().gtk_box_pack_start)(outer, status, 0, 0, 0);
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
            history: History::new(initial),
            generation: Arc::new(AtomicU64::new(0)),
            address: ptr::null_mut(),
            list,
            status,
            hidden: ptr::null_mut(),
            entries: Vec::new(),
            render_index: 0,
            skipped: 0,
            error: None,
            notice: None,
            converter: ptr::null_mut(),
            window,
            operation: None,
            operation_id: 0,
            search_entry: ptr::null_mut(),
            case_toggle: ptr::null_mut(),
            search_query: String::new(),
            search_case: false,
        }));
        let p = app as P;
        let search_entry = (api().gtk_entry_new)();
        (*app).search_entry = search_entry;
        (api().gtk_entry_set_placeholder_text)(
            search_entry,
            c("Filter filenames in this folder…").as_ptr(),
        );
        (api().gtk_box_pack_start)(search, search_entry, 1, 1, 0);
        connect(search_entry, "activate", search_apply as P, p);
        let case_toggle = (api().gtk_check_button_new_with_label)(c("Case sensitive").as_ptr());
        (*app).case_toggle = case_toggle;
        (api().gtk_box_pack_start)(search, case_toggle, 0, 0, 0);
        button(search, "Search", search_apply as P, p);
        button(search, "Clear", search_clear as P, p);
        button(operations, "New file", new_file as P, p);
        button(operations, "New folder", new_folder as P, p);
        button(operations, "Rename…", rename as P, p);
        button(operations, "Duplicate file…", duplicate as P, p);
        button(operations, "Copy to…", copy_to as P, p);
        button(operations, "Move to…", move_to as P, p);
        button(
            operations,
            "Delete permanently…",
            delete_permanently as P,
            p,
        );
        button(operations, "Cancel copy", cancel_operation as P, p);
        button(top, "‹", back as P, p);
        button(top, "›", forward as P, p);
        button(top, "↑", up as P, p);
        button(top, "↻", refresh as P, p);
        button(top, "Converter", show_converter as P, p);
        let entry = (api().gtk_entry_new)();
        (*app).address = entry;
        (api().gtk_box_pack_start)(top, entry, 1, 1, 0);
        connect(entry, "activate", address as P, p);
        let hidden = (api().gtk_check_button_new_with_label)(c("Hidden").as_ptr());
        (*app).hidden = hidden;
        (api().gtk_box_pack_start)(top, hidden, 0, 0, 4);
        connect(hidden, "toggled", refresh as P, p);
        connect(list, "row-activated", row_activated as P, p);
        connect(window, "destroy", quit as P, p);
        connect(window, "key-press-event", key_press as P, p);
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for (name, path) in [
                ("Home", home.clone()),
                ("Desktop", home.join("Desktop")),
                ("Downloads", home.join("Downloads")),
                ("Documents", home.join("Documents")),
                ("Pictures", home.join("Pictures")),
                ("Music", home.join("Music")),
                ("Videos", home.join("Videos")),
                ("Filesystem", PathBuf::from("/")),
            ] {
                let data = Box::into_raw(Box::new(Shortcut {
                    app: app as usize,
                    path,
                })) as P;
                button(sidebar, name, shortcut as P, data);
            }
        }
        button(sidebar, "Converter", show_converter as P, p);
        (*app).converter = create_converter(window);
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
            Ok(rows) => {
                converter.last_output = Some(update.output);
                (api().gtk_progress_bar_set_fraction)(converter.progress, 1.0);
                (api().gtk_label_set_text)(
                    converter.status,
                    c(&format!("Done: {rows} rows converted.")).as_ptr(),
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
        let result = zfm::converter::csv_to_json(&input, &output, &cancel, |bytes| {
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
        })
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
            "Save JSON as"
        } else {
            "Choose CSV file"
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
    let row = (api().gtk_list_box_get_selected_row)(app.list);
    if !row.is_null() && !converter.running {
        let i = (api().gtk_list_box_row_get_index)(row);
        if i >= 0 {
            if let Some(entry) = app.entries.get(i as usize) {
                if !entry.is_dir
                    && entry
                        .path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map_or(false, |s| s.eq_ignore_ascii_case("csv"))
                {
                    (api().gtk_entry_set_text)(
                        converter.input,
                        c(&entry.path.to_string_lossy()).as_ptr(),
                    );
                    (api().gtk_entry_set_text)(
                        converter.output,
                        c(&entry.path.with_extension("json").to_string_lossy()).as_ptr(),
                    );
                }
            }
        }
    }
    (api().gtk_widget_show_all)(converter.window);
}
unsafe fn create_converter(parent: P) -> P {
    let window = (api().gtk_window_new)(0);
    (api().gtk_window_set_title)(window, c("ZFM — CSV to JSON").as_ptr());
    (api().gtk_window_set_transient_for)(window, parent);
    (api().gtk_window_set_default_size)(window, 600, 240);
    let layout = (api().gtk_box_new)(1, 8);
    (api().gtk_widget_set_margin_start)(layout, 12);
    (api().gtk_widget_set_margin_end)(layout, 12);
    (api().gtk_widget_set_margin_top)(layout, 12);
    (api().gtk_widget_set_margin_bottom)(layout, 12);
    (api().gtk_container_add)(window, layout);
    let caption = label("Offline converter · CSV → JSON (UTF-8, first row is headers)");
    (api().gtk_label_set_xalign)(caption, 0.0);
    (api().gtk_box_pack_start)(layout, caption, 0, 0, 0);
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
    (api().gtk_widget_show_all)(dialog);
    if !entry.is_null() {
        (api().gtk_widget_grab_focus)(entry);
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
    let row = (api().gtk_list_box_get_selected_row)(app.list);
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
    MoveTo(PathBuf, PathBuf),
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
            if app.history.current() == message.directory
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
                operations::duplicate_file(&path, OsStr::new(&name), &cancel, |_| {})
                    .map(|p| format!("Created {}", p.display()))
            }
            Operation::CopyTo(path, destination) => {
                let name = path.file_name().unwrap_or_default();
                operations::copy_file(&path, &destination, name, &cancel, |_| {})
                    .map(|p| format!("Copied to {}", p.display()))
            }
            Operation::MoveTo(path, destination) => operations::move_into(&path, &destination)
                .map(|p| format!("Moved to {}", p.display())),
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
    let dir = (*(data as *mut App)).history.current().to_path_buf();
    if let Some(name) = prompt(parent, "New file", "Enter a file name:", Some("")) {
        start_operation(data, dir.clone(), Operation::CreateFile(dir, name));
    }
}
unsafe extern "C" fn new_folder(_: P, data: P) {
    let parent = (*(data as *mut App)).window;
    let dir = (*(data as *mut App)).history.current().to_path_buf();
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
        "Duplicate file",
        "Enter a name for the copy (regular files only):",
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
