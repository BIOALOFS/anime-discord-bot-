//! Image engine: collage, podium and tier-list renders for profiles.
//!
//! Network fetching is async and bounded; all drawing is pure CPU work run in `spawn_blocking`.
//! The bundled font has Latin glyphs only (no Arabic shaping), so text that needs Arabic stays in embeds
//! and glyphs missing from the font are dropped from the images.

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

pub use draw::tier_order;

pub const CELL_W: u32 = 230;
pub const CELL_H: u32 = 345;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const FETCH_CONCURRENCY: usize = 8;
const CACHE_BYTES: u64 = 48 * 1024 * 1024;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CellSpec {
    pub label: String,
    /// 1-3 get a gold/silver/bronze badge.
    pub rank: Option<u8>,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct HeaderSpec {
    pub name: String,
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

pub type Images = HashMap<String, Arc<RgbaImage>>;

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
        draw: F,
    ) -> Option<Arc<Rendered>>
    where
        F: FnOnce(&Images) -> Option<Rendered> + Send + 'static,
    {
        if let Some(hit) = self.outputs.get(&key).await {
            return Some(hit);
        }
        let images = self.fetch_all(urls).await;
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
        scale: f32,
    ) -> Option<Arc<Rendered>> {
        if cells.is_empty() {
            return None;
        }
        let key = key_of(&("collage", &cells, &header, accent, scale.to_bits()));
        let urls = urls_of(cells.iter(), header.as_ref());
        self.cached_or_render(key, urls, move |imgs| {
            draw::collage(&cells, header.as_ref(), accent, scale, imgs)
        })
        .await
    }

    pub async fn podium(
        &self,
        top: Vec<CellSpec>,
        header: Option<HeaderSpec>,
        accent: Option<u32>,
    ) -> Option<Arc<Rendered>> {
        if top.is_empty() {
            return None;
        }
        let key = key_of(&("podium", &top, &header, accent));
        let urls = urls_of(top.iter(), header.as_ref());
        self.cached_or_render(key, urls, move |imgs| {
            draw::podium(&top, header.as_ref(), accent, imgs)
        })
        .await
    }

    pub async fn tier_list(
        &self,
        rows: Vec<TierRow>,
        header: Option<HeaderSpec>,
        accent: Option<u32>,
    ) -> Option<Arc<Rendered>> {
        if rows.iter().all(|r| r.cells.is_empty()) {
            return None;
        }
        let key = key_of(&("tier", &rows, &header, accent));
        let urls = urls_of(rows.iter().flat_map(|r| r.cells.iter()), header.as_ref());
        self.cached_or_render(key, urls, move |imgs| {
            draw::tier_list(&rows, header.as_ref(), accent, imgs)
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
        .chain(header.and_then(|h| h.avatar_url.clone()))
        .collect()
}

#[cfg(test)]
mod tests;
