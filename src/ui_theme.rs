// Modified: compact reference-inspired file-manager layout and visuals.
//! Dark-slate visual theme, anti-aliased SDF rendering primitives, vector
//! icons, layout geometry, and table column formatters for the file manager.

use crate::directory::Entry;
use std::cmp::Ordering;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Win32 `COLORREF` (`0x00BBGGRR`).
    pub const fn colorref(self) -> u32 {
        (self.0 as u32) | ((self.1 as u32) << 8) | ((self.2 as u32) << 16)
    }

    /// Top-down 32-bit `BI_RGB` pixel (`0x00RRGGBB`).
    pub const fn pixel(self) -> u32 {
        ((self.0 as u32) << 16) | ((self.1 as u32) << 8) | (self.2 as u32)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let ch = |a: u8, b: u8| -> u8 { (a as f32 + (b as f32 - a as f32) * t + 0.5) as u8 };
        Self(ch(self.0, other.0), ch(self.1, other.1), ch(self.2, other.2))
    }
}

// Dark-slate palette calibrated to the ZFM reference design.
pub const CAPTION_BG: Rgb = Rgb(30, 37, 48);
pub const HEADER_TOP: Rgb = Rgb(27, 35, 44);
pub const HEADER_BOTTOM: Rgb = Rgb(34, 43, 53);
pub const TAB_STRIP_BG: Rgb = Rgb(33, 41, 51);
pub const TAB_ACTIVE_BG: Rgb = Rgb(46, 51, 58);
pub const TAB_INACTIVE_BG: Rgb = Rgb(29, 34, 40);
pub const DECK_TOP: Rgb = Rgb(46, 53, 62);
pub const DECK_BOTTOM: Rgb = Rgb(36, 40, 47);
pub const SIDEBAR_BG: Rgb = Rgb(28, 35, 39);
pub const SIDEBAR_HOVER: Rgb = Rgb(44, 49, 55);
pub const SIDEBAR_ACTIVE: Rgb = Rgb(53, 60, 69);
pub const SPLITTER_DARK: Rgb = Rgb(20, 22, 26);
pub const SPLITTER_LIGHT: Rgb = Rgb(86, 90, 96);
pub const COL_HEADER_BG: Rgb = Rgb(29, 30, 32);
pub const COL_HEADER_HOVER: Rgb = Rgb(46, 49, 54);
pub const ROW_EVEN: Rgb = Rgb(28, 28, 28);
pub const ROW_ODD: Rgb = Rgb(33, 33, 33);
pub const ROW_LINE: Rgb = Rgb(33, 33, 33);
pub const ROW_SELECTED_BG: Rgb = Rgb(59, 59, 61);
pub const ROW_SELECTED_BORDER: Rgb = Rgb(78, 84, 93);
pub const STATUS_BG: Rgb = Rgb(48, 49, 50);

pub const BORDER_SUBTLE: Rgb = Rgb(61, 67, 75);
pub const BORDER_STRONG: Rgb = Rgb(79, 87, 97);
pub const BORDER_FOCUS: Rgb = Rgb(92, 148, 208);

pub const BTN_TOP: Rgb = Rgb(55, 61, 69);
pub const BTN_BOTTOM: Rgb = Rgb(42, 47, 54);
pub const BTN_HOVER_TOP: Rgb = Rgb(68, 76, 86);
pub const BTN_HOVER_BOTTOM: Rgb = Rgb(52, 59, 68);
pub const BTN_PRESS_TOP: Rgb = Rgb(35, 39, 45);
pub const BTN_PRESS_BOTTOM: Rgb = Rgb(41, 46, 53);

pub const ADDRESS_CHIP_BG: Rgb = Rgb(42, 50, 58);
pub const ADDRESS_EDIT_BG: Rgb = Rgb(50, 60, 68);
pub const FILTER_EDIT_BG: Rgb = Rgb(49, 59, 68);
pub const NAME_EDIT_BG: Rgb = Rgb(31, 34, 38);

pub const TEXT_BRIGHT: Rgb = Rgb(236, 241, 246);
pub const TEXT_PRIMARY: Rgb = Rgb(216, 223, 231);
pub const TEXT_MUTED: Rgb = Rgb(154, 163, 174);
pub const TEXT_DIM: Rgb = Rgb(118, 127, 138);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl BoxRect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn right(self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(self) -> i32 {
        self.y + self.height
    }

    pub const fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Modified,
    Kind,
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionIcon {
    Back,
    Forward,
    Up,
    Root,
    Refresh,
    Go,
    NewTab,
    CloseTab,
    Search,
    NewFile,
    NewFolder,
    Rename,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemIcon {
    Desktop,
    Documents,
    Downloads,
    Music,
    Videos,
    Contacts,
    Favorites,
    Links,
    Drives,
    DriveDisk,
    Folder,
    FileGeneric,
    FileCode,
    FileArchive,
    FileMedia,
    FileExec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Details,
    Compact,
}

/// Complete client-area geometry for `ZFMNativeWindow`.
#[derive(Clone, Copy, Debug)]
pub struct WindowMetrics {
    pub width: i32,
    pub height: i32,
    pub brand_title: BoxRect,
    pub brand_subtitle: BoxRect,
    pub nav_back: BoxRect,
    pub nav_forward: BoxRect,
    pub nav_up: BoxRect,
    pub nav_root: BoxRect,
    pub nav_refresh: BoxRect,
    pub address_frame: BoxRect,
    pub address_chip: BoxRect,
    pub address_edit: BoxRect,
    pub nav_go: BoxRect,
    pub tab_strip_top: i32,
    pub deck_top: i32,
    pub actions_btn: BoxRect,
    pub new_tab: BoxRect,
    pub close_tab: BoxRect,
    pub dynamic_tabs_left: i32,
    pub dynamic_tabs_right: i32,
    pub filter_label: BoxRect,
    pub filter_frame: BoxRect,
    pub filter_edit: BoxRect,
    pub search_btn: BoxRect,
    pub clear_btn: BoxRect,
    pub hidden_btn: BoxRect,
    pub name_label: BoxRect,
    pub name_frame: BoxRect,
    pub name_edit: BoxRect,
    pub new_file_btn: BoxRect,
    pub new_folder_btn: BoxRect,
    pub rename_btn: BoxRect,
    pub delete_btn: BoxRect,
    pub split_top: i32,
    pub status_top: i32,
    pub sidebar: BoxRect,
    pub splitter_x: i32,
    pub col_header: BoxRect,
    pub col_name: BoxRect,
    pub col_modified: BoxRect,
    pub col_type: BoxRect,
    pub col_size: BoxRect,
    pub list: BoxRect,
    pub status_bar: BoxRect,
    pub view_details_btn: BoxRect,
    pub view_compact_btn: BoxRect,
}

impl WindowMetrics {
    pub fn new(client_width: i32, client_height: i32) -> Self {
        let width = client_width.max(720);
        let height = client_height.max(440);
        // Keep the navigation and command rows compact, like a classic
        // details-view file manager, while retaining all existing controls.
        let margin = 6;
        let right = width - margin;
        let brand_title = BoxRect::new(0, 0, 0, 0);
        let brand_subtitle = BoxRect::new(0, 0, 0, 0);

        let nav_y = 4;
        let nav_h = 26;
        let nav_w = 27;
        let nav_gap = 3;
        let nav_back = BoxRect::new(margin, nav_y, nav_w, nav_h);
        let nav_forward = BoxRect::new(margin + (nav_w + nav_gap), nav_y, nav_w, nav_h);
        let nav_up = BoxRect::new(margin + 2 * (nav_w + nav_gap), nav_y, nav_w, nav_h);
        let nav_root = BoxRect::new(margin + 3 * (nav_w + nav_gap), nav_y, nav_w, nav_h);
        let nav_refresh = BoxRect::new(margin + 4 * (nav_w + nav_gap), nav_y, nav_w, nav_h);

        let hidden_btn = BoxRect::new(right - 112, nav_y, 112, nav_h);
        let clear_btn = BoxRect::new(hidden_btn.x - 4 - 44, nav_y, 44, nav_h);
        let search_btn = BoxRect::new(clear_btn.x - 4 - 70, nav_y, 70, nav_h);
        let filter_w = (width * 16 / 100).clamp(110, 175);
        let filter_x = search_btn.x - 4 - filter_w;
        let filter_label = BoxRect::new(0, 0, 0, 0);
        let filter_frame = BoxRect::new(filter_x, nav_y, filter_w, nav_h);
        let filter_edit = BoxRect::new(filter_x + 8, nav_y + 4, filter_w - 32, 18);
        let nav_go = BoxRect::new(filter_x - 4 - 50, nav_y, 50, nav_h);
        let addr_x = nav_refresh.right() + 5;
        let addr_w = (nav_go.x - 5 - addr_x).max(140);
        let address_frame = BoxRect::new(addr_x, nav_y, addr_w, nav_h);
        let chip_w = 34;
        let address_chip = BoxRect::new(addr_x, nav_y, chip_w, nav_h);
        let address_edit = BoxRect::new(
            address_chip.right() + 8,
            nav_y + 4,
            (address_frame.right() - address_chip.right() - 14).max(40),
            nav_h - 8,
        );

        let tab_strip_top = 33;
        let deck_top = 61;
        let tab_y = 35;
        let tab_h = deck_top - tab_y;
        let actions_btn = BoxRect::new(margin, tab_y, 84, tab_h);
        let new_tab = BoxRect::new(actions_btn.right() + 4, tab_y, 90, tab_h);
        let close_tab = BoxRect::new(new_tab.right() + 4, tab_y, 92, tab_h);
        let dynamic_tabs_left = close_tab.right() + 8;
        let dynamic_tabs_right = right;

        let row_h = 26;
        let name_y = deck_top + 4;
        let delete_btn = BoxRect::new(right - 84, name_y, 84, row_h);
        let rename_btn = BoxRect::new(delete_btn.x - 4 - 84, name_y, 84, row_h);
        let new_folder_btn = BoxRect::new(rename_btn.x - 4 - 102, name_y, 102, row_h);
        let new_file_btn = BoxRect::new(new_folder_btn.x - 4 - 88, name_y, 88, row_h);
        let name_label = BoxRect::new(margin, name_y, 70, row_h);
        let name_x = name_label.right() + 4;
        let name_w = (new_file_btn.x - 6 - name_x).max(60);
        let name_frame = BoxRect::new(name_x, name_y, name_w, row_h);
        let name_edit = BoxRect::new(name_x + 8, name_y + 4, (name_w - 16).max(28), 18);

        let split_top = name_y + row_h + 4;
        let status_h = 48;
        let status_top = (height - status_h).max(split_top + 132);
        let body_h = (status_top - split_top).max(132);

        let sidebar_w = (width * 16 / 100).clamp(150, 190);
        let sidebar = BoxRect::new(0, split_top, sidebar_w, body_h);
        let splitter_x = sidebar_w;
        let table_x = sidebar_w + 4;
        let table_w = (width - table_x).max(200);
        let col_h = 24;
        let col_header = BoxRect::new(table_x, split_top, table_w, col_h);

        let scrollbar_pad = 18;
        let usable_w = (table_w - scrollbar_pad).max(180);
        let size_w = (usable_w * 13 / 100).clamp(76, 108);
        let type_w = (usable_w * 20 / 100).clamp(96, 168);
        let mod_w = (usable_w * 27 / 100).clamp(140, 214);
        let name_col_w = (usable_w - size_w - type_w - mod_w).max(120);

        let col_name = BoxRect::new(table_x, split_top, name_col_w, col_h);
        let col_modified = BoxRect::new(col_name.right(), split_top, mod_w, col_h);
        let col_type = BoxRect::new(col_modified.right(), split_top, type_w, col_h);
        let col_size = BoxRect::new(col_type.right(), split_top, size_w, col_h);

        let list = BoxRect::new(
            table_x,
            split_top + col_h,
            table_w,
            (status_top - (split_top + col_h)).max(100),
        );

        let status_bar = BoxRect::new(0, status_top, width, status_h);
        let view_compact_btn = BoxRect::new(width - 34, status_top + 12, 24, 24);
        let view_details_btn = BoxRect::new(width - 62, status_top + 12, 24, 24);

        Self {
            width,
            height,
            brand_title,
            brand_subtitle,
            nav_back,
            nav_forward,
            nav_up,
            nav_root,
            nav_refresh,
            address_frame,
            address_chip,
            address_edit,
            nav_go,
            tab_strip_top,
            deck_top,
            actions_btn,
            new_tab,
            close_tab,
            dynamic_tabs_left,
            dynamic_tabs_right,
            filter_label,
            filter_frame,
            filter_edit,
            search_btn,
            clear_btn,
            hidden_btn,
            name_label,
            name_frame,
            name_edit,
            new_file_btn,
            new_folder_btn,
            rename_btn,
            delete_btn,
            split_top,
            status_top,
            sidebar,
            splitter_x,
            col_header,
            col_name,
            col_modified,
            col_type,
            col_size,
            list,
            status_bar,
            view_details_btn,
            view_compact_btn,
        }
    }
}

pub fn blend_pixel(buf: &mut [u32], width: i32, x: i32, y: i32, color: Rgb, alpha: f32) {
    if x < 0 || y < 0 || x >= width || alpha <= 0.001 {
        return;
    }
    let idx = (y * width + x) as usize;
    if idx >= buf.len() {
        return;
    }
    if alpha >= 0.999 {
        buf[idx] = color.pixel();
        return;
    }
    let dst = buf[idx];
    let dr = ((dst >> 16) & 0xff) as f32;
    let dg = ((dst >> 8) & 0xff) as f32;
    let db = (dst & 0xff) as f32;
    let inv = 1.0 - alpha;
    let r = (dr * inv + color.0 as f32 * alpha + 0.5) as u8;
    let g = (dg * inv + color.1 as f32 * alpha + 0.5) as u8;
    let b = (db * inv + color.2 as f32 * alpha + 0.5) as u8;
    buf[idx] = Rgb(r, g, b).pixel();
}

pub fn fill_rect(buf: &mut [u32], stride: i32, height: i32, rect: BoxRect, color: Rgb) {
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = rect.right().min(stride);
    let y1 = rect.bottom().min(height);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let px = color.pixel();
    for y in y0..y1 {
        let start = (y * stride + x0) as usize;
        let end = (y * stride + x1) as usize;
        if end <= buf.len() {
            buf[start..end].fill(px);
        }
    }
}

pub fn fill_vgrad(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    rect: BoxRect,
    top_color: Rgb,
    bottom_color: Rgb,
) {
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = rect.right().min(stride);
    let y1 = rect.bottom().min(height);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let span = (rect.height - 1).max(1) as f32;
    for y in y0..y1 {
        let t = (y - rect.y) as f32 / span;
        let px = top_color.lerp(bottom_color, t).pixel();
        let start = (y * stride + x0) as usize;
        let end = (y * stride + x1) as usize;
        if end <= buf.len() {
            buf[start..end].fill(px);
        }
    }
}

pub fn sd_rounded_box(
    px: f32,
    py: f32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
) -> f32 {
    let cx = (left + right) * 0.5;
    let cy = (top + bottom) * 0.5;
    let hx = (right - left) * 0.5 - radius;
    let hy = (bottom - top) * 0.5 - radius;
    let qx = (px - cx).abs() - hx;
    let qy = (py - cy).abs() - hy;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - radius
}

pub fn paint_rounded_box(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    rect: BoxRect,
    radius: f32,
    top_fill: Rgb,
    bottom_fill: Rgb,
    border: Option<Rgb>,
) {
    let x0 = (rect.x - 1).max(0);
    let y0 = (rect.y - 1).max(0);
    let x1 = (rect.right() + 1).min(stride);
    let y1 = (rect.bottom() + 1).min(height);
    let l = rect.x as f32;
    let t = rect.y as f32;
    let r = rect.right() as f32;
    let b = rect.bottom() as f32;
    let span = (b - t).max(1.0);
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        let fill = top_fill.lerp(bottom_fill, ((py - t) / span).clamp(0.0, 1.0));
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let d = sd_rounded_box(px, py, l, t, r, b, radius);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_pixel(buf, stride, x, y, fill, cov);
            }
            if let Some(stroke) = border {
                let bcov = (1.0 - (d + 0.5).abs()).clamp(0.0, 1.0);
                if bcov > 0.0 {
                    blend_pixel(buf, stride, x, y, stroke, bcov);
                }
            }
        }
    }
}

/// Top-rounded browser/folder tab whose flat bottom sits cleanly on `rect.bottom()`.
pub fn paint_top_tab(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    rect: BoxRect,
    radius: f32,
    top_fill: Rgb,
    bottom_fill: Rgb,
    border: Rgb,
    active: bool,
) {
    let x0 = (rect.x - 1).max(0);
    let y0 = (rect.y - 1).max(0);
    let x1 = (rect.right() + 1).min(stride);
    let y1 = rect.bottom().min(height);
    let l = rect.x as f32;
    let t = rect.y as f32;
    let r = rect.right() as f32;
    // Extend bottom below clip so bottom corners are square.
    let b = (rect.bottom() + 12) as f32;
    let span = (rect.height).max(1) as f32;
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        let fill = top_fill.lerp(bottom_fill, ((py - t) / span).clamp(0.0, 1.0));
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let d = sd_rounded_box(px, py, l, t, r, b, radius);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_pixel(buf, stride, x, y, fill, cov);
            }
            let bcov = (1.0 - (d + 0.5).abs()).clamp(0.0, 1.0);
            if bcov > 0.0 {
                blend_pixel(buf, stride, x, y, border, bcov);
            }
        }
    }
    if !active && rect.bottom() > 0 && rect.bottom() <= height {
        fill_rect(
            buf,
            stride,
            height,
            BoxRect::new(rect.x, rect.bottom() - 1, rect.width, 1),
            BORDER_STRONG,
        );
    }
}

pub fn draw_segment_aa(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
    color: Rgb,
) {
    let half = thickness * 0.5;
    let pad = half.ceil() as i32 + 2;
    let min_x = (x0.min(x1) as i32 - pad).max(0);
    let max_x = (x0.max(x1) as i32 + pad).min(stride);
    let min_y = (y0.min(y1) as i32 - pad).max(0);
    let max_y = (y0.max(y1) as i32 + pad).min(height);
    let vx = x1 - x0;
    let vy = y1 - y0;
    let len_sq = vx * vx + vy * vy;
    for y in min_y..max_y {
        let py = y as f32 + 0.5;
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let t = if len_sq > 1e-5 {
                (((px - x0) * vx + (py - y0) * vy) / len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let dx = px - (x0 + vx * t);
            let dy = py - (y0 + vy * t);
            let dist = (dx * dx + dy * dy).sqrt() - half;
            let cov = (0.5 - dist).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_pixel(buf, stride, x, y, color, cov);
            }
        }
    }
}

pub fn draw_circle_aa(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    cx: f32,
    cy: f32,
    radius: f32,
    fill: Option<Rgb>,
    stroke: Option<(f32, Rgb)>,
) {
    let pad = radius.ceil() as i32 + 3;
    let min_x = (cx as i32 - pad).max(0);
    let max_x = (cx as i32 + pad).min(stride);
    let min_y = (cy as i32 - pad).max(0);
    let max_y = (cy as i32 + pad).min(height);
    for y in min_y..max_y {
        let py = y as f32 + 0.5;
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let d = ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt() - radius;
            if let Some(color) = fill {
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    blend_pixel(buf, stride, x, y, color, cov);
                }
            }
            if let Some((w, color)) = stroke {
                let cov = (w * 0.5 + 0.5 - d.abs()).clamp(0.0, 1.0);
                if cov > 0.0 {
                    blend_pixel(buf, stride, x, y, color, cov);
                }
            }
        }
    }
}

pub fn paint_chevron(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    cx: f32,
    cy: f32,
    down: bool,
    color: Rgb,
) {
    if down {
        draw_segment_aa(buf, stride, height, cx - 3.5, cy - 1.5, cx, cy + 2.0, 1.5, color);
        draw_segment_aa(buf, stride, height, cx, cy + 2.0, cx + 3.5, cy - 1.5, 1.5, color);
    } else {
        draw_segment_aa(buf, stride, height, cx - 1.5, cy - 3.5, cx + 2.0, cy, 1.5, color);
        draw_segment_aa(buf, stride, height, cx + 2.0, cy, cx - 1.5, cy + 3.5, 1.5, color);
    }
}

pub fn paint_sort_arrow(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    cx: f32,
    cy: f32,
    ascending: bool,
    color: Rgb,
) {
    if ascending {
        draw_segment_aa(buf, stride, height, cx - 4.0, cy + 2.0, cx, cy - 2.5, 1.6, color);
        draw_segment_aa(buf, stride, height, cx, cy - 2.5, cx + 4.0, cy + 2.0, 1.6, color);
    } else {
        draw_segment_aa(buf, stride, height, cx - 4.0, cy - 2.0, cx, cy + 2.5, 1.6, color);
        draw_segment_aa(buf, stride, height, cx, cy + 2.5, cx + 4.0, cy - 2.0, 1.6, color);
    }
}

pub fn paint_toggle_switch(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    x: i32,
    y: i32,
    on: bool,
    hovered: bool,
) {
    let w = 32;
    let h = 16;
    let track = if on {
        Rgb(58, 128, 206)
    } else if hovered {
        Rgb(52, 59, 68)
    } else {
        Rgb(39, 44, 51)
    };
    let border = if on {
        Rgb(92, 158, 232)
    } else {
        Rgb(90, 98, 108)
    };
    paint_rounded_box(
        buf,
        stride,
        height,
        BoxRect::new(x, y, w, h),
        8.0,
        track,
        track,
        Some(border),
    );
    let knob_x = if on {
        (x + w - 8) as f32
    } else {
        (x + 8) as f32
    };
    let knob_y = (y + h / 2) as f32;
    let knob_color = if on {
        Rgb(246, 250, 255)
    } else {
        Rgb(166, 175, 186)
    };
    draw_circle_aa(buf, stride, height, knob_x, knob_y, 5.2, Some(knob_color), None);
}

pub fn paint_view_icon(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    rect: BoxRect,
    mode: ViewMode,
    active: bool,
    hovered: bool,
) {
    if active || hovered {
        let fill = if active {
            Rgb(54, 61, 70)
        } else {
            Rgb(44, 49, 56)
        };
        let border = if active {
            Some(BORDER_STRONG)
        } else {
            Some(BORDER_SUBTLE)
        };
        paint_rounded_box(buf, stride, height, rect, 4.0, fill, fill, border);
    }
    let ink = if active { TEXT_BRIGHT } else { TEXT_MUTED };
    let x0 = rect.x + 5;
    let y0 = rect.y + 5;
    match mode {
        ViewMode::Details => {
            for row in 0..3 {
                let ry = y0 + 2 + row * 5;
                fill_rect(buf, stride, height, BoxRect::new(x0, ry, 3, 2), ink);
                fill_rect(buf, stride, height, BoxRect::new(x0 + 5, ry, 9, 2), ink);
            }
        }
        ViewMode::Compact => {
            for r in 0..2 {
                for c in 0..2 {
                    paint_rounded_box(
                        buf,
                        stride,
                        height,
                        BoxRect::new(x0 + c * 8, y0 + 1 + r * 7, 6, 5),
                        1.2,
                        ink,
                        ink,
                        None,
                    );
                }
            }
        }
    }
}

/// Paint crisp 16×16 toolbar/navigation vector icons centered around `(cx, cy)`.
pub fn paint_action_icon(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    cx: f32,
    cy: f32,
    icon: ActionIcon,
    ink: Rgb,
) {
    match icon {
        ActionIcon::Back => {
            draw_segment_aa(buf, stride, height, cx + 5.5, cy, cx - 5.0, cy, 1.7, ink);
            draw_segment_aa(buf, stride, height, cx - 5.0, cy, cx - 0.5, cy - 4.5, 1.7, ink);
            draw_segment_aa(buf, stride, height, cx - 5.0, cy, cx - 0.5, cy + 4.5, 1.7, ink);
        }
        ActionIcon::Forward => {
            draw_segment_aa(buf, stride, height, cx - 5.5, cy, cx + 5.0, cy, 1.7, ink);
            draw_segment_aa(buf, stride, height, cx + 5.0, cy, cx + 0.5, cy - 4.5, 1.7, ink);
            draw_segment_aa(buf, stride, height, cx + 5.0, cy, cx + 0.5, cy + 4.5, 1.7, ink);
        }
        ActionIcon::Up => {
            draw_segment_aa(buf, stride, height, cx, cy + 5.5, cx, cy - 5.0, 1.7, ink);
            draw_segment_aa(buf, stride, height, cx, cy - 5.0, cx - 4.5, cy - 0.5, 1.7, ink);
            draw_segment_aa(buf, stride, height, cx, cy - 5.0, cx + 4.5, cy - 0.5, 1.7, ink);
        }
        ActionIcon::Root => {
            draw_segment_aa(buf, stride, height, cx, cy + 3.0, cx, cy - 5.0, 1.7, ink);
            draw_segment_aa(buf, stride, height, cx, cy - 5.0, cx - 4.0, cy - 1.0, 1.7, ink);
            draw_segment_aa(buf, stride, height, cx, cy - 5.0, cx + 4.0, cy - 1.0, 1.7, ink);
            draw_segment_aa(buf, stride, height, cx - 5.5, cy + 5.5, cx + 5.5, cy + 5.5, 1.7, ink);
        }
        ActionIcon::Refresh => {
            let r = 5.2f32;
            let steps = 20;
            for i in 0..steps {
                let a0 = -2.4 + (i as f32 / steps as f32) * 4.8;
                let a1 = -2.4 + ((i + 1) as f32 / steps as f32) * 4.8;
                draw_segment_aa(
                    buf,
                    stride,
                    height,
                    cx + a0.cos() * r,
                    cy + a0.sin() * r,
                    cx + a1.cos() * r,
                    cy + a1.sin() * r,
                    1.6,
                    ink,
                );
            }
            let tip_x = cx + (-2.4f32).cos() * r;
            let tip_y = cy + (-2.4f32).sin() * r;
            draw_segment_aa(buf, stride, height, tip_x, tip_y, tip_x + 3.8, tip_y - 0.6, 1.6, ink);
            draw_segment_aa(buf, stride, height, tip_x, tip_y, tip_x + 0.8, tip_y + 3.8, 1.6, ink);
        }
        ActionIcon::Go => {
            draw_segment_aa(buf, stride, height, cx - 5.5, cy - 4.5, cx - 5.5, cy + 4.5, 1.6, ink);
            draw_segment_aa(buf, stride, height, cx - 2.5, cy, cx + 5.5, cy, 1.6, ink);
            draw_segment_aa(buf, stride, height, cx + 5.5, cy, cx + 1.8, cy - 3.6, 1.6, ink);
            draw_segment_aa(buf, stride, height, cx + 5.5, cy, cx + 1.8, cy + 3.6, 1.6, ink);
        }
        ActionIcon::NewTab => {
            let x = cx as i32 - 6;
            let y = cy as i32 - 5;
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x, y + 2, 12, 9),
                1.8,
                Rgb(52, 58, 66),
                Rgb(44, 50, 58),
                Some(ink),
            );
            fill_rect(buf, stride, height, BoxRect::new(x + 1, y, 5, 3), ink);
        }
        ActionIcon::CloseTab => {
            draw_segment_aa(buf, stride, height, cx - 4.0, cy - 4.0, cx + 4.0, cy + 4.0, 1.6, ink);
            draw_segment_aa(buf, stride, height, cx + 4.0, cy - 4.0, cx - 4.0, cy + 4.0, 1.6, ink);
        }
        ActionIcon::Search => {
            draw_circle_aa(buf, stride, height, cx - 1.2, cy - 1.2, 4.3, None, Some((1.6, ink)));
            draw_segment_aa(buf, stride, height, cx + 2.0, cy + 2.0, cx + 5.5, cy + 5.5, 1.8, ink);
        }
        ActionIcon::NewFile => {
            let x = cx as i32 - 5;
            let y = cy as i32 - 6;
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x, y, 10, 12),
                1.6,
                Rgb(44, 50, 58),
                Rgb(40, 45, 52),
                Some(ink),
            );
            draw_segment_aa(buf, stride, height, cx, cy - 2.2, cx, cy + 2.8, 1.4, ink);
            draw_segment_aa(buf, stride, height, cx - 2.5, cy + 0.3, cx + 2.5, cy + 0.3, 1.4, ink);
        }
        ActionIcon::NewFolder => {
            let x = cx as i32 - 6;
            let y = cy as i32 - 5;
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x, y + 2, 12, 9),
                1.6,
                Rgb(44, 50, 58),
                Rgb(40, 45, 52),
                Some(ink),
            );
            fill_rect(buf, stride, height, BoxRect::new(x + 1, y, 5, 2), ink);
            draw_segment_aa(buf, stride, height, cx, cy - 0.8, cx, cy + 3.2, 1.4, ink);
            draw_segment_aa(buf, stride, height, cx - 2.0, cy + 1.2, cx + 2.0, cy + 1.2, 1.4, ink);
        }
        ActionIcon::Rename => {
            draw_segment_aa(buf, stride, height, cx - 4.2, cy + 4.2, cx + 3.8, cy - 3.8, 2.2, ink);
            draw_segment_aa(buf, stride, height, cx - 5.2, cy + 5.2, cx - 3.2, cy + 4.6, 1.4, ink);
        }
        ActionIcon::Delete => {
            let x = cx as i32 - 5;
            let y = cy as i32 - 5;
            draw_segment_aa(buf, stride, height, cx - 5.5, cy - 3.5, cx + 5.5, cy - 3.5, 1.5, ink);
            draw_segment_aa(buf, stride, height, cx - 2.0, cy - 5.2, cx + 2.0, cy - 5.2, 1.5, ink);
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x + 1, y + 2, 8, 9),
                1.4,
                Rgb(44, 48, 55),
                Rgb(38, 42, 48),
                Some(ink),
            );
            draw_segment_aa(buf, stride, height, cx - 1.2, cy - 1.0, cx - 1.2, cy + 3.8, 1.1, ink);
            draw_segment_aa(buf, stride, height, cx + 1.2, cy - 1.0, cx + 1.2, cy + 3.8, 1.1, ink);
        }
    }
}

/// Paint colorful 18×18 sidebar and file-table icons matching the reference UI.
pub fn paint_item_icon(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    x: i32,
    y: i32,
    icon: ItemIcon,
) {
    match icon {
        ItemIcon::Desktop => {
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x + 1, y + 2, 16, 11),
                2.2,
                Rgb(52, 190, 255),
                Rgb(26, 128, 224),
                Some(Rgb(96, 210, 255)),
            );
            fill_rect(
                buf,
                stride,
                height,
                BoxRect::new(x + 7, y + 13, 4, 2),
                Rgb(154, 168, 184),
            );
            fill_rect(
                buf,
                stride,
                height,
                BoxRect::new(x + 5, y + 15, 8, 2),
                Rgb(180, 192, 206),
            );
        }
        ItemIcon::Documents => {
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x + 3, y + 1, 12, 16),
                2.0,
                Rgb(226, 232, 240),
                Rgb(182, 192, 206),
                Some(Rgb(142, 154, 170)),
            );
            for r in 0..3 {
                fill_rect(
                    buf,
                    stride,
                    height,
                    BoxRect::new(x + 5, y + 5 + r * 3, 8, 1),
                    Rgb(96, 108, 124),
                );
            }
        }
        ItemIcon::Downloads => {
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x + 2, y + 2, 14, 14),
                3.0,
                Rgb(44, 198, 138),
                Rgb(24, 150, 104),
                Some(Rgb(84, 222, 166)),
            );
            let cx = x as f32 + 9.0;
            let cy = y as f32 + 9.0;
            let w = Rgb(245, 252, 248);
            draw_segment_aa(buf, stride, height, cx, cy - 4.0, cx, cy + 2.2, 1.8, w);
            draw_segment_aa(buf, stride, height, cx, cy + 2.2, cx - 3.0, cy - 0.8, 1.8, w);
            draw_segment_aa(buf, stride, height, cx, cy + 2.2, cx + 3.0, cy - 0.8, 1.8, w);
            draw_segment_aa(buf, stride, height, cx - 3.8, cy + 4.2, cx + 3.8, cy + 4.2, 1.6, w);
        }
        ItemIcon::Music => {
            let cx = x as f32 + 9.0;
            let cy = y as f32 + 9.0;
            draw_circle_aa(
                buf,
                stride,
                height,
                cx,
                cy,
                7.6,
                Some(Rgb(222, 94, 102)),
                Some((1.0, Rgb(242, 132, 138))),
            );
            let w = Rgb(252, 246, 248);
            draw_segment_aa(buf, stride, height, cx - 1.5, cy + 2.5, cx - 1.5, cy - 3.5, 1.5, w);
            draw_segment_aa(buf, stride, height, cx + 3.0, cy + 1.8, cx + 3.0, cy - 4.2, 1.5, w);
            draw_segment_aa(buf, stride, height, cx - 1.5, cy - 3.5, cx + 3.0, cy - 4.2, 1.8, w);
            draw_circle_aa(buf, stride, height, cx - 2.6, cy + 2.8, 1.8, Some(w), None);
            draw_circle_aa(buf, stride, height, cx + 1.9, cy + 2.1, 1.8, Some(w), None);
        }
        ItemIcon::Videos => {
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x + 2, y + 2, 14, 14),
                3.0,
                Rgb(152, 98, 246),
                Rgb(108, 58, 214),
                Some(Rgb(182, 140, 255)),
            );
            let w = Rgb(248, 245, 255);
            let cx = x as f32 + 8.5;
            let cy = y as f32 + 9.0;
            draw_segment_aa(buf, stride, height, cx - 2.0, cy - 3.2, cx - 2.0, cy + 3.2, 1.8, w);
            draw_segment_aa(buf, stride, height, cx - 2.0, cy - 3.2, cx + 3.2, cy, 1.8, w);
            draw_segment_aa(buf, stride, height, cx - 2.0, cy + 3.2, cx + 3.2, cy, 1.8, w);
        }
        ItemIcon::Contacts => {
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x + 1, y + 3, 16, 12),
                2.5,
                Rgb(66, 165, 245),
                Rgb(34, 122, 208),
                Some(Rgb(112, 192, 255)),
            );
            let w = Rgb(242, 249, 255);
            draw_circle_aa(buf, stride, height, x as f32 + 9.0, y as f32 + 7.2, 2.2, Some(w), None);
            draw_segment_aa(
                buf,
                stride,
                height,
                x as f32 + 5.5,
                y as f32 + 12.2,
                x as f32 + 12.5,
                y as f32 + 12.2,
                2.2,
                w,
            );
        }
        ItemIcon::Favorites => {
            paint_folder_glyph(buf, stride, height, x, y, Rgb(246, 196, 78), Rgb(218, 152, 38));
            draw_circle_aa(
                buf,
                stride,
                height,
                x as f32 + 9.0,
                y as f32 + 10.0,
                2.3,
                Some(Rgb(255, 248, 214)),
                None,
            );
        }
        ItemIcon::Links => {
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x + 2, y + 2, 14, 14),
                3.0,
                Rgb(42, 188, 198),
                Rgb(24, 138, 152),
                Some(Rgb(96, 220, 228)),
            );
            let w = Rgb(244, 254, 255);
            let cx = x as f32 + 9.0;
            let cy = y as f32 + 9.0;
            draw_segment_aa(buf, stride, height, cx - 3.2, cy + 3.0, cx + 2.8, cy - 3.0, 1.8, w);
            draw_segment_aa(buf, stride, height, cx - 0.5, cy - 3.0, cx + 2.8, cy - 3.0, 1.8, w);
            draw_segment_aa(buf, stride, height, cx + 2.8, cy - 3.0, cx + 2.8, cy + 0.4, 1.8, w);
        }
        ItemIcon::Drives => {
            for idx in 0..2 {
                let ry = y + 2 + idx * 7;
                paint_rounded_box(
                    buf,
                    stride,
                    height,
                    BoxRect::new(x + 1, ry, 16, 6),
                    1.8,
                    Rgb(132, 148, 166),
                    Rgb(88, 102, 118),
                    Some(Rgb(164, 180, 198)),
                );
                draw_circle_aa(
                    buf,
                    stride,
                    height,
                    x as f32 + 13.5,
                    ry as f32 + 3.0,
                    1.2,
                    Some(Rgb(66, 184, 255)),
                    None,
                );
            }
        }
        ItemIcon::DriveDisk => {
            paint_rounded_box(
                buf,
                stride,
                height,
                BoxRect::new(x + 1, y + 5, 16, 8),
                2.0,
                Rgb(150, 162, 176),
                Rgb(94, 104, 116),
                Some(Rgb(182, 192, 204)),
            );
            fill_rect(
                buf,
                stride,
                height,
                BoxRect::new(x + 3, y + 9, 12, 2),
                Rgb(52, 58, 66),
            );
            draw_circle_aa(
                buf,
                stride,
                height,
                x as f32 + 13.2,
                y as f32 + 10.0,
                1.1,
                Some(Rgb(96, 206, 255)),
                None,
            );
        }
        ItemIcon::Folder => {
            paint_folder_glyph(buf, stride, height, x, y, Rgb(151, 156, 158), Rgb(111, 116, 119));
        }
        ItemIcon::FileCode => {
            paint_file_glyph(buf, stride, height, x, y, Rgb(72, 178, 242));
        }
        ItemIcon::FileArchive => {
            paint_file_glyph(buf, stride, height, x, y, Rgb(236, 168, 58));
        }
        ItemIcon::FileMedia => {
            paint_file_glyph(buf, stride, height, x, y, Rgb(172, 114, 248));
        }
        ItemIcon::FileExec => {
            paint_file_glyph(buf, stride, height, x, y, Rgb(86, 204, 152));
        }
        ItemIcon::FileGeneric => {
            paint_file_glyph(buf, stride, height, x, y, Rgb(142, 156, 174));
        }
    }
}

fn paint_folder_glyph(
    buf: &mut [u32],
    stride: i32,
    height: i32,
    x: i32,
    y: i32,
    front_top: Rgb,
    back_tab: Rgb,
) {
    paint_rounded_box(
        buf,
        stride,
        height,
        BoxRect::new(x + 1, y + 2, 7, 5),
        1.6,
        back_tab,
        back_tab,
        None,
    );
    paint_rounded_box(
        buf,
        stride,
        height,
        BoxRect::new(x + 1, y + 4, 16, 11),
        2.2,
        front_top,
        back_tab,
        Some(front_top.lerp(Rgb(230, 230, 230), 0.25)),
    );
}

fn paint_file_glyph(buf: &mut [u32], stride: i32, height: i32, x: i32, y: i32, accent: Rgb) {
    paint_rounded_box(
        buf,
        stride,
        height,
        BoxRect::new(x + 3, y + 1, 12, 16),
        2.0,
        Rgb(220, 228, 238),
        Rgb(176, 186, 200),
        Some(Rgb(136, 148, 164)),
    );
    fill_rect(buf, stride, height, BoxRect::new(x + 5, y + 5, 8, 2), accent);
    fill_rect(
        buf,
        stride,
        height,
        BoxRect::new(x + 5, y + 9, 8, 1),
        Rgb(102, 114, 130),
    );
    fill_rect(
        buf,
        stride,
        height,
        BoxRect::new(x + 5, y + 12, 6, 1),
        Rgb(102, 114, 130),
    );
}

pub fn classify_entry_icon(entry: &Entry) -> ItemIcon {
    if entry.is_dir {
        if entry.name.eq_ignore_ascii_case("desktop") {
            return ItemIcon::Desktop;
        }
        if entry.name.eq_ignore_ascii_case("documents") {
            return ItemIcon::Documents;
        }
        if entry.name.eq_ignore_ascii_case("downloads") {
            return ItemIcon::Downloads;
        }
        if entry.name.eq_ignore_ascii_case("music") {
            return ItemIcon::Music;
        }
        if entry.name.eq_ignore_ascii_case("videos") {
            return ItemIcon::Videos;
        }
        if entry.name.eq_ignore_ascii_case("contacts") {
            return ItemIcon::Contacts;
        }
        if entry.name.eq_ignore_ascii_case("favorites") {
            return ItemIcon::Favorites;
        }
        if entry.name.eq_ignore_ascii_case("links") {
            return ItemIcon::Links;
        }
        return ItemIcon::Folder;
    }

    let ext = Path::new(&entry.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "lnk" | "url" => ItemIcon::Links,
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "bz2" | "zst" => ItemIcon::FileArchive,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "mp3" | "wav"
        | "flac" | "ogg" | "mp4" | "mkv" | "webm" | "mov" => ItemIcon::FileMedia,
        "exe" | "msi" | "bat" | "cmd" | "ps1" | "sh" => ItemIcon::FileExec,
        "rs" | "py" | "js" | "ts" | "json" | "csv" | "toml" | "yaml" | "yml" | "xml" | "html"
        | "css" | "c" | "cpp" | "h" => ItemIcon::FileCode,
        "txt" | "md" | "doc" | "docx" | "pdf" => ItemIcon::Documents,
        _ => ItemIcon::FileGeneric,
    }
}

pub fn entry_type_label(entry: &Entry) -> String {
    if entry.is_dir {
        if entry.name.eq_ignore_ascii_case("links") {
            return "Folder".to_string();
        }
        return "File folder".to_string();
    }
    let ext = Path::new(&entry.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if ext.is_empty() {
        return "File".to_string();
    }
    let lower = ext.to_ascii_lowercase();
    match lower.as_str() {
        "lnk" => "Shortcut".to_string(),
        "exe" | "msi" | "com" => "Application".to_string(),
        "bat" | "cmd" => "Windows Batch File".to_string(),
        "txt" | "log" => "Text Document".to_string(),
        "md" => "Markdown Document".to_string(),
        "json" => "JSON File".to_string(),
        "csv" => "CSV File".to_string(),
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" => "Compressed Folder".to_string(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" => {
            format!("{} Image", lower.to_ascii_uppercase())
        }
        _ if ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric()) => {
            format!("{} File", ext.to_ascii_uppercase())
        }
        _ => "File".to_string(),
    }
}

pub fn format_human_size(entry: &Entry) -> String {
    if entry.is_dir {
        return String::new();
    }
    let bytes = match entry.size {
        Some(b) => b,
        None => return String::new(),
    };
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        let tenths = (bytes * 10 + GB / 2) / GB;
        format!("{}.{} GB", tenths / 10, tenths % 10)
    } else if bytes >= MB {
        let tenths = (bytes * 10 + MB / 2) / MB;
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else if bytes >= KB {
        format!("{} KB", (bytes + KB / 2) / KB)
    } else if bytes > 0 {
        "1 KB".to_string()
    } else {
        "0 KB".to_string()
    }
}

/// Convert UNIX seconds (`UTC` or pre-adjusted local seconds) into
/// `MM/DD/YYYY h:mm AM` format (`12/03/2022 1:35 AM`).
pub fn format_timestamp_us(epoch_seconds: i64) -> String {
    let days = epoch_seconds.div_euclid(86_400);
    let secs_of_day = epoch_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour24 = (secs_of_day / 3600) as u32;
    let minute = ((secs_of_day % 3600) / 60) as u32;
    let (hour12, ampm) = match hour24 {
        0 => (12, "AM"),
        1..=11 => (hour24, "AM"),
        12 => (12, "PM"),
        _ => (hour24 - 12, "PM"),
    };
    format!("{month:02}/{day:02}/{year:04} {hour12}:{minute:02} {ampm}")
}

pub fn format_entry_modified(modified: Option<SystemTime>, tz_offset_seconds: i64) -> String {
    let Some(ts) = modified else {
        return String::new();
    };
    let unix = match ts.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(err) => -(err.duration().as_secs() as i64),
    };
    format_timestamp_us(unix + tz_offset_seconds)
}

fn civil_from_days(days_since_1970: i64) -> (i32, u32, u32) {
    let z = days_since_1970 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (year, m, d)
}

pub fn sort_entries(entries: &mut [Entry], column: SortColumn, ascending: bool) {
    entries.sort_by(|a, b| {
        // Keep folders grouped above files regardless of column or direction.
        match (a.is_dir, b.is_dir) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
        let primary = match column {
            SortColumn::Name => a
                .name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name)),
            SortColumn::Modified => a
                .modified
                .cmp(&b.modified)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            SortColumn::Kind => entry_type_label(a)
                .to_lowercase()
                .cmp(&entry_type_label(b).to_lowercase())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            SortColumn::Size => a
                .size
                .unwrap_or(0)
                .cmp(&b.size.unwrap_or(0))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        };
        if ascending {
            primary
        } else {
            primary.reverse()
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn sample_entry(name: &str, is_dir: bool, size: Option<u64>, secs: u64) -> Entry {
        Entry {
            path: PathBuf::from(name),
            name: name.to_string(),
            is_dir,
            is_file: !is_dir,
            size,
            modified: Some(UNIX_EPOCH + Duration::from_secs(secs)),
        }
    }

    #[test]
    fn rgb_encodings_match_win32_and_dib_layouts() {
        let slate = CAPTION_BG;
        assert_eq!(slate.colorref(), 0x0030_251e);
        assert_eq!(slate.pixel(), 0x001e_2530);
    }

    #[test]
    fn window_metrics_preserve_toolbar_and_list_clearance() {
        for (w, h) in [(720, 480), (1024, 600), (1440, 900)] {
            let m = WindowMetrics::new(w, h);
            assert!(m.actions_btn.right() <= m.new_tab.x);
            assert!(m.new_tab.right() <= m.close_tab.x);
            assert!(m.close_tab.right() <= m.dynamic_tabs_left);
            assert!(m.dynamic_tabs_left < m.dynamic_tabs_right);
            assert!(m.list.y > m.name_edit.bottom());
            assert!(m.address_frame.right() <= m.nav_go.x);
            assert!(m.nav_go.right() <= m.filter_frame.x);
            assert!(m.filter_frame.right() <= m.search_btn.x);
            assert!(m.search_btn.right() <= m.clear_btn.x);
            assert!(m.clear_btn.right() <= m.hidden_btn.x);
            assert!(m.name_frame.right() <= m.new_file_btn.x);
            assert!(m.new_folder_btn.right() <= m.rename_btn.x);
            assert!(m.split_top <= 100, "chrome should leave room for files");
            assert!(m.list.height >= 100);
            assert!(m.sidebar.right() <= m.col_header.x);
            assert!(m.col_name.right() <= m.col_modified.x);
            assert!(m.col_modified.right() <= m.col_type.x);
            assert!(m.col_type.right() <= m.col_size.x);
            assert!(m.col_size.right() <= m.list.right());
        }
    }

    #[test]
    fn timestamp_size_and_type_formatters_match_reference() {
        // 2022-12-03 01:35:00 UTC = 1670031300
        assert_eq!(format_timestamp_us(1_670_031_300), "12/03/2022 1:35 AM");
        // 2022-12-03 13:54:00 UTC = 1670075640
        assert_eq!(format_timestamp_us(1_670_075_640), "12/03/2022 1:54 PM");

        let dir = sample_entry("Documents", true, None, 0);
        assert_eq!(entry_type_label(&dir), "File folder");
        assert_eq!(format_human_size(&dir), "");
        assert_eq!(classify_entry_icon(&dir), ItemIcon::Documents);

        let file = sample_entry("archive.zip", false, Some(94_208), 0);
        assert_eq!(entry_type_label(&file), "Compressed Folder");
        assert_eq!(format_human_size(&file), "92 KB");
        assert_eq!(classify_entry_icon(&file), ItemIcon::FileArchive);
    }

    #[test]
    fn sort_entries_keeps_folders_first_across_columns() {
        let mut entries = vec![
            sample_entry("zeta.txt", false, Some(100), 10),
            sample_entry("beta", true, None, 5),
            sample_entry("alpha.txt", false, Some(5000), 30),
            sample_entry("amedia", true, None, 20),
        ];
        sort_entries(&mut entries, SortColumn::Size, false);
        assert_eq!(entries[0].name, "beta");
        assert_eq!(entries[1].name, "amedia");
        assert_eq!(entries[2].name, "alpha.txt");
        assert_eq!(entries[3].name, "zeta.txt");
    }

    #[test]
    fn vector_icons_and_controls_render_visible_pixels() {
        let w = 48;
        let h = 48;
        let mut buf = vec![ROW_EVEN.pixel(); (w * h) as usize];
        paint_item_icon(&mut buf, w, h, 4, 4, ItemIcon::Desktop);
        paint_action_icon(&mut buf, w, h, 32.0, 14.0, ActionIcon::Refresh, TEXT_BRIGHT);
        paint_toggle_switch(&mut buf, w, h, 8, 28, true, false);
        assert!(buf.iter().any(|&px| px != ROW_EVEN.pixel()));
    }
}
