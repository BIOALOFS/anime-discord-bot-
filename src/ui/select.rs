//! Result-picker select menu (up to 10 entries).

use super::{embeds::fit, format};
use crate::{
    anilist::models::{Character, Media},
    i18n,
};
use poise::serenity_prelude as serenity;

pub const PICK_ID: &str = "pick";
pub const MAX_OPTIONS: usize = 10;

pub struct Choice {
    pub label: String,
    pub description: Option<String>,
    pub value: String,
}

pub fn media_choices(list: &[Media]) -> Vec<Choice> {
    list.iter()
        .take(MAX_OPTIONS)
        .map(|m| {
            let mut parts = Vec::new();
            if let Some(f) = &m.format {
                parts.push(format::format_ar(f).to_string());
            }
            if let Some(y) = m.season_year {
                parts.push(y.to_string());
            }
            if m.average_score.is_some() {
                parts.push(
                    format::score_text(m.average_score)
                        .split(' ')
                        .next()
                        .unwrap_or("")
                        .to_string(),
                );
            }
            Choice {
                label: fit(m.title.main(), 100),
                description: (!parts.is_empty()).then(|| fit(&parts.join(" · "), 100)),
                value: m.id.to_string(),
            }
        })
        .collect()
}

pub fn character_choices(list: &[Character]) -> Vec<Choice> {
    list.iter()
        .take(MAX_OPTIONS)
        .map(|c| Choice {
            label: fit(c.display_name(), 100),
            description: c.main_work().map(|w| fit(w, 100)),
            value: c.id.to_string(),
        })
        .collect()
}

pub fn select_row(
    custom_id: &str,
    placeholder: &str,
    choices: Vec<Choice>,
    enabled: bool,
) -> Option<serenity::CreateActionRow> {
    if choices.is_empty() {
        return None;
    }
    let options = choices
        .into_iter()
        .map(|c| {
            let o = serenity::CreateSelectMenuOption::new(c.label, c.value);
            match c.description {
                Some(d) => o.description(d),
                None => o,
            }
        })
        .collect();
    Some(serenity::CreateActionRow::SelectMenu(
        serenity::CreateSelectMenu::new(
            custom_id,
            serenity::CreateSelectMenuKind::String { options },
        )
        .placeholder(placeholder)
        .disabled(!enabled),
    ))
}

pub fn pick_row(choices: Vec<Choice>, enabled: bool) -> Option<serenity::CreateActionRow> {
    select_row(PICK_ID, i18n::PICK_PLACEHOLDER, choices, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn media_choices_are_capped_and_labelled() {
        let list: Vec<Media> = (0..15)
            .map(|i| {
                serde_json::from_value(json!({
                    "id": i, "title": {"romaji": "T".repeat(300)}, "format": "TV", "seasonYear": 2020, "averageScore": 82
                }))
                .expect("media")
            })
            .collect();
        let c = media_choices(&list);
        assert_eq!(c.len(), MAX_OPTIONS);
        assert!(c[0].label.chars().count() <= 100);
        assert_eq!(c[0].description.as_deref(), Some("مسلسل · 2020 · 8.2/10"));
    }

    #[test]
    fn empty_choices_make_no_row() {
        assert!(pick_row(Vec::new(), true).is_none());
    }
}
