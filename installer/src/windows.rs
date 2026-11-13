//! ZFM setup: a black-and-white wizard, a silent installer for automation, and
//! the uninstaller that the installer writes next to the application.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::ptr;
use std::thread;

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
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_BORDER: u32 = 0x0080_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const ES_MULTILINE: u32 = 0x0004;
const ES_AUTOVSCROLL: u32 = 0x0040;
const ES_AUTOHSCROLL: u32 = 0x0080;
const ES_READONLY: u32 = 0x0800;
const SS_EDITCONTROL: u32 = 0x0000_2000;
const BS_OWNERDRAW: u32 = 0x0000_000b;
const BS_AUTOCHECKBOX: u32 = 0x0000_0003;
const BM_GETCHECK: u32 = 0x00f0;
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

// COLORREF is 0x00BBGGRR; the setup wizard uses the same palette as the
// application: paper-white surfaces, near-black ink and solid black actions.
const BACKGROUND: u32 = 0x00ff_ffff;
const INK: u32 = 0x000a_0a0a;
const MUTED: u32 = 0x005a_5a5a;
const BUTTON_BG: u32 = 0x00f5_f5f5;
const BUTTON_DOWN: u32 = 0x00de_dede;
const ACCENT_DOWN: u32 = 0x002e_2e2e;

// The "Z" monogram from packaging/zfm.svg, in the artwork's 128-unit grid.
const Z_POINTS: [(i32, i32); 10] = [
    (30, 33),
    (98, 33),
    (98, 51),
    (62, 77),
    (98, 77),
    (98, 95),
    (30, 95),
    (30, 77),
    (66, 51),
    (30, 51),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Welcome,
    Folder,
    License,
    Installing,
    Finish,
}

struct Palette {
    background: Hbrush,
    surface: Hbrush,
    button: Hbrush,
    pressed: Hbrush,
    accent: Hbrush,
    accent_pressed: Hbrush,
    border: Hbrush,
    ink: Hbrush,
    title_font: Hfont,
    body_font: Hfont,
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
            background: win32::CreateSolidBrush(BACKGROUND),
            surface: win32::CreateSolidBrush(BACKGROUND),
            button: win32::CreateSolidBrush(BUTTON_BG),
            pressed: win32::CreateSolidBrush(BUTTON_DOWN),
            accent: win32::CreateSolidBrush(INK),
            accent_pressed: win32::CreateSolidBrush(ACCENT_DOWN),
            border: win32::CreateSolidBrush(INK),
            ink: win32::CreateSolidBrush(INK),
            title_font: font(-26, 700),
            body_font: font(-16, 400),
            logo: win32::icon_from_png(payload::LOGO),
        };
        if [
            palette.background,
            palette.surface,
            palette.button,
            palette.pressed,
            palette.accent,
            palette.accent_pressed,
            palette.border,
            palette.ink,
            palette.title_font,
            palette.body_font,
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
            for object in [
                self.background,
                self.surface,
                self.button,
                self.pressed,
                self.accent,
                self.accent_pressed,
                self.border,
                self.ink,
            ] {
                if !object.is_null() {
                    win32::DeleteObject(object);
                }
            }
            for font in [self.title_font, self.body_font] {
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
    dir: String,
    status: String,
    percent: i32,
    busy: bool,
    license_ready: bool,
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

struct Done {
    result: Result<(), String>,
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

fn install_uninstaller(dir: &Path, exe: &Path) -> Result<(), String> {
    let target = dir.join("Uninstall.exe");
    fs::copy(exe, &target)
        .map_err(|error| format!("Could not install {}: {error}", target.display()))?;
    Ok(())
}

fn perform_install<F>(dir: &Path, exe: &Path, mut report: F) -> Result<(), String>
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

    let outcome = (|| -> Result<(), String> {
        report(20, "Copying the application…");
        payload::install_files(dir)?;
        report(55, "Installing the uninstaller…");
        install_uninstaller(dir, exe)?;
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
fn relaunch_from_temp(exe: &Path, dir: &Path) -> Option<()> {
    let temp = win32::temporary_folder()?;
    // One fixed name keeps at most one copy behind; a concurrent uninstall
    // simply falls back to the in-place removal below.
    let copy = temp.join("zfm-uninstall.exe");
    // Remove copies left behind by earlier uninstalls; one run needs one copy.
    if let Ok(entries) = fs::read_dir(&temp) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("zfm-uninstall") && name.ends_with(".exe") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    fs::copy(exe, &copy).ok()?;
    // The parent has already asked or was started silently, so the copy never
    // prompts. It also receives the folder, so it never depends on the registry
    // entry it is about to remove.
    let started = std::process::Command::new(&copy)
        .arg("/UNINSTALL")
        .arg("/S")
        .arg(format!("/D={}", dir.display()))
        .spawn();
    match started {
        Ok(_) => Some(()),
        Err(_) => {
            let _ = fs::remove_file(&copy);
            None
        }
    }
}

/// Wait until the original uninstaller has exited, then delete its file. Only
/// a relocated copy calls this; the file is still locked while the process that
/// started us is shutting down.
fn remove_original_uninstaller(dir: &Path) {
    let original = dir.join("Uninstall.exe");
    for _ in 0..100 {
        match fs::remove_file(&original) {
            Ok(()) => return,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(_) => unsafe { win32::Sleep(100) },
        }
    }
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
        if relaunch_from_temp(exe, &dir).is_some() {
            return 0;
        }
    } else if relocated_copy(exe) {
        remove_original_uninstaller(&dir);
    }

    let mut problems = Vec::new();
    problems.extend(remove_shortcuts());
    problems.extend(remove_registry());
    problems.extend(payload::remove_files_except(
        &dir,
        if inside { Some("Uninstall.exe") } else { None },
    ));
    if inside || relocated_copy(exe) {
        // The running copy can only be removed at the next restart.
        win32::delete_on_reboot(exe);
    }
    payload::remove_folder_if_empty(&dir);

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
    let heading = child(hwnd, "STATIC", "", 0, HEADING);
    let body = child(hwnd, "STATIC", "", SS_EDITCONTROL, BODY);
    let status_control = child(hwnd, "STATIC", "", SS_EDITCONTROL, STATUS);
    let path_edit = child(
        hwnd,
        "EDIT",
        "",
        WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL,
        PATH_EDIT,
    );
    let browse = child(
        hwnd,
        "BUTTON",
        "Browse…",
        WS_TABSTOP | BS_OWNERDRAW,
        BROWSE,
    );
    let license = child(
        hwnd,
        "EDIT",
        "",
        WS_BORDER | WS_VSCROLL | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL,
        LICENSE_EDIT,
    );
    let launch = child(
        hwnd,
        "BUTTON",
        "Launch ZFM now",
        WS_TABSTOP | BS_AUTOCHECKBOX,
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

fn place(control: Hwnd, x: i32, y: i32, width: i32, height: i32) {
    if control.is_null() {
        return;
    }
    unsafe {
        win32::MoveWindow(control, x, y, width.max(1), height.max(1), 1);
    }
}

unsafe fn layout(hwnd: Hwnd, wizard: &Wizard) {
    let mut client = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    win32::GetClientRect(hwnd, &mut client);
    let width = client.right.max(480);
    let height = client.bottom.max(360);
    let footer = height - 68;
    place(wizard.heading, 92, 25, width - 116, 38);
    place(wizard.body, 24, 100, width - 48, 68);
    let content = 176;
    place(wizard.path_edit, 24, content + 6, width - 168, 30);
    place(wizard.browse, width - 132, content + 6, 108, 30);
    place(wizard.license, 24, content, width - 48, footer - content - 16);
    let (status_top, status_height) = match wizard.page {
        Page::Folder => {
            let top = content + 48;
            (top, (footer - 12 - top).max(48))
        }
        Page::Finish => (content + 8, 48),
        _ => (content + 8, 28),
    };
    place(wizard.status_control, 24, status_top, width - 48, status_height);
    place(wizard.launch, 24, content + 68, width - 48, 26);
    place(wizard.cancel, width - 116, footer + 12, 92, 32);
    place(wizard.next, width - 236, footer + 12, 108, 32);
    place(wizard.back, width - 340, footer + 12, 92, 32);
}

unsafe fn draw_logo(dc: Hdc, wizard: &Wizard, x: i32, y: i32, size: i32) {
    let old_brush = win32::SelectObject(dc, wizard.palette.ink);
    let old_pen = win32::SelectObject(dc, win32::GetStockObject(win32::NULL_PEN));
    let corner = size * 61 / 128;
    win32::RoundRect(dc, x, y, x + size, y + size, corner, corner);
    let border = (size * 9 / 128).max(2);
    win32::SelectObject(dc, wizard.palette.background);
    let inner_corner = size * 43 / 128;
    win32::RoundRect(
        dc,
        x + border,
        y + border,
        x + size - border,
        y + size - border,
        inner_corner,
        inner_corner,
    );
    win32::SelectObject(dc, wizard.palette.ink);
    let mut points: Vec<win32::Point> = Z_POINTS
        .iter()
        .map(|(left, top)| win32::Point {
            x: x + left * size / 128,
            y: y + top * size / 128,
        })
        .collect();
    win32::Polygon(dc, points.as_mut_ptr(), points.len() as i32);
    win32::SelectObject(dc, old_brush);
    win32::SelectObject(dc, old_pen);
}

unsafe fn draw_progress(dc: Hdc, wizard: &Wizard, x: i32, y: i32, width: i32, height: i32) {
    let outer = Rect {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    win32::FrameRect(dc, &outer, wizard.palette.border);
    let background = Rect {
        left: x + 1,
        top: y + 1,
        right: x + width - 1,
        bottom: y + height - 1,
    };
    win32::FillRect(dc, &background, wizard.palette.background);
    let percent = wizard.percent.clamp(0, 100);
    let filled = (width - 4) * percent / 100;
    if filled > 0 {
        let inner = Rect {
            left: x + 2,
            top: y + 2,
            right: x + 2 + filled,
            bottom: y + height - 2,
        };
        win32::FillRect(dc, &inner, wizard.palette.ink);
    }
}

unsafe fn paint(hwnd: Hwnd, wizard: &Wizard) {
    let mut paint: PaintStruct = std::mem::zeroed();
    let dc = win32::BeginPaint(hwnd, &mut paint);
    let mut client = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    win32::GetClientRect(hwnd, &mut client);
    let width = client.right;
    let height = client.bottom;
    draw_logo(dc, wizard, 24, 18, 52);
    let rule = Rect {
        left: 24,
        top: 88,
        right: width - 24,
        bottom: 90,
    };
    win32::FillRect(dc, &rule, wizard.palette.border);
    let footer = Rect {
        left: 24,
        top: height - 68,
        right: width - 24,
        bottom: height - 66,
    };
    win32::FillRect(dc, &footer, wizard.palette.border);
    if wizard.page == Page::Installing {
        draw_progress(dc, wizard, 24, 224, width - 48, 18);
    }
    win32::EndPaint(hwnd, &paint);
}

unsafe fn draw_button(item: &mut DrawItem, wizard: &Wizard) -> LResult {
    let primary = item.control_id as usize == NEXT;
    let disabled = item.item_state & 4 != 0;
    let pressed = item.item_state & 1 != 0;
    let background = if disabled {
        wizard.palette.button
    } else if primary {
        if pressed {
            wizard.palette.accent_pressed
        } else {
            wizard.palette.accent
        }
    } else if pressed {
        wizard.palette.pressed
    } else {
        wizard.palette.button
    };
    win32::FillRect(item.dc, &item.rect, background);
    if !primary || disabled {
        win32::FrameRect(item.dc, &item.rect, wizard.palette.border);
    }
    win32::SetBkMode(item.dc, 1);
    win32::SetTextColor(
        item.dc,
        if disabled {
            MUTED
        } else if primary {
            BACKGROUND
        } else {
            INK
        },
    );
    let label = win32::text(&win32::window_text(item.hwnd));
    let mut bounds = Rect {
        left: item.rect.left + 4,
        top: item.rect.top,
        right: item.rect.right - 4,
        bottom: item.rect.bottom,
    };
    let old_font = win32::SelectObject(item.dc, wizard.palette.body_font);
    win32::DrawTextW(
        item.dc,
        label.as_ptr(),
        -1,
        &mut bounds,
        win32::DT_CENTER | win32::DT_VCENTER | win32::DT_SINGLELINE | win32::DT_NOPREFIX,
    );
    win32::SelectObject(item.dc, old_font);
    1
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
            "ZFM is installed in the folder below. Setup adds a Start Menu shortcut, an entry in Programs and Features and an uninstaller. Other files already in the folder are left untouched.",
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
    win32::set_text(wizard.heading, heading);
    win32::set_text(wizard.body, body);
    win32::set_text(wizard.status_control, &wizard.status);
    if page == Page::Folder {
        win32::set_text(wizard.path_edit, &wizard.dir);
    }
    if !wizard.license_ready {
        win32::set_text(wizard.license, &crlf(payload::LICENSE));
        wizard.license_ready = true;
    }
    win32::show(wizard.path_edit, page == Page::Folder);
    win32::show(wizard.browse, page == Page::Folder);
    win32::show(wizard.license, page == Page::License);
    win32::show(wizard.launch, page == Page::Finish);
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
            Page::License => "Install",
            Page::Finish => "Finish",
            _ => "Next",
        },
    );
    win32::EnableWindow(wizard.next, if wizard.busy { 0 } else { 1 });
    win32::EnableWindow(wizard.back, if wizard.busy { 0 } else { 1 });
    win32::EnableWindow(wizard.cancel, if wizard.busy { 0 } else { 1 });
    layout(hwnd, wizard);
    win32::InvalidateRect(hwnd, ptr::null(), 1);
}

unsafe fn begin_install(hwnd: Hwnd, wizard: &mut Wizard) {
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
        let result = perform_install(&dir, &exe, |percent, text| {
            let message = Box::into_raw(Box::new(Progress {
                percent,
                text: text.to_string(),
            }));
            unsafe {
                if win32::PostMessageW(window as Hwnd, WM_PROGRESS, 0, message as LParam) == 0 {
                    drop(Box::from_raw(message));
                }
            }
        });
        let message = Box::into_raw(Box::new(Done { result }));
        unsafe {
            if win32::PostMessageW(window as Hwnd, WM_DONE, 0, message as LParam) == 0 {
                drop(Box::from_raw(message));
            }
        }
    });
}

unsafe fn finish(hwnd: Hwnd, wizard: &mut Wizard) {
    if win32::SendMessageW(wizard.launch, BM_GETCHECK, 0, 0) == 1 {
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
        Page::License => begin_install(hwnd, wizard),
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
    }
}

unsafe fn command(hwnd: Hwnd, wizard: &mut Wizard, wparam: WParam) -> Option<String> {
    let id = wparam & 0xffff;
    let code = (wparam >> 16) & 0xffff;
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
        _ => {}
    }
    None
}

unsafe fn completed(hwnd: Hwnd, wizard: &mut Wizard, lparam: LParam) -> Option<String> {
    let message = Box::from_raw(lparam as *mut Done);
    wizard.busy = false;
    match message.result {
        Ok(()) => {
            wizard.percent = 100;
            wizard.status = format!("Installed in {}", wizard.dir);
            wizard.page = Page::Finish;
            render(hwnd, wizard);
            None
        }
        Err(error) => {
            wizard.page = Page::Folder;
            wizard.status = error.clone();
            render(hwnd, wizard);
            Some(format!("Setup could not finish:\n\n{error}"))
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
        WM_COMMAND => {
            failure = command(hwnd, wizard, wparam);
            0
        }
        WM_DRAWITEM if lparam != 0 => {
            let item = &mut *(lparam as *mut DrawItem);
            draw_button(item, wizard)
        }
        WM_CTLCOLORSTATIC => {
            win32::SetTextColor(wparam as Hdc, INK);
            win32::SetBkColor(wparam as Hdc, BACKGROUND);
            wizard.palette.background as LResult
        }
        WM_CTLCOLOREDIT => {
            win32::SetTextColor(wparam as Hdc, INK);
            win32::SetBkColor(wparam as Hdc, BACKGROUND);
            wizard.palette.surface as LResult
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
                win32::InvalidateRect(hwnd, ptr::null(), 0);
            }
            0
        }
        WM_DONE => {
            if lparam != 0 {
                failure = completed(hwnd, wizard, lparam);
            }
            0
        }
        _ => win32::DefWindowProcW(hwnd, message, wparam, lparam),
    };
    if let Some(error) = failure {
        win32::message(hwnd, &error, "ZFM Setup", win32::MB_ICONERROR);
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
        let dir = install_directory(&options)
            .map(|path| path_text(&path))
            .unwrap_or_default();
        let wizard = Box::into_raw(Box::new(Wizard {
            page: Page::Welcome,
            dir,
            status: String::new(),
            percent: 0,
            busy: false,
            license_ready: false,
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
        if !register_class(instance, (*wizard).palette.background, &class) {
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
            win32::text("ZFM File Manager — Setup").as_ptr(),
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            0x8000_0000u32 as i32,
            0x8000_0000u32 as i32,
            676,
            500,
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
        return uninstall(&exe, &options);
    }
    if options.silent {
        return install_silently(&exe, &options);
    }
    show_setup(exe, options)
}
