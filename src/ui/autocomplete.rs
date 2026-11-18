//! Autocomplete plumbing. Discord gives an autocomplete answer 3 seconds, so everything here is built around that:
//! the whole call (debounce included) has a hard deadline of 2.3 s; the lookup itself runs in the background and
//! keeps going after the deadline to warm a cache, and a call that runs out of time answers from that cache
//! (the same query, else the nearest shorter prefix) instead of an empty list.

use moka::future::Cache;
use poise::serenity_prelude as serenity;
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const MIN_CHARS: usize = 2;
pub const MAX_CHOICES: usize = 25;
const DEBOUNCE: Duration = Duration::from_millis(150);
/// Discord allows 3 s; the whole call, debounce included, stays below this.
const DEADLINE: Duration = Duration::from_millis(2300);
const WARM_TTL: Duration = Duration::from_secs(600);

type Key = (u64, &'static str);
/// `(label, value)` pairs.
pub type Items = Vec<(String, String)>;

/// Where an answer came from (for the debug log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Cache,
    Live,
    Fallback,
    Superseded,
}

pub struct AutoState {
    latest: Mutex<HashMap<Key, u64>>,
    counter: AtomicU64,
    /// Answers of finished lookups, whether or not the call that started them was still waiting.
    warm: Cache<(&'static str, String, String), Arc<Items>>,
    debounce: Duration,
    deadline: Duration,
}

impl Default for AutoState {
    fn default() -> Self {
        Self::with_timing(DEBOUNCE, DEADLINE)
    }
}

/// The one cleaning step every autocomplete answer goes through: non-empty trimmed label and value, labels cut to
/// 100 characters with an ellipsis, values over 100 characters dropped (they would be rejected), duplicates by
/// value dropped, at most 25 kept.
pub fn sanitize(items: Items) -> Items {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter_map(|(label, value)| {
            let (label, value) = (label.trim(), value.trim());
            if label.is_empty() || value.is_empty() || value.chars().count() > 100 {
                return None;
            }
            seen.insert(value.to_string())
                .then(|| (super::embeds::fit(label, 100), value.to_string()))
        })
        .take(MAX_CHOICES)
        .collect()
}

impl AutoState {
    pub fn with_timing(debounce: Duration, deadline: Duration) -> Self {
        Self {
            latest: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
            warm: Cache::builder()
                .max_capacity(20_000)
                .time_to_live(WARM_TTL)
                .build(),
            debounce,
            deadline,
        }
    }

    fn mark(&self, key: Key) -> u64 {
        let seq = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut m) = self.latest.lock() {
            m.insert(key, seq);
        }
        seq
    }

    fn is_latest(&self, key: Key, seq: u64) -> bool {
        self.latest
            .lock()
            .map(|m| m.get(&key) == Some(&seq))
            .unwrap_or(true)
    }

    /// The nearest earlier answer: same text, else the longest shorter prefix, narrowed to what still matches.
    async fn fallback(&self, kind: &'static str, scope: &str, text: &str) -> Items {
        let chars: Vec<char> = text.chars().collect();
        for len in (0..=chars.len()).rev() {
            let prefix: String = chars[..len].iter().collect();
            if let Some(items) = self.warm.get(&(kind, scope.to_string(), prefix)).await {
                let narrowed: Items = items
                    .iter()
                    .filter(|(l, _)| l.to_lowercase().contains(text))
                    .cloned()
                    .collect();
                return if narrowed.is_empty() {
                    (*items).clone()
                } else {
                    narrowed
                };
            }
        }
        Vec::new()
    }

    /// Suggestions for `partial` (at least [`MIN_CHARS`] characters).
    pub async fn items<F, Fut>(
        &self,
        user: u64,
        kind: &'static str,
        partial: &str,
        fetch: F,
    ) -> Items
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Items> + Send + 'static,
    {
        self.items_scoped(user, kind, "", partial, MIN_CHARS, fetch)
            .await
    }

    /// `scope` is whatever other option changes the answer (source, mature setting…); `min_chars` 0 asks the
    /// fetch for its "popular" list on an empty input.
    pub async fn items_scoped<F, Fut>(
        &self,
        user: u64,
        kind: &'static str,
        scope: &str,
        partial: &str,
        min_chars: usize,
        fetch: F,
    ) -> Items
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Items> + Send + 'static,
    {
        let started = Instant::now();
        let text = partial.trim().to_lowercase();
        if text.chars().count() < min_chars {
            return Vec::new();
        }
        let key = (user, kind);
        let wkey = (kind, scope.to_string(), text.clone());
        let (items, source) = if let Some(hit) = self.warm.get(&wkey).await {
            ((*hit).clone(), Source::Cache)
        } else {
            let seq = self.mark(key);
            tokio::time::sleep(self.debounce).await;
            if !self.is_latest(key, seq) {
                (Vec::new(), Source::Superseded)
            } else {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let (warm, wk) = (self.warm.clone(), wkey.clone());
                let fut = fetch(partial.trim().to_string());
                // The lookup is not tied to this call: past the deadline it still finishes and fills the cache.
                tokio::spawn(crate::anilist::AUTOCOMPLETE.scope(true, async move {
                    let items = sanitize(fut.await);
                    if !items.is_empty() {
                        warm.insert(wk, Arc::new(items.clone())).await;
                    }
                    let _ = tx.send(items);
                }));
                let left = self.deadline.saturating_sub(started.elapsed());
                match tokio::time::timeout(left, rx).await {
                    Ok(Ok(items)) if !items.is_empty() => (items, Source::Live),
                    // Empty or timed out: whatever was answered earlier for this text or its start.
                    _ => (self.fallback(kind, scope, &text).await, Source::Fallback),
                }
            }
        };
        tracing::debug!(
            kind,
            input_len = text.chars().count(),
            ms = started.elapsed().as_millis() as u64,
            results = items.len(),
            source = ?source,
            fell_back = source == Source::Fallback,
            "autocomplete"
        );
        items
    }
}

pub fn to_choices(items: Items) -> Vec<serenity::AutocompleteChoice> {
    sanitize(items)
        .into_iter()
        .map(|(label, value)| serenity::AutocompleteChoice::new(label, value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn state() -> AutoState {
        AutoState::with_timing(Duration::from_millis(30), Duration::from_millis(200))
    }

    fn one() -> Items {
        vec![("a".into(), "id:1".into())]
    }

    #[tokio::test]
    async fn short_input_is_empty_without_fetching() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let out = state()
            .items(1, "t", "a", move |_| async move {
                c.fetch_add(1, Ordering::SeqCst);
                one()
            })
            .await;
        assert!(out.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn empty_input_asks_for_the_popular_list_when_allowed() {
        let out = state()
            .items_scoped(1, "t", "", "", 0, |q| async move {
                assert!(q.is_empty());
                vec![("Popular".into(), "id:9".into())]
            })
            .await;
        assert_eq!(out.len(), 1);
    }

    #[tokio::test]
    async fn keystroke_burst_makes_one_upstream_call() {
        let s = state();
        let calls = Arc::new(AtomicUsize::new(0));
        let f = |c: Arc<AtomicUsize>| {
            move |_q: String| async move {
                c.fetch_add(1, Ordering::SeqCst);
                one()
            }
        };
        let (a, b, c) = tokio::join!(
            s.items(1, "t", "na", f(calls.clone())),
            async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                s.items(1, "t", "nar", f(calls.clone())).await
            },
            async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                s.items(1, "t", "naru", f(calls.clone())).await
            },
        );
        assert!(
            a.is_empty() && b.is_empty(),
            "superseded requests answer empty"
        );
        assert_eq!(c, one());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_same_query_is_served_from_the_cache() {
        let s = state();
        let calls = Arc::new(AtomicUsize::new(0));
        for _ in 0..3 {
            let c = calls.clone();
            let out = s
                .items(1, "t", "naruto", move |_| async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    one()
                })
                .await;
            assert_eq!(out, one());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_slow_lookup_answers_from_an_earlier_prefix_and_still_warms_the_cache() {
        let s = state();
        // "nar" was answered before
        let out = s
            .items(1, "t", "nar", |_| async {
                vec![
                    ("Naruto".into(), "id:1".into()),
                    ("Boruto".into(), "id:2".into()),
                ]
            })
            .await;
        assert_eq!(out.len(), 2);
        // "naru" is slow: the deadline passes, the earlier answer narrowed to what matches is returned
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let started = Instant::now();
        let out = s
            .items(1, "t", "naru", move |_| async move {
                c.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(400)).await;
                vec![("Naruto Shippuden".into(), "id:3".into())]
            })
            .await;
        assert!(
            started.elapsed() < Duration::from_millis(350),
            "kept to the deadline"
        );
        assert_eq!(out, vec![("Naruto".to_string(), "id:1".to_string())]);
        // the lookup finished in the background: the next call is a cache hit
        tokio::time::sleep(Duration::from_millis(400)).await;
        let c = calls.clone();
        let out = s
            .items(1, "t", "naru", move |_| async move {
                c.fetch_add(1, Ordering::SeqCst);
                one()
            })
            .await;
        assert_eq!(out[0].1, "id:3");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "no second lookup");
    }

    #[tokio::test]
    async fn timeout_with_nothing_cached_is_empty_not_an_error() {
        let out = state()
            .items(1, "t", "naruto", |_| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                one()
            })
            .await;
        assert!(out.is_empty());
    }

    #[test]
    fn sanitize_truncates_dedupes_drops_empties_and_caps_at_25() {
        let long = "x".repeat(300);
        let mut items: Items = vec![
            (long.clone(), "id:1".into()),
            ("  ".into(), "id:2".into()),
            ("ok".into(), "".into()),
            ("dup".into(), "id:1".into()),
            ("far too long value".into(), "v".repeat(101)),
        ];
        items.extend((10..60).map(|i| (format!("n{i}"), format!("id:{i}"))));
        let out = sanitize(items);
        assert_eq!(out.len(), MAX_CHOICES);
        assert_eq!(out[0].0.chars().count(), 100);
        assert!(out[0].0.ends_with('…'));
        assert!(out
            .iter()
            .all(|(l, v)| !l.is_empty() && v.chars().count() <= 100));
        let values: std::collections::HashSet<_> = out.iter().map(|(_, v)| v).collect();
        assert_eq!(values.len(), out.len(), "values are unique");
        assert_eq!(to_choices(vec![(long, "id:1".into())]).len(), 1);
    }
}
