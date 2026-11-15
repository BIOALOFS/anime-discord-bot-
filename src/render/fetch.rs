//! Allow-listed, size-capped image downloads.

use image::{imageops::FilterType, RgbaImage};
use moka::future::Cache;
use std::{sync::Arc, time::Duration};

const ALLOWED_HOSTS: &[&str] = &[
    "s4.anilist.co",
    "cdn.discordapp.com",
    "media.discordapp.net",
    "images.igdb.com",
    "t.vndb.org",
];
const MAX_DOWNLOAD: usize = 6 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(6);
/// Sources are shrunk to this box; collages never need more.
const MAX_W: u32 = 300;
const MAX_H: u32 = 450;

/// Only https images from known hosts (never arbitrary URLs: SSRF guard).
pub fn host_allowed(url: &str) -> bool {
    let Some(rest) = url.trim().strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.contains('@') && ALLOWED_HOSTS.contains(&host.to_lowercase().as_str())
}

pub fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    if w > MAX_W || h > MAX_H {
        let f = (MAX_W as f32 / w as f32).min(MAX_H as f32 / h as f32);
        let (nw, nh) = (
            ((w as f32 * f).round() as u32).max(1),
            ((h as f32 * f).round() as u32).max(1),
        );
        return Some(image::imageops::resize(&img, nw, nh, FilterType::Triangle));
    }
    Some(img)
}

pub async fn fetch_image(
    http: &reqwest::Client,
    cache: &Cache<String, Arc<RgbaImage>>,
    url: &str,
) -> Option<Arc<RgbaImage>> {
    if let Some(hit) = cache.get(url).await {
        return Some(hit);
    }
    if !host_allowed(url) {
        return None;
    }
    let resp = http.get(url).timeout(TIMEOUT).send().await.ok()?;
    if !resp.status().is_success()
        || resp
            .content_length()
            .is_some_and(|n| n > MAX_DOWNLOAD as u64)
    {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    if bytes.len() > MAX_DOWNLOAD {
        return None;
    }
    let img = Arc::new(
        tokio::task::spawn_blocking(move || decode(&bytes))
            .await
            .ok()??,
    );
    cache.insert(url.to_string(), img.clone()).await;
    Some(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_hosts_are_fetched() {
        assert!(host_allowed(
            "https://s4.anilist.co/file/anilistcdn/character/large/b1.png"
        ));
        assert!(host_allowed(
            "https://cdn.discordapp.com/avatars/1/a.png?size=128"
        ));
        assert!(!host_allowed("http://s4.anilist.co/a.png"));
        assert!(!host_allowed("https://s4.anilist.co.evil.example/a.png"));
        assert!(!host_allowed("https://evil.example/s4.anilist.co/a.png"));
        assert!(!host_allowed("https://user@s4.anilist.co/a.png"));
        assert!(!host_allowed("https://127.0.0.1/a.png"));
        assert!(!host_allowed("file:///etc/passwd"));
    }

    #[test]
    fn decode_rejects_garbage_and_shrinks_big_images() {
        assert!(decode(b"not an image").is_none());
        let big = RgbaImage::from_pixel(900, 1350, image::Rgba([10, 20, 30, 255]));
        let mut png = Vec::new();
        big.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("png");
        let d = decode(&png).expect("decoded");
        assert!(d.width() <= MAX_W && d.height() <= MAX_H);
    }
}
