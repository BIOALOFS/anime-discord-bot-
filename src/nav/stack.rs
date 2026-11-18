//! The Back stack of one message: descriptors only (a component id and its values, which `components::parse` turns
//! back into a view request), never rendered content. The root view is implicit and has no descriptor.

use serde::{Deserialize, Serialize};

/// Deepest trail kept; opening another view drops the oldest.
pub const MAX_DEPTH: usize = 10;

/// What opened a view: the id of the component that was pressed, and its selected values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Desc {
    pub id: String,
    #[serde(default)]
    pub values: Vec<String>,
}

impl Desc {
    pub fn new(id: impl Into<String>, values: &[String]) -> Self {
        Self {
            id: id.into(),
            values: values.to_vec(),
        }
    }
}

/// The views above the root, oldest first; the last one is on screen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trail {
    views: Vec<Desc>,
}

impl Trail {
    /// Open a view on top of the current one.
    pub fn open(&mut self, d: Desc) {
        self.views.push(d);
        if self.views.len() > MAX_DEPTH {
            self.views.remove(0);
        }
    }

    /// Replace the view on screen (a page button of a list that is already open).
    pub fn replace(&mut self, d: Desc) {
        match self.views.last_mut() {
            Some(top) => *top = d,
            None => self.open(d),
        }
    }

    /// Go back one view. `Some(None)` is "back to the root view", `None` means there was nothing to go back from.
    pub fn back(&mut self) -> Option<Option<&Desc>> {
        self.views.pop()?;
        Some(self.views.last())
    }

    /// The view on screen (`None` = the root view).
    pub fn current(&self) -> Option<&Desc> {
        self.views.last()
    }

    pub fn depth(&self) -> usize {
        self.views.len()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.views).unwrap_or_else(|_| "[]".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(n: usize) -> Desc {
        Desc::new(format!("showwork:anilist:{n}"), &[])
    }

    #[test]
    fn the_root_has_no_back_and_opening_adds_one() {
        let mut t = Trail::default();
        assert!(t.current().is_none() && t.depth() == 0);
        t.open(d(1));
        assert_eq!(t.depth(), 1);
        assert_eq!(t.current(), Some(&d(1)));
    }

    #[test]
    fn back_restores_the_previous_view_and_ends_at_the_root() {
        let mut t = Trail::default();
        t.open(d(1));
        t.open(d(2));
        t.open(d(3));
        assert_eq!(t.back(), Some(Some(&d(2))));
        assert_eq!(t.back(), Some(Some(&d(1))));
        assert_eq!(t.back(), Some(None), "back from the first view is the root");
        assert_eq!(t.depth(), 0);
        assert_eq!(t.back(), None, "nothing left to go back from");
    }

    #[test]
    fn the_depth_is_ten_and_the_oldest_is_dropped() {
        let mut t = Trail::default();
        for n in 0..25 {
            t.open(d(n));
        }
        assert_eq!(t.depth(), MAX_DEPTH);
        assert_eq!(t.current(), Some(&d(24)));
        for _ in 0..9 {
            t.back();
        }
        assert_eq!(t.current(), Some(&d(15)), "the ten newest are kept");
    }

    #[test]
    fn replacing_swaps_the_view_on_screen_without_growing_the_trail() {
        let mut t = Trail::default();
        t.open(d(1));
        t.replace(d(2));
        assert_eq!((t.depth(), t.current()), (1, Some(&d(2))));
        let mut empty = Trail::default();
        empty.replace(d(5));
        assert_eq!(empty.depth(), 1);
    }

    #[test]
    fn the_trail_serialises_as_a_json_list_of_descriptors() {
        let mut t = Trail::default();
        t.open(Desc::new("pickwork", &["anilist:9".to_string()]));
        t.open(d(2));
        let parsed: Vec<Desc> = serde_json::from_str(&t.to_json()).expect("json");
        assert_eq!(
            parsed,
            vec![Desc::new("pickwork", &["anilist:9".to_string()]), d(2)]
        );
    }
}
