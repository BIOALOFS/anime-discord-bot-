//! Result-picker select menu (up to 10 entries).

use super::{embeds::fit, format};
use crate::{
    anilist::models::{Character, Media},
    i18n,
};
use poise::serenity_prelude as serenity;

pub const PICK_ID: &str = "pick";
pub const MAX_OPTIONS: usize = 10;
pub const MAX_SELECT_OPTIONS: usize = 25;

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

/// Cast of a work: name, role as the description, character id as the value.
pub fn cast_choices(cast: &[(Character, String)]) -> Vec<Choice> {
    normalize_choices(
        cast.iter()
            .map(|(c, role)| Choice {
                label: fit(c.display_name(), 100),
                description: Some(fit(role, 100)).filter(|r| !r.is_empty()),
                value: c.id.to_string(),
            })
            .collect(),
    )
}

/// Make choices fit Discord's select menu: at most 25 options, labels and descriptions at most 100 characters,
/// unique non-empty values, no empty labels.
pub fn normalize_choices(choices: Vec<Choice>) -> Vec<Choice> {
    let mut seen = std::collections::HashSet::new();
    choices
        .into_iter()
        .filter(|c| {
            !c.value.is_empty() && !c.label.trim().is_empty() && seen.insert(c.value.clone())
        })
        .take(MAX_SELECT_OPTIONS)
        .map(|c| Choice {
            label: fit(&c.label, 100),
            description: c
                .description
                .map(|d| fit(&d, 100))
                .filter(|d| !d.is_empty()),
            value: fit(&c.value, 100),
        })
        .collect()
}

pub fn select_row(
    custom_id: &str,
    placeholder: &str,
    choices: Vec<Choice>,
    enabled: bool,
) -> Option<serenity::CreateActionRow> {
    let choices = normalize_choices(choices);
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
    fn cast_choices_are_capped_unique_and_carry_the_role() {
        let cast: Vec<(Character, String)> = (0..40)
            .map(|i| {
                (
                    serde_json::from_value(
                        json!({"id": i % 30, "name": {"full": "N".repeat(300)}}),
                    )
                    .expect("character"),
                    "MAIN".to_string(),
                )
            })
            .collect();
        let c = cast_choices(&cast);
        assert_eq!(c.len(), MAX_SELECT_OPTIONS, "25 options at most");
        let mut seen = std::collections::HashSet::new();
        assert!(
            c.iter().all(|c| seen.insert(c.value.clone())),
            "unique values"
        );
        assert!(c.iter().all(|c| c.label.chars().count() <= 100));
        assert_eq!(c[0].description.as_deref(), Some("MAIN"));
        assert!(cast_choices(&[]).is_empty());
    }

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

    fn choice(label: &str, value: &str) -> Choice {
        Choice {
            label: label.into(),
            description: Some("d".repeat(300)),
            value: value.into(),
        }
    }

    #[test]
    fn select_menus_are_capped_truncated_and_unique() {
        let many: Vec<Choice> = (0..40)
            .map(|i| choice(&format!("Name {i}"), &format!("anilist:{i}")))
            .collect();
        let out = normalize_choices(many);
        assert_eq!(out.len(), 25, "Discord allows at most 25 options");
        assert_eq!(out[0].value, "anilist:0");

        let long = normalize_choices(vec![choice(&"x".repeat(250), "a")]);
        assert!(
            long[0].label.chars().count() <= 100,
            "long names are cut to 100 characters"
        );
        assert!(long[0]
            .description
            .as_deref()
            .is_some_and(|d| d.chars().count() <= 100));

        let dup = normalize_choices(vec![
            choice("A", "same"),
            choice("B", "same"),
            choice("C", "other"),
        ]);
        assert_eq!(
            dup.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(),
            vec!["same", "other"],
            "values are unique"
        );

        assert!(
            normalize_choices(vec![choice("  ", "v"), choice("ok", "")]).is_empty(),
            "empty labels/values are dropped"
        );
        assert!(select_row("id", "p", Vec::new(), true).is_none());
        assert!(select_row("id", "p", vec![choice("A", "a")], true).is_some());
    }
}
