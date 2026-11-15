//! Profile display styles: the enum plus pure embed builders for the text/embed styles.

use super::{embeds::Emb, format::thousands};
use crate::{
    db::profile::{Entry, Profile},
    i18n,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Collage,
    Names,
    Cards,
    Gallery,
    Podium,
    Compact,
    Tier,
    Random,
}

impl Style {
    pub const ALL: [Style; 8] = [
        Style::Collage,
        Style::Names,
        Style::Cards,
        Style::Gallery,
        Style::Podium,
        Style::Compact,
        Style::Tier,
        Style::Random,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Collage => "collage",
            Self::Names => "names",
            Self::Cards => "cards",
            Self::Gallery => "gallery",
            Self::Podium => "podium",
            Self::Compact => "compact",
            Self::Tier => "tier",
            Self::Random => "random",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|st| st.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Collage => i18n::STYLE_COLLAGE,
            Self::Names => i18n::STYLE_NAMES,
            Self::Cards => i18n::STYLE_CARDS,
            Self::Gallery => i18n::STYLE_GALLERY,
            Self::Podium => i18n::STYLE_PODIUM,
            Self::Compact => i18n::STYLE_COMPACT,
            Self::Tier => i18n::STYLE_TIER,
            Self::Random => i18n::STYLE_RANDOM,
        }
    }

    /// Characters per page.
    pub fn per_page(self) -> usize {
        match self {
            Self::Names => 25,
            Self::Cards | Self::Gallery | Self::Random => 1,
            Self::Collage => 20,
            Self::Podium => 12,
            Self::Compact => 5,
            Self::Tier => 40,
        }
    }

    /// Styles shown as one page regardless of size.
    pub fn single_page(self) -> bool {
        matches!(self, Self::Compact | Self::Random | Self::Tier)
    }
}

pub fn style_strs() -> Vec<&'static str> {
    Style::ALL.iter().map(|s| s.as_str()).collect()
}

fn work_link(e: &Entry) -> Option<String> {
    let w = e.main_work.as_deref()?;
    Some(match &e.main_work_url {
        Some(u) => format!("[{w}]({u})"),
        None => w.to_string(),
    })
}

/// Profile header shared by the list styles.
pub fn header(p: &Profile, owner_name: &str, avatar: Option<&str>, total: i64) -> Emb {
    let mut desc = Vec::new();
    if let Some(b) = &p.bio {
        desc.push(b.clone());
    }
    if let Some(q) = &p.quote {
        desc.push(format!("*« {q} »*"));
    }
    desc.push(i18n::profile_stats(total, i18n::privacy_ar(&p.privacy)));
    Emb::new()
        .title(format!("{} {owner_name}", i18n::PROFILE_TITLE))
        .author(owner_name)
        .author_icon(avatar.map(str::to_string))
        .colour(p.accent_color.map(|c| c as u32))
        .description(desc.join("\n"))
}

pub fn names_embed(base: Emb, entries: &[&Entry], page: usize, pages: usize) -> Emb {
    let lines: Vec<String> = entries
        .iter()
        .map(|e| {
            let tier = e
                .tier
                .as_deref()
                .map(|t| format!("`{t}` "))
                .unwrap_or_default();
            let native = e
                .name_native
                .as_deref()
                .filter(|n| !n.is_empty())
                .map(|n| format!(" ({n})"))
                .unwrap_or_default();
            let work = work_link(e).map(|w| format!(" — {w}")).unwrap_or_default();
            format!("**{}.** {tier}{}{native}{work}", e.position, e.name())
        })
        .collect();
    let mut emb = base;
    let head = emb.description.take().unwrap_or_default();
    let body = if lines.is_empty() {
        i18n::PROFILE_EMPTY.to_string()
    } else {
        lines.join("\n")
    };
    emb.description = Some(format!("{head}\n\n{body}"));
    emb.footer(format!("{} · {}/{}", i18n::POWERED_BY, page + 1, pages))
}

pub fn compact_embed(base: Emb, entries: &[&Entry], total: i64) -> Emb {
    let lines: Vec<String> = entries
        .iter()
        .map(|e| format!("**{}.** {}", e.position, e.name()))
        .collect();
    let mut emb = base;
    let head = emb.description.take().unwrap_or_default();
    let mut body = if lines.is_empty() {
        i18n::PROFILE_EMPTY.to_string()
    } else {
        lines.join("\n")
    };
    let more = total - entries.len() as i64;
    if more > 0 {
        body.push_str(&format!("\n{}", i18n::more_n(more)));
    }
    emb.description = Some(format!("{head}\n\n{body}"));
    emb
}

/// Fallback card built only from cached metadata (AniList unreachable): same simplified layout.
pub fn cached_card(e: &Entry, total: i64, note: Option<&str>) -> Emb {
    let name = match e.name_native.as_deref().filter(|n| !n.is_empty()) {
        Some(n) => format!("{} ({n})", e.name()),
        None => e.name().to_string(),
    };
    let mut desc = Vec::new();
    if let Some(w) = work_link(e) {
        desc.push(format!("📺 {w}"));
    }
    if let Some(n) = note {
        desc.push(format!("📝 **{}:** {n}", i18n::NOTE_LABEL));
    }
    let mut emb = Emb::new()
        .title(name)
        .author(i18n::position_of(e.position, total))
        .image(e.image_url.clone());
    if !desc.is_empty() {
        emb = emb.description(desc.join("\n"));
    }
    if let Some(f) = e.favourites {
        emb = emb.field("❤", thousands(f), true);
    }
    emb
}

/// A bare thumbnail embed, used for the "next characters" strip of the gallery style.
pub fn thumb_embed(e: &Entry) -> Emb {
    let mut emb = Emb::new().title(e.name()).thumbnail(e.image_url.clone());
    emb.footer = None;
    emb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pos: i64, name: &str) -> Entry {
        Entry {
            character_id: pos,
            position: pos,
            group_id: None,
            tier: None,
            note: None,
            name_romaji: Some(name.into()),
            name_native: Some("名".into()),
            image_url: Some("https://img".into()),
            is_safe: Some(true),
            main_work: Some("Work".into()),
            main_work_url: Some("https://anilist.co/anime/1".into()),
            favourites: Some(1500),
            fetched_at: Some(1),
        }
    }

    fn profile() -> Profile {
        Profile {
            user_id: 1,
            bio: Some("bio".into()),
            accent_color: Some(0xff0000),
            quote: Some("quote".into()),
            banner: None,
            default_style: "names".into(),
            privacy: "public".into(),
        }
    }

    #[test]
    fn style_round_trip() {
        for s in Style::ALL {
            assert_eq!(Style::parse(s.as_str()), Some(s));
            assert!(!s.label().is_empty());
        }
        assert_eq!(Style::parse("nope"), None);
        assert_eq!(Style::Collage.per_page(), 20);
        assert_eq!(Style::Names.per_page(), 25);
    }

    #[test]
    fn names_list_is_numbered_and_linked() {
        let e1 = entry(1, "Alpha");
        let mut e2 = entry(2, "Beta");
        e2.tier = Some("S".into());
        let emb = names_embed(header(&profile(), "User", None, 2), &[&e1, &e2], 0, 1);
        let d = emb.description.as_deref().expect("desc");
        assert!(d.contains("**1.** Alpha (名) — [Work](https://anilist.co/anime/1)"));
        assert!(d.contains("**2.** `S` Beta"));
        assert!(d.contains("bio") && d.contains("« quote »"));
        assert_eq!(emb.colour, Some(0xff0000));
    }

    #[test]
    fn compact_shows_more_counter() {
        let es: Vec<Entry> = (1..=5).map(|i| entry(i, "X")).collect();
        let refs: Vec<&Entry> = es.iter().collect();
        let emb = compact_embed(header(&profile(), "U", None, 143), &refs, 143);
        assert!(emb
            .description
            .as_deref()
            .is_some_and(|d| d.contains("+138")));
    }

    #[test]
    fn cached_card_is_simple() {
        let e = entry(12, "Alpha");
        let emb = cached_card(&e, 143, Some("my note"));
        assert_eq!(emb.author.as_deref(), Some("12 / 143"));
        assert!(emb.image.is_some());
        let d = emb.description.as_deref().expect("d");
        assert!(d.contains("📺 [Work]") && d.contains("my note"));
    }
}
