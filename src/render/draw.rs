//! Pure drawing code (no I/O): grid layout, cells, text, collage / podium / tier-list canvases.

#![allow(clippy::too_many_arguments)]

use super::{CellSpec, HeaderSpec, Images, Rendered, TierRow, CELL_H, CELL_W, MAX_OUTPUT_BYTES};
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use image::{imageops, imageops::FilterType, Rgba, RgbaImage};
use std::sync::OnceLock;

const FONT_BYTES: &[u8] = include_bytes!("../../assets/Inter-SemiBold.otf");
const GAP: u32 = 16;
const PAD: u32 = 24;
const HEADER_H: u32 = 120;
const DEFAULT_ACCENT: [u8; 3] = [0x3D, 0xB4, 0xF2];

/// Loaded on first use so idle bots do not pay for it.
fn font() -> Option<&'static FontArc> {
    static FONT: OnceLock<Option<FontArc>> = OnceLock::new();
    FONT.get_or_init(|| FontArc::try_from_slice(FONT_BYTES).ok())
        .as_ref()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub cols: u32,
    pub rows: u32,
    pub cell_w: u32,
    pub cell_h: u32,
    pub width: u32,
    pub height: u32,
}

/// 4-6 columns depending on how many cells there are.
pub fn grid_layout(n: usize, scale: f32, header: bool) -> Grid {
    let n = n.max(1) as u32;
    let cols = match n {
        0..=4 => n,
        5..=12 => 4,
        13..=20 => 5,
        _ => 6,
    };
    let rows = n.div_ceil(cols);
    let cell_w = ((CELL_W as f32 * scale).round() as u32).max(40);
    let cell_h = ((CELL_H as f32 * scale).round() as u32).max(60);
    let gap = ((GAP as f32 * scale).round() as u32).max(6);
    let header_h = if header { HEADER_H } else { 0 };
    Grid {
        cols,
        rows,
        cell_w,
        cell_h,
        width: PAD * 2 + cols * cell_w + (cols - 1) * gap,
        height: PAD * 2 + header_h + rows * cell_h + (rows - 1) * gap,
    }
}

// ---- pixels -----------------------------------------------------------------------------

fn blend_px(canvas: &mut RgbaImage, x: i64, y: i64, rgb: [u8; 3], a: f32) {
    if x < 0
        || y < 0
        || x >= i64::from(canvas.width())
        || y >= i64::from(canvas.height())
        || a <= 0.0
    {
        return;
    }
    let a = a.min(1.0);
    let p = canvas.get_pixel_mut(x as u32, y as u32);
    for (dst, src) in p.0.iter_mut().zip(rgb) {
        *dst = (f32::from(src) * a + f32::from(*dst) * (1.0 - a)).round() as u8;
    }
    p.0[3] = 255;
}

fn in_rounded_rect(px: i64, py: i64, x: i64, y: i64, w: i64, h: i64, r: i64) -> bool {
    if px < x || py < y || px >= x + w || py >= y + h {
        return false;
    }
    let cx = if px < x + r {
        x + r
    } else if px >= x + w - r {
        x + w - r - 1
    } else {
        px
    };
    let cy = if py < y + r {
        y + r
    } else if py >= y + h - r {
        y + h - r - 1
    } else {
        py
    };
    let (dx, dy) = (px - cx, py - cy);
    dx * dx + dy * dy <= r * r
}

fn fill_rrect(
    canvas: &mut RgbaImage,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    r: i64,
    rgb: [u8; 3],
    a: f32,
) {
    for py in y..y + h {
        for px in x..x + w {
            if in_rounded_rect(px, py, x, y, w, h, r) {
                blend_px(canvas, px, py, rgb, a);
            }
        }
    }
}

fn fill_circle(canvas: &mut RgbaImage, cx: i64, cy: i64, r: i64, rgb: [u8; 3], a: f32) {
    fill_rrect(canvas, cx - r, cy - r, r * 2, r * 2, r, rgb, a);
}

fn rgb_of(c: Option<u32>) -> [u8; 3] {
    c.map_or(DEFAULT_ACCENT, |c| {
        [(c >> 16) as u8, (c >> 8) as u8, c as u8]
    })
}

fn shade(c: [u8; 3], f: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (f32::from(c[i]) * f).clamp(0.0, 255.0) as u8)
}

fn gradient(w: u32, h: u32, top: [u8; 3], bottom: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        let t = y as f32 / h.max(1) as f32;
        let px = Rgba(
            [0, 1, 2]
                .map(|i| (f32::from(top[i]) * (1.0 - t) + f32::from(bottom[i]) * t) as u8)
                .into_array_rgba(),
        );
        for x in 0..w {
            img.put_pixel(x, y, px);
        }
    }
    img
}

trait IntoArrayRgba {
    fn into_array_rgba(self) -> [u8; 4];
}
impl IntoArrayRgba for [u8; 3] {
    fn into_array_rgba(self) -> [u8; 4] {
        [self[0], self[1], self[2], 255]
    }
}

pub fn average_colour(img: &RgbaImage) -> [u8; 3] {
    let small = imageops::thumbnail(img, 8, 8);
    let n = (small.width() * small.height()).max(1) as u32;
    let mut sum = [0u32; 3];
    for p in small.pixels() {
        for (acc, v) in sum.iter_mut().zip(p.0) {
            *acc += u32::from(v);
        }
    }
    [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
}

/// Center-crop to the target aspect ratio, then scale.
fn cover(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (iw, ih) = img.dimensions();
    let target = w as f32 / h as f32;
    let (cw, ch) = if iw as f32 / ih as f32 > target {
        (((ih as f32 * target).round() as u32).clamp(1, iw), ih)
    } else {
        (iw, ((iw as f32 / target).round() as u32).clamp(1, ih))
    };
    let view = imageops::crop_imm(img, (iw - cw) / 2, (ih - ch) / 2, cw, ch).to_image();
    imageops::resize(&view, w, h, FilterType::Triangle)
}

fn round_corners(img: &mut RgbaImage, r: u32) {
    let (w, h) = (i64::from(img.width()), i64::from(img.height()));
    for y in 0..h {
        for x in 0..w {
            if !in_rounded_rect(x, y, 0, 0, w, h, i64::from(r)) {
                img.get_pixel_mut(x as u32, y as u32).0[3] = 0;
            }
        }
    }
}

// ---- text -------------------------------------------------------------------------------

/// Drop characters the bundled font cannot draw (e.g. Arabic, CJK).
pub fn sanitize(font: &FontArc, text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_control() && (c.is_whitespace() || font.glyph_id(*c).0 != 0))
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn text_width(font: &FontArc, px: f32, text: &str) -> f32 {
    let scaled = font.as_scaled(PxScale::from(px));
    text.chars()
        .map(|c| scaled.h_advance(scaled.glyph_id(c)))
        .sum()
}

/// Largest size in `[min_px, max_px]` that fits, else truncate with an ellipsis at `min_px`.
pub fn fit_text(font: &FontArc, text: &str, max_w: f32, max_px: f32, min_px: f32) -> (String, f32) {
    let mut px = max_px;
    while px > min_px {
        if text_width(font, px, text) <= max_w {
            return (text.to_string(), px);
        }
        px -= 1.0;
    }
    if text_width(font, min_px, text) <= max_w {
        return (text.to_string(), min_px);
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
        if text_width(font, min_px, &candidate) <= max_w {
            return (candidate, min_px);
        }
    }
    (String::new(), min_px)
}

fn draw_text(
    canvas: &mut RgbaImage,
    font: &FontArc,
    px: f32,
    x: f32,
    y: f32,
    rgb: [u8; 3],
    a: f32,
    text: &str,
) {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut caret = x;
    for ch in text.chars() {
        let mut glyph = scaled.scaled_glyph(ch);
        let advance = scaled.h_advance(glyph.id);
        glyph.position = point(caret, y + scaled.ascent());
        if let Some(outline) = font.outline_glyph(glyph) {
            let b = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                blend_px(
                    canvas,
                    b.min.x as i64 + i64::from(gx),
                    b.min.y as i64 + i64::from(gy),
                    rgb,
                    a * cov,
                );
            });
        }
        caret += advance;
    }
}

fn draw_centered(
    canvas: &mut RgbaImage,
    font: &FontArc,
    px: f32,
    cx: f32,
    y: f32,
    rgb: [u8; 3],
    text: &str,
) {
    let w = text_width(font, px, text);
    draw_text(canvas, font, px, cx - w / 2.0, y, rgb, 1.0, text);
}

// ---- building blocks --------------------------------------------------------------------

fn shadow(canvas: &mut RgbaImage, x: i64, y: i64, w: i64, h: i64, r: i64) {
    for i in 0..6i64 {
        fill_rrect(
            canvas,
            x - i * 2,
            y + 4 - i,
            w + i * 4,
            h + i * 4,
            r + i * 2,
            [0, 0, 0],
            0.07,
        );
    }
}

const BADGE_COLOURS: [[u8; 3]; 3] = [[0xFF, 0xD5, 0x4A], [0xC9, 0xD1, 0xD9], [0xCD, 0x7F, 0x32]];

fn draw_cell(
    canvas: &mut RgbaImage,
    font: Option<&FontArc>,
    x: i64,
    y: i64,
    w: u32,
    h: u32,
    cell: &CellSpec,
    img: Option<&RgbaImage>,
) {
    let radius = i64::from((w / 16).max(6));
    shadow(canvas, x, y, i64::from(w), i64::from(h), radius);
    let mut tile = match img {
        Some(i) => cover(i, w, h),
        None => {
            // Placeholder when the image is missing.
            let mut t = RgbaImage::from_pixel(w, h, Rgba([54, 58, 70, 255]));
            if let Some(f) = font {
                draw_centered(
                    &mut t,
                    f,
                    h as f32 / 4.0,
                    w as f32 / 2.0,
                    h as f32 * 0.35,
                    [150, 155, 170],
                    "?",
                );
            }
            t
        }
    };
    // Gradient scrim for the label.
    let scrim_h = (h / 5).max(30);
    for sy in 0..scrim_h {
        let t = sy as f32 / scrim_h as f32;
        for sx in 0..w {
            blend_px(
                &mut tile,
                i64::from(sx),
                i64::from(h - scrim_h + sy),
                [0, 0, 0],
                0.80 * t.powf(1.2),
            );
        }
    }
    if let Some(f) = font {
        let label = sanitize(f, &cell.label);
        if !label.is_empty() {
            let (text, px) = fit_text(
                f,
                &label,
                w as f32 - 16.0,
                (w as f32 / 10.5).max(12.0),
                11.0,
            );
            draw_text(
                &mut tile,
                f,
                px,
                8.0,
                h as f32 - px - 10.0,
                [255, 255, 255],
                1.0,
                &text,
            );
        }
    }
    round_corners(&mut tile, radius as u32);
    imageops::overlay(canvas, &tile, x, y);

    if let Some(rank @ 1..=3) = cell.rank {
        let r = i64::from((w / 11).max(12));
        let (cx, cy) = (x + r + 8, y + r + 8);
        fill_circle(canvas, cx, cy, r + 2, [0, 0, 0], 0.45);
        fill_circle(canvas, cx, cy, r, BADGE_COLOURS[usize::from(rank) - 1], 1.0);
        if let Some(f) = font {
            let px = r as f32 * 1.3;
            draw_centered(
                canvas,
                f,
                px,
                cx as f32,
                cy as f32 - px * 0.62,
                [30, 30, 30],
                &rank.to_string(),
            );
        }
    }
}

fn background(accent: Option<u32>, first: Option<&RgbaImage>, w: u32, h: u32) -> RgbaImage {
    let base = match (accent, first) {
        (Some(_), _) | (None, None) => rgb_of(accent),
        (None, Some(img)) => average_colour(img),
    };
    gradient(w, h, shade(base, 0.45), shade(base, 0.12))
}

fn draw_header(
    canvas: &mut RgbaImage,
    font: Option<&FontArc>,
    header: &HeaderSpec,
    images: &Images,
    accent: [u8; 3],
) {
    let (cx, cy, r) = (i64::from(PAD) + 42, i64::from(PAD) + 52, 40);
    fill_circle(canvas, cx, cy, r + 3, accent, 1.0);
    match header.avatar_url.as_ref().and_then(|u| images.get(u)) {
        Some(av) => {
            let mut a = cover(av, 80, 80);
            round_corners(&mut a, 40);
            imageops::overlay(canvas, &a, cx - r, cy - r);
        }
        None => fill_circle(canvas, cx, cy, r, [54, 58, 70], 1.0),
    }
    let Some(f) = font else { return };
    let text_x = (PAD + 100) as f32;
    let max_w = canvas.width() as f32 - text_x - PAD as f32 - 110.0;
    let name = sanitize(f, &header.name);
    if !name.is_empty() {
        let (t, px) = fit_text(f, &name, max_w, 34.0, 18.0);
        draw_text(
            canvas,
            f,
            px,
            text_x,
            PAD as f32 + 14.0,
            [255, 255, 255],
            1.0,
            &t,
        );
    }
    if let Some(bio) = header
        .bio
        .as_deref()
        .map(|b| sanitize(f, b))
        .filter(|b| !b.is_empty())
    {
        let (t, px) = fit_text(f, &bio, max_w, 20.0, 13.0);
        draw_text(
            canvas,
            f,
            px,
            text_x,
            PAD as f32 + 62.0,
            [200, 205, 215],
            0.9,
            &t,
        );
    }
    // Count pill on the right.
    let count = header.count.to_string();
    let pw = text_width(f, 30.0, &count) + 36.0;
    let px0 = i64::from(canvas.width()) - i64::from(PAD) - pw as i64;
    fill_rrect(
        canvas,
        px0,
        i64::from(PAD) + 30,
        pw as i64,
        46,
        23,
        accent,
        0.9,
    );
    draw_text(
        canvas,
        f,
        30.0,
        px0 as f32 + 18.0,
        PAD as f32 + 36.0,
        [20, 20, 25],
        1.0,
        &count,
    );
}

// ---- encoding ---------------------------------------------------------------------------

fn encode_png(img: &RgbaImage) -> Option<Rendered> {
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .ok()?;
    (buf.len() <= MAX_OUTPUT_BYTES).then(|| Rendered {
        bytes: buf,
        ext: "png",
        width: img.width(),
        height: img.height(),
    })
}

/// Lossless WebP; shrink up to three times if over the size cap; PNG if WebP fails.
fn encode(mut img: RgbaImage) -> Option<Rendered> {
    for _ in 0..4 {
        let (w, h) = img.dimensions();
        let mut buf = Vec::new();
        let enc = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
        if enc
            .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .is_err()
        {
            return encode_png(&img);
        }
        if buf.len() <= MAX_OUTPUT_BYTES {
            return Some(Rendered {
                bytes: buf,
                ext: "webp",
                width: w,
                height: h,
            });
        }
        img = imageops::resize(&img, w * 3 / 4, h * 3 / 4, FilterType::Triangle);
    }
    encode_png(&img)
}

// ---- canvases ---------------------------------------------------------------------------

fn first_image<'a>(
    cells: impl Iterator<Item = &'a CellSpec>,
    images: &'a Images,
) -> Option<&'a RgbaImage> {
    cells
        .filter_map(|c| c.image_url.as_ref())
        .find_map(|u| images.get(u))
        .map(|a| a.as_ref())
}

pub fn collage(
    cells: &[CellSpec],
    header: Option<&HeaderSpec>,
    accent: Option<u32>,
    scale: f32,
    images: &Images,
) -> Option<Rendered> {
    if cells.is_empty() {
        return None;
    }
    let g = grid_layout(cells.len(), scale, header.is_some());
    let font = font();
    let mut canvas = background(accent, first_image(cells.iter(), images), g.width, g.height);
    if let Some(h) = header {
        draw_header(&mut canvas, font, h, images, shade(rgb_of(accent), 1.2));
    }
    let gap = ((GAP as f32 * scale).round() as u32).max(6);
    let top = PAD + if header.is_some() { HEADER_H } else { 0 };
    for (i, cell) in cells.iter().enumerate() {
        let (col, row) = (i as u32 % g.cols, i as u32 / g.cols);
        let x = i64::from(PAD + col * (g.cell_w + gap));
        let y = i64::from(top + row * (g.cell_h + gap));
        let img = cell
            .image_url
            .as_ref()
            .and_then(|u| images.get(u))
            .map(|a| a.as_ref());
        draw_cell(&mut canvas, font, x, y, g.cell_w, g.cell_h, cell, img);
    }
    encode(canvas)
}

/// 2nd | 1st (larger, centred) | 3rd, standing on blocks.
pub fn podium(
    top: &[CellSpec],
    header: Option<&HeaderSpec>,
    accent: Option<u32>,
    images: &Images,
) -> Option<Rendered> {
    if top.is_empty() {
        return None;
    }
    let font = font();
    let gap = 28u32;
    // (cell index, scale, block height)
    let slots: [(usize, f32, u32); 3] = [(1, 1.0, 64), (0, 1.2, 96), (2, 0.9, 40)];
    let present: Vec<(usize, f32, u32)> = slots
        .iter()
        .copied()
        .filter(|(i, _, _)| *i < top.len())
        .collect();
    let dims = |s: f32| {
        (
            ((CELL_W as f32 * s).round() as u32),
            ((CELL_H as f32 * s).round() as u32),
        )
    };
    let inner_w: u32 =
        present.iter().map(|(_, s, _)| dims(*s).0).sum::<u32>() + gap * (present.len() as u32 - 1);
    let tallest = present
        .iter()
        .map(|(_, s, b)| dims(*s).1 + b)
        .max()
        .unwrap_or(CELL_H);
    let head = if header.is_some() { HEADER_H } else { 0 };
    let (w, h) = (inner_w + PAD * 2, PAD * 2 + head + tallest + 16);
    let mut canvas = background(accent, first_image(top.iter(), images), w, h);
    let accent_rgb = shade(rgb_of(accent), 1.2);
    if let Some(hd) = header {
        draw_header(&mut canvas, font, hd, images, accent_rgb);
    }
    let base_y = i64::from(PAD + head + tallest);
    let mut x = i64::from(PAD);
    for (idx, s, block) in present {
        let (cw, ch) = dims(s);
        let cell = &top[idx];
        let img = cell
            .image_url
            .as_ref()
            .and_then(|u| images.get(u))
            .map(|a| a.as_ref());
        let y = base_y - i64::from(block) - i64::from(ch);
        let mut c = cell.clone();
        c.rank = Some(idx as u8 + 1);
        draw_cell(&mut canvas, font, x, y, cw, ch, &c, img);
        let by = base_y - i64::from(block) + 6;
        fill_rrect(
            &mut canvas,
            x,
            by,
            i64::from(cw),
            i64::from(block) - 6,
            10,
            accent_rgb,
            0.95,
        );
        if let Some(f) = font {
            let px = block as f32 * 0.6;
            draw_centered(
                &mut canvas,
                f,
                px,
                x as f32 + cw as f32 / 2.0,
                by as f32 + (block as f32 - 6.0 - px) / 2.0 - 2.0,
                [20, 20, 25],
                &(idx + 1).to_string(),
            );
        }
        x += i64::from(cw + gap);
    }
    encode(canvas)
}

const TIER_COLOURS: [[u8; 3]; 8] = [
    [0xFF, 0x7F, 0x7F],
    [0xFF, 0xBF, 0x7F],
    [0xFF, 0xDF, 0x7F],
    [0xBF, 0xFF, 0x7F],
    [0x7F, 0xFF, 0xFF],
    [0x7F, 0xBF, 0xFF],
    [0xBF, 0x7F, 0xFF],
    [0xFF, 0x7F, 0xDF],
];

/// S/A/B/C first, then any custom tier names alphabetically.
pub fn tier_order(name: &str) -> (usize, String) {
    let n = name.trim().to_uppercase();
    let rank = ["S", "A", "B", "C", "D"]
        .iter()
        .position(|t| *t == n)
        .unwrap_or(usize::MAX);
    (rank, n)
}

pub fn tier_list(
    rows: &[TierRow],
    header: Option<&HeaderSpec>,
    accent: Option<u32>,
    images: &Images,
) -> Option<Rendered> {
    let rows: Vec<&TierRow> = rows.iter().filter(|r| !r.cells.is_empty()).collect();
    if rows.is_empty() {
        return None;
    }
    let font = font();
    let (tw, th, tgap, label_w) = (80u32, 120u32, 8u32, 130u32);
    let per_line = 10u32;
    let width = PAD * 2 + label_w + 12 + per_line * (tw + tgap);
    let head = if header.is_some() { HEADER_H } else { 0 };
    let row_h = |n: usize| (n as u32).div_ceil(per_line).max(1) * (th + tgap) + tgap;
    let height = PAD * 2 + head + rows.iter().map(|r| row_h(r.cells.len()) + 10).sum::<u32>();
    let all = rows.iter().flat_map(|r| r.cells.iter());
    let mut canvas = background(accent, first_image(all, images), width, height);
    if let Some(hd) = header {
        draw_header(&mut canvas, font, hd, images, shade(rgb_of(accent), 1.2));
    }
    let mut y = i64::from(PAD + head);
    for (ri, row) in rows.iter().enumerate() {
        let rh = i64::from(row_h(row.cells.len()));
        fill_rrect(
            &mut canvas,
            i64::from(PAD),
            y,
            i64::from(width - PAD * 2),
            rh,
            12,
            [0, 0, 0],
            0.35,
        );
        let colour = TIER_COLOURS[ri % TIER_COLOURS.len()];
        fill_rrect(
            &mut canvas,
            i64::from(PAD),
            y,
            i64::from(label_w),
            rh,
            12,
            colour,
            1.0,
        );
        if let Some(f) = font {
            let name = sanitize(f, &row.name);
            let (t, px) = fit_text(f, &name, label_w as f32 - 16.0, 44.0, 16.0);
            draw_centered(
                &mut canvas,
                f,
                px,
                (PAD + label_w / 2) as f32,
                y as f32 + rh as f32 / 2.0 - px * 0.62,
                [20, 20, 25],
                &t,
            );
        }
        for (i, cell) in row.cells.iter().enumerate() {
            let (col, line) = (i as u32 % per_line, i as u32 / per_line);
            let x = i64::from(PAD + label_w + 12 + col * (tw + tgap));
            let cy = y + i64::from(tgap + line * (th + tgap));
            let img = cell
                .image_url
                .as_ref()
                .and_then(|u| images.get(u))
                .map(|a| a.as_ref());
            let mut small = cell.clone();
            small.label = String::new();
            small.rank = None;
            draw_cell(&mut canvas, font, x, cy, tw, th, &small, img);
        }
        y += rh + 10;
    }
    encode(canvas)
}
