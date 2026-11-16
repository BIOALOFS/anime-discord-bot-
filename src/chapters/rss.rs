//! Generic RSS 2.0 / Atom source for feeds an admin added in `/admin`. Feeds are fetched with SSRF protection (see
//! `ssrf`), parsed without executing or following anything, and mapped to a tracked work by the admin's title match.
//! Entries are deduplicated by their id (guid) and by chapter number.

use super::{normalize_number, Chapter, SourceKind};

/// One feed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub link: String,
    pub published: Option<i64>,
}

const MAX_ENTRIES: usize = 30;

fn unescape(s: &str) -> String {
    let s = s.trim();
    let s = s
        .strip_prefix("<![CDATA[")
        .and_then(|t| t.strip_suffix("]]>"))
        .unwrap_or(s);
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

/// Text between `<tag ...>` and `</tag>` (first occurrence).
fn tag_text<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = block[from..].find(&open) {
        let start = from + i + open.len();
        let after = block[start..].chars().next()?;
        if after == '>' || after.is_whitespace() || after == '/' {
            let gt = block[start..].find('>')? + start;
            if block[..gt].ends_with('/') {
                return None;
            }
            let end = block[gt + 1..].find(&format!("</{tag}>"))? + gt + 1;
            return Some(&block[gt + 1..end]);
        }
        from = start;
    }
    None
}

/// `href="..."` of the first `<link ...>` tag with one (Atom).
fn atom_href(block: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = block[from..].find("<link") {
        let start = from + i;
        let end = block[start..].find('>')? + start;
        let tag = &block[start..end];
        if let Some(h) = tag.find("href=\"") {
            let rest = &tag[h + 6..];
            return rest.find('"').map(|q| unescape(&rest[..q]));
        }
        from = end;
    }
    None
}

fn parse_date(s: &str) -> Option<i64> {
    let s = s.trim();
    chrono::DateTime::parse_from_rfc2822(s)
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(s))
        .ok()
        .map(|d| d.timestamp())
}

/// Entries of an RSS or Atom document, newest first as listed (at most 30). Anything that is not a feed gives none.
pub fn parse_entries(xml: &str) -> Vec<Entry> {
    let (open, close) = if xml.contains("<item") {
        ("<item", "</item>")
    } else {
        ("<entry", "</entry>")
    };
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(open) {
        let tail = &rest[i..];
        let Some(end) = tail.find(close) else { break };
        let block = &tail[..end];
        rest = &tail[end + close.len()..];
        let Some(title) = tag_text(block, "title")
            .map(unescape)
            .filter(|t| !t.is_empty())
        else {
            continue;
        };
        let link = tag_text(block, "link")
            .map(unescape)
            .filter(|l| l.starts_with("http"))
            .or_else(|| atom_href(block));
        let Some(link) = link.filter(|l| l.starts_with("https://") && l.len() <= 512) else {
            continue;
        };
        let id = tag_text(block, "guid")
            .or_else(|| tag_text(block, "id"))
            .map(unescape)
            .filter(|g| !g.is_empty())
            .unwrap_or_else(|| link.clone());
        let published = tag_text(block, "pubDate")
            .or_else(|| tag_text(block, "published"))
            .or_else(|| tag_text(block, "updated"))
            .and_then(parse_date);
        out.push(Entry {
            id,
            title,
            link,
            published,
        });
        if out.len() >= MAX_ENTRIES {
            break;
        }
    }
    out
}

/// The chapter number in an entry title: the number after "chapter", "ch." or "فصل", else the last number.
pub fn number_in_title(title: &str) -> Option<String> {
    let lower = title.to_lowercase();
    for marker in ["chapter", "ch.", "ch ", "فصل", "الفصل"] {
        if let Some(i) = lower.find(marker) {
            let digits: String = lower[i + marker.len()..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Some(n) = normalize_number(digits.trim_end_matches('.')) {
                return Some(n);
            }
        }
    }
    let digits: Vec<String> = lower
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter(|s| s.chars().any(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .collect();
    digits
        .last()
        .and_then(|d| normalize_number(d.trim_matches('.')))
}

/// Entries as chapters of one work.
pub fn to_chapters(entries: &[Entry], work_id: i64) -> Vec<Chapter> {
    entries
        .iter()
        .map(|e| {
            let number = number_in_title(&e.title);
            Chapter {
                source: SourceKind::Rss,
                work_id,
                key: number.clone().unwrap_or_else(|| format!("rss:{}", e.id)),
                number,
                title: Some(e.title.clone()),
                url: e.link.clone(),
                lang: None,
                published: e.published,
            }
        })
        .collect()
}
