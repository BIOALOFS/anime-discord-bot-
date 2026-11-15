//! Profile card pieces: badges, the header embed and the item card.

use super::{embeds::Emb, format::thousands};
use crate::{
    commands::profile::items::{Item, Section},
    db::profile::Profile,
    i18n,
};

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
