//! Image engine: profile banners and the guessing-game crops.
//!
//! Network fetching is async and bounded; all drawing is pure CPU work run in `spawn_blocking`.
//! The bundled font has Latin glyphs only (no Arabic shaping), so text that needs Arabic stays in embeds
//! and glyphs missing from the font are dropped from the images.

mod draw;
mod fetch;

use futures::StreamExt;
use image::RgbaImage;
use moka::future::Cache;
use std::{collections::HashMap, sync::Arc, time::Duration};

const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const FETCH_CONCURRENCY: usize = 8;
const CACHE_BYTES: u64 = 48 * 1024 * 1024;

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

pub struct Renderer {
    http: reqwest::Client,
    images: Cache<String, Arc<RgbaImage>>,
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

    /// The 1200x400 strip above the profile card: a ready-made banner, a colour, or (legacy) an allow-listed https
    /// picture. Uploaded pictures are attached as stored and never go through here.
    pub async fn banner(&self, spec: BannerSpec, title: String) -> Option<Rendered> {
        let base = match &spec {
            BannerSpec::Url(u) => {
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
    pub async fn guess_image(&self, url: &str, level: u8, seed: u64) -> Option<Rendered> {
        let images = self.fetch_all(vec![url.to_string()]).await;
        let img = images.get(url)?.clone();
        tokio::task::spawn_blocking(move || draw::guess_image(&img, level, seed))
            .await
            .ok()?
    }
}

#[cfg(test)]
mod tests;

/// Test helper: a preset drawn without a name.
#[cfg(test)]
pub fn draw_banner_for_tests(key: &str) -> Rendered {
    draw::banner(None, &BannerSpec::Preset(key.to_string()), "").expect("banner")
}
