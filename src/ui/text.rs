//! Turning AniList description HTML into short, safe Discord markdown.

use crate::{anilist::safety, i18n};

pub const MAX_DESCRIPTION_CHARS: usize = 350;

fn tag_replacement(tag: &str) -> &'static str {
    let name = tag
        .trim()
        .trim_start_matches('/')
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match name.as_str() {
        "br" => "\n",
        "i" | "em" => "*",
        "b" | "strong" => "**",
        _ => "",
    }
}

/// `<br>` -> newline, `<i>` -> `*`, `<b>` -> `**`, other tags dropped, entities decoded,
/// AniList spoilers `~!x!~` -> Discord spoilers `||x||`.
pub fn html_to_markdown(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut tag = String::new();
    let mut in_tag = false;
    for ch in raw.chars() {
        match (in_tag, ch) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                out.push_str(tag_replacement(&tag));
            }
            (true, c) => tag.push(c),
            (false, c) => out.push(c),
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&amp;", "&")
        .replace("~!", "||")
        .replace("!~", "||")
}

/// Trim each line and keep at most one blank line in a row.
pub fn collapse_blank_lines(s: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in s.lines().map(str::trim) {
        if line.is_empty() && out.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

/// Truncate to `max` chars at a word boundary, adding an ellipsis when cut.
pub fn truncate_words(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    let cut = head.rfind(char::is_whitespace).unwrap_or(head.len());
    format!("{}…", head[..cut].trim_end())
}

/// Cut at the last sentence end in the window (if it keeps at least half of it), else at a word.
/// Returns the text and whether it was cut.
fn cut_at_boundary(s: &str, max: usize) -> (String, bool) {
    if s.chars().count() <= max {
        return (s.to_string(), false);
    }
    let window: String = s.chars().take(max).collect();
    let min_keep = max / 2;
    let chars: Vec<(usize, char)> = window.char_indices().collect();
    let sentence_end = chars.iter().enumerate().rev().find_map(|(i, (byte, c))| {
        let is_end = matches!(c, '.' | '!' | '?' | '؟' | '。' | '…');
        let next_is_space = chars.get(i + 1).is_none_or(|(_, n)| n.is_whitespace());
        (is_end && next_is_space && i >= min_keep).then_some(byte + c.len_utf8())
    });
    if let Some(end) = sentence_end {
        return (window[..end].trim_end().to_string(), true);
    }
    let cut = window
        .rfind(char::is_whitespace)
        .filter(|&c| c > 0)
        .unwrap_or(window.len());
    (window[..cut].trim_end().to_string(), true)
}

/// A cut can leave a spoiler or emphasis open, which would swallow the rest of the embed.
fn balance_markup(mut s: String) -> String {
    if s.matches("||").count() % 2 == 1 {
        s.push_str("||");
    }
    let no_spoilers = s.replace("||", "");
    if no_spoilers.matches("**").count() % 2 == 1 {
        s.push_str("**");
    }
    if no_spoilers.replace("**", "").matches('*').count() % 2 == 1 {
        s.push('*');
    }
    s
}

/// Cleaned description for embeds. `None` when empty or when it contains explicit terms.
/// Truncated text ends with "..." and, when `url` is given, a "read more" link.
pub fn clean_description(raw: &str, url: Option<&str>) -> Option<String> {
    let text = collapse_blank_lines(&html_to_markdown(raw));
    if text.is_empty() || safety::contains_explicit_term(&text) {
        return None;
    }
    let (body, cut) = cut_at_boundary(&text, MAX_DESCRIPTION_CHARS);
    let mut out = balance_markup(body);
    if cut {
        out.push_str("...");
        if let Some(u) = url {
            out.push_str(&format!(" [{}]({u})", i18n::READ_MORE));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_and_spoilers() {
        let d = clean_description(
            "Hello<br>world &amp; <i>it</i> <b>bold</b> ~!secret!~ <a href=\"x\">link</a>",
            None,
        )
        .expect("d");
        assert_eq!(d, "Hello\nworld & *it* **bold** ||secret|| link");
    }

    #[test]
    fn empty_and_explicit_are_none() {
        assert!(clean_description("", None).is_none());
        assert!(clean_description("  <br> <br> ", None).is_none());
        assert!(clean_description("a sexual scene", None).is_none());
        assert!(clean_description("Essex is a county", None).is_some());
    }

    #[test]
    fn blank_lines_collapse() {
        assert_eq!(collapse_blank_lines("a\n\n\n\nb\n  \n c "), "a\n\nb\n\nc");
    }

    #[test]
    fn truncates_at_sentence_then_word() {
        let sentence = "This is a sentence that ends here. ".repeat(20);
        let d = clean_description(&sentence, Some("https://anilist.co/x")).expect("d");
        assert!(d.contains("](https://anilist.co/x)"));
        let body = d.split(" [").next().expect("body").trim_end_matches("...");
        assert!(body.chars().count() <= MAX_DESCRIPTION_CHARS);
        assert!(body.ends_with('.'), "cut after a full sentence: {body}");

        let words = "word ".repeat(200);
        let d = clean_description(&words, None).expect("d");
        assert!(d.ends_with("..."));
        assert!(!d.contains("wor..."), "cut on a word boundary");
    }

    #[test]
    fn short_text_has_no_ellipsis_or_link() {
        let d = clean_description("short", Some("https://x")).expect("d");
        assert_eq!(d, "short");
    }

    #[test]
    fn truncation_keeps_markup_balanced() {
        let raw = format!("start ~!{}!~ end", "word ".repeat(100));
        let d = clean_description(&raw, None).expect("d");
        assert_eq!(d.matches("||").count() % 2, 0);
        let raw = format!("<b>{}</b>", "word ".repeat(100));
        let d = clean_description(&raw, None).expect("d");
        assert_eq!(d.replace("||", "").matches("**").count() % 2, 0);
    }

    #[test]
    fn truncate_words_basic() {
        assert_eq!(truncate_words("short", 10), "short");
        let t = truncate_words("alpha beta gamma delta", 12);
        assert!(t.chars().count() <= 12 && t.ends_with('…'));
        assert!(!t.contains("gam"));
    }
}
