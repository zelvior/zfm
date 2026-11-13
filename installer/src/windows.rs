// Modified: add the interactive uninstaller and safer removal error handling.
//! ZFM setup: a native graphical wizard, a silent installer for automation, and
//! the uninstaller that the installer writes next to the application.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::ptr;
use std::thread;

use crate::canvas::{self, Backdrop, BoxRect, ButtonStyle, Metrics, ScenePage};
use crate::payload;
use crate::plan::{self, Options};
use crate::win32::{
    self, DrawItem, Hbrush, Hdc, Hfont, Hicon, Hwnd, LParam, LResult, Msg, PaintStruct, Rect,
    WParam,
};

const WM_SIZE: u32 = 0x0005;
const WM_PAINT: u32 = 0x000f;
const WM_CLOSE: u32 = 0x0010;
const WM_DESTROY: u32 = 0x0002;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_SETCURSOR: u32 = 0x0020;
const WM_COMMAND: u32 = 0x0111;
const WM_DRAWITEM: u32 = 0x002b;
const WM_CTLCOLORSTATIC: u32 = 0x0138;
const WM_CTLCOLOREDIT: u32 = 0x0133;
const WM_SETFONT: u32 = 0x0030;
const WM_PROGRESS: u32 = 0x8001;
const WM_DONE: u32 = 0x8002;

const WS_CAPTION: u32 = 0x00c0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const WS_CLIPCHILDREN: u32 = 0x0200_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const ES_MULTILINE: u32 = 0x0004;
const ES_AUTOVSCROLL: u32 = 0x0040;
const ES_AUTOHSCROLL: u32 = 0x0080;
const ES_READONLY: u32 = 0x0800;
const SS_OWNERDRAW: u32 = 0x0000_000d;
const BS_OWNERDRAW: u32 = 0x0000_000b;
const ODT_BUTTON: u32 = 4;
const ODT_STATIC: u32 = 5;
const EN_SETFOCUS: usize = 0x0100;
const EN_KILLFOCUS: usize = 0x0200;
const SW_SHOWNORMAL: i32 = 1;

const IDOK: usize = 1;
const IDCANCEL: usize = 2;
const BACK: usize = 101;
const NEXT: usize = 102;
const CANCEL: usize = 103;
const BROWSE: usize = 104;
const PATH_EDIT: usize = 105;
const LICENSE_EDIT: usize = 106;
const LAUNCH: usize = 107;
const STATUS: usize = 108;
const HEADING: usize = 109;
const BODY: usize = 110;

const APP_KEY: &str = "Software\\ZFM";
const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ZFM";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Welcome,
    Folder,
    License,
    Installing,
    Finish,
}

struct Palette {
    window_brush: Hbrush,
    edit_brush: Hbrush,
    title_font: Hfont,
    body_font: Hfont,
    button_font: Hfont,
    license_font: Hfont,
    hand_cursor: win32::Handle,
    logo: Hicon,
}

impl Palette {
    unsafe fn new() -> Option<Self> {
        let font = |height: i32, weight: i32| {
            win32::CreateFontW(
                height,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                win32::text("Segoe UI").as_ptr(),
            )
        };
        let palette = Self {
            window_brush: win32::CreateSolidBrush(canvas::CAPTION_TINT.colorref()),
            edit_brush: win32::CreateSolidBrush(canvas::EDIT_SURFACE.colorref()),
            title_font: font(-28, 700),
            body_font: font(-16, 400),
            button_font: font(-16, 600),
            license_font: font(-15, 400),
            hand_cursor: win32::LoadCursorW(ptr::null_mut(), 32649usize as *const u16),
            logo: win32::icon_from_png(payload::LOGO),
        };
        if [
            palette.window_brush,
            palette.edit_brush,
            palette.title_font,
            palette.body_font,
            palette.button_font,
            palette.license_font,
        ]
        .iter()
        .any(|handle| handle.is_null())
        {
            return None;
        }
        Some(palette)
    }
}

impl Drop for Palette {
    fn drop(&mut self) {
        unsafe {
            for brush in [self.window_brush, self.edit_brush] {
                if !brush.is_null() {
                    win32::DeleteObject(brush);
                }
            }
            for font in [
                self.title_font,
                self.body_font,
                self.button_font,
                self.license_font,
            ] {
                if !font.is_null() {
                    win32::DeleteObject(font);
                }
            }
            if !self.logo.is_null() {
                win32::DestroyIcon(self.logo);
            }
        }
    }
}

struct Wizard {
    page: Page,
    removing: bool,
    dir: String,
    status: String,
    percent: i32,
    busy: bool,
    license_ready: bool,
    launch_checked: bool,
    path_focused: bool,
    hovered: Hwnd,
    backdrop: Backdrop,
    palette: Palette,
    heading: Hwnd,
    body: Hwnd,
    status_control: Hwnd,
    path_edit: Hwnd,
    browse: Hwnd,
    license: Hwnd,
    launch: Hwnd,
    back: Hwnd,
    next: Hwnd,
    cancel: Hwnd,
}

struct Progress {
    percent: i32,
    text: String,
}

struct InstallFailure {
    message: String,
    retryable: bool,
    blocked_path: Option<PathBuf>,
}
impl From<String> for InstallFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            retryable: false,
            blocked_path: None,
        }
    }
}
impl From<payload::InstallFileError> for InstallFailure {
    fn from(error: payload::InstallFileError) -> Self {
        Self {
            message: error.message,
            retryable: error.retryable,
            blocked_path: error.blocked_path,
        }
    }
}
struct Done {
    result: Result<(), InstallFailure>,
}

/// The single wizard instance, published before the window exists so the
/// window procedure can reach it.
static mut WIZARD: *mut Wizard = ptr::null_mut();

fn crlf(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn registered_directory() -> Option<PathBuf> {
    win32::get_string(win32::HKEY_CURRENT_USER, APP_KEY, "InstallDir").map(PathBuf::from)
}

fn create_shortcuts(dir: &Path) -> Result<(), String> {
    let _com = win32::ComGuard::new().map_err(|code| {
        format!(
            "Could not initialize COM for Start Menu shortcuts (error {}).",
            win32::format_hresult(code)
        )
    })?;
    let programs = win32::programs_folder()
        .ok_or_else(|| "Could not find the Start Menu programs folder.".to_string())?;
    let folder = programs.join("ZFM");
    fs::create_dir_all(&folder)
        .map_err(|error| format!("Could not create {}: {error}", folder.display()))?;
    let app = dir.join("zfm.exe");
    let icon = dir.join("zfm.ico");
    win32::create_shortcut(
        &folder.join("ZFM File Manager.lnk"),
        &app,
        &icon,
        "Browse and manage files",
        dir,
    )?;
    win32::create_shortcut(
        &folder.join("Uninstall ZFM.lnk"),
        &dir.join("Uninstall.exe"),
        &icon,
        "Remove ZFM File Manager",
        dir,
    )
}

fn remove_shortcuts() -> Vec<String> {
    let mut problems = Vec::new();
    let folder = match win32::programs_folder() {
        Some(programs) => programs.join("ZFM"),
        None => return problems,
    };
    for name in ["ZFM File Manager.lnk", "Uninstall ZFM.lnk"] {
        let link = folder.join(name);
        match fs::remove_file(&link) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => problems.push(format!("{}: {error}", link.display())),
        }
    }
    let _ = fs::remove_dir(&folder);
    problems
}

fn register(dir: &Path) -> Result<(), String> {
    let location = path_text(dir);
    let app_key = win32::create_key(win32::HKEY_CURRENT_USER, APP_KEY)?;
    let result: Result<(), String> = (|| {
        win32::set_string(app_key, "InstallDir", &location)?;
        win32::set_string(app_key, "Version", payload::VERSION)
    })();
    win32::close_key(app_key);
    result?;

    let key = win32::create_key(win32::HKEY_CURRENT_USER, UNINSTALL_KEY)?;
    let uninstaller = format!("\"{}\\Uninstall.exe\"", location);
    let estimated = (payload::installed_size() / 1024).max(1) as u32;
    let result: Result<(), String> = (|| {
        win32::set_string(key, "DisplayName", payload::APP_NAME)?;
        win32::set_string(key, "DisplayVersion", payload::VERSION)?;
        win32::set_string(key, "Publisher", payload::PUBLISHER)?;
        win32::set_string(key, "DisplayIcon", &format!("{}\\zfm.ico", location))?;
        win32::set_string(key, "InstallLocation", &location)?;
        win32::set_string(key, "UninstallString", &uninstaller)?;
        win32::set_string(key, "QuietUninstallString", &format!("{} /S", uninstaller))?;
        win32::set_dword(key, "NoModify", 1)?;
        win32::set_dword(key, "NoRepair", 1)?;
        win32::set_dword(key, "EstimatedSize", estimated)
    })();
    win32::close_key(key);
    result
}

fn remove_registry() -> Vec<String> {
    let mut problems = Vec::new();
    for key in [UNINSTALL_KEY, APP_KEY] {
        let result = win32::delete_key(win32::HKEY_CURRENT_USER, key);
        if result != 0 && result != win32::ERROR_FILE_NOT_FOUND {
            problems.push(format!(
                "Could not remove the registry key {key} (error {result})."
            ));
        }
    }
    problems
}

fn perform_install<F>(dir: &Path, exe: &Path, mut report: F) -> Result<(), InstallFailure>
where
    F: FnMut(i32, &str),
{
    report(5, "Checking the folder…");
    let registered = registered_directory();
    let upgrading = registered
        .as_deref()
        .map(|existing| plan::same_directory(existing, dir))
        .unwrap_or(false)
        || payload::is_previous_install(dir);
    plan::may_install(dir, registered.as_deref())?;

    let outcome = (|| -> Result<(), InstallFailure> {
        report(20, "Copying the application…");
        payload::install_files(dir)?;
        report(55, "Installing the uninstaller…");
        payload::install_uninstaller(dir, exe)?;
        report(70, "Creating Start Menu shortcuts…");
        create_shortcuts(dir)?;
        report(90, "Registering the application…");
        register(dir)?;
        Ok(())
    })();

    if outcome.is_err() && !upgrading {
        if registered.is_none() {
            let _ = remove_shortcuts();
            let _ = remove_registry();
        }
        let _ = payload::remove_files(dir);
        payload::remove_folder_if_empty(dir);
    }

    outcome?;
    report(100, "Finishing…");
    Ok(())
}

/// Restart the uninstaller from the temporary folder so that the copy in the
/// installation folder is no longer running and can be removed. A running
/// executable can be copied, but it can neither be deleted nor moved to another
/// volume, so the copy does the removal and this process only hands over.
fn relaunch_from_temp(exe: &Path, dir: &Path, silent: bool) -> Option<()> {
    let temp = win32::temporary_folder()?;
    // Create a unique file without overwriting or cleaning other temp files.
    let mut copy = None;
    for attempt in 0..100 {
        let path = temp.join(format!("zfm-uninstall-{}-{attempt}.exe", std::process::id()));
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut output) => {
                let result = fs::File::open(exe).and_then(|mut input| io::copy(&mut input, &mut output));
                if result.is_err() { let _ = fs::remove_file(&path); return None; }
                copy = Some(path);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    let copy = copy?;
    let mut command = std::process::Command::new(&copy);
    command.arg("/UNINSTALL");
    if silent { command.arg("/S"); }
    command.arg(format!("/D={}", dir.display()));
    match command.spawn() {
        Ok(_) => Some(()),
        Err(_) => { let _ = fs::remove_file(&copy); None }
    }
}

/// Wait until the original uninstaller has exited, then delete its file. Only
/// a relocated copy calls this; the file is still locked while the process that
/// started us is shutting down.
fn remove_original_uninstaller(dir: &Path) -> io::Result<()> {
    let original = dir.join("Uninstall.exe");
    for _ in 0..100 {
        match fs::remove_file(&original) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => unsafe { win32::Sleep(100) },
        }
    }
    fs::remove_file(&original)
}

fn relocated_copy(exe: &Path) -> bool {
    exe.file_name()
        .map(|name| name.to_string_lossy().starts_with("zfm-uninstall"))
        .unwrap_or(false)
}

fn uninstall(exe: &Path, options: &Options) -> i32 {
    let running = exe.parent().map(Path::to_path_buf);
    let registered = registered_directory();
    // A relocated copy is given the folder explicitly, so it never depends on
    // the registry entry it is about to remove.
    let requested = options.directory.clone();
    let dir = match requested.or_else(|| {
        plan::uninstall_directory(running.as_deref(), registered.as_deref())
    }) {
        Some(dir) => dir,
        None => {
            if !options.silent {
                win32::message(
                    ptr::null_mut(),
                    "Could not determine the installation folder.",
                    "Uninstall ZFM",
                    win32::MB_ICONERROR,
                );
            }
            return 1;
        }
    };
    // Do not remove a different registered installation's shortcuts/keys.
    if !registered.as_ref().map_or(false, |path| plan::same_directory(path, &dir)) {
        if !options.silent {
            win32::message(ptr::null_mut(), "No matching registered ZFM installation was found; no files were removed.", "Uninstall ZFM", win32::MB_ICONERROR);
        }
        return 1;
    }
    if !options.silent {
        let prompt = format!(
            "Remove ZFM File Manager from\n{}\n\nYour own files in that folder are kept.",
            dir.display()
        );
        if !win32::confirm(ptr::null_mut(), &prompt, "Uninstall ZFM") {
            return 0;
        }
    }

    let running_is_uninstaller = exe
        .file_name()
        .map(|name| name.to_string_lossy().eq_ignore_ascii_case("uninstall.exe"))
        .unwrap_or(false);
    let inside = running_is_uninstaller
        && exe
            .parent()
            .map(|parent| plan::same_directory(parent, &dir))
            .unwrap_or(false);

    if inside {
        // Hand the removal to a copy in the temporary folder; otherwise the
        // running file would keep the installation folder alive. The window
        // closes immediately and the copy finishes the removal.
        return if relaunch_from_temp(exe, &dir, true).is_some() { 0 } else { 1 };
    }

    // Keep the uninstaller and registration until managed payload removal has
    // succeeded so a locked file can be retried instead of orphaning ZFM.
    let mut problems = payload::remove_files_except(&dir, Some("Uninstall.exe"));
    if problems.is_empty() { problems.extend(remove_shortcuts()); }
    if problems.is_empty() {
        if inside {
            problems.push("Could not start the temporary uninstaller; retry after closing ZFM.".to_string());
        } else if let Err(error) = remove_original_uninstaller(&dir) {
            problems.push(format!("{}: {error}", dir.join("Uninstall.exe").display()));
        }
    }
    if problems.is_empty() { problems.extend(remove_registry()); }
    if problems.is_empty() { payload::remove_folder_if_empty(&dir); }

    if problems.is_empty() {
        0
    } else {
        if !options.silent {
            win32::message(
                ptr::null_mut(),
                &format!(
                    "Some files could not be removed:\n\n{}",
                    problems.join("\n")
                ),
                "Uninstall ZFM",
                win32::MB_ICONERROR,
            );
        }
        1
    }
}

unsafe fn child(parent: Hwnd, class: &str, value: &str, style: u32, id: usize) -> Hwnd {
    win32::CreateWindowExW(
        0,
        win32::text(class).as_ptr(),
        win32::text(value).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        1,
        1,
        parent,
        id as win32::Handle,
        win32::GetModuleHandleW(ptr::null()),
        ptr::null_mut(),
    )
}

unsafe fn create_controls(hwnd: Hwnd, wizard: &mut Wizard) -> bool {
    let body_font = wizard.palette.body_font;
    let title_font = wizard.palette.title_font;
    let license_font = wizard.palette.license_font;
    let heading = child(hwnd, "STATIC", "", SS_OWNERDRAW, HEADING);
    let body = child(hwnd, "STATIC", "", SS_OWNERDRAW, BODY);
    let status_control = child(hwnd, "STATIC", "", SS_OWNERDRAW, STATUS);
    let path_edit = child(hwnd, "EDIT", "", WS_TABSTOP | ES_AUTOHSCROLL, PATH_EDIT);
    let browse = child(
        hwnd,
        "BUTTON",
        "Browse...",
        WS_TABSTOP | BS_OWNERDRAW,
        BROWSE,
    );
    let license = child(
        hwnd,
        "EDIT",
        "",
        WS_TABSTOP | WS_VSCROLL | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL,
        LICENSE_EDIT,
    );
    let launch = child(
        hwnd,
        "BUTTON",
        "Launch ZFM now",
        WS_TABSTOP | BS_OWNERDRAW,
        LAUNCH,
    );
    let back = child(hwnd, "BUTTON", "Back", WS_TABSTOP | BS_OWNERDRAW, BACK);
    let next = child(hwnd, "BUTTON", "Next", WS_TABSTOP | BS_OWNERDRAW, NEXT);
    let cancel = child(
        hwnd,
        "BUTTON",
        "Cancel",
        WS_TABSTOP | BS_OWNERDRAW,
        CANCEL,
    );
    let controls = [
        heading,
        body,
        status_control,
        path_edit,
        browse,
        license,
        launch,
        back,
        next,
        cancel,
    ];
    if controls.iter().any(|control| control.is_null()) {
        return false;
    }
    for control in controls {
        win32::SendMessageW(control, WM_SETFONT, body_font as usize, 1);
    }
    win32::SendMessageW(heading, WM_SETFONT, title_font as usize, 1);
    win32::SendMessageW(license, WM_SETFONT, license_font as usize, 1);
    win32::apply_explorer_scrollbar(license);
    wizard.heading = heading;
    wizard.body = body;
    wizard.status_control = status_control;
    wizard.path_edit = path_edit;
    wizard.browse = browse;
    wizard.license = license;
    wizard.launch = launch;
    wizard.back = back;
    wizard.next = next;
    wizard.cancel = cancel;
    true
}

fn place_rect(control: Hwnd, rect: BoxRect) {
    if control.is_null() {
        return;
    }
    unsafe {
        win32::MoveWindow(
            control,
            rect.x,
            rect.y,
            rect.width.max(1),
            rect.height.max(1),
            1,
        );
    }
}

fn scene_page(wizard: &Wizard) -> ScenePage {
    match wizard.page {
        Page::Welcome => ScenePage::Welcome,
        Page::Folder => ScenePage::Folder {
            focused: wizard.path_focused,
        },
        Page::License => ScenePage::License,
        Page::Installing => ScenePage::Installing {
            percent: wizard.percent,
        },
        Page::Finish => ScenePage::Finish,
    }
}

unsafe fn rebuild_backdrop(hwnd: Hwnd, wizard: &mut Wizard) {
    let mut client = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    win32::GetClientRect(hwnd, &mut client);
    wizard.backdrop = Backdrop::render(client.right, client.bottom, scene_page(wizard));
}

unsafe fn layout(hwnd: Hwnd, wizard: &mut Wizard) {
    let mut client = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    win32::GetClientRect(hwnd, &mut client);
    let metrics = Metrics::new(client.right, client.bottom);
    place_rect(wizard.heading, metrics.heading);
    place_rect(wizard.body, metrics.body);
    place_rect(wizard.path_edit, metrics.path_edit);
    place_rect(wizard.browse, metrics.browse_pill.with_pill_padding());
    place_rect(wizard.license, metrics.license_edit);
    let status_rect = match wizard.page {
        Page::Folder => metrics.folder_status,
        Page::Finish => metrics.finish_status,
        _ => metrics.progress_status,
    };
    place_rect(wizard.status_control, status_rect);
    place_rect(wizard.launch, metrics.launch);
    place_rect(wizard.cancel, metrics.cancel_pill.with_pill_padding());
    place_rect(wizard.next, metrics.next_pill.with_pill_padding());
    place_rect(wizard.back, metrics.back_pill.with_pill_padding());
    rebuild_backdrop(hwnd, wizard);
}

unsafe fn paint(hwnd: Hwnd, wizard: &Wizard) {
    let mut paint: PaintStruct = std::mem::zeroed();
    let dc = win32::BeginPaint(hwnd, &mut paint);
    win32::blit_pixels(
        dc,
        0,
        0,
        wizard.backdrop.width,
        wizard.backdrop.height,
        &wizard.backdrop.pixels,
        |_| {},
    );
    win32::EndPaint(hwnd, &paint);
}

unsafe fn draw_item(parent: Hwnd, item: &mut DrawItem, wizard: &Wizard) -> LResult {
    let width = (item.rect.right - item.rect.left).max(1);
    let height = (item.rect.bottom - item.rect.top).max(1);
    let (origin_x, origin_y) = win32::child_origin(parent, item.hwnd);
    let mut pixels = wizard.backdrop.slice(origin_x, origin_y, width, height);
    let id = item.control_id as usize;

    if item.control_type == ODT_STATIC {
        let label = win32::text(&win32::window_text(item.hwnd));
        let (font, color, format) = match id {
            HEADING => (
                wizard.palette.title_font,
                canvas::HEADING_INK.colorref(),
                win32::DT_VCENTER | win32::DT_SINGLELINE | win32::DT_NOPREFIX,
            ),
            STATUS if wizard.page == Page::Folder => (
                wizard.palette.body_font,
                canvas::ERROR_INK.colorref(),
                win32::DT_WORDBREAK | win32::DT_EDITCONTROL | win32::DT_NOPREFIX,
            ),
            STATUS => (
                wizard.palette.body_font,
                canvas::STATUS_INK.colorref(),
                win32::DT_WORDBREAK | win32::DT_EDITCONTROL | win32::DT_NOPREFIX,
            ),
            _ => (
                wizard.palette.body_font,
                canvas::BODY_INK.colorref(),
                win32::DT_WORDBREAK | win32::DT_EDITCONTROL | win32::DT_NOPREFIX,
            ),
        };
        win32::blit_pixels(
            item.dc,
            item.rect.left,
            item.rect.top,
            width,
            height,
            &pixels,
            |mem_dc| {
                win32::SetBkMode(mem_dc, 1);
                win32::SetTextColor(mem_dc, color);
                let old_font = win32::SelectObject(mem_dc, font);
                let mut bounds = Rect {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                };
                win32::DrawTextW(mem_dc, label.as_ptr(), -1, &mut bounds, format);
                win32::SelectObject(mem_dc, old_font);
            },
        );
        return 1;
    }

    if item.control_type == ODT_BUTTON {
        let disabled = item.item_state & 4 != 0;
        let pressed = item.item_state & 1 != 0;
        let focused = item.item_state & 16 != 0;
        let hovered = !disabled && wizard.hovered == item.hwnd;
        let label = win32::text(&win32::window_text(item.hwnd));

        if id == LAUNCH {
            canvas::paint_checkbox(
                &mut pixels,
                width,
                height,
                wizard.launch_checked,
                hovered,
                focused,
            );
            win32::blit_pixels(
                item.dc,
                item.rect.left,
                item.rect.top,
                width,
                height,
                &pixels,
                |mem_dc| {
                    win32::SetBkMode(mem_dc, 1);
                    win32::SetTextColor(mem_dc, canvas::BODY_INK.colorref());
                    let old_font = win32::SelectObject(mem_dc, wizard.palette.body_font);
                    let mut bounds = Rect {
                        left: 32,
                        top: 0,
                        right: width,
                        bottom: height,
                    };
                    win32::DrawTextW(
                        mem_dc,
                        label.as_ptr(),
                        -1,
                        &mut bounds,
                        win32::DT_VCENTER | win32::DT_SINGLELINE | win32::DT_NOPREFIX,
                    );
                    win32::SelectObject(mem_dc, old_font);
                },
            );
            return 1;
        }

        let primary = id == NEXT;
        canvas::paint_pill_button(
            &mut pixels,
            width,
            height,
            ButtonStyle {
                primary,
                hovered,
                pressed,
                disabled,
                focused,
            },
        );
        let text_color = if disabled {
            canvas::DISABLED_TEXT.colorref()
        } else if primary {
            canvas::PRIMARY_TEXT.colorref()
        } else {
            canvas::SECONDARY_TEXT.colorref()
        };
        let font = if primary {
            wizard.palette.button_font
        } else {
            wizard.palette.body_font
        };
        let shift = if pressed && !disabled { 1 } else { 0 };
        win32::blit_pixels(
            item.dc,
            item.rect.left,
            item.rect.top,
            width,
            height,
            &pixels,
            |mem_dc| {
                win32::SetBkMode(mem_dc, 1);
                win32::SetTextColor(mem_dc, text_color);
                let old_font = win32::SelectObject(mem_dc, font);
                let mut bounds = Rect {
                    left: canvas::PILL_PAD_X,
                    top: canvas::PILL_PAD_TOP + shift - 1,
                    right: width - canvas::PILL_PAD_X,
                    bottom: height - canvas::PILL_PAD_BOTTOM + shift,
                };
                win32::DrawTextW(
                    mem_dc,
                    label.as_ptr(),
                    -1,
                    &mut bounds,
                    win32::DT_CENTER | win32::DT_VCENTER | win32::DT_SINGLELINE | win32::DT_NOPREFIX,
                );
                win32::SelectObject(mem_dc, old_font);
            },
        );
        return 1;
    }

    0
}

unsafe fn render(hwnd: Hwnd, wizard: &mut Wizard) {
    let page = wizard.page;
    let (heading, body) = match page {
        Page::Welcome => (
            "Welcome to ZFM",
            "ZFM File Manager is a lightweight native file manager. Setup installs it for the current user, so no administrator rights or extra runtime are needed. Choose Next to pick a folder.",
        ),
        Page::Folder => (
            "Choose a folder",
            "ZFM is installed in the folder below. Setup adds a Start Menu shortcut, an entry in Programs and Features, and an uninstaller. Other files already in the folder are left untouched.",
        ),
        Page::License => (
            "License agreement",
            "ZFM is released under the Apache License 2.0. Review the terms below, then choose Install to accept them.",
        ),
        Page::Installing => (
            "Installing ZFM",
            "Setup is copying the application files and registering ZFM for this user.",
        ),
        Page::Finish => (
            "Setup complete",
            "ZFM is installed. Start it from the Start Menu, or choose Finish to close setup.",
        ),
    };
    let removal_body = format!("ZFM will be removed from:\n{}\n\nOnly ZFM-managed files, shortcuts and registration are removed. Your other files are kept.", wizard.dir);
    let (heading, body) = if wizard.removing {
        match page {
            Page::Welcome => ("Uninstall ZFM", removal_body.as_str()),
            Page::Installing => ("Removing ZFM", "Removing managed files and registration. Please wait."),
            Page::Finish => ("Uninstall complete", "ZFM has been removed. Your other files were left untouched."),
            _ => (heading, body),
        }
    } else { (heading, body) };
    win32::set_text(wizard.heading, heading);
    win32::set_text(wizard.body, body);
    win32::set_text(wizard.status_control, &wizard.status);
    if page == Page::Folder {
        win32::set_text(wizard.path_edit, &wizard.dir);
    } else {
        wizard.path_focused = false;
    }
    if !wizard.license_ready {
        win32::set_text(wizard.license, &crlf(payload::LICENSE));
        wizard.license_ready = true;
    }
    win32::show(wizard.path_edit, page == Page::Folder);
    win32::show(wizard.browse, page == Page::Folder);
    win32::show(wizard.license, page == Page::License);
    win32::show(wizard.launch, page == Page::Finish && !wizard.removing);
    win32::show(
        wizard.status_control,
        matches!(page, Page::Folder | Page::Installing | Page::Finish),
    );
    win32::show(
        wizard.back,
        !matches!(page, Page::Welcome | Page::Installing | Page::Finish),
    );
    win32::show(wizard.next, page != Page::Installing);
    win32::show(
        wizard.cancel,
        !matches!(page, Page::Installing | Page::Finish),
    );
    win32::set_text(
        wizard.next,
        match page {
            Page::Welcome if wizard.removing => "Uninstall",
            Page::License => "Install",
            Page::Finish => "Finish",
            _ => "Next",
        },
    );
    win32::EnableWindow(wizard.next, if wizard.busy { 0 } else { 1 });
    win32::EnableWindow(wizard.back, if wizard.busy { 0 } else { 1 });
    win32::EnableWindow(wizard.cancel, if wizard.busy { 0 } else { 1 });
    layout(hwnd, wizard);
    win32::InvalidateRect(hwnd, ptr::null(), 0);
    for control in [
        wizard.heading,
        wizard.body,
        wizard.status_control,
        wizard.browse,
        wizard.launch,
        wizard.back,
        wizard.next,
        wizard.cancel,
    ] {
        if !control.is_null() {
            win32::InvalidateRect(control, ptr::null(), 0);
        }
    }
}

unsafe fn begin_install(hwnd: Hwnd, wizard: &mut Wizard, close_existing: bool) {
    wizard.page = Page::Installing;
    wizard.busy = true;
    wizard.percent = 0;
    wizard.status = "Starting…".to_string();
    render(hwnd, wizard);
    let dir = PathBuf::from(&wizard.dir);
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            wizard.busy = false;
            wizard.page = Page::Folder;
            wizard.status = format!("Could not locate the setup program: {error}");
            render(hwnd, wizard);
            return;
        }
    };
    let window = hwnd as usize;
    thread::spawn(move || {
        // COM initialization is per-thread; the UI thread's apartment does not
        // cover this worker, which creates the Start Menu shell links.
        let _com = win32::ComGuard::new().ok();
        let close_result = if close_existing {
            crate::running_app::close_and_wait(&dir.join("zfm.exe"))
        } else { Ok(()) };
        let result = close_result.map_err(|message| InstallFailure { message, retryable: true, blocked_path: None }).and_then(|_| perform_install(&dir, &exe, |percent, text| {
            let message = Box::into_raw(Box::new(Progress {
                percent,
                text: text.to_string(),
            }));
            unsafe {
                if win32::PostMessageW(window as Hwnd, WM_PROGRESS, 0, message as LParam) == 0 {
                    drop(Box::from_raw(message));
                }
            }
        }));
        let message = Box::into_raw(Box::new(Done { result }));
        unsafe {
            if win32::PostMessageW(window as Hwnd, WM_DONE, 0, message as LParam) == 0 {
                drop(Box::from_raw(message));
            }
        }
    });
}

unsafe fn begin_uninstall(hwnd: Hwnd, wizard: &mut Wizard) {
    wizard.page = Page::Installing;
    wizard.busy = true;
    wizard.percent = 15;
    wizard.status = "Removing ZFM…".to_string();
    render(hwnd, wizard);
    let dir = PathBuf::from(&wizard.dir);
    let window = hwnd as usize;
    thread::spawn(move || {
        let result = match std::env::current_exe() {
            Ok(exe) => {
                let options = Options { uninstall: true, silent: true, directory: Some(dir) };
                if uninstall(&exe, &options) == 0 { Ok(()) } else {
                    Err(InstallFailure { message: "Some files could not be removed. Close ZFM and retry; registration is retained when possible.".into(), retryable: false, blocked_path: None })
                }
            }
            Err(e) => Err(InstallFailure { message: format!("Could not locate the uninstaller: {e}"), retryable: false, blocked_path: None }),
        };
        let done = Box::into_raw(Box::new(Done { result }));
        unsafe { if win32::PostMessageW(window as Hwnd, WM_DONE, 0, done as LParam) == 0 { drop(Box::from_raw(done)); } }
    });
}

unsafe fn finish(hwnd: Hwnd, wizard: &mut Wizard) {
    if wizard.launch_checked {
        let dir = PathBuf::from(&wizard.dir);
        let exe = dir.join("zfm.exe");
        // Start the application detached from setup; the standard library does
        // the CreateProcessW plumbing correctly.
        let _ = std::process::Command::new(&exe).current_dir(&dir).spawn();
    }
    win32::DestroyWindow(hwnd);
}

unsafe fn advance(hwnd: Hwnd, wizard: &mut Wizard) {
    match wizard.page {
        Page::Welcome if wizard.removing => begin_uninstall(hwnd, wizard),
        Page::Welcome => {
            wizard.page = Page::Folder;
            render(hwnd, wizard);
        }
        Page::Folder => {
            let typed = win32::window_text(wizard.path_edit);
            match plan::validate_directory(&typed).and_then(|path| {
                plan::may_install(&path, registered_directory().as_deref())?;
                Ok(path)
            }) {
                Ok(path) => {
                    wizard.dir = path_text(&path);
                    wizard.status.clear();
                    wizard.page = Page::License;
                    render(hwnd, wizard);
                }
                Err(error) => {
                    wizard.dir = typed;
                    wizard.status = error;
                    render(hwnd, wizard);
                }
            }
        }
        Page::License => begin_install(hwnd, wizard, false),
        Page::Finish => finish(hwnd, wizard),
        Page::Installing => {}
    }
}

unsafe fn go_back(hwnd: Hwnd, wizard: &mut Wizard) {
    match wizard.page {
        Page::Folder => {
            wizard.dir = win32::window_text(wizard.path_edit);
            wizard.status.clear();
            wizard.page = Page::Welcome;
            render(hwnd, wizard);
        }
        Page::License => {
            wizard.page = Page::Folder;
            render(hwnd, wizard);
        }
        _ => {}
    }
}

unsafe fn browse_directory(hwnd: Hwnd, wizard: &mut Wizard) {
    if let Some(folder) = win32::browse_for_folder(hwnd, "Select the ZFM installation folder") {
        wizard.dir = path_text(&folder);
        wizard.status.clear();
        win32::set_text(wizard.path_edit, &wizard.dir);
        win32::set_text(wizard.status_control, "");
        win32::InvalidateRect(wizard.status_control, ptr::null(), 0);
    }
}

unsafe fn command(hwnd: Hwnd, wizard: &mut Wizard, wparam: WParam) -> Option<String> {
    let id = wparam & 0xffff;
    let code = (wparam >> 16) & 0xffff;
    if id == PATH_EDIT && (code == EN_SETFOCUS || code == EN_KILLFOCUS) {
        let focused = code == EN_SETFOCUS;
        if wizard.path_focused != focused {
            wizard.path_focused = focused;
            if wizard.page == Page::Folder {
                rebuild_backdrop(hwnd, wizard);
                win32::InvalidateRect(hwnd, ptr::null(), 0);
            }
        }
        return None;
    }
    if code != 0 {
        return None;
    }
    match id {
        NEXT => advance(hwnd, wizard),
        IDOK => {
            if !wizard.busy && wizard.page != Page::Installing {
                let focus = win32::GetFocus();
                if focus == wizard.back {
                    go_back(hwnd, wizard);
                } else if focus == wizard.cancel {
                    win32::DestroyWindow(hwnd);
                } else if focus == wizard.browse && wizard.page == Page::Folder {
                    browse_directory(hwnd, wizard);
                } else {
                    advance(hwnd, wizard);
                }
            }
        }
        BACK => go_back(hwnd, wizard),
        CANCEL => {
            if !wizard.busy {
                win32::DestroyWindow(hwnd);
            }
        }
        IDCANCEL => {
            if !wizard.busy && !matches!(wizard.page, Page::Installing | Page::Finish) {
                win32::DestroyWindow(hwnd);
            }
        }
        BROWSE => browse_directory(hwnd, wizard),
        LAUNCH => {
            wizard.launch_checked = !wizard.launch_checked;
            win32::InvalidateRect(wizard.launch, ptr::null(), 0);
        }
        _ => {}
    }
    None
}

unsafe fn completed(hwnd: Hwnd, wizard: &mut Wizard, lparam: LParam) -> Option<InstallFailure> {
    let message = Box::from_raw(lparam as *mut Done);
    wizard.busy = false;
    match message.result {
        Ok(()) => {
            wizard.percent = 100;
            wizard.status = if wizard.removing { "Removal completed.".to_string() } else { format!("Installed in {}", wizard.dir) };
            wizard.page = Page::Finish;
            render(hwnd, wizard);
            None
        }
        Err(error) => {
            wizard.page = if wizard.removing { Page::Welcome } else { Page::Folder };
            wizard.status = error.message.clone();
            render(hwnd, wizard);
            Some(error)
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: WParam,
    lparam: LParam,
) -> LResult {
    if WIZARD.is_null() {
        return win32::DefWindowProcW(hwnd, message, wparam, lparam);
    }
    let wizard = &mut *WIZARD;
    let mut failure: Option<String> = None;
    let mut install_failure: Option<InstallFailure> = None;
    let result = match message {
        WM_CLOSE => {
            if !wizard.busy {
                win32::DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            win32::PostQuitMessage(0);
            0
        }
        WM_ERASEBKGND => 1,
        WM_SETCURSOR => {
            let target = wparam as Hwnd;
            let interactive = [
                wizard.next,
                wizard.back,
                wizard.cancel,
                wizard.browse,
                wizard.launch,
            ]
            .contains(&target);
            let next_hover = if interactive { target } else { ptr::null_mut() };
            if wizard.hovered != next_hover {
                let previous = wizard.hovered;
                wizard.hovered = next_hover;
                if !previous.is_null() {
                    win32::InvalidateRect(previous, ptr::null(), 0);
                }
                if !next_hover.is_null() {
                    win32::InvalidateRect(next_hover, ptr::null(), 0);
                }
            }
            if interactive && !wizard.busy && !wizard.palette.hand_cursor.is_null() {
                if (lparam & 0xffff) == 1 {
                    win32::SetCursor(wizard.palette.hand_cursor);
                    return 1;
                }
            }
            win32::DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_COMMAND => {
            failure = command(hwnd, wizard, wparam);
            0
        }
        WM_DRAWITEM if lparam != 0 => {
            let item = &mut *(lparam as *mut DrawItem);
            draw_item(hwnd, item, wizard)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
            win32::SetTextColor(wparam as Hdc, canvas::BODY_INK.colorref());
            win32::SetBkColor(wparam as Hdc, canvas::EDIT_SURFACE.colorref());
            wizard.palette.edit_brush as LResult
        }
        WM_PAINT => {
            paint(hwnd, wizard);
            0
        }
        WM_SIZE => {
            layout(hwnd, wizard);
            0
        }
        WM_PROGRESS => {
            if lparam != 0 {
                let message = Box::from_raw(lparam as *mut Progress);
                wizard.percent = message.percent;
                wizard.status = message.text;
                win32::set_text(wizard.status_control, &wizard.status);
                rebuild_backdrop(hwnd, wizard);
                win32::InvalidateRect(hwnd, ptr::null(), 0);
                win32::InvalidateRect(wizard.status_control, ptr::null(), 0);
            }
            0
        }
        WM_DONE => {
            if lparam != 0 {
                install_failure = completed(hwnd, wizard, lparam);
            }
            0
        }
        _ => win32::DefWindowProcW(hwnd, message, wparam, lparam),
    };
    if let Some(error) = failure {
        win32::message(hwnd, &error, "ZFM Setup", win32::MB_ICONERROR);
    }
    if let Some(error) = install_failure {
        let message = format!("{} could not finish:\n\n{}", if (*WIZARD).removing { "Uninstall" } else { "Setup" }, error.message);
        if (*WIZARD).removing {
            win32::message(hwnd, &message, "Uninstall ZFM", win32::MB_ICONERROR);
        } else if error.retryable {
            let target = PathBuf::from(&(*WIZARD).dir).join("zfm.exe");
            if error.blocked_path.as_deref() == Some(target.as_path()) && crate::running_app::is_running(&target) {
                let prompt = format!("{}\n\nZFM is running from this installation. May setup ask it to close and retry? Save any work first. Choosing No leaves ZFM open.", message);
                if win32::confirm(hwnd, &prompt, "ZFM Setup") {
                    begin_install(hwnd, &mut *WIZARD, true);
                }
            } else if win32::retry(hwnd, &message, "ZFM Setup") {
                begin_install(hwnd, &mut *WIZARD, false);
            }
        } else {
            win32::message(hwnd, &message, "ZFM Setup", win32::MB_ICONERROR);
        }
    }
    result
}

unsafe fn register_class(instance: win32::Hinstance, background: Hbrush, class: &[u16]) -> bool {
    let window_class = win32::WndClass {
        style: 0,
        proc: Some(window_proc),
        class_extra: 0,
        window_extra: 0,
        instance,
        icon: ptr::null_mut(),
        cursor: win32::LoadCursorW(ptr::null_mut(), 32512usize as *const u16),
        background,
        menu: ptr::null(),
        class: class.as_ptr(),
    };
    win32::RegisterClassW(&window_class) != 0
}

fn default_folder() -> Option<PathBuf> {
    plan::default_directory(
        win32::environment("LOCALAPPDATA").as_deref(),
        win32::environment("USERPROFILE").as_deref(),
    )
}

/// `/D=` wins, then a previous installation, then the per-user default.
fn install_directory(options: &Options) -> Option<PathBuf> {
    if let Some(path) = &options.directory {
        return Some(path.to_path_buf());
    }
    if let Some(path) = registered_directory() {
        if path.join("Uninstall.exe").is_file() {
            return Some(path);
        }
    }
    default_folder()
}

/// `/S`: install without any window or question. Failures are reported through
/// the exit code only, so unattended installs never block on a dialog.
fn install_silently(exe: &Path, options: &Options) -> i32 {
    let dir = match install_directory(options) {
        Some(dir) => dir,
        None => return 1,
    };
    let _com = win32::ComGuard::new().ok();
    match perform_install(&dir, exe, |_, _| {}) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn show_setup(exe: PathBuf, options: Options) -> i32 {
    unsafe {
        let instance = win32::GetModuleHandleW(ptr::null());
        if instance.is_null() {
            win32::message(
                ptr::null_mut(),
                "Could not start setup.",
                "ZFM Setup",
                win32::MB_ICONERROR,
            );
            return 1;
        }
        // Keep a COM apartment on the UI thread for the folder picker.
        let _com = win32::ComGuard::new().ok();
        let palette = match Palette::new() {
            Some(palette) => palette,
            None => {
                win32::message(
                    ptr::null_mut(),
                    "Could not initialize the setup interface.",
                    "ZFM Setup",
                    win32::MB_ICONERROR,
                );
                return 1;
            }
        };
        let dir = if options.uninstall {
            options.directory.clone().or_else(|| plan::uninstall_directory(exe.parent(), registered_directory().as_deref()))
        } else { install_directory(&options) }
        .map(|path| path_text(&path)).unwrap_or_default();
        let wizard = Box::into_raw(Box::new(Wizard {
            page: Page::Welcome,
            removing: options.uninstall,
            dir,
            status: String::new(),
            percent: 0,
            busy: false,
            license_ready: false,
            launch_checked: false,
            path_focused: false,
            hovered: ptr::null_mut(),
            backdrop: Backdrop::render(764, 536, ScenePage::Welcome),
            palette,
            heading: ptr::null_mut(),
            body: ptr::null_mut(),
            status_control: ptr::null_mut(),
            path_edit: ptr::null_mut(),
            browse: ptr::null_mut(),
            license: ptr::null_mut(),
            launch: ptr::null_mut(),
            back: ptr::null_mut(),
            next: ptr::null_mut(),
            cancel: ptr::null_mut(),
        }));
        WIZARD = wizard;
        let class = win32::text("ZFMSetupWindow");
        if !register_class(instance, (*wizard).palette.window_brush, &class) {
            win32::message(
                ptr::null_mut(),
                "Could not register the setup window.",
                "ZFM Setup",
                win32::MB_ICONERROR,
            );
            return 1;
        }
        let window = win32::CreateWindowExW(
            0,
            class.as_ptr(),
            win32::text(if options.uninstall { "ZFM File Manager — Uninstall" } else { "ZFM File Manager — Setup" }).as_ptr(),
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
            0x8000_0000u32 as i32,
            0x8000_0000u32 as i32,
            780,
            576,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            wizard as *mut std::ffi::c_void,
        );
        if window.is_null() {
            win32::message(
                ptr::null_mut(),
                "Could not create the setup window.",
                "ZFM Setup",
                win32::MB_ICONERROR,
            );
            return 1;
        }
        win32::set_icon(window, (*wizard).palette.logo);
        win32::apply_window_chrome(
            window,
            canvas::CAPTION_TINT.colorref(),
            canvas::BORDER_TINT.colorref(),
            canvas::HEADING_INK.colorref(),
        );
        if !create_controls(window, &mut *wizard) {
            win32::message(
                window,
                "Could not create the setup controls.",
                "ZFM Setup",
                win32::MB_ICONERROR,
            );
            return 1;
        }
        render(window, &mut *wizard);
        win32::ShowWindow(window, SW_SHOWNORMAL);
        win32::UpdateWindow(window);
        let mut message: Msg = std::mem::zeroed();
        loop {
            let status = win32::GetMessageW(&mut message, ptr::null_mut(), 0, 0);
            if status <= 0 {
                break;
            }
            if win32::IsDialogMessageW(window, &mut message) == 0 {
                win32::TranslateMessage(&message);
                win32::DispatchMessageW(&message);
            }
        }
        let _ = exe;
        0
    }
}

/// Entry point used by `main` on Windows.
pub fn main() -> i32 {
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    let arguments: Vec<std::ffi::OsString> = arguments.collect();
    // A silent run must never block on a dialog, not even to report bad usage.
    let silent_requested = arguments.iter().any(|argument| {
        let text = argument.to_string_lossy().to_ascii_lowercase();
        text == "/s" || text == "/silent"
    });
    let options = match plan::parse(arguments) {
        Ok(options) => options,
        Err(error) => {
            if !silent_requested {
                win32::message(ptr::null_mut(), &error, "ZFM Setup", win32::MB_ICONERROR);
            }
            return 2;
        }
    };
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            win32::message(
                ptr::null_mut(),
                &format!("Could not locate the setup program: {error}"),
                "ZFM Setup",
                win32::MB_ICONERROR,
            );
            return 1;
        }
    };
    let uninstaller = options.uninstall
        || exe
            .file_name()
            .map(|name| name.to_string_lossy().eq_ignore_ascii_case("uninstall.exe"))
            .unwrap_or(false);
    if uninstaller {
        // Schedule cleanup even if the user cancels or removal fails.
        if relocated_copy(&exe) && exe.parent().zip(win32::temporary_folder().as_deref())
            .map_or(false, |(parent, temp)| plan::same_directory(parent, temp)) {
            win32::delete_on_reboot(&exe);
        }
        if options.silent { return uninstall(&exe, &options); }
        let dir = options.directory.clone().or_else(|| plan::uninstall_directory(exe.parent(), registered_directory().as_deref()));
        let Some(dir) = dir else {
            win32::message(ptr::null_mut(), "Could not determine the installation folder.", "Uninstall ZFM", win32::MB_ICONERROR);
            return 1;
        };
        let inside = exe.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("Uninstall.exe")).unwrap_or(false)
            && exe.parent().map(|p| plan::same_directory(p, &dir)).unwrap_or(false);
        if inside {
            if relaunch_from_temp(&exe, &dir, false).is_some() { return 0; }
            win32::message(ptr::null_mut(), "Could not start the temporary uninstaller. No files were removed.", "Uninstall ZFM", win32::MB_ICONERROR);
            return 1;
        }
        return show_setup(exe, Options { uninstall: true, silent: false, directory: Some(dir) });
    }
    if options.silent {
        return install_silently(&exe, &options);
    }
    show_setup(exe, options)
}
