//! Profile display styles: the enum plus pure embed builders for the text/embed styles.

use super::{embeds::Emb, format::thousands};
use crate::{
    commands::profile::items::{Item, Section},
    db::profile::Profile,
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

/// Profile badges, computed from activity (nothing is stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    pub letter: char,
    pub label: &'static str,
}

pub fn badges(characters: i64, works: i64, watched: i64, guess_correct: i64) -> Vec<Badge> {
    let mut out = Vec::new();
    if characters + works >= 50 {
        out.push(Badge {
            letter: 'C',
            label: i18n::BADGE_COLLECTOR,
        });
    }
    if guess_correct >= 20 {
        out.push(Badge {
            letter: 'G',
            label: i18n::BADGE_GAMER,
        });
    }
    if watched >= 10 {
        out.push(Badge {
            letter: 'W',
            label: i18n::BADGE_WATCHER,
        });
    }
    out
}

fn subtitle_link(i: &Item) -> Option<String> {
    let s = i.subtitle.as_deref()?;
    Some(match &i.subtitle_url {
        Some(u) => format!("[{s}]({u})"),
        None => s.to_string(),
    })
}

/// Profile header shared by the list styles.
pub fn header(
    p: &Profile,
    owner_name: &str,
    avatar: Option<&str>,
    counts: (i64, i64),
    badges: &[Badge],
) -> Emb {
    let mut desc = Vec::new();
    if let Some(b) = &p.bio {
        desc.push(b.clone());
    }
    if let Some(q) = &p.quote {
        desc.push(format!("*« {q} »*"));
    }
    desc.push(i18n::profile_stats(
        counts.0,
        counts.1,
        i18n::privacy_ar(&p.privacy),
    ));
    if !badges.is_empty() {
        desc.push(
            badges
                .iter()
                .map(|b| b.label)
                .collect::<Vec<_>>()
                .join(" · "),
        );
    }
    Emb::new()
        .title(format!("{} {owner_name}", i18n::PROFILE_TITLE))
        .author(owner_name)
        .author_icon(avatar.map(str::to_string))
        .colour(p.accent_color.map(|c| c as u32))
        .description(desc.join("\n"))
}

pub fn names_embed(base: Emb, items: &[&Item], page: usize, pages: usize) -> Emb {
    let lines: Vec<String> = items
        .iter()
        .map(|i| {
            let tier = i
                .tier
                .as_deref()
                .map(|t| format!("`{t}` "))
                .unwrap_or_default();
            let native = i
                .native
                .as_deref()
                .filter(|n| !n.is_empty())
                .map(|n| format!(" ({n})"))
                .unwrap_or_default();
            let title = i
                .display_title()
                .map(|t| format!(" ✨ *{t}*"))
                .unwrap_or_default();
            let sub = subtitle_link(i)
                .map(|w| format!(" — {w}"))
                .unwrap_or_default();
            format!("**{}.** {tier}{}{native}{title}{sub}", i.position, i.name)
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

pub fn compact_embed(base: Emb, items: &[&Item], total: i64) -> Emb {
    let lines: Vec<String> = items
        .iter()
        .map(|i| format!("**{}.** {}", i.position, i.name))
        .collect();
    let mut emb = base;
    let head = emb.description.take().unwrap_or_default();
    let mut body = if lines.is_empty() {
        i18n::PROFILE_EMPTY.to_string()
    } else {
        lines.join("\n")
    };
    let more = total - items.len() as i64;
    if more > 0 {
        body.push_str(&format!("\n{}", i18n::more_n(more)));
    }
    emb.description = Some(format!("{head}\n\n{body}"));
    emb
}

/// One item as a simple card, built from cached data only (no AniList call at render time).
pub fn item_card(i: &Item, total: i64, own: bool) -> Emb {
    let name = match i.native.as_deref().filter(|n| !n.is_empty()) {
        Some(n) => format!("{} ({n})", i.name),
        None => i.name.clone(),
    };
    let mut desc = Vec::new();
    if let Some(t) = i.display_title() {
        desc.push(format!("✨ **{t}**"));
    }
    if let Some(w) = subtitle_link(i) {
        desc.push(if i.section == Section::Characters {
            format!("📺 {w}")
        } else {
            w
        });
    }
    if own {
        if let Some(n) = i.note.as_deref() {
            desc.push(format!("📝 **{}:** {n}", i18n::NOTE_LABEL));
        }
    }
    let mut emb = Emb::new()
        .title(name)
        .author(i18n::position_of(i.position, total))
        .image(i.image_url.clone());
    if !desc.is_empty() {
        emb = emb.description(desc.join("\n"));
    }
    if let Some(f) = i.favourites {
        emb = emb.field("❤", thousands(f), true);
    }
    emb
}

/// A bare thumbnail embed, used for the "next items" strip of the gallery style.
pub fn thumb_embed(i: &Item) -> Emb {
    let mut emb = Emb::new()
        .title(i.name.clone())
        .thumbnail(i.image_url.clone());
    emb.footer = None;
    emb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(pos: i64, name: &str) -> Item {
        Item {
            section: Section::Characters,
            key: pos.to_string(),
            position: pos,
            name: name.into(),
            native: Some("名".into()),
            subtitle: Some("Work".into()),
            subtitle_url: Some("https://anilist.co/anime/1".into()),
            image_url: Some("https://img".into()),
            tier: None,
            note: Some("my note".into()),
            custom_title: None,
            custom_description: None,
            custom_image_id: None,
            favourites: Some(1500),
            visible: true,
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
            template: "classic".into(),
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
    fn names_list_is_numbered_linked_and_shows_personal_titles() {
        let e1 = item(1, "Alpha");
        let mut e2 = item(2, "Beta");
        e2.tier = Some("S".into());
        e2.custom_title = Some("Best girl".into());
        let emb = names_embed(
            header(&profile(), "User", None, (2, 0), &[]),
            &[&e1, &e2],
            0,
            1,
        );
        let d = emb.description.as_deref().expect("desc");
        assert!(d.contains("**1.** Alpha (名) — [Work](https://anilist.co/anime/1)"));
        assert!(d.contains("**2.** `S` Beta (名) ✨ *Best girl*"));
        assert!(d.contains("bio") && d.contains("« quote »"));
        assert_eq!(emb.colour, Some(0xff0000));
    }

    #[test]
    fn compact_shows_more_counter() {
        let es: Vec<Item> = (1..=5).map(|i| item(i, "X")).collect();
        let refs: Vec<&Item> = es.iter().collect();
        let emb = compact_embed(header(&profile(), "U", None, (143, 0), &[]), &refs, 143);
        assert!(emb
            .description
            .as_deref()
            .is_some_and(|d| d.contains("+138")));
    }

    #[test]
    fn item_card_is_simple_and_notes_are_private() {
        let mut i = item(12, "Alpha");
        i.custom_title = Some("Mine".into());
        let own = item_card(&i, 143, true);
        assert_eq!(own.author.as_deref(), Some("12 / 143"));
        let d = own.description.as_deref().expect("d");
        assert!(d.contains("✨ **Mine**") && d.contains("📺 [Work]") && d.contains("my note"));
        let other = item_card(&i, 143, false);
        assert!(
            !other
                .description
                .as_deref()
                .unwrap_or("")
                .contains("my note"),
            "notes are only for the owner"
        );
    }

    #[test]
    fn badges_follow_activity() {
        assert!(badges(0, 0, 0, 0).is_empty());
        let b = badges(40, 15, 10, 20);
        assert_eq!(
            b.iter().map(|b| b.letter).collect::<Vec<_>>(),
            vec!['C', 'G', 'W']
        );
        assert_eq!(badges(10, 0, 9, 19).len(), 0);
    }
}
