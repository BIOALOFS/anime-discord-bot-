//! Custom-image validation and re-encoding. Bytes are judged by their magic numbers, never by file name
//! or content type, then decoded, shrunk and re-encoded so only clean PNG/GIF data is ever stored.

use image::{
    codecs::gif::{GifDecoder, GifEncoder, Repeat},
    imageops::FilterType,
    AnimationDecoder, Frame, ImageReader, Limits,
};
use sha2::{Digest, Sha256};
use std::io::Cursor;

pub const MAX_BYTES: usize = 3 * 1024 * 1024;
pub const MAX_SIDE: u32 = 800;
const MAX_INPUT_SIDE: u32 = 8_000;
const MAX_FRAMES: usize = 60;
/// frames x width x height after shrinking: keeps animated GIFs from exhausting a small host.
const MAX_ANIMATION_PIXELS: u64 = 30_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Webp,
    Gif,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImageError {
    TooLarge,
    UnknownFormat,
    Corrupt,
    TooBig,
}

pub fn sniff(b: &[u8]) -> Option<Format> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(Format::Png)
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else {
        None
    }
}

/// PNG, JPG, WebP and GIF are accepted (for characters and banners alike); anything else is refused.
pub fn is_supported(bytes: &[u8]) -> bool {
    sniff(bytes).is_some()
}

#[derive(Debug)]
pub struct Processed {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    pub animated: bool,
    pub hash: String,
}

fn fit(w: u32, h: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= MAX_SIDE {
        return (w, h);
    }
    let f = MAX_SIDE as f32 / long as f32;
    (
        ((w as f32 * f).round() as u32).max(1),
        ((h as f32 * f).round() as u32).max(1),
    )
}

fn hash_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Validate and re-encode an uploaded picture.
pub fn process(input: &[u8]) -> Result<Processed, ImageError> {
    if input.len() > MAX_BYTES {
        return Err(ImageError::TooLarge);
    }
    match sniff(input).ok_or(ImageError::UnknownFormat)? {
        Format::Gif => process_gif(input),
        other => process_static(input, other),
    }
}

/// Most pixels a downloaded picture may have, and the smallest side that is still a picture and not a dot.
pub const REMOTE_MAX_PIXELS: u64 = 40_000_000;
pub const REMOTE_MIN_SIDE: u32 = 16;
/// Most bytes of a downloaded picture.
pub const REMOTE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// A picture downloaded from an address: up to 8 MB, PNG, JPEG, WebP or GIF by magic bytes (SVG and everything else
/// is refused), a GIF becomes its first frame, the pixel count and the smallest side are checked from the header
/// before anything is decoded, and the result is a clean re-encode (no metadata) of a bounded size.
pub fn process_remote(input: &[u8]) -> Result<Processed, ImageError> {
    if input.len() > REMOTE_MAX_BYTES {
        return Err(ImageError::TooLarge);
    }
    let format = sniff(input).ok_or(ImageError::UnknownFormat)?;
    let fmt = match format {
        Format::Png => image::ImageFormat::Png,
        Format::Jpeg => image::ImageFormat::Jpeg,
        Format::Webp => image::ImageFormat::WebP,
        Format::Gif => image::ImageFormat::Gif,
    };
    let (w, h) = ImageReader::with_format(Cursor::new(input), fmt)
        .into_dimensions()
        .map_err(|_| ImageError::Corrupt)?;
    if u64::from(w) * u64::from(h) > REMOTE_MAX_PIXELS {
        return Err(ImageError::TooBig);
    }
    if w < REMOTE_MIN_SIDE || h < REMOTE_MIN_SIDE {
        return Err(ImageError::Corrupt);
    }
    let mut reader = ImageReader::with_format(Cursor::new(input), fmt);
    let mut l = Limits::default();
    l.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(l);
    let img = reader.decode().map_err(|_| ImageError::Corrupt)?.to_rgba8();
    let (w, h) = fit(img.width(), img.height());
    let img = if (w, h) == img.dimensions() {
        img
    } else {
        image::imageops::resize(&img, w, h, FilterType::Triangle)
    };
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|_| ImageError::Corrupt)?;
    Ok(Processed {
        hash: hash_of(&out),
        bytes: out,
        mime: "image/png",
        width: w,
        height: h,
        animated: false,
    })
}

fn limits() -> Limits {
    let mut l = Limits::default();
    l.max_image_width = Some(MAX_INPUT_SIDE);
    l.max_image_height = Some(MAX_INPUT_SIDE);
    l
}

fn process_static(input: &[u8], format: Format) -> Result<Processed, ImageError> {
    let fmt = match format {
        Format::Png => image::ImageFormat::Png,
        Format::Jpeg => image::ImageFormat::Jpeg,
        _ => image::ImageFormat::WebP,
    };
    let mut reader = ImageReader::with_format(Cursor::new(input), fmt);
    reader.limits(limits());
    let img = reader.decode().map_err(|_| ImageError::Corrupt)?.to_rgba8();
    let (w, h) = fit(img.width(), img.height());
    let img = if (w, h) == img.dimensions() {
        img
    } else {
        image::imageops::resize(&img, w, h, FilterType::Triangle)
    };
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|_| ImageError::Corrupt)?;
    Ok(Processed {
        hash: hash_of(&out),
        bytes: out,
        mime: "image/png",
        width: w,
        height: h,
        animated: false,
    })
}

fn process_gif(input: &[u8]) -> Result<Processed, ImageError> {
    let decoder = GifDecoder::new(Cursor::new(input)).map_err(|_| ImageError::Corrupt)?;
    let mut frames: Vec<Frame> = Vec::new();
    for frame in decoder.into_frames() {
        frames.push(frame.map_err(|_| ImageError::Corrupt)?);
        if frames.len() > MAX_FRAMES {
            return Err(ImageError::TooBig);
        }
    }
    let first = frames.first().ok_or(ImageError::Corrupt)?;
    let (sw, sh) = first.buffer().dimensions();
    if sw > MAX_INPUT_SIDE || sh > MAX_INPUT_SIDE {
        return Err(ImageError::TooBig);
    }
    let (w, h) = fit(sw, sh);
    if frames.len() as u64 * u64::from(w) * u64::from(h) > MAX_ANIMATION_PIXELS {
        return Err(ImageError::TooBig);
    }
    let animated = frames.len() > 1;
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite)
            .map_err(|_| ImageError::Corrupt)?;
        let resized = frames.into_iter().map(|f| {
            let delay = f.delay();
            let buf = if (w, h) == f.buffer().dimensions() {
                f.into_buffer()
            } else {
                image::imageops::resize(f.buffer(), w, h, FilterType::Triangle)
            };
            Frame::from_parts(buf, 0, 0, delay)
        });
        enc.encode_frames(resized)
            .map_err(|_| ImageError::Corrupt)?;
    }
    if out.len() > MAX_BYTES * 2 {
        return Err(ImageError::TooBig);
    }
    Ok(Processed {
        hash: hash_of(&out),
        bytes: out,
        mime: "image/gif",
        width: w,
        height: h,
        animated,
    })
}

// ---- banners ---------------------------------------------------------------------------------------

/// A banner is accepted as it is: any shape, any size. Only technical protections remain.
pub const BANNER_MAX_BYTES: usize = 8 * 1024 * 1024;
/// A decompression-bomb guard: a picture with more pixels than this is refused before it is decoded.
pub const BANNER_MAX_PIXELS: u64 = 40_000_000;
/// Frames x pixels an animated banner may add up to (it is decoded once to prove it is a real GIF).
const BANNER_MAX_ANIMATION_PIXELS: u64 = 400_000_000;
const BANNER_MAX_FRAMES: usize = 300;
/// Static banners are re-encoded (this strips metadata); a picture longer than this is scaled down, never cropped,
/// because Discord scales it on display anyway and the file has to fit one message.
const BANNER_MAX_SIDE: u32 = 1920;

/// Validate a banner. A GIF keeps its bytes and its animation; a static picture is decoded and re-encoded as PNG at
/// its own proportions. Nothing is cropped and no shape is refused.
pub fn process_banner(input: &[u8]) -> Result<Processed, ImageError> {
    if input.len() > BANNER_MAX_BYTES {
        return Err(ImageError::TooLarge);
    }
    match sniff(input).ok_or(ImageError::UnknownFormat)? {
        Format::Gif => banner_gif(input),
        other => banner_static(input, other),
    }
}

fn banner_gif(input: &[u8]) -> Result<Processed, ImageError> {
    let decoder = GifDecoder::new(Cursor::new(input)).map_err(|_| ImageError::Corrupt)?;
    let (w, h) = image::ImageDecoder::dimensions(&decoder);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > BANNER_MAX_PIXELS {
        return Err(ImageError::TooBig);
    }
    let mut frames = 0usize;
    for frame in decoder.into_frames() {
        frame.map_err(|_| ImageError::Corrupt)?;
        frames += 1;
        if frames > BANNER_MAX_FRAMES
            || frames as u64 * u64::from(w) * u64::from(h) > BANNER_MAX_ANIMATION_PIXELS
        {
            return Err(ImageError::TooBig);
        }
    }
    if frames == 0 {
        return Err(ImageError::Corrupt);
    }
    Ok(Processed {
        hash: hash_of(input),
        bytes: input.to_vec(),
        mime: "image/gif",
        width: w,
        height: h,
        animated: frames > 1,
    })
}

fn banner_static(input: &[u8], format: Format) -> Result<Processed, ImageError> {
    let fmt = match format {
        Format::Png => image::ImageFormat::Png,
        Format::Jpeg => image::ImageFormat::Jpeg,
        _ => image::ImageFormat::WebP,
    };
    let reader = || {
        let mut r = ImageReader::with_format(Cursor::new(input), fmt);
        r.limits(limits());
        r
    };
    let (w, h) = reader()
        .into_dimensions()
        .map_err(|_| ImageError::Corrupt)?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > BANNER_MAX_PIXELS {
        return Err(ImageError::TooBig);
    }
    let img = reader()
        .decode()
        .map_err(|_| ImageError::Corrupt)?
        .to_rgba8();
    let long = img.width().max(img.height());
    let img = if long > BANNER_MAX_SIDE {
        let f = BANNER_MAX_SIDE as f32 / long as f32;
        image::imageops::resize(
            &img,
            ((img.width() as f32 * f).round() as u32).max(1),
            ((img.height() as f32 * f).round() as u32).max(1),
            FilterType::Triangle,
        )
    } else {
        img
    };
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|_| ImageError::Corrupt)?;
    if out.len() > BANNER_MAX_BYTES {
        return Err(ImageError::TooLarge);
    }
    Ok(Processed {
        hash: hash_of(&out),
        bytes: out,
        mime: "image/png",
        width: img.width(),
        height: img.height(),
        animated: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_shape_and_size_is_accepted_without_cropping() {
        for (w, h) in [(500, 200), (1000, 700), (700, 1400), (1900, 500), (1, 1)] {
            let out = process_banner(&png_bytes(w, h)).expect("accepted");
            assert!(!out.animated && out.mime == "image/png");
            assert_eq!(
                (out.width, out.height),
                (w, h),
                "{w}x{h} keeps its own proportions and size"
            );
        }
    }

    #[test]
    fn a_huge_still_is_scaled_down_never_cropped() {
        let out = process_banner(&png_bytes(3840, 1080)).expect("accepted");
        assert_eq!(out.width, BANNER_MAX_SIDE);
        assert_eq!(out.height, 540, "the ratio is kept");
    }

    #[test]
    fn an_animated_gif_is_accepted_whole_whatever_its_shape() {
        for (w, h) in [(800, 300), (700, 700), (200, 900)] {
            let bytes = gif_bytes(3, w, h);
            let g = process_banner(&bytes).expect("accepted");
            assert!(g.animated && g.mime == "image/gif");
            assert_eq!(g.bytes, bytes, "the animation is stored untouched");
            assert_eq!((g.width, g.height), (w, h));
        }
    }

    #[test]
    fn a_wrong_type_a_corrupt_file_and_an_oversized_file_are_refused() {
        assert_eq!(
            process_banner(b"<svg/>").err(),
            Some(ImageError::UnknownFormat)
        );
        let mut broken = png_bytes(50, 50);
        broken.truncate(40);
        assert_eq!(process_banner(&broken).err(), Some(ImageError::Corrupt));
        assert_eq!(
            process_banner(&vec![0u8; BANNER_MAX_BYTES + 1]).err(),
            Some(ImageError::TooLarge)
        );
    }

    #[test]
    fn the_pixel_guard_refuses_a_decompression_bomb_before_decoding_it() {
        // 7000 x 7000 = 49M pixels of one colour: tiny on disk, huge in memory.
        let bomb = png_bytes(7000, 7000);
        assert!(bomb.len() < BANNER_MAX_BYTES);
        assert_eq!(process_banner(&bomb).err(), Some(ImageError::TooBig));
    }

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        RgbaImage::from_pixel(w, h, Rgba([9, 9, 9, 255]))
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("png");
        out
    }

    fn gif_bytes(frames: usize, w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut out);
            let list = (0..frames).map(|i| {
                let c = (i * 60 % 255) as u8;
                Frame::from_parts(
                    RgbaImage::from_pixel(w, h, Rgba([c, 0, 255 - c, 255])),
                    0,
                    0,
                    Delay::from_numer_denom_ms(80, 1),
                )
            });
            enc.encode_frames(list).expect("gif");
        }
        out
    }

    #[test]
    fn png_jpeg_webp_and_gif_are_supported_and_everything_else_is_refused() {
        let gif = b"GIF89a\x01\x00";
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0];
        assert!(is_supported(gif) && is_supported(&png) && is_supported(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(is_supported(b"RIFF\x00\x00\x00\x00WEBPVP8 "));
        assert!(!is_supported(b"<svg/>") && !is_supported(b"") && !is_supported(b"%PDF-1.7"));
    }
    use image::{Delay, Rgba, RgbaImage};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, Rgba([10, 200, 90, 255]));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("png");
        out
    }

    fn gif(frames: usize, w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut out);
            let list = (0..frames).map(|i| {
                let c = (i * 40 % 255) as u8;
                Frame::from_parts(
                    RgbaImage::from_pixel(w, h, Rgba([c, 0, 255 - c, 255])),
                    0,
                    0,
                    Delay::from_numer_denom_ms(80, 1),
                )
            });
            enc.encode_frames(list).expect("gif");
        }
        out
    }

    #[test]
    fn formats_are_detected_by_magic_bytes_not_names() {
        assert_eq!(sniff(&png(2, 2)), Some(Format::Png));
        assert_eq!(sniff(&gif(1, 2, 2)), Some(Format::Gif));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some(Format::Jpeg));
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some(Format::Webp));
        assert_eq!(
            sniff(b"<html>fake.png</html>"),
            None,
            "a fake extension proves nothing"
        );
        assert_eq!(sniff(b"MZ\x90\x00 executable"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn rejects_unknown_corrupt_and_oversized_files() {
        assert_eq!(
            process(b"definitely not an image").unwrap_err(),
            ImageError::UnknownFormat
        );
        assert_eq!(
            process(&vec![0u8; MAX_BYTES + 1]).unwrap_err(),
            ImageError::TooLarge
        );
        let mut truncated = png(50, 50);
        truncated.truncate(40);
        assert_eq!(process(&truncated).unwrap_err(), ImageError::Corrupt);
        let mut lying = b"\x89PNG\r\n\x1a\n".to_vec();
        lying.extend_from_slice(&[0u8; 64]);
        assert_eq!(
            process(&lying).unwrap_err(),
            ImageError::Corrupt,
            "right header, wrong body"
        );
    }

    #[test]
    fn large_pictures_are_downscaled_and_reencoded_as_png() {
        let out = process(&png(1600, 2400)).expect("ok");
        assert_eq!(out.mime, "image/png");
        assert!(out.width.max(out.height) <= MAX_SIDE);
        assert_eq!((out.width, out.height), (533, 800), "aspect ratio is kept");
        assert!(!out.animated);
        assert_eq!(sniff(&out.bytes), Some(Format::Png));
        assert_eq!(out.hash.len(), 64);
    }

    #[test]
    fn identical_input_gives_the_same_hash() {
        let a = process(&png(300, 300)).expect("a");
        let b = process(&png(300, 300)).expect("b");
        assert_eq!(a.hash, b.hash, "deduplication relies on this");
        assert_ne!(a.hash, process(&png(301, 300)).expect("c").hash);
    }

    #[test]
    fn animated_gifs_stay_animated() {
        let out = process(&gif(4, 40, 30)).expect("ok");
        assert!(out.animated);
        assert_eq!(out.mime, "image/gif");
        assert_eq!((out.width, out.height), (40, 30));
        let n = GifDecoder::new(Cursor::new(&out.bytes))
            .expect("decoder")
            .into_frames()
            .count();
        assert_eq!(n, 4, "all frames are kept");
    }

    #[test]
    fn a_single_frame_gif_is_not_animated() {
        let out = process(&gif(1, 20, 20)).expect("ok");
        assert!(!out.animated);
    }

    #[test]
    fn too_many_frames_are_refused() {
        assert_eq!(
            process(&gif(MAX_FRAMES + 2, 8, 8)).unwrap_err(),
            ImageError::TooBig
        );
    }

    #[test]
    fn gif_dimensions_are_capped() {
        let out = process(&gif(2, 1200, 600)).expect("ok");
        assert_eq!((out.width, out.height), (800, 400));
    }

    #[test]
    fn a_remote_picture_is_checked_before_it_is_decoded_and_cleaned() {
        // fake extension / not an image / SVG
        assert_eq!(
            process_remote(b"<svg xmlns='http://www.w3.org/2000/svg'/>").err(),
            Some(ImageError::UnknownFormat)
        );
        assert_eq!(
            process_remote(b"<html>not a picture</html>").err(),
            Some(ImageError::UnknownFormat)
        );
        // too small
        assert_eq!(process_remote(&png(4, 4)).err(), Some(ImageError::Corrupt));
        // a GIF becomes one still frame
        let g = process_remote(&gif(3, 40, 40)).expect("gif");
        assert!(!g.animated);
        assert_eq!(g.mime, "image/png");
        // big pictures are shrunk, small ones kept
        let big = process_remote(&png(1600, 900)).expect("png");
        assert!(big.width.max(big.height) <= MAX_SIDE);
        assert_eq!(process_remote(&png(100, 100)).expect("small").width, 100);
        // oversized input is refused outright
        assert_eq!(
            process_remote(&vec![0u8; REMOTE_MAX_BYTES + 1]).err(),
            Some(ImageError::TooLarge)
        );
    }
}
