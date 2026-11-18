//! New-chapter notifications for tracked manga, manhwa and manhua. A work is *tracked* in a server when a member of
//! that server has it in their profile or has a reminder on it. `ChapterSource` is separate from `CatalogProvider`:
//! MangaDex (official API, no key), MangaUpdates (release info) and RSS/Atom feeds an admin added. There is no
//! scraping of websites and no site extensions.

pub mod digest;
pub mod mangadex;
pub mod mangaupdates;
pub mod poll;
pub mod rss;
pub mod ssrf;

use std::collections::HashSet;

/// How old a chapter may be and still be announced the first time a work is checked in a server (older ones are
/// recorded silently so enabling a source never floods the channel).
pub const BASELINE_FRESH_SECS: i64 = 6 * 3600;
/// Works checked per poll and server (the rest wait for the next round).
pub const MAX_WORKS_PER_POLL: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    MangaDex,
    MangaUpdates,
    Rss,
}

impl SourceKind {
    /// Priority order: the first source that reports a chapter wins.
    pub const ORDER: [SourceKind; 3] = [
        SourceKind::MangaDex,
        SourceKind::MangaUpdates,
        SourceKind::Rss,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::MangaDex => "mangadex",
            Self::MangaUpdates => "mangaupdates",
            Self::Rss => "rss",
        }
    }

    pub fn credit(self) -> &'static str {
        match self {
            Self::MangaDex => "MangaDex",
            Self::MangaUpdates => "MangaUpdates",
            Self::Rss => "RSS",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub source: SourceKind,
    /// AniList id of the work.
    pub work_id: i64,
    /// Dedupe key: the normalised chapter number when known, else the source's own id.
    pub key: String,
    pub number: Option<String>,
    pub title: Option<String>,
    pub url: String,
    pub lang: Option<String>,
    pub published: Option<i64>,
}

/// A tracked work and the titles it is known by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracked {
    pub work_id: i64,
    pub titles: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("http: {0}")]
    Http(String),
    #[error("bad response: {0}")]
    Parse(String),
}

pub trait ChapterSource: Send + Sync {
    /// Recent chapters of one tracked work (newest first); `lang` is the preferred translation language.
    fn fetch(
        &self,
        work: &Tracked,
        lang: &str,
    ) -> impl std::future::Future<Output = Result<Vec<Chapter>, SourceError>> + Send;
}

/// "12", "12.0", " 012 " -> "12"; "12.5" stays. Empty or non-numeric text is `None`.
pub fn normalize_number(raw: &str) -> Option<String> {
    let t = raw.trim();
    let n: f64 = t
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite() && *n >= 0.0)?;
    Some(if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    })
}

/// Chapter number as an integer ordinal (tenths), for the reminders' dedupe column.
pub fn ordinal(number: &str) -> Option<i64> {
    let n: f64 = number.parse().ok()?;
    Some((n * 10.0).round() as i64)
}

/// Lower-cased letters and digits only, so "Solo Leveling!" and "solo-leveling" match.
pub fn normalize_title(t: &str) -> String {
    t.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Does a feed's `title_match` name this work? (Either contains the other, with at least 3 characters.)
pub fn title_matches(title_match: &str, work: &Tracked) -> bool {
    let m = normalize_title(title_match);
    m.chars().count() >= 3
        && work
            .titles
            .iter()
            .map(|t| normalize_title(t))
            .any(|t| !t.is_empty() && (t.contains(&m) || m.contains(&t)))
}

/// What to do with the chapters of one work in one server: which to announce and which to just record.
/// Sources are given in priority order; the same chapter reported twice is announced once.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub announce: Vec<Chapter>,
    pub record_only: Vec<Chapter>,
}

pub fn plan(chapters: Vec<Chapter>, posted: &HashSet<String>, now: i64) -> Plan {
    let first_time = posted.is_empty();
    let mut seen: HashSet<String> = HashSet::new();
    let (mut announce, mut record_only) = (Vec::new(), Vec::new());
    for c in chapters {
        if posted.contains(&c.key) || !seen.insert(c.key.clone()) {
            continue;
        }
        let fresh = c.published.is_none_or(|p| now - p <= BASELINE_FRESH_SECS);
        if first_time && !fresh {
            record_only.push(c);
        } else {
            announce.push(c);
        }
    }
    // Oldest first, so the channel reads in order.
    announce.sort_by_key(|c| c.published.unwrap_or(i64::MAX));
    Plan {
        announce,
        record_only,
    }
}

/// Keep the chapters of one language (when they carry one).
pub fn only_language(chapters: Vec<Chapter>, lang: &str) -> Vec<Chapter> {
    chapters
        .into_iter()
        .filter(|c| c.lang.as_deref().is_none_or(|l| l == lang))
        .collect()
}

#[cfg(test)]
mod tests;
