//! An in-memory cache of the processed pictures the profile shows (custom character pictures, GIFs and banners), so
//! navigation does not read and copy them from the database on every click. Weighed by bytes; the size comes from
//! `IMAGE_CACHE_MB` (default 64). Images are immutable rows (a new picture is a new id), so nothing needs invalidating.

use crate::db::custom::{self, StoredImage};
use moka::future::Cache;
use sqlx::SqlitePool;
use std::sync::Arc;

pub const DEFAULT_MB: u64 = 64;

#[derive(Clone)]
pub struct ImageCache {
    inner: Cache<i64, Arc<StoredImage>>,
}

impl ImageCache {
    pub fn new(mb: u64) -> Self {
        Self::with_bytes(mb.max(1) * 1024 * 1024)
    }

    pub fn with_bytes(bytes: u64) -> Self {
        Self {
            inner: Cache::builder()
                .weigher(|_, v: &Arc<StoredImage>| u32::try_from(v.bytes.len()).unwrap_or(u32::MAX))
                .max_capacity(bytes)
                .build(),
        }
    }

    /// The picture with this id: from memory, else from the database (and remembered). `None` when it is missing.
    pub async fn get(&self, pool: &SqlitePool, id: i64) -> Option<Arc<StoredImage>> {
        if let Some(hit) = self.inner.get(&id).await {
            return Some(hit);
        }
        let img = Arc::new(custom::get_image(pool, id).await.ok().flatten()?);
        self.inner.insert(id, img.clone()).await;
        Some(img)
    }

    #[cfg(test)]
    pub async fn contains(&self, id: i64) -> bool {
        self.inner.get(&id).await.is_some()
    }

    #[cfg(test)]
    pub async fn settle(&self) {
        self.inner.run_pending_tasks().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store(pool: &SqlitePool, tag: u8, size: usize) -> i64 {
        let img = custom::NewImage {
            hash: format!("h{tag}"),
            mime: "image/png".into(),
            bytes: vec![tag; size],
            width: 1,
            height: 1,
            animated: false,
        };
        match custom::store_image(pool, 1, &img, 50).await.expect("store") {
            custom::StoreOutcome::Stored(id) | custom::StoreOutcome::Existing(id) => id,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_picture_is_read_once_and_then_served_from_memory() {
        let pool = crate::db::connect_memory().await;
        crate::db::profile::create_profile(&pool, 1)
            .await
            .expect("profile");
        let id = store(&pool, 1, 100).await;
        let cache = ImageCache::new(1);
        let first = cache.get(&pool, id).await.expect("found");
        // deleting the row proves the second read does not touch the database
        sqlx::query("DELETE FROM custom_images WHERE id = ?1")
            .bind(id)
            .execute(&pool)
            .await
            .expect("delete");
        let second = cache.get(&pool, id).await.expect("from memory");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(
            cache.get(&pool, 9_999).await.is_none(),
            "a missing picture is None"
        );
    }

    #[tokio::test]
    async fn the_cache_is_bounded_by_bytes_and_evicts() {
        let pool = crate::db::connect_memory().await;
        crate::db::profile::create_profile(&pool, 1)
            .await
            .expect("profile");
        let cache = ImageCache::with_bytes(1000);
        let mut ids = Vec::new();
        for tag in 0..8u8 {
            ids.push(store(&pool, tag, 400).await);
        }
        for id in &ids {
            cache.get(&pool, *id).await.expect("found");
            cache.settle().await;
        }
        let mut kept = 0;
        for id in &ids {
            if cache.contains(*id).await {
                kept += 1;
            }
        }
        assert!(
            kept <= 2,
            "1000 bytes hold at most two 400-byte pictures, kept {kept}"
        );
        assert!(kept >= 1);
    }
}
