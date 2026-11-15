//! Image engine: collage, podium and tier-list renders for profiles.
//!
//! Network fetching is async and bounded; all drawing is pure CPU work run in `spawn_blocking`.
//! The bundled font has Latin glyphs only (no Arabic shaping), so text that needs Arabic stays in embeds
//! and glyphs missing from the font are dropped from the images.
//! (The grid, podium, tier and share renderers are no longer reachable from a command; the banner and guessing-game
//! renderers are the live part.)
#![allow(dead_code)]

mod draw;
mod fetch;

use futures::StreamExt;
use image::RgbaImage;
use moka::future::Cache;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

pub const CELL_W: u32 = 230;
pub const CELL_H: u32 = 345;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const FETCH_CONCURRENCY: usize = 8;
const CACHE_BYTES: u64 = 48 * 1024 * 1024;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CellSpec {
    pub label: String,
    /// Personal title shown under the name.
    pub subtitle: Option<String>,
    /// 1-3 get a gold/silver/bronze badge.
    pub rank: Option<u8>,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct HeaderSpec {
    pub name: String,
    /// Single letters drawn as small badges (C collector, G gamer, W loyal watcher).
    pub badges: Vec<char>,
    pub bio: Option<String>,
    pub count: i64,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TierRow {
    pub name: String,
    pub cells: Vec<CellSpec>,
}

#[derive(Debug)]
pub struct Rendered {
    pub bytes: Vec<u8>,
    /// "webp" or "png".
    pub ext: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Where a profile banner comes from. Stored as text in `profiles.banner`:
/// `preset:<key>`, `color:#rrggbb`, `image:<custom image id>` or (legacy) an allow-listed https URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BannerSpec {
    Preset(String),
    Color(u32),
    Image(i64),
    Url(String),
}

/// Ready-made banners, drawn in code (no bundled pictures, nothing copyrighted): `(key, Arabic name)`.
pub const BANNER_PRESETS: &[(&str, &str)] = &[
    ("slate", "رمادي هادئ"),
    ("sunset", "غروب"),
    ("ocean", "محيط"),
    ("forest", "غابة"),
    ("sakura", "ساكورا"),
    ("night", "ليل"),
    ("aurora", "شفق قطبي"),
    ("ember", "جمر"),
    ("stripes", "خطوط مائلة"),
    ("dots", "نقاط"),
    ("grid", "شبكة"),
    ("waves", "أمواج"),
    ("rings", "حلقات"),
    ("checker", "رقعة"),
    ("mountains", "جبال"),
    ("sunburst", "أشعة"),
];

pub const DEFAULT_BANNER_PRESET: &str = "slate";

impl BannerSpec {
    /// Unknown or empty values give the neutral default gradient.
    pub fn parse(stored: Option<&str>) -> Self {
        let s = stored.map(str::trim).unwrap_or("");
        if let Some(k) = s.strip_prefix("preset:") {
            if BANNER_PRESETS.iter().any(|(key, _)| *key == k) {
                return Self::Preset(k.to_string());
            }
        } else if let Some(c) = s.strip_prefix("color:") {
            let hex = c.trim_start_matches('#');
            if hex.len() == 6 {
                if let Ok(v) = u32::from_str_radix(hex, 16) {
                    return Self::Color(v);
                }
            }
        } else if let Some(id) = s.strip_prefix("image:") {
            if let Ok(id) = id.parse() {
                return Self::Image(id);
            }
        } else if s.starts_with("https://") {
            return Self::Url(s.to_string());
        }
        Self::Preset(DEFAULT_BANNER_PRESET.to_string())
    }

    pub fn encode(&self) -> String {
        match self {
            Self::Preset(k) => format!("preset:{k}"),
            Self::Color(c) => format!("color:#{c:06x}"),
            Self::Image(id) => format!("image:{id}"),
            Self::Url(u) => u.clone(),
        }
    }
}

pub type Images = HashMap<String, Arc<RgbaImage>>;

/// Colour/frame theme chosen with `/profile` -> template.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Default)]
pub struct Theme(u8);

pub const THEMES: [&str; 5] = ["classic", "midnight", "sakura", "ocean", "mono"];

impl Theme {
    pub fn by_name(name: &str) -> Self {
        Self(THEMES.iter().position(|t| *t == name).unwrap_or(0) as u8)
    }

    /// Accent used when the profile has no colour of its own.
    pub fn default_accent(self) -> Option<u32> {
        match self.0 {
            1 => Some(0x5865F2),
            2 => Some(0xF78FB3),
            3 => Some(0x1ABC9C),
            4 => Some(0x9AA0A6),
            _ => None,
        }
    }

    /// Outline drawn around every cell.
    pub fn frame(self) -> Option<[u8; 3]> {
        match self.0 {
            1 => Some([0x8C, 0x96, 0xFF]),
            2 => Some([0xFF, 0xC2, 0xD6]),
            3 => Some([0x7F, 0xF0, 0xD8]),
            4 => Some([0xDD, 0xDD, 0xDD]),
            _ => None,
        }
    }

    /// Background brightness factors (top, bottom).
    pub fn darkness(self) -> (f32, f32) {
        match self.0 {
            1 => (0.30, 0.06),
            2 => (0.55, 0.20),
            4 => (0.35, 0.10),
            _ => (0.45, 0.12),
        }
    }
}

pub fn is_theme(name: &str) -> bool {
    THEMES.contains(&name)
}

/// First frame of a stored custom image (PNG/JPEG/WebP/GIF), shrunk for grids.
pub fn first_frame(bytes: &[u8]) -> Option<RgbaImage> {
    fetch::decode(bytes)
}

pub struct Renderer {
    http: reqwest::Client,
    images: Cache<String, Arc<RgbaImage>>,
    outputs: Cache<u64, Arc<Rendered>>,
}

fn key_of(parts: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

impl Renderer {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            images: Cache::builder()
                .weigher(|_: &String, v: &Arc<RgbaImage>| {
                    v.width().saturating_mul(v.height()).saturating_mul(4)
                })
                .max_capacity(CACHE_BYTES)
                .time_to_live(Duration::from_secs(30 * 60))
                .build(),
            outputs: Cache::builder()
                .weigher(|_: &u64, v: &Arc<Rendered>| {
                    u32::try_from(v.bytes.len()).unwrap_or(u32::MAX)
                })
                .max_capacity(CACHE_BYTES)
                .time_to_live(Duration::from_secs(60 * 60))
                .build(),
        }
    }

    /// Download (bounded concurrency, allow-listed hosts) every distinct URL once.
    async fn fetch_all(&self, urls: Vec<String>) -> Images {
        let mut distinct: Vec<String> = Vec::new();
        for u in urls {
            if !distinct.contains(&u) {
                distinct.push(u);
            }
        }
        futures::stream::iter(distinct)
            .map(|u| async move {
                let img = fetch::fetch_image(&self.http, &self.images, &u).await;
                (u, img)
            })
            .buffer_unordered(FETCH_CONCURRENCY)
            .filter_map(|(u, img)| async move { img.map(|i| (u, i)) })
            .collect()
            .await
    }

    async fn cached_or_render<F>(
        &self,
        key: u64,
        urls: Vec<String>,
        extra: Images,
        draw: F,
    ) -> Option<Arc<Rendered>>
    where
        F: FnOnce(&Images) -> Option<Rendered> + Send + 'static,
    {
        if let Some(hit) = self.outputs.get(&key).await {
            return Some(hit);
        }
        let mut images = self.fetch_all(urls).await;
        images.extend(extra);
        let rendered = tokio::task::spawn_blocking(move || draw(&images))
            .await
            .ok()??;
        let rendered = Arc::new(rendered);
        self.outputs.insert(key, rendered.clone()).await;
        Some(rendered)
    }

    pub async fn collage(
        &self,
        cells: Vec<CellSpec>,
        header: Option<HeaderSpec>,
        accent: Option<u32>,
        theme: Theme,
        scale: f32,
        extra: Images,
    ) -> Option<Arc<Rendered>> {
        if cells.is_empty() {
            return None;
        }
        let key = key_of(&("collage", &cells, &header, accent, theme, scale.to_bits()));
        let urls = urls_of(cells.iter(), header.as_ref());
        self.cached_or_render(key, urls, extra, move |imgs| {
            draw::collage(&cells, header.as_ref(), accent, theme, scale, imgs)
        })
        .await
    }

    pub async fn podium(
        &self,
        top: Vec<CellSpec>,
        header: Option<HeaderSpec>,
        accent: Option<u32>,
        theme: Theme,
        extra: Images,
    ) -> Option<Arc<Rendered>> {
        if top.is_empty() {
            return None;
        }
        let key = key_of(&("podium", &top, &header, accent, theme));
        let urls = urls_of(top.iter(), header.as_ref());
        self.cached_or_render(key, urls, extra, move |imgs| {
            draw::podium(&top, header.as_ref(), accent, theme, imgs)
        })
        .await
    }

    pub async fn share(
        &self,
        cells: Vec<CellSpec>,
        header: Option<HeaderSpec>,
        accent: Option<u32>,
        theme: Theme,
        extra: Images,
    ) -> Option<Arc<Rendered>> {
        if cells.is_empty() {
            return None;
        }
        let key = key_of(&("share", &cells, &header, accent, theme));
        let urls = urls_of(cells.iter(), header.as_ref());
        self.cached_or_render(key, urls, extra, move |imgs| {
            draw::share_image(&cells, header.as_ref(), accent, theme, imgs)
        })
        .await
    }

    /// The 1200x300 strip above the profile card. `uploaded` is the stored picture for `BannerSpec::Image`.
    pub async fn banner(
        &self,
        spec: BannerSpec,
        uploaded: Option<Vec<u8>>,
        title: String,
    ) -> Option<Rendered> {
        let base = match (&spec, uploaded) {
            (BannerSpec::Image(_), Some(bytes)) => {
                tokio::task::spawn_blocking(move || first_frame(&bytes))
                    .await
                    .ok()?
            }
            (BannerSpec::Url(u), _) => {
                let images = self.fetch_all(vec![u.clone()]).await;
                images.get(u).map(|i| (**i).clone())
            }
            _ => None,
        };
        tokio::task::spawn_blocking(move || draw::banner(base.as_ref(), &spec, &title))
            .await
            .ok()?
    }

    /// A cropped, blurred piece of a character picture for the guessing game (never cached: random).
    pub async fn guess_image(&self, url: &str, level: u8) -> Option<Rendered> {
        let images = self.fetch_all(vec![url.to_string()]).await;
        let img = images.get(url)?.clone();
        let seed = rand::random::<u64>();
        tokio::task::spawn_blocking(move || draw::guess_image(&img, level, seed))
            .await
            .ok()?
    }

    pub async fn tier_list(
        &self,
        rows: Vec<TierRow>,
        header: Option<HeaderSpec>,
        accent: Option<u32>,
        theme: Theme,
        extra: Images,
    ) -> Option<Arc<Rendered>> {
        if rows.iter().all(|r| r.cells.is_empty()) {
            return None;
        }
        let key = key_of(&("tier", &rows, &header, accent, theme));
        let urls = urls_of(rows.iter().flat_map(|r| r.cells.iter()), header.as_ref());
        self.cached_or_render(key, urls, extra, move |imgs| {
            draw::tier_list(&rows, header.as_ref(), accent, theme, imgs)
        })
        .await
    }
}

fn urls_of<'a>(
    cells: impl Iterator<Item = &'a CellSpec>,
    header: Option<&HeaderSpec>,
) -> Vec<String> {
    cells
        .filter_map(|c| c.image_url.clone())
        .filter(|u| !u.starts_with("custom:"))
        .chain(header.and_then(|h| h.avatar_url.clone()))
        .collect()
}

#[cfg(test)]
mod tests;

/// Test helper: a preset drawn without a name.
#[cfg(test)]
pub fn draw_banner_for_tests(key: &str) -> Rendered {
    draw::banner(None, &BannerSpec::Preset(key.to_string()), "").expect("banner")
}
