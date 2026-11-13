// Only the Windows setup wizard blits these buffers to a window; other hosts
// compile the module so the geometry and rendering unit tests run everywhere.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

//! Software-rendered, anti-aliased surfaces and controls for the setup wizard.
//!
//! Plain GDI primitives (`RoundRect`, `Polygon`) have no anti-aliasing, no
//! gradients and no soft shadows. Rendering into a 32-bit pixel buffer with
//! signed-distance fields produces smooth pill buttons, the layered geometric
//! header banner, the subtle background monogram and soft shadows on Windows 7
//! through Windows 11 without any external crate or runtime dependency.

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
        let inv = 1.0 - t;
        Self(
            (self.0 as f32 * inv + other.0 as f32 * t + 0.5) as u8,
            (self.1 as f32 * inv + other.1 as f32 * t + 0.5) as u8,
            (self.2 as f32 * inv + other.2 as f32 * t + 0.5) as u8,
        )
    }
}

pub const CAPTION_TINT: Rgb = Rgb(174, 192, 209);
pub const BORDER_TINT: Rgb = Rgb(152, 172, 192);
pub const HEADING_INK: Rgb = Rgb(17, 22, 28);
pub const BODY_INK: Rgb = Rgb(22, 28, 36);
pub const STATUS_INK: Rgb = Rgb(37, 50, 65);
pub const ERROR_INK: Rgb = Rgb(155, 34, 38);
pub const EDIT_SURFACE: Rgb = Rgb(255, 255, 255);
pub const PRIMARY_TEXT: Rgb = Rgb(255, 255, 255);
pub const SECONDARY_TEXT: Rgb = Rgb(21, 26, 33);
pub const DISABLED_TEXT: Rgb = Rgb(132, 140, 150);

const CARD_SURFACE: Rgb = Rgb(250, 252, 254);
const WATERMARK: Rgb = Rgb(242, 245, 250);

// The "Z" monogram from packaging/zfm.svg, in the artwork's 128-unit grid.
const Z_POINTS: [(f32, f32); 10] = [
    (30.0, 33.0),
    (98.0, 33.0),
    (98.0, 51.0),
    (62.0, 77.0),
    (98.0, 77.0),
    (98.0, 95.0),
    (30.0, 95.0),
    (30.0, 77.0),
    (66.0, 51.0),
    (30.0, 51.0),
];

/// Padding around each pill button's logical bounds so its drop shadow fits
/// inside the owner-drawn button rectangle without clipping.
pub const PILL_PAD_X: i32 = 6;
pub const PILL_PAD_TOP: i32 = 4;
pub const PILL_PAD_BOTTOM: i32 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenePage {
    Welcome,
    Folder { focused: bool },
    License,
    Installing { percent: i32 },
    Finish,
}

#[derive(Clone, Copy, Debug)]
pub struct BoxRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl BoxRect {
    pub const fn right(self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(self) -> i32 {
        self.y + self.height
    }

    pub const fn with_pill_padding(self) -> Self {
        Self {
            x: self.x - PILL_PAD_X,
            y: self.y - PILL_PAD_TOP,
            width: self.width + PILL_PAD_X * 2,
            height: self.height + PILL_PAD_TOP + PILL_PAD_BOTTOM,
        }
    }
}

/// Shared layout geometry for both the background renderer and `MoveWindow`.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub width: i32,
    pub height: i32,
    pub card_left: i32,
    pub card_top: i32,
    pub card_right: i32,
    pub card_bottom: i32,
    pub header_height: i32,
    pub footer_top: i32,
    pub heading: BoxRect,
    pub body: BoxRect,
    pub input_card: BoxRect,
    pub path_edit: BoxRect,
    pub browse_pill: BoxRect,
    pub license_card: BoxRect,
    pub license_edit: BoxRect,
    pub progress_bar: BoxRect,
    pub folder_status: BoxRect,
    pub progress_status: BoxRect,
    pub finish_status: BoxRect,
    pub launch: BoxRect,
    pub back_pill: BoxRect,
    pub next_pill: BoxRect,
    pub cancel_pill: BoxRect,
}

impl Metrics {
    pub fn new(client_width: i32, client_height: i32) -> Self {
        let width = client_width.max(540);
        let height = client_height.max(420);
        let card_left = 6;
        let card_top = 4;
        let card_right = width - 6;
        let card_bottom = height - 6;
        let card_width = card_right - card_left;
        let header_height = 88;
        let footer_top = card_bottom - 72;

        let margin_x = card_left + 28;
        let content_width = card_width - 56;

        let heading = BoxRect {
            x: card_left + 102,
            y: card_top + 22,
            width: (card_right - 24) - (card_left + 102),
            height: 44,
        };
        let body = BoxRect {
            x: margin_x,
            y: card_top + header_height + 22,
            width: content_width,
            height: 72,
        };

        let browse_pill = BoxRect {
            x: card_right - 28 - 124,
            y: body.bottom() + 14,
            width: 124,
            height: 40,
        };
        let input_card = BoxRect {
            x: margin_x,
            y: browse_pill.y,
            width: (browse_pill.x - 16) - margin_x,
            height: 40,
        };
        let path_edit = BoxRect {
            x: input_card.x + 14,
            y: input_card.y + 9,
            width: (input_card.width - 28).max(60),
            height: 22,
        };

        let license_card = BoxRect {
            x: margin_x,
            y: card_top + header_height + 78,
            width: content_width,
            height: (footer_top - 16 - (card_top + header_height + 78)).max(100),
        };
        let license_edit = BoxRect {
            x: license_card.x + 14,
            y: license_card.y + 10,
            width: (license_card.width - 18).max(80),
            height: (license_card.height - 20).max(60),
        };

        let folder_status_top = input_card.bottom() + 14;
        let folder_status = BoxRect {
            x: margin_x,
            y: folder_status_top,
            width: content_width,
            height: (footer_top - 12 - folder_status_top).max(96),
        };

        let progress_status = BoxRect {
            x: margin_x,
            y: body.bottom() + 16,
            width: content_width,
            height: 28,
        };
        let progress_bar = BoxRect {
            x: margin_x,
            y: progress_status.bottom() + 10,
            width: content_width,
            height: 18,
        };

        let finish_status = BoxRect {
            x: margin_x,
            y: body.bottom() + 12,
            width: content_width,
            height: 50,
        };
        let launch = BoxRect {
            x: margin_x,
            y: finish_status.bottom() + 12,
            width: 260,
            height: 30,
        };

        let pill_w = 110;
        let pill_h = 40;
        let gap = 14;
        let pill_y = footer_top + 16;
        let cancel_pill = BoxRect {
            x: card_right - 22 - pill_w,
            y: pill_y,
            width: pill_w,
            height: pill_h,
        };
        let next_pill = BoxRect {
            x: cancel_pill.x - gap - pill_w,
            y: pill_y,
            width: pill_w,
            height: pill_h,
        };
        let back_pill = BoxRect {
            x: next_pill.x - gap - pill_w,
            y: pill_y,
            width: pill_w,
            height: pill_h,
        };

        Self {
            width,
            height,
            card_left,
            card_top,
            card_right,
            card_bottom,
            header_height,
            footer_top,
            heading,
            body,
            input_card,
            path_edit,
            browse_pill,
            license_card,
            license_edit,
            progress_bar,
            folder_status,
            progress_status,
            finish_status,
            launch,
            back_pill,
            next_pill,
            cancel_pill,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ButtonStyle {
    pub primary: bool,
    pub hovered: bool,
    pub pressed: bool,
    pub disabled: bool,
    pub focused: bool,
}

pub struct Backdrop {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u32>,
}

impl Backdrop {
    pub fn render(client_width: i32, client_height: i32, page: ScenePage) -> Self {
        let m = Metrics::new(client_width, client_height);
        let w = m.width;
        let h = m.height;
        let mut pixels = vec![0u32; (w * h) as usize];

        // 1. Slate-blue surround matching the window frame.
        for y in 0..h {
            let row_color = Rgb(178, 197, 214)
                .lerp(Rgb(150, 171, 192), y as f32 / h.max(1) as f32)
                .pixel();
            let start = (y * w) as usize;
            pixels[start..start + w as usize].fill(row_color);
        }

        let cl = m.card_left as f32;
        let ct = m.card_top as f32;
        let cr = m.card_right as f32;
        let cb = m.card_bottom as f32;
        let cw = cr - cl;
        let hh = m.header_height as f32;

        // 2. Rounded main card with soft outer drop shadow.
        for y in 0..h {
            let py = y as f32 + 0.5;
            let row = (y * w) as usize;
            for x in 0..w {
                let px = x as f32 + 0.5;
                let shadow_d = rrect_sdf(px, py - 2.0, cl, ct, cr, cb, 14.0, 14.0);
                if shadow_d < 6.0 {
                    let alpha = 0.18 * smoothstep(6.0, -2.0, shadow_d);
                    pixels[row + x as usize] =
                        blend_pixel(pixels[row + x as usize], Rgb(40, 60, 82), alpha);
                }
                let card_d = rrect_sdf(px, py, cl, ct, cr, cb, 14.0, 14.0);
                let cov = (0.5 - card_d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    pixels[row + x as usize] =
                        blend_pixel(pixels[row + x as usize], CARD_SURFACE, cov);
                }
            }
        }

        // 3. Subtle centered Z watermark in the body area.
        let wm_size = 152.0f32;
        let wm_x = (w as f32 - wm_size) * 0.5;
        let body_top = ct + hh;
        let body_span = (m.footer_top as f32 - body_top - wm_size).max(0.0);
        let wm_y = body_top + body_span * 0.54;
        let wm_poly = scale_z(wm_x, wm_y, wm_size);
        fill_polygon_clipped(
            &mut pixels,
            w,
            h,
            &wm_poly,
            (wm_x as i32 - 2, wm_y as i32 - 2, (wm_x + wm_size) as i32 + 3, (wm_y + wm_size) as i32 + 3),
            |_, _| WATERMARK,
            1.0,
        );

        // 4. Multi-layer geometric header banner.
        let header_silhouette = [
            (cl, ct),
            (cr, ct),
            (cr, ct + 0.72 * hh),
            (cl + 0.605 * cw, ct + 0.84 * hh),
            (cl + 0.575 * cw, ct + 0.98 * hh),
            (cl, ct + 0.98 * hh),
        ];
        let blue_bar = [
            (cl + 0.33 * cw, ct),
            (cr, ct),
            (cr, ct + 0.42 * hh),
            (cl + 0.33 * cw, ct + 0.42 * hh),
        ];
        let right_ribbon = [
            (cl + 0.592 * cw, ct + 0.05 * hh),
            (cl + 0.620 * cw, ct + 0.05 * hh),
            (cl + 0.642 * cw, ct + 0.28 * hh),
            (cr, ct + 0.21 * hh),
            (cr, ct + 0.72 * hh),
            (cl + 0.605 * cw, ct + 0.84 * hh),
            (cl + 0.575 * cw, ct + 0.98 * hh),
            (cl + 0.535 * cw, ct + 0.98 * hh),
        ];
        let bevel_strip = [
            (cl + 0.592 * cw, ct + 0.05 * hh),
            (cl + 0.620 * cw, ct + 0.05 * hh),
            (cl + 0.575 * cw, ct + 0.98 * hh),
            (cl + 0.538 * cw, ct + 0.98 * hh),
        ];
        let left_plate = [
            (cl, ct),
            (cl + 0.355 * cw, ct),
            (cl + 0.420 * cw, ct + 0.16 * hh),
            (cl + 0.595 * cw, ct + 0.05 * hh),
            (cl + 0.542 * cw, ct + 0.98 * hh),
            (cl, ct + 0.98 * hh),
        ];

        let header_max_y = (m.card_top + m.header_height + 16).min(h);
        for y in m.card_top..header_max_y {
            let py = y as f32 + 0.5;
            let row = (y * w) as usize;
            let vt = ((py - ct) / hh).clamp(0.0, 1.0);
            for x in m.card_left..m.card_right {
                let px = x as f32 + 0.5;
                let card_cov = (0.5 - rrect_sdf(px, py, cl, ct, cr, cb, 14.0, 14.0)).clamp(0.0, 1.0);
                if card_cov <= 0.0 {
                    continue;
                }
                let idx = row + x as usize;
                let sh_d = poly_sdf(&header_silhouette, px, py - 3.5);
                if sh_d < 8.5 {
                    let alpha = card_cov * 0.16 * smoothstep(8.5, -2.0, sh_d);
                    pixels[idx] = blend_pixel(pixels[idx], Rgb(32, 50, 70), alpha);
                }
                let cov_b = (0.5 - poly_sdf(&blue_bar, px, py)).clamp(0.0, 1.0) * card_cov;
                if cov_b > 0.0 {
                    let c = Rgb(88, 125, 158).lerp(Rgb(69, 104, 136), (py - ct) / (0.42 * hh));
                    pixels[idx] = blend_pixel(pixels[idx], c, cov_b);
                }
                let cov_rr = (0.5 - poly_sdf(&right_ribbon, px, py)).clamp(0.0, 1.0) * card_cov;
                if cov_rr > 0.0 {
                    let c = Rgb(236, 241, 247).lerp(Rgb(216, 222, 230), vt);
                    pixels[idx] = blend_pixel(pixels[idx], c, cov_rr);
                }
                let cov_lp = (0.5 - poly_sdf(&left_plate, px, py)).clamp(0.0, 1.0) * card_cov;
                if cov_lp > 0.0 {
                    let c = Rgb(240, 244, 249).lerp(Rgb(228, 233, 240), vt);
                    pixels[idx] = blend_pixel(pixels[idx], c, cov_lp);
                }
                let cov_bv = (0.5 - poly_sdf(&bevel_strip, px, py)).clamp(0.0, 1.0) * card_cov;
                if cov_bv > 0.0 {
                    let c = Rgb(250, 252, 255).lerp(Rgb(236, 241, 247), vt);
                    pixels[idx] = blend_pixel(pixels[idx], c, cov_bv * 0.92);
                }
            }
        }

        // 5. Rounded [Z] badge in the header.
        paint_logo_badge(&mut pixels, w, h, cl + 26.0, ct + 15.0, 58.0);

        // 6. Page-specific framed surfaces.
        match page {
            ScenePage::Folder { focused } => {
                let border = if focused {
                    Rgb(52, 110, 154)
                } else {
                    Rgb(198, 206, 216)
                };
                let stroke = if focused { 1.6 } else { 1.2 };
                paint_framed_box(
                    &mut pixels,
                    w,
                    h,
                    m.input_card,
                    10.0,
                    EDIT_SURFACE,
                    border,
                    stroke,
                );
            }
            ScenePage::License => {
                paint_framed_box(
                    &mut pixels,
                    w,
                    h,
                    m.license_card,
                    10.0,
                    EDIT_SURFACE,
                    Rgb(198, 206, 216),
                    1.2,
                );
            }
            ScenePage::Installing { percent } => {
                paint_progress_bar(&mut pixels, w, h, m.progress_bar, percent);
            }
            ScenePage::Welcome | ScenePage::Finish => {}
        }

        // 7. Soft cool-grey footer tray at the bottom of the card.
        let ft = m.footer_top as f32;
        let fh = (m.card_bottom - m.footer_top).max(1) as f32;
        for y in (m.footer_top - 2).max(0)..m.card_bottom.min(h) {
            let py = y as f32 + 0.5;
            let row = (y * w) as usize;
            let vt = ((py - ft) / fh).clamp(0.0, 1.0);
            let tray_color = Rgb(230, 234, 239).lerp(Rgb(221, 226, 232), vt);
            for x in m.card_left..m.card_right {
                let px = x as f32 + 0.5;
                let card_cov = (0.5 - rrect_sdf(px, py, cl, ct, cr, cb, 14.0, 14.0)).clamp(0.0, 1.0);
                if card_cov <= 0.0 {
                    continue;
                }
                let tray_cov =
                    (0.5 - rrect_sdf(px, py, cl, ft, cr, cb, 14.0, 14.0)).clamp(0.0, 1.0) * card_cov;
                if tray_cov > 0.0 {
                    let idx = row + x as usize;
                    pixels[idx] = blend_pixel(pixels[idx], tray_color, tray_cov);
                }
            }
        }

        Self {
            width: w,
            height: h,
            pixels,
        }
    }

    /// Copy the rectangle at `(src_x, src_y)` so an owner-drawn child control
    /// composites directly onto the backdrop underneath it.
    pub fn slice(&self, src_x: i32, src_y: i32, width: i32, height: i32) -> Vec<u32> {
        let w = width.max(1);
        let h = height.max(1);
        let mut out = vec![CARD_SURFACE.pixel(); (w * h) as usize];
        if self.width <= 0 || self.height <= 0 {
            return out;
        }
        for row in 0..h {
            let sy = (src_y + row).clamp(0, self.height - 1);
            let dst_base = (row * w) as usize;
            let src_base = (sy * self.width) as usize;
            for col in 0..w {
                let sx = (src_x + col).clamp(0, self.width - 1);
                out[dst_base + col as usize] = self.pixels[src_base + sx as usize];
            }
        }
        out
    }
}

/// Paint an anti-aliased pill button (plus soft drop shadow) onto a slice of
/// the window backdrop sized with `BoxRect::with_pill_padding`.
pub fn paint_pill_button(pixels: &mut [u32], width: i32, height: i32, style: ButtonStyle) {
    let l = PILL_PAD_X as f32;
    let t = PILL_PAD_TOP as f32;
    let r = (width - PILL_PAD_X) as f32;
    let b = (height - PILL_PAD_BOTTOM) as f32;
    if r <= l || b <= t {
        return;
    }
    let radius = (b - t) * 0.5;

    let (top_fill, bot_fill, border_color, shadow_alpha) = if style.disabled {
        (
            Rgb(232, 235, 240),
            Rgb(219, 223, 229),
            Rgb(196, 202, 210),
            0.06,
        )
    } else if style.primary {
        if style.pressed {
            (Rgb(17, 68, 103), Rgb(11, 51, 80), Rgb(9, 43, 68), 0.16)
        } else if style.hovered {
            (Rgb(48, 108, 150), Rgb(21, 80, 121), Rgb(16, 65, 99), 0.30)
        } else {
            (Rgb(39, 95, 134), Rgb(16, 70, 107), Rgb(14, 58, 90), 0.27)
        }
    } else if style.pressed {
        (
            Rgb(207, 212, 219),
            Rgb(192, 198, 206),
            Rgb(166, 173, 182),
            0.09,
        )
    } else if style.hovered {
        (
            Rgb(246, 248, 251),
            Rgb(217, 222, 229),
            Rgb(176, 184, 194),
            0.19,
        )
    } else {
        (
            Rgb(239, 242, 246),
            Rgb(208, 213, 220),
            Rgb(184, 191, 200),
            0.16,
        )
    };

    let border_color = if style.focused && !style.primary && !style.disabled {
        Rgb(52, 110, 154)
    } else {
        border_color
    };

    let shadow_tint = if style.primary {
        Rgb(14, 34, 54)
    } else {
        Rgb(38, 50, 64)
    };
    let shadow_offset = if style.pressed { 1.5 } else { 3.0 };

    for y in 0..height {
        let py = y as f32 + 0.5;
        let vt = ((py - t) / (b - t)).clamp(0.0, 1.0);
        let fill = top_fill.lerp(bot_fill, vt);
        let row = (y * width) as usize;
        for x in 0..width {
            let px = x as f32 + 0.5;
            let idx = row + x as usize;
            let sd = rrect_sdf(px, py - shadow_offset, l, t, r, b, radius, radius);
            if sd < 6.0 {
                let sa = shadow_alpha * smoothstep(6.0, -1.5, sd);
                pixels[idx] = blend_pixel(pixels[idx], shadow_tint, sa);
            }
            let d = rrect_sdf(px, py, l, t, r, b, radius, radius);
            let outer = (0.5 - d).clamp(0.0, 1.0);
            let inner = (0.5 - (d + 1.15)).clamp(0.0, 1.0);
            if outer > 0.0 {
                pixels[idx] = blend_pixel(pixels[idx], border_color, outer);
            }
            if inner > 0.0 {
                pixels[idx] = blend_pixel(pixels[idx], fill, inner);
            }
        }
    }
}

/// Paint a modern rounded checkbox on the left of the `"Launch ZFM now"` control.
pub fn paint_checkbox(
    pixels: &mut [u32],
    width: i32,
    height: i32,
    checked: bool,
    hovered: bool,
    focused: bool,
) {
    let size = 20.0f32;
    let l = 2.0f32;
    let t = ((height as f32 - size) * 0.5).max(1.0);
    let r = l + size;
    let b = t + size;

    let (fill, border) = if checked {
        let top = if hovered {
            Rgb(48, 108, 150)
        } else {
            Rgb(35, 90, 129)
        };
        (top, Rgb(16, 65, 99))
    } else {
        let border = if focused || hovered {
            Rgb(52, 110, 154)
        } else {
            Rgb(176, 185, 196)
        };
        (EDIT_SURFACE, border)
    };

    for y in 0..height {
        let py = y as f32 + 0.5;
        let row = (y * width) as usize;
        for x in 0..(r as i32 + 4).min(width) {
            let px = x as f32 + 0.5;
            let d = rrect_sdf(px, py, l, t, r, b, 5.0, 5.0);
            let outer = (0.5 - d).clamp(0.0, 1.0);
            let inner = (0.5 - (d + 1.4)).clamp(0.0, 1.0);
            let idx = row + x as usize;
            if outer > 0.0 {
                pixels[idx] = blend_pixel(pixels[idx], border, outer);
            }
            if inner > 0.0 {
                pixels[idx] = blend_pixel(pixels[idx], fill, inner);
            }
        }
    }

    if checked {
        let check = [
            (l + 4.5, t + 10.2),
            (l + 8.5, t + 14.2),
            (l + 15.8, t + 6.4),
            (l + 14.2, t + 4.9),
            (l + 8.5, t + 11.0),
            (l + 6.1, t + 8.6),
        ];
        fill_polygon_clipped(
            pixels,
            width,
            height,
            &check,
            (l as i32, t as i32, r as i32 + 1, b as i32 + 1),
            |_, _| PRIMARY_TEXT,
            1.0,
        );
    }
}

fn paint_logo_badge(pixels: &mut [u32], w: i32, h: i32, lx: f32, ly: f32, size: f32) {
    let min_x = (lx as i32 - 4).max(0);
    let min_y = (ly as i32 - 4).max(0);
    let max_x = ((lx + size) as i32 + 6).min(w);
    let max_y = ((ly + size) as i32 + 7).min(h);
    let z_poly = scale_z(lx, ly, size);

    for y in min_y..max_y {
        let py = y as f32 + 0.5;
        let row = (y * w) as usize;
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let idx = row + x as usize;
            let sd = rrect_sdf(px, py - 2.0, lx, ly, lx + size, ly + size, 14.0, 14.0);
            if sd < 4.0 {
                let sa = 0.14 * smoothstep(4.0, -1.0, sd);
                pixels[idx] = blend_pixel(pixels[idx], Rgb(20, 30, 42), sa);
            }
            let d = rrect_sdf(px, py, lx, ly, lx + size, ly + size, 14.0, 14.0);
            let outer = (0.5 - d).clamp(0.0, 1.0);
            let inner = (0.5 - (d + 3.4)).clamp(0.0, 1.0);
            if outer > 0.0 {
                pixels[idx] = blend_pixel(pixels[idx], Rgb(18, 22, 28), outer);
            }
            if inner > 0.0 {
                pixels[idx] = blend_pixel(pixels[idx], EDIT_SURFACE, inner);
            }
            let zc = (0.5 - poly_sdf(&z_poly, px, py)).clamp(0.0, 1.0);
            if zc > 0.0 {
                pixels[idx] = blend_pixel(pixels[idx], Rgb(16, 20, 24), zc);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_framed_box(
    pixels: &mut [u32],
    w: i32,
    h: i32,
    rect: BoxRect,
    radius: f32,
    fill: Rgb,
    border: Rgb,
    stroke: f32,
) {
    let l = rect.x as f32;
    let t = rect.y as f32;
    let r = rect.right() as f32;
    let b = rect.bottom() as f32;
    let min_x = (rect.x - 3).max(0);
    let min_y = (rect.y - 3).max(0);
    let max_x = (rect.right() + 4).min(w);
    let max_y = (rect.bottom() + 5).min(h);

    for y in min_y..max_y {
        let py = y as f32 + 0.5;
        let row = (y * w) as usize;
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let idx = row + x as usize;
            let sd = rrect_sdf(px, py - 1.5, l, t, r, b, radius, radius);
            if sd < 3.5 {
                let sa = 0.06 * smoothstep(3.5, -1.0, sd);
                pixels[idx] = blend_pixel(pixels[idx], Rgb(35, 50, 68), sa);
            }
            let d = rrect_sdf(px, py, l, t, r, b, radius, radius);
            let outer = (0.5 - d).clamp(0.0, 1.0);
            let inner = (0.5 - (d + stroke)).clamp(0.0, 1.0);
            if outer > 0.0 {
                pixels[idx] = blend_pixel(pixels[idx], border, outer);
            }
            if inner > 0.0 {
                pixels[idx] = blend_pixel(pixels[idx], fill, inner);
            }
        }
    }
}

fn paint_progress_bar(pixels: &mut [u32], w: i32, h: i32, rect: BoxRect, percent: i32) {
    let radius = rect.height as f32 * 0.5;
    paint_framed_box(
        pixels,
        w,
        h,
        rect,
        radius,
        Rgb(229, 234, 240),
        Rgb(196, 205, 216),
        1.2,
    );
    let clamped = percent.clamp(0, 100);
    if clamped <= 0 {
        return;
    }
    let l = rect.x as f32 + 2.0;
    let t = rect.y as f32 + 2.0;
    let b = rect.bottom() as f32 - 2.0;
    let max_fill = (rect.width - 4).max(0) as f32;
    let fill_w = (max_fill * clamped as f32 / 100.0).max(b - t).min(max_fill);
    let r = l + fill_w;
    let fill_rad = (b - t) * 0.5;

    let min_x = rect.x.max(0);
    let min_y = rect.y.max(0);
    let max_x = rect.right().min(w);
    let max_y = rect.bottom().min(h);
    for y in min_y..max_y {
        let py = y as f32 + 0.5;
        let vt = ((py - t) / (b - t).max(1.0)).clamp(0.0, 1.0);
        let fill = Rgb(39, 95, 134).lerp(Rgb(16, 70, 107), vt);
        let row = (y * w) as usize;
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let d = rrect_sdf(px, py, l, t, r, b, fill_rad, fill_rad);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                let idx = row + x as usize;
                pixels[idx] = blend_pixel(pixels[idx], fill, cov);
            }
        }
    }
}

fn scale_z(origin_x: f32, origin_y: f32, size: f32) -> [(f32, f32); 10] {
    let mut out = [(0.0f32, 0.0f32); 10];
    for (index, (x, y)) in Z_POINTS.iter().enumerate() {
        out[index] = (origin_x + x * size / 128.0, origin_y + y * size / 128.0);
    }
    out
}

fn fill_polygon_clipped<F>(
    pixels: &mut [u32],
    w: i32,
    h: i32,
    points: &[(f32, f32)],
    bounds: (i32, i32, i32, i32),
    color_at: F,
    opacity: f32,
) where
    F: Fn(f32, f32) -> Rgb,
{
    let min_x = bounds.0.max(0);
    let min_y = bounds.1.max(0);
    let max_x = bounds.2.min(w);
    let max_y = bounds.3.min(h);
    for y in min_y..max_y {
        let py = y as f32 + 0.5;
        let row = (y * w) as usize;
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let cov = (0.5 - poly_sdf(points, px, py)).clamp(0.0, 1.0) * opacity;
            if cov > 0.0 {
                let idx = row + x as usize;
                pixels[idx] = blend_pixel(pixels[idx], color_at(px, py), cov);
            }
        }
    }
}

fn rrect_sdf(px: f32, py: f32, l: f32, t: f32, r: f32, b: f32, rt: f32, rb: f32) -> f32 {
    let cx = (l + r) * 0.5;
    let cy = (t + b) * 0.5;
    let hx = (r - l) * 0.5;
    let hy = (b - t) * 0.5;
    let rad = (if py < cy { rt } else { rb }).min(hx).min(hy);
    let qx = (px - cx).abs() - (hx - rad);
    let qy = (py - cy).abs() - (hy - rad);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    let outside = (ox * ox + oy * oy).sqrt();
    let inside = qx.max(qy).min(0.0);
    outside + inside - rad
}

fn poly_sdf(points: &[(f32, f32)], px: f32, py: f32) -> f32 {
    let mut min_d2 = f32::MAX;
    let mut inside = false;
    let n = points.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        let ex = xj - xi;
        let ey = yj - yi;
        let wx = px - xi;
        let wy = py - yi;
        let denom = ex * ex + ey * ey;
        let t = if denom > 0.0 {
            ((wx * ex + wy * ey) / denom).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let dx = wx - ex * t;
        let dy = wy - ey * t;
        let d2 = dx * dx + dy * dy;
        if d2 < min_d2 {
            min_d2 = d2;
        }
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    let dist = min_d2.sqrt();
    if inside {
        -dist
    } else {
        dist
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn blend_pixel(dst: u32, src: Rgb, alpha: f32) -> u32 {
    if alpha <= 0.0 {
        return dst;
    }
    if alpha >= 1.0 {
        return src.pixel();
    }
    let dr = ((dst >> 16) & 0xff) as f32;
    let dg = ((dst >> 8) & 0xff) as f32;
    let db = (dst & 0xff) as f32;
    let inv = 1.0 - alpha;
    let r = (dr * inv + src.0 as f32 * alpha + 0.5) as u32;
    let g = (dg * inv + src.1 as f32 * alpha + 0.5) as u32;
    let b = (db * inv + src.2 as f32 * alpha + 0.5) as u32;
    (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_encodings_keep_channels_ordered() {
        let teal = Rgb(0x27, 0x5f, 0x86);
        assert_eq!(teal.colorref(), 0x0086_5f27);
        assert_eq!(teal.pixel(), 0x0027_5f86);
    }

    #[test]
    fn footer_button_windows_do_not_overlap() {
        let m = Metrics::new(764, 536);
        let back = m.back_pill.with_pill_padding();
        let next = m.next_pill.with_pill_padding();
        let cancel = m.cancel_pill.with_pill_padding();
        assert!(back.right() < next.x);
        assert!(next.right() < cancel.x);
        assert!(m.body.height >= 64);
        assert!(m.folder_status.height >= 90);
    }

    #[test]
    fn renders_backdrop_and_controls_without_panicking() {
        for page in [
            ScenePage::Welcome,
            ScenePage::Folder { focused: true },
            ScenePage::License,
            ScenePage::Installing { percent: 65 },
            ScenePage::Finish,
        ] {
            let backdrop = Backdrop::render(764, 536, page);
            assert_eq!(backdrop.pixels.len(), (764 * 536) as usize);
            let mut slice = backdrop.slice(500, 480, 122, 50);
            paint_pill_button(
                &mut slice,
                122,
                50,
                ButtonStyle {
                    primary: true,
                    hovered: false,
                    pressed: false,
                    disabled: false,
                    focused: false,
                },
            );
            let mut check = backdrop.slice(34, 260, 260, 30);
            paint_checkbox(&mut check, 260, 30, true, false, false);
        }
    }
}
