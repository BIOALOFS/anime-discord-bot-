//! Chapter digests: instead of one message per chapter, a server gets one message every few hours with up to
//! 15 new chapters; the rest wait behind a "المزيد" button (kept in memory for a day).

use crate::{
    i18n,
    ui::embeds::{fit, Emb},
};
use moka::future::Cache;
use poise::serenity_prelude as serenity;
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

/// Lines in one digest message.
pub const PER_DIGEST: usize = 15;
pub const MORE_PREFIX: &str = "chmore:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub work: String,
    pub chapter: String,
    pub url: String,
}

/// The overflow of digests that were sent, by the id of the message that carries the button.
fn overflow() -> &'static Cache<u64, Arc<Vec<Line>>> {
    static C: OnceLock<Cache<u64, Arc<Vec<Line>>>> = OnceLock::new();
    C.get_or_init(|| {
        Cache::builder()
            .max_capacity(500)
            .time_to_live(Duration::from_secs(24 * 3600))
            .build()
    })
}

pub async fn keep_overflow(message: u64, lines: Vec<Line>) {
    overflow().insert(message, Arc::new(lines)).await;
}

pub async fn take_overflow(message: u64) -> Option<Arc<Vec<Line>>> {
    overflow().get(&message).await
}

pub fn line_text(l: &Line) -> String {
    if l.url.starts_with("https://") {
        format!(
            "• **{}** — {} · [{}]({})",
            fit(&l.work, 70),
            l.chapter,
            i18n::BTN_READ,
            l.url
        )
    } else {
        format!("• **{}** — {}", fit(&l.work, 70), l.chapter)
    }
}

/// One digest message: the first `PER_DIGEST` lines and a note about how many wait behind the button.
pub fn embed(lines: &[Line], total: usize) -> Emb {
    let shown = &lines[..lines.len().min(PER_DIGEST)];
    let mut body = shown.iter().map(line_text).collect::<Vec<_>>().join("\n");
    if total > shown.len() {
        body.push_str(&format!(
            "\n\n… و{} فصلًا آخر. اضغط «المزيد» لتراها.",
            total - shown.len()
        ));
    }
    Emb::new()
        .title(i18n::CHAPTER_DIGEST_TITLE)
        .description(fit(&body, 3900))
}

pub fn more_button(remaining: usize) -> serenity::CreateButton {
    serenity::CreateButton::new(format!("{MORE_PREFIX}0"))
        .label(format!("{} ({remaining})", i18n::CHAPTER_DIGEST_MORE))
        .style(serenity::ButtonStyle::Secondary)
}

/// Split the overflow into the next message (`PER_DIGEST` lines) and what remains.
pub fn next_page(rest: &[Line]) -> (&[Line], &[Line]) {
    rest.split_at(rest.len().min(PER_DIGEST))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<Line> {
        (0..n)
            .map(|i| Line {
                work: format!("Work {i}"),
                chapter: format!("فصل {i}"),
                url: format!("https://mangadex.org/chapter/{i}"),
            })
            .collect()
    }

    #[test]
    fn a_digest_shows_fifteen_and_says_how_many_wait() {
        let all = lines(22);
        let e = embed(&all, 22);
        let d = e.description.as_deref().unwrap_or("");
        assert_eq!(d.matches("• **").count(), PER_DIGEST);
        assert!(d.contains("و7 فصلًا آخر") && d.contains("المزيد"), "{d}");
        let few = embed(&lines(3), 3);
        assert!(!few.description.as_deref().unwrap_or("").contains("المزيد"));
    }

    #[test]
    fn unsafe_links_are_not_rendered_as_links() {
        let l = Line {
            work: "W".into(),
            chapter: "c".into(),
            url: "javascript:alert(1)".into(),
        };
        assert!(!line_text(&l).contains("]("));
        assert!(line_text(&lines(1)[0]).contains("](https://mangadex.org/chapter/0)"));
    }

    #[test]
    fn the_overflow_pages_through_in_fifteens() {
        let all = lines(40);
        let (a, rest) = next_page(&all);
        assert_eq!((a.len(), rest.len()), (15, 25));
        let (b, rest) = next_page(rest);
        let (c, rest) = next_page(rest);
        assert_eq!((b.len(), c.len(), rest.len()), (15, 10, 0));
    }

    #[tokio::test]
    async fn the_overflow_is_remembered_per_message() {
        keep_overflow(77, lines(5)).await;
        assert_eq!(take_overflow(77).await.expect("kept").len(), 5);
        assert!(take_overflow(78).await.is_none());
    }
}
