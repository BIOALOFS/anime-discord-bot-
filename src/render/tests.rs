use super::{draw, fetch, CellSpec, HeaderSpec, Images, Theme, TierRow};
use image::{Rgba, RgbaImage};
use std::{collections::HashMap, sync::Arc};

fn solid(w: u32, h: u32, c: [u8; 3]) -> Arc<RgbaImage> {
    let mut img = RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        // Not flat, so the lossless encoder has real work to do.
        *p = Rgba([
            c[0].wrapping_add((x / 3) as u8),
            c[1].wrapping_add((y / 5) as u8),
            c[2],
            255,
        ]);
    }
    Arc::new(img)
}

fn cell(i: usize, with_image: bool) -> CellSpec {
    CellSpec {
        subtitle: None,
        label: format!("Character number {i} with a rather long name"),
        rank: (i < 3).then_some(i as u8 + 1),
        image_url: with_image.then(|| format!("https://s4.anilist.co/{i}.png")),
    }
}

fn images_for(n: usize) -> Images {
    (0..n)
        .map(|i| {
            (
                format!("https://s4.anilist.co/{i}.png"),
                solid(230, 345, [(i * 11) as u8, 90, 160]),
            )
        })
        .collect::<HashMap<_, _>>()
}

fn header() -> HeaderSpec {
    HeaderSpec {
        badges: vec!['C', 'W'],
        name: "User مستخدم".into(),
        bio: Some("Loves اnime".into()),
        count: 143,
        avatar_url: None,
    }
}

#[test]
fn grid_uses_four_to_six_columns() {
    assert_eq!(super::draw::grid_layout(1, 1.0, false).cols, 1);
    assert_eq!(super::draw::grid_layout(4, 1.0, false).cols, 4);
    assert_eq!(super::draw::grid_layout(8, 1.0, false).cols, 4);
    assert_eq!(super::draw::grid_layout(20, 1.0, true).cols, 5);
    assert_eq!(super::draw::grid_layout(20, 1.0, true).rows, 4);
    assert_eq!(super::draw::grid_layout(40, 1.0, true).cols, 6);
    let g = super::draw::grid_layout(20, 1.0, true);
    assert_eq!((g.cell_w, g.cell_h), (230, 345));
}

#[test]
fn twenty_cell_collage_renders_small_and_fast() {
    let cells: Vec<_> = (0..20).map(|i| cell(i, true)).collect();
    let started = std::time::Instant::now();
    let out = draw::collage(
        &cells,
        Some(&header()),
        Some(0x3DB4F2),
        Theme::default(),
        1.0,
        &images_for(20),
    )
    .expect("rendered");
    assert!(
        started.elapsed().as_secs() < 20,
        "took {:?}",
        started.elapsed()
    );
    assert!(out.bytes.len() <= super::MAX_OUTPUT_BYTES);
    let g = super::draw::grid_layout(20, 1.0, true);
    assert_eq!((out.width, out.height), (g.width, g.height));
    let decoded = image::load_from_memory(&out.bytes).expect("valid image");
    assert_eq!(decoded.width(), out.width);
    assert!(matches!(out.ext, "webp" | "png"));
}

#[test]
fn missing_images_fall_back_to_placeholders() {
    let cells: Vec<_> = (0..6).map(|i| cell(i, false)).collect();
    let out = draw::collage(&cells, None, None, Theme::default(), 1.0, &HashMap::new())
        .expect("placeholders render");
    assert!(image::load_from_memory(&out.bytes).is_ok());
    // Images referenced but not downloaded behave the same.
    let cells: Vec<_> = (0..6).map(|i| cell(i, true)).collect();
    assert!(draw::collage(&cells, None, None, Theme::default(), 1.0, &HashMap::new()).is_some());
}

#[test]
fn empty_input_renders_nothing() {
    assert!(draw::collage(&[], None, None, Theme::default(), 1.0, &HashMap::new()).is_none());
    assert!(draw::podium(&[], None, None, Theme::default(), &HashMap::new()).is_none());
    assert!(draw::tier_list(&[], None, None, Theme::default(), &HashMap::new()).is_none());
}

#[test]
fn podium_with_one_two_or_three() {
    for n in 1..=3 {
        let top: Vec<_> = (0..n).map(|i| cell(i, true)).collect();
        let out = draw::podium(
            &top,
            Some(&header()),
            None,
            Theme::default(),
            &images_for(3),
        )
        .expect("podium");
        assert!(image::load_from_memory(&out.bytes).is_ok());
    }
}

#[test]
fn tier_list_renders_rows_and_orders_tiers() {
    let rows = vec![
        TierRow {
            name: "S".into(),
            cells: (0..13).map(|i| cell(i, true)).collect(),
        },
        TierRow {
            name: "الأقوى".into(),
            cells: vec![cell(1, false)],
        },
        TierRow {
            name: "empty".into(),
            cells: vec![],
        },
    ];
    let out = draw::tier_list(
        &rows,
        None,
        Some(0xff0000),
        Theme::default(),
        &images_for(13),
    )
    .expect("tier list");
    assert!(image::load_from_memory(&out.bytes).is_ok());
    let mut names = vec!["custom", "C", "s", "A", "B"];
    names.sort_by_key(|n| draw::tier_order(n));
    assert_eq!(names, vec!["s", "A", "B", "C", "custom"]);
}

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
fn cache_keys_are_stable_and_sensitive() {
    let a: Vec<_> = (0..3).map(|i| cell(i, true)).collect();
    let mut b = a.clone();
    let k = |cells: &Vec<CellSpec>, accent: Option<u32>| super::key_of(&("collage", cells, accent));
    assert_eq!(k(&a, None), k(&b, None));
    b.swap(0, 1);
    assert_ne!(k(&a, None), k(&b, None), "order changes the key");
    assert_ne!(k(&a, None), k(&a, Some(1)), "accent changes the key");
}

#[test]
fn hosts_are_allow_listed() {
    assert!(fetch::host_allowed("https://s4.anilist.co/a.png"));
    assert!(!fetch::host_allowed("https://example.com/a.png"));
}

#[test]
fn average_colour_of_flat_image() {
    let img = RgbaImage::from_pixel(40, 40, Rgba([200, 100, 50, 255]));
    assert_eq!(draw::average_colour(&img), [200, 100, 50]);
}

/// Dev helper: `SAMPLE_DIR=/tmp/x cargo test write_samples -- --ignored` writes PNGs to eyeball the layouts.
#[test]
#[ignore]
fn write_samples() {
    let Ok(dir) = std::env::var("SAMPLE_DIR") else {
        return;
    };
    let save = |name: &str, r: super::Rendered| {
        let img = image::load_from_memory(&r.bytes).expect("decode");
        img.save(std::path::Path::new(&dir).join(name))
            .expect("save");
    };
    let cells: Vec<_> = (0..12).map(|i| cell(i, true)).collect();
    save(
        "collage.png",
        draw::collage(
            &cells,
            Some(&header()),
            Some(0xe4a15d),
            Theme::default(),
            1.0,
            &images_for(12),
        )
        .expect("c"),
    );
    save(
        "podium.png",
        draw::podium(
            &cells[..3],
            Some(&header()),
            None,
            Theme::default(),
            &images_for(3),
        )
        .expect("p"),
    );
    let rows = vec![
        TierRow {
            name: "S".into(),
            cells: cells[..5].to_vec(),
        },
        TierRow {
            name: "A".into(),
            cells: cells[5..12].to_vec(),
        },
    ];
    save(
        "tier.png",
        draw::tier_list(
            &rows,
            Some(&header()),
            None,
            Theme::default(),
            &images_for(12),
        )
        .expect("t"),
    );
}

#[test]
fn every_theme_renders_with_subtitles_and_badges() {
    for name in super::THEMES {
        let theme = Theme::by_name(name);
        let mut cells: Vec<_> = (0..5).map(|i| cell(i, true)).collect();
        cells[0].subtitle = Some("The Best".into());
        let out =
            draw::collage(&cells, Some(&header()), None, theme, 1.0, &images_for(5)).expect(name);
        assert!(image::load_from_memory(&out.bytes).is_ok(), "{name}");
    }
    assert_eq!(
        Theme::by_name("nope"),
        Theme::default(),
        "unknown themes fall back to classic"
    );
    assert!(super::is_theme("sakura") && !super::is_theme("rainbow"));
}

#[test]
fn custom_image_keys_are_not_downloaded() {
    let cells = [CellSpec {
        label: "x".into(),
        subtitle: None,
        rank: None,
        image_url: Some("custom:abc".into()),
    }];
    assert!(super::urls_of(cells.iter(), None).is_empty());
}

#[test]
fn first_frame_of_an_animated_gif() {
    use image::{codecs::gif::GifEncoder, Delay, Frame};
    let mut bytes = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut bytes);
        let red = RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 255, 255]));
        enc.encode_frames([
            Frame::from_parts(red, 0, 0, Delay::from_numer_denom_ms(100, 1)),
            Frame::from_parts(blue, 0, 0, Delay::from_numer_denom_ms(100, 1)),
        ])
        .expect("encode");
    }
    let frame = super::first_frame(&bytes).expect("decoded");
    let p = frame.get_pixel(0, 0);
    assert!(
        p.0[0] > 200 && p.0[2] < 60,
        "the grid uses the first (red) frame, got {p:?}"
    );
}

#[test]
fn share_image_is_1200_by_630() {
    let cells: Vec<_> = (0..7).map(|i| cell(i, true)).collect();
    let out = draw::share_image(
        &cells,
        Some(&header()),
        None,
        Theme::default(),
        &images_for(7),
    )
    .expect("share");
    assert_eq!((out.width, out.height), (1200, 630));
    assert!(draw::share_image(&[], None, None, Theme::default(), &HashMap::new()).is_none());
}

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
