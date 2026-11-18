//! Which messages a running session is looking after right now. The global press handler leaves those alone (the session
//! answers them); a press on any other message is a notification, a stale button or a message from before a restart.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

fn registry() -> &'static Mutex<HashMap<i64, Option<u64>>> {
    static R: OnceLock<Mutex<HashMap<i64, Option<u64>>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// Is a session driving this message?
pub fn is_live(message: i64) -> bool {
    registry().lock().is_ok_and(|m| m.contains_key(&message))
}

/// Held by a session for as long as it runs; the message stops being live when it is dropped.
pub struct Guard(i64);

pub fn enter(message: i64, owner: Option<u64>) -> Guard {
    if let Ok(mut m) = registry().lock() {
        m.insert(message, owner);
    }
    Guard(message)
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut m) = registry().lock() {
            m.remove(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_live_while_its_guard_is_held() {
        assert!(!is_live(9_001));
        {
            let _g = enter(9_001, Some(7));
            assert!(is_live(9_001));
        }
        assert!(!is_live(9_001));
    }
}
