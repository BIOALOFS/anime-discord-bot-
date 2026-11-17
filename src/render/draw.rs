//! Pure drawing code (no I/O): text, the profile banners and the guessing-game crops.

#![allow(clippy::too_many_arguments)]

use super::{Rendered, MAX_OUTPUT_BYTES};
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use image::{imageops, imageops::FilterType, Rgba, RgbaImage};
use std::sync::OnceLock;

const FONT_BYTES: &[u8] = include_bytes!("../../assets/Inter-SemiBold.otf");
const DEFAULT_ACCENT: [u8; 3] = [0x3D, 0xB4, 0xF2];

/// Loaded on first use so idle bots do not pay for it.
fn font() -> Option<&'static FontArc> {
    static FONT: OnceLock<Option<FontArc>> = OnceLock::new();
    FONT.get_or_init(|| FontArc::try_from_slice(FONT_BYTES).ok())
        .as_ref()
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

// ---- building blocks --------------------------------------------------------------------

// ---- banner -----------------------------------------------------------------------------

pub const BANNER_W: u32 = 1200;
pub const BANNER_H: u32 = 400;

fn lerp3(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (f32::from(a[i]) * (1.0 - t) + f32::from(b[i]) * t) as u8)
}

fn paint(f: impl Fn(u32, u32) -> [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::new(BANNER_W, BANNER_H);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let c = f(x, y);
        *p = Rgba([c[0], c[1], c[2], 255]);
    }
    img
}

/// One of the ready-made banners (see `BANNER_PRESETS`); unknown keys give the neutral gradient.
pub fn preset_canvas(key: &str) -> RgbaImage {
    let (w, h) = (BANNER_W as f32, BANNER_H as f32);
    let two = |a: [u8; 3], b: [u8; 3]| gradient(BANNER_W, BANNER_H, a, b);
    match key {
        "sunset" => two([255, 138, 92], [120, 40, 110]),
        "ocean" => two([56, 189, 248], [30, 58, 138]),
        "forest" => two([74, 190, 120], [18, 70, 50]),
        "sakura" => two([255, 183, 213], [190, 80, 130]),
        "night" => two([88, 70, 200], [14, 12, 40]),
        "ember" => two([255, 170, 60], [90, 14, 24]),
        // Diagonal sweep from teal to violet.
        "aurora" => paint(|x, y| {
            lerp3(
                [32, 201, 151],
                [124, 58, 237],
                (x as f32 / w * 0.8) + (y as f32 / h * 0.2),
            )
        }),
        "stripes" => paint(|x, y| {
            if (x + y) / 28 % 2 == 0 {
                [37, 99, 235]
            } else {
                [29, 78, 216]
            }
        }),
        "dots" => paint(|x, y| {
            let (dx, dy) = ((x % 40) as f32 - 20.0, (y % 40) as f32 - 20.0);
            if dx * dx + dy * dy < 49.0 {
                [250, 204, 21]
            } else {
                [30, 41, 59]
            }
        }),
        "grid" => paint(|x, y| {
            if x % 50 < 2 || y % 50 < 2 {
                [96, 165, 250]
            } else {
                [15, 23, 42]
            }
        }),
        "waves" => paint(|x, y| {
            let fx = x as f32 / w * std::f32::consts::TAU;
            let wave = (h * 0.5) + (fx * 2.0).sin() * 40.0 + (fx * 5.0).sin() * 12.0;
            let t = (y as f32 - wave) / h;
            if y as f32 > wave {
                lerp3([14, 116, 144], [8, 47, 73], t * 2.0)
            } else {
                lerp3([186, 230, 253], [125, 211, 252], y as f32 / h)
            }
        }),
        "rings" => paint(|x, y| {
            let (dx, dy) = (x as f32 - w * 0.5, (y as f32 - h * 0.5) * 2.0);
            let d = (dx * dx + dy * dy).sqrt();
            if ((d / 34.0) as u32).is_multiple_of(2) {
                [124, 58, 237]
            } else {
                [76, 29, 149]
            }
        }),
        "checker" => paint(|x, y| {
            if (x / 50 + y / 50) % 2 == 0 {
                [226, 232, 240]
            } else {
                [148, 163, 184]
            }
        }),
        "mountains" => paint(|x, y| {
            let fx = x as f32 / w * std::f32::consts::TAU;
            let far = h * 0.55 + (fx * 3.0).sin() * 30.0;
            let near = h * 0.72 + (fx * 5.0 + 1.0).sin() * 24.0 + (fx * 11.0).sin() * 6.0;
            let y = y as f32;
            if y > near {
                [30, 41, 59]
            } else if y > far {
                [71, 85, 105]
            } else {
                lerp3([254, 215, 170], [148, 163, 184], y / h * 1.4)
            }
        }),
        "sunburst" => paint(|x, y| {
            let (dx, dy) = (x as f32 - w * 0.5, y as f32 - h * 1.1);
            let angle = dy.atan2(dx);
            if ((angle * 10.0).floor() as i32).rem_euclid(2) == 0 {
                [251, 191, 36]
            } else {
                [245, 158, 11]
            }
        }),
        // "slate" and anything unknown.
        _ => two([52, 58, 76], [24, 27, 38]),
    }
}

/// The profile banner strip: an uploaded picture, a solid colour or a ready-made banner, with the name drawn on it
/// when `title` is not empty.
pub fn banner(base: Option<&RgbaImage>, spec: &super::BannerSpec, title: &str) -> Option<Rendered> {
    use super::BannerSpec as B;
    let mut img = match (base, spec) {
        (Some(b), _) => cover(b, BANNER_W, BANNER_H),
        (None, B::Color(c)) => gradient(
            BANNER_W,
            BANNER_H,
            rgb_of(Some(*c)),
            shade(rgb_of(Some(*c)), 0.85),
        ),
        (None, B::Preset(k)) => preset_canvas(k),
        (None, _) => preset_canvas(super::DEFAULT_BANNER_PRESET),
    };
    if let Some(f) = font() {
        let clean = sanitize(f, title);
        if !clean.trim().is_empty() {
            let (text, px) = fit_text(f, &clean, BANNER_W as f32 - 100.0, 64.0, 28.0);
            // A soft band keeps the name readable on any picture.
            fill_rrect(
                &mut img,
                24,
                i64::from(BANNER_H) - 110,
                text_width(f, px, &text) as i64 + 48,
                84,
                16,
                [0, 0, 0],
                0.45,
            );
            draw_text(
                &mut img,
                f,
                px,
                48.0,
                BANNER_H as f32 - 100.0,
                [255, 255, 255],
                1.0,
                &text,
            );
        }
    }
    encode_png(&img)
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

/// Crop + blur for the guessing game: `level` 0 (easy) to 2 (hard); `seed` picks where the crop lands.
pub fn guess_crop(img: &RgbaImage, level: u8, seed: u64) -> RgbaImage {
    let level = usize::from(level.min(2));
    let frac = [0.6f32, 0.42, 0.3][level];
    let sigma = [2.0f32, 4.0, 6.0][level];
    let (iw, ih) = img.dimensions();
    let (cw, ch) = (
        ((iw as f32 * frac) as u32).clamp(1, iw),
        ((ih as f32 * frac) as u32).clamp(1, ih),
    );
    let fx = (seed & 0xff) as f32 / 255.0;
    let fy = ((seed >> 8) & 0xff) as f32 / 255.0;
    let (x0, y0) = (
        ((iw - cw) as f32 * fx) as u32,
        ((ih - ch) as f32 * fy) as u32,
    );
    let view = imageops::crop_imm(img, x0, y0, cw, ch).to_image();
    let width = 360u32;
    let height = ((ch as f32 / cw as f32) * width as f32).round().max(1.0) as u32;
    let scaled = imageops::resize(&view, width, height, FilterType::Triangle);
    imageops::blur(&scaled, sigma)
}

pub fn guess_image(img: &RgbaImage, level: u8, seed: u64) -> Option<Rendered> {
    encode(guess_crop(img, level, seed))
}
