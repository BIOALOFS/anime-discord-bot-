//! Page math and the navigation row shared by every paginated view.
//! Owner-only handling and the 3-minute timeout live in `session`.

use crate::i18n;
use poise::serenity_prelude as serenity;

pub fn page_count(total: usize, per_page: usize) -> usize {
    total.div_ceil(per_page.max(1)).max(1)
}

pub fn clamp_page(page: usize, pages: usize) -> usize {
    page.min(pages.saturating_sub(1))
}

/// Items on `page` (0-based); out-of-range pages are clamped to the last one.
pub fn page_slice<T>(items: &[T], page: usize, per_page: usize) -> &[T] {
    let per_page = per_page.max(1);
    let page = clamp_page(page, page_count(items.len(), per_page));
    let start = (page * per_page).min(items.len());
    &items[start..(start + per_page).min(items.len())]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    First,
    Prev,
    Next,
    Last,
}

pub const NAV_FIRST: &str = "nav:first";
pub const NAV_PREV: &str = "nav:prev";
pub const NAV_NEXT: &str = "nav:next";
pub const NAV_LAST: &str = "nav:last";
pub const CLOSE: &str = "close";
pub const BACK: &str = "back";

pub fn parse_nav(id: &str) -> Option<Nav> {
    match id {
        NAV_FIRST => Some(Nav::First),
        NAV_PREV => Some(Nav::Prev),
        NAV_NEXT => Some(Nav::Next),
        NAV_LAST => Some(Nav::Last),
        _ => None,
    }
}

pub fn apply_nav(nav: Nav, page: usize, pages: usize) -> usize {
    let last = pages.saturating_sub(1);
    match nav {
        Nav::First => 0,
        Nav::Prev => page.saturating_sub(1),
        Nav::Next => (page + 1).min(last),
        Nav::Last => last,
    }
}

/// ⏮ ◀ [n/m] ▶ ⏭
pub fn nav_row(page: usize, pages: usize, enabled: bool) -> serenity::CreateActionRow {
    let at_start = page == 0;
    let at_end = page + 1 >= pages;
    let btn = |id: &str, label: &str, off: bool| {
        serenity::CreateButton::new(id)
            .label(label)
            .style(serenity::ButtonStyle::Secondary)
            .disabled(!enabled || off)
    };
    serenity::CreateActionRow::Buttons(vec![
        btn(NAV_FIRST, "⏮", at_start),
        btn(NAV_PREV, "◀", at_start),
        serenity::CreateButton::new("nav:indicator")
            .label(format!("{}/{}", page + 1, pages))
            .style(serenity::ButtonStyle::Secondary)
            .disabled(true),
        btn(NAV_NEXT, "▶", at_end),
        btn(NAV_LAST, "⏭", at_end),
    ])
}

pub fn close_button(enabled: bool) -> serenity::CreateButton {
    serenity::CreateButton::new(CLOSE)
        .label(i18n::BTN_CLOSE)
        .emoji('❌')
        .style(serenity::ButtonStyle::Danger)
        .disabled(!enabled)
}

pub fn back_button(enabled: bool) -> serenity::CreateButton {
    serenity::CreateButton::new(BACK)
        .label(i18n::BTN_BACK)
        .emoji('↩')
        .style(serenity::ButtonStyle::Secondary)
        .disabled(!enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_counts() {
        assert_eq!(page_count(0, 8), 1);
        assert_eq!(page_count(1, 8), 1);
        assert_eq!(page_count(8, 8), 1);
        assert_eq!(page_count(9, 8), 2);
        assert_eq!(page_count(16, 8), 2);
        assert_eq!(page_count(17, 8), 3);
        assert_eq!(page_count(5, 0), 5, "zero page size is treated as 1");
    }

    #[test]
    fn slices_and_bounds() {
        let v: Vec<i32> = (0..17).collect();
        assert_eq!(page_slice(&v, 0, 8), &v[0..8]);
        assert_eq!(page_slice(&v, 1, 8), &v[8..16]);
        assert_eq!(page_slice(&v, 2, 8), &v[16..17]);
        assert_eq!(
            page_slice(&v, 99, 8),
            &v[16..17],
            "clamped to the last page"
        );
        assert!(page_slice::<i32>(&[], 0, 8).is_empty());
        let exact: Vec<i32> = (0..16).collect();
        assert_eq!(page_slice(&exact, 1, 8), &exact[8..16]);
        assert_eq!(page_slice(&exact, 2, 8), &exact[8..16]);
    }

    #[test]
    fn navigation_never_leaves_bounds() {
        assert_eq!(apply_nav(Nav::Prev, 0, 5), 0);
        assert_eq!(apply_nav(Nav::Next, 4, 5), 4);
        assert_eq!(apply_nav(Nav::Next, 1, 5), 2);
        assert_eq!(apply_nav(Nav::Last, 0, 5), 4);
        assert_eq!(apply_nav(Nav::First, 3, 5), 0);
        assert_eq!(apply_nav(Nav::Last, 0, 1), 0);
        assert_eq!(apply_nav(Nav::Next, 0, 0), 0);
    }

    #[test]
    fn parses_ids() {
        assert_eq!(parse_nav("nav:next"), Some(Nav::Next));
        assert_eq!(parse_nav("other"), None);
    }
}
