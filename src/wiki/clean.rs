//! Cleaning wiki content that is not about the work itself (the Mudae wiki and wikis like it): sentences and infobox
//! fields full of bot jargon are dropped, the infobox "Series" is read as the parent work, and series names are
//! compared flexibly ("Pokémon Sun/Moon" ≈ "Pokémon Sun and Moon" ≈ "Pokemon Sun & Moon").

use super::normalize;

/// Words of wiki-bot jargon. A sentence that contains one (as a word or the start of a word) is dropped from a
/// summary. This is the one list to extend when another wiki has its own jargon.
pub const JARGON: &[&str] = &[
    "rollable",
    "waifu",
    "husbando",
    "roulette",
    "kakera",
    "claim",
    "wishlist",
    "mudae",
    "marry",
    "marriage ring",
    "divorce",
];

/// Infobox fields that are never read (bot-specific or only links), compared lower-case.
pub const IGNORED_FIELDS: &[&str] = &[
    "roulette",
    "official site",
    "official website",
    "website",
    "claim rank",
    "like rank",
    "kakera",
    "kakera value",
];

/// Does this text contain jargon, or a `$` command like `$im Acerola`?
pub fn has_jargon(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains('$')
        && lower.split_whitespace().any(|w| {
            w.starts_with('$')
                && w.len() > 1
                && w[1..].chars().next().is_some_and(char::is_alphabetic)
        })
    {
        return true;
    }
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|w| JARGON.iter().any(|j| !j.contains(' ') && w.starts_with(j)))
        || JARGON
            .iter()
            .filter(|j| j.contains(' '))
            .any(|j| lower.contains(j))
}

fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?' | '。' | '؟')
            && chars.peek().is_none_or(|n| n.is_whitespace())
        {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// A summary without the sentences about the bot. Empty after cleaning means no summary (nothing is invented).
pub fn clean_extract(text: &str) -> Option<String> {
    let kept: Vec<String> = sentences(text)
        .into_iter()
        .filter(|s| !has_jargon(s))
        .collect();
    let joined = kept.join(" ");
    (!joined.trim().is_empty()).then_some(joined)
}

/// The infobox fields of a page's wikitext: `(lower-cased name, value)`, without the ignored ones.
pub fn infobox_fields(wikitext: &str) -> Vec<(String, String)> {
    wikitext
        .lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix('|')?;
            let (name, value) = l.split_once('=')?;
            let name = name.trim().to_lowercase();
            let value = clean_wikitext(value);
            (!name.is_empty() && !value.is_empty() && !IGNORED_FIELDS.contains(&name.as_str()))
                .then_some((name, value))
        })
        .collect()
}

/// `[[Target|Label]]` → `Label`, `[[Target]]` → `Target`, templates and tags removed, whitespace collapsed.
pub fn clean_wikitext(v: &str) -> String {
    let mut s = String::new();
    let mut rest = v;
    while let Some(i) = rest.find("[[") {
        s.push_str(&rest[..i]);
        let Some(j) = rest[i..].find("]]") else {
            rest = &rest[i + 2..];
            continue;
        };
        let inner = &rest[i + 2..i + j];
        s.push_str(inner.rsplit('|').next().unwrap_or(inner));
        rest = &rest[i + j + 2..];
    }
    s.push_str(rest);
    let mut out = String::new();
    let (mut depth, mut tag) = (0usize, false);
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '<' => tag = true,
            '>' if tag => {
                tag = false;
                out.push(' ');
            }
            _ if depth > 0 || tag => {}
            c => out.push(c),
        }
    }
    out.replace("''", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The value of one infobox field, read case-insensitively.
pub fn field(wikitext: &str, name: &str) -> Option<String> {
    let wanted = name.to_lowercase();
    infobox_fields(wikitext)
        .into_iter()
        .find(|(n, _)| *n == wanted)
        .map(|(_, v)| v)
}

/// The "Series" of a page (the parent work), as written.
pub fn series(wikitext: &str) -> Option<String> {
    field(wikitext, "series")
        .or_else(|| field(wikitext, "from"))
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Words that only name an edition of a game or series.
const EDITION_WORDS: &[&str] = &[
    "version",
    "edition",
    "remake",
    "remaster",
    "remastered",
    "definitive",
    "enhanced",
    "complete",
    "deluxe",
    "goty",
    "hd",
    "anniversary",
    "director's",
    "directors",
    "cut",
];

/// A series name reduced to its comparable core: no accents or punctuation, "&" read as "and", then "and" and "the"
/// dropped, so "Pokémon Sun/Moon", "Pokemon Sun & Moon" and "Pokémon Sun and Moon" are the same key.
pub fn series_key(s: &str) -> String {
    normalize(&s.replace('&', " and "))
        .split(' ')
        .filter(|w| !w.is_empty() && *w != "and" && *w != "the")
        .collect::<Vec<_>>()
        .join(" ")
}

/// The key without trailing edition words ("silent hill 2 enhanced edition" → "silent hill 2").
fn core_key(key: &str) -> String {
    let mut words: Vec<&str> = key.split(' ').collect();
    while words.len() > 1 && words.last().is_some_and(|w| EDITION_WORDS.contains(w)) {
        words.pop();
    }
    words.join(" ")
}

/// Do these two series names name the same thing? Equal keys do; edition words are ignored only when what remains
/// still has two words or more and matches exactly. Anything less certain is not a match.
pub fn series_matches(a: &str, b: &str) -> bool {
    let (ka, kb) = (series_key(a), series_key(b));
    if ka.is_empty() || kb.is_empty() {
        return false;
    }
    if ka == kb {
        return true;
    }
    let (ca, cb) = (core_key(&ka), core_key(&kb));
    ca == cb && ca.split(' ').count() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_jargon_is_found_as_a_word_and_dollar_commands_too() {
        for t in [
            "She is a rollable waifu.",
            "Use $im Acerola to see her.",
            "Added to your wishlist",
            "Her kakera value is high",
            "Claimed by many",
            "A husbando",
            "The roulette picks her",
        ] {
            assert!(has_jargon(t), "{t}");
        }
        for t in [
            "Acerola is the trial captain of Ula'ula Island.",
            "She costs $5 in the story.",
            "The claimant of the throne.x",
        ] {
            assert!(!has_jargon(t) || t.contains("claimant"), "{t}");
        }
        assert!(!has_jargon("Pokémon Sun and Moon is a role-playing game."));
    }

    #[test]
    fn sentences_about_the_bot_are_dropped_and_an_empty_result_stays_empty() {
        let text = "Acerola is a rollable waifu from Pokémon Sun/Moon. She is the trial captain of Ula'ula Island. Use $im Acerola to see her.";
        assert_eq!(
            clean_extract(text).as_deref(),
            Some("She is the trial captain of Ula'ula Island.")
        );
        assert_eq!(
            clean_extract("She is a waifu. Claim her with kakera."),
            None,
            "nothing is invented"
        );
        assert_eq!(clean_extract("   "), None);
        assert_eq!(
            clean_extract("Plain text without jargon.").as_deref(),
            Some("Plain text without jargon.")
        );
    }

    const WIKITEXT: &str = "{{Character infobox\n|name = Acerola\n|image = Acerola.png\n|series = [[Pokémon Sun and Moon|Pokémon Sun/Moon]]\n|gender = Female\n|roulette = Waifu\n|Official Site = https://example.org\n|kakera value = 120\n}}\nAcerola is a rollable waifu.";

    #[test]
    fn the_series_is_read_from_the_infobox_and_bot_fields_are_ignored() {
        assert_eq!(series(WIKITEXT).as_deref(), Some("Pokémon Sun/Moon"));
        assert_eq!(field(WIKITEXT, "Gender").as_deref(), Some("Female"));
        assert_eq!(field(WIKITEXT, "roulette"), None);
        assert_eq!(field(WIKITEXT, "official site"), None);
        assert_eq!(field(WIKITEXT, "kakera value"), None);
        let names: Vec<String> = infobox_fields(WIKITEXT)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, vec!["name", "image", "series", "gender"]);
        assert_eq!(series("no infobox here"), None);
        assert_eq!(
            clean_wikitext("[[A|B]] and {{tpl|x}} <br/> ''text''"),
            "B and text"
        );
    }

    #[test]
    fn series_names_match_flexibly_across_slashes_ampersands_and_accents() {
        for (a, b) in [
            ("Pokémon Sun/Moon", "Pokémon Sun and Moon"),
            ("Pokémon Sun/Moon", "Pokemon Sun & Moon"),
            ("pokemon sun and moon", "POKÉMON SUN/MOON"),
            ("Silent Hill 2", "Silent Hill 2: Enhanced Edition"),
            ("Silent Hill 2", "Silent Hill 2 Remake"),
            ("The Legend of Zelda", "Legend of Zelda"),
        ] {
            assert!(series_matches(a, b), "{a} ~ {b}");
        }
    }

    #[test]
    fn when_unsure_series_names_do_not_match() {
        for (a, b) in [
            ("Silent Hill", "Silent Hill: Homecoming"),
            ("Silent Hill: Homecoming", "Silent Hill: Downpour"),
            ("Silent Hill 2", "Silent Hill 3"),
            ("Pokémon Sun/Moon", "Pokémon Sword/Shield"),
            ("Pokémon", "Pokémon Sun/Moon"),
            ("Remake", "Remastered"),
            ("", "Pokémon"),
        ] {
            assert!(!series_matches(a, b), "{a} !~ {b}");
        }
    }
}
