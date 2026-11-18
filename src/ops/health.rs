//! What each provider last did: a success time and the last (short, sanitised) error. Filled by the HTTP clients and
//! shown in the owner/admin status panel. Nothing here ever holds a key or a token.

use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub last_ok: Option<i64>,
    pub last_error: Option<(i64, String)>,
}

fn table() -> &'static Mutex<BTreeMap<String, Entry>> {
    static T: OnceLock<Mutex<BTreeMap<String, Entry>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn ok(provider: &str) {
    if let Ok(mut t) = table().lock() {
        t.entry(provider.to_string()).or_default().last_ok = Some(now());
    }
}

/// Remember a failure. `message` is cut to 120 characters and flattened.
pub fn err(provider: &str, message: &str) {
    let short: String = message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect();
    if let Ok(mut t) = table().lock() {
        t.entry(provider.to_string()).or_default().last_error = Some((now(), short));
    }
    super::stats::bump("error", provider);
}

pub fn get(provider: &str) -> Entry {
    table()
        .lock()
        .ok()
        .and_then(|t| t.get(provider).cloned())
        .unwrap_or_default()
}

/// A provider is healthy when its last success is newer than its last error (or it never failed).
pub fn healthy(e: &Entry) -> bool {
    match (&e.last_ok, &e.last_error) {
        (_, None) => true,
        (Some(ok), Some((err, _))) => ok >= err,
        (None, Some(_)) => false,
    }
}

/// `YYYY-MM-DD HH:MM` (UTC) of a timestamp, for the panel.
pub fn when(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_and_error_are_remembered_and_the_newer_one_decides_health() {
        let p = "test-provider-a";
        assert_eq!(get(p), Entry::default());
        assert!(healthy(&get(p)), "never failed counts as healthy");
        err(p, "400   Bad\nRequest");
        let e = get(p);
        assert_eq!(
            e.last_error.as_ref().map(|x| x.1.as_str()),
            Some("400 Bad Request")
        );
        assert!(!healthy(&e));
        ok(p);
        assert!(healthy(&get(p)), "a success after the error clears it");
    }

    #[test]
    fn long_errors_are_cut() {
        err("test-provider-b", &"x".repeat(500));
        assert_eq!(
            get("test-provider-b")
                .last_error
                .map(|e| e.1.chars().count()),
            Some(120)
        );
    }

    #[test]
    fn timestamps_print_in_utc() {
        assert_eq!(when(0), "1970-01-01 00:00 UTC");
    }
}
