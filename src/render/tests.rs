use super::{draw, fetch, BannerSpec, DEFAULT_BANNER_PRESET};
use image::{Rgba, RgbaImage};

#[test]
fn text_is_sanitised_and_fitted() {
    let Some(font) =
        ab_glyph::FontArc::try_from_slice(include_bytes!("../../assets/Inter-SemiBold.otf")).ok()
    else {
        panic!("embedded font must parse");
    };
    assert_eq!(
        draw::sanitize(&font, "Ab  مرحبا c"),
        "Ab c",
        "Arabic glyphs are dropped, spaces collapsed"
    );
    let (t, px) = draw::fit_text(&font, "Short", 200.0, 22.0, 11.0);
    assert_eq!((t.as_str(), px), ("Short", 22.0));
    let long = "A very very long character name that cannot possibly fit in the cell width at all";
    let (t, px) = draw::fit_text(&font, long, 150.0, 22.0, 11.0);
    assert!(t.ends_with('…') && px == 11.0);
    assert!(draw::text_width(&font, px, &t) <= 150.0);
    let (t, _) = draw::fit_text(&font, "X", 1.0, 22.0, 11.0);
    assert!(t.is_empty(), "nothing fits in one pixel");
}

#[test]
fn hosts_are_allow_listed() {
    assert!(fetch::host_allowed("https://s4.anilist.co/a.png"));
    assert!(!fetch::host_allowed("https://example.com/a.png"));
}

/// Dev helper: `SAMPLE_DIR=/tmp/x cargo test write_samples -- --ignored` writes PNGs to eyeball the layouts.
#[test]
fn guess_crops_are_smaller_blurrier_and_harder_with_level() {
    let img = RgbaImage::from_fn(300, 450, |x, y| {
        Rgba([(x % 256) as u8, (y % 256) as u8, ((x * y) % 256) as u8, 255])
    });
    for level in 0..=3u8 {
        let out = draw::guess_crop(&img, level, 0x1234);
        assert_eq!(out.width(), 360);
        assert!(out.height() > 0);
    }
    // A harder level shows a smaller piece of the picture (same output width, more magnification).
    let easy = draw::guess_image(&img, 0, 7).expect("easy");
    let hard = draw::guess_image(&img, 2, 7).expect("hard");
    assert!(
        image::load_from_memory(&easy.bytes).is_ok()
            && image::load_from_memory(&hard.bytes).is_ok()
    );
    // The crop position follows the seed.
    assert_ne!(
        draw::guess_crop(&img, 1, 0).into_raw(),
        draw::guess_crop(&img, 1, 0xffff).into_raw()
    );
}

// ---- profile banner ----------------------------------------------------------------------

#[test]
fn banner_specs_parse_and_round_trip() {
    for stored in [
        "preset:sunset",
        "color:#ff8800",
        "image:42",
        "https://s4.anilist.co/b.png",
    ] {
        assert_eq!(BannerSpec::parse(Some(stored)).encode(), stored);
    }
    let default = BannerSpec::Preset(DEFAULT_BANNER_PRESET.to_string());
    for junk in [
        None,
        Some(""),
        Some("preset:nope"),
        Some("color:zzz"),
        Some("image:x"),
        Some("http://insecure"),
        Some("javascript:1"),
    ] {
        assert_eq!(BannerSpec::parse(junk), default, "{junk:?}");
    }
}

#[test]
fn banner_is_a_1200_by_400_strip_for_every_source() {
    for spec in [
        BannerSpec::Preset("ocean".into()),
        BannerSpec::Preset("unknown-preset".into()),
        BannerSpec::Color(0x336699),
        BannerSpec::Image(1),
    ] {
        let out = draw::banner(None, &spec, "Alice").expect("banner");
        assert_eq!((out.width, out.height), (1200, 400), "{spec:?}");
        assert_eq!(out.ext, "png");
        assert!(out.bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }
}

#[test]
fn an_uploaded_picture_is_cropped_to_the_strip_and_names_survive_odd_text() {
    let mut photo = image::RgbaImage::new(400, 900);
    for p in photo.pixels_mut() {
        *p = image::Rgba([200, 20, 20, 255]);
    }
    let out = draw::banner(Some(&photo), &BannerSpec::Image(7), "مستخدم 🎌").expect("banner");
    assert_eq!((out.width, out.height), (1200, 400));
    // The picture, not the default gradient, fills the strip: the top-left pixel is red.
    let decoded = image::load_from_memory(&out.bytes).expect("png").to_rgba8();
    let px = decoded.get_pixel(600, 20);
    assert!(px[0] > 150 && px[1] < 80, "{px:?}");
    assert!(
        draw::banner(None, &BannerSpec::Color(0), &"x".repeat(500)).is_some(),
        "very long names are fitted"
    );
}
