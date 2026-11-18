//! The last random picks of each member (per category), so "another random one" does not repeat itself.
//! Memory only: a restart forgets them, which is harmless.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, OnceLock},
};

/// How many recent picks are avoided.
pub const KEEP: usize = 5;

type Store = Mutex<HashMap<(i64, String), VecDeque<String>>>;

fn store() -> &'static Store {
    static S: OnceLock<Store> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Was `key` one of the member's last picks in `scope`?
pub fn seen(user: i64, scope: &str, key: &str) -> bool {
    store()
        .lock()
        .map(|m| {
            m.get(&(user, scope.to_string()))
                .is_some_and(|q| q.iter().any(|k| k == key))
        })
        .unwrap_or(false)
}

/// Remember a pick (the oldest of the last five is forgotten).
pub fn remember(user: i64, scope: &str, key: &str) {
    let Ok(mut m) = store().lock() else { return };
    let q = m.entry((user, scope.to_string())).or_default();
    q.retain(|k| k != key);
    q.push_back(key.to_string());
    while q.len() > KEEP {
        q.pop_front();
    }
    // Bound the map itself: members who stopped playing do not pile up.
    if m.len() > 5_000 {
        m.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_five_are_avoided_and_older_ones_return() {
        let (u, s) = (9_001, "t-five");
        for i in 0..5 {
            remember(u, s, &format!("k{i}"));
        }
        assert!((0..5).all(|i| seen(u, s, &format!("k{i}"))));
        remember(u, s, "k5");
        assert!(!seen(u, s, "k0"), "the sixth pick pushes the oldest out");
        assert!(seen(u, s, "k5"));
    }

    #[test]
    fn members_and_scopes_are_independent() {
        remember(9_002, "a", "x");
        assert!(!seen(9_003, "a", "x"));
        assert!(!seen(9_002, "b", "x"));
    }
}
