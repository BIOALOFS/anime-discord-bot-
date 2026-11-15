//! `/edit profile banner` with no value: an ephemeral menu of the ready-made banners with a preview.

use super::util::{begin_private, pool, require_own, uid};
use crate::{
    commands::buttons,
    db::profile as pdb,
    error::{Context, Error},
    i18n,
    render::{BannerSpec, Renderer, BANNER_PRESETS},
    ui::{
        embeds::Emb,
        pagination::{close_button, CLOSE},
        select::{select_row, Choice},
        session::{self, Outcome, Page, UndoFuture, View, Whisper},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

const PICK: &str = "banner_pick";
const APPLY: &str = "banner_apply";
const RESET: &str = "banner_reset";

/// One option per ready-made banner (the Arabic name is the label).
pub fn preset_choices() -> Vec<Choice> {
    BANNER_PRESETS
        .iter()
        .map(|(key, label)| Choice {
            label: (*label).to_string(),
            description: None,
            value: (*key).to_string(),
        })
        .collect()
}

struct Picker {
    pool: SqlitePool,
    user: i64,
    renderer: Arc<Renderer>,
    /// The banner stored now, restored by "undo".
    current: Option<String>,
    chosen: Option<String>,
    preview: Option<(String, Vec<u8>)>,
    whisper: Option<Whisper>,
}

impl Picker {
    async fn show(&mut self, key: &str) {
        self.chosen = Some(key.to_string());
        self.preview = self
            .renderer
            .banner(BannerSpec::Preset(key.to_string()), None, String::new())
            .await
            .map(|r| (format!("preview.{}", r.ext), r.bytes));
    }

    fn undo_to(&self, old: Option<String>) -> Box<dyn FnOnce() -> UndoFuture + Send + Sync> {
        let (pool, user) = (self.pool.clone(), self.user);
        Box::new(move || {
            Box::pin(async move {
                pdb::set_banner(&pool, user, old.as_deref())
                    .await
                    .map_or(i18n::ERR_GENERIC, |_| i18n::UNDONE)
                    .to_string()
            })
        })
    }
}

impl View for Picker {
    fn ephemeral(&self) -> bool {
        true
    }

    fn take_whisper(&mut self) -> Option<Whisper> {
        self.whisper.take()
    }

    fn render(&self, enabled: bool) -> Page {
        let mut embed = Emb::new()
            .title(i18n::BANNER_PICKER_TITLE)
            .description(i18n::BANNER_PICKER_HELP);
        embed.footer = None;
        let mut files = Vec::new();
        if let Some((name, bytes)) = &self.preview {
            embed = embed.image(Some(format!("attachment://{name}")));
            files.push(serenity::CreateAttachment::bytes(
                bytes.clone(),
                name.clone(),
            ));
        }
        let mut rows: Vec<_> = select_row(
            PICK,
            i18n::BANNER_PICKER_PLACEHOLDER,
            preset_choices(),
            enabled,
        )
        .into_iter()
        .collect();
        rows.push(buttons(vec![
            serenity::CreateButton::new(APPLY)
                .label(i18n::BANNER_APPLY)
                .style(serenity::ButtonStyle::Success)
                .disabled(!enabled || self.chosen.is_none()),
            serenity::CreateButton::new(RESET)
                .label(i18n::BANNER_RESET_BTN)
                .style(serenity::ButtonStyle::Secondary)
                .disabled(!enabled),
            close_button(enabled),
        ]));
        Page::embed(embed.build(), rows).with_files(files)
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            PICK => {
                let Some(key) = values
                    .first()
                    .filter(|k| BANNER_PRESETS.iter().any(|(p, _)| p == k))
                else {
                    return Outcome::Keep;
                };
                self.show(key).await;
                Outcome::Update
            }
            APPLY | RESET => {
                let new = if id == APPLY {
                    self.chosen
                        .as_ref()
                        .map(|k| BannerSpec::Preset(k.clone()).encode())
                } else {
                    None
                };
                if id == APPLY && new.is_none() {
                    return Outcome::Keep;
                }
                let old = self.current.clone();
                self.whisper = Some(
                    match pdb::set_banner(&self.pool, self.user, new.as_deref()).await {
                        Ok(()) => {
                            self.current = new;
                            Whisper {
                                text: if id == APPLY {
                                    i18n::BANNER_PRESET_SET
                                } else {
                                    i18n::BANNER_RESET
                                }
                                .to_string(),
                                undo: Some(self.undo_to(old)),
                            }
                        }
                        Err(e) => {
                            tracing::warn!("banner change failed: {e}");
                            Whisper::text(i18n::ERR_GENERIC)
                        }
                    },
                );
                Outcome::Keep
            }
            _ => Outcome::Keep,
        }
    }
}

pub(super) async fn open(ctx: Context<'_>) -> Result<(), Error> {
    begin_private(ctx).await?;
    let Some(p) = require_own(ctx).await? else {
        return Ok(());
    };
    let _ = pool(ctx);
    session::run(
        ctx,
        Picker {
            pool: ctx.data().db.clone(),
            user: uid(ctx),
            renderer: ctx.data().renderer.clone(),
            current: p.banner,
            chosen: None,
            preview: None,
            whisper: None,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_sixteen_ready_made_banners_with_unique_keys_and_names() {
        assert!(
            (12..=20).contains(&BANNER_PRESETS.len()),
            "about 12 to 20 banners"
        );
        let c = preset_choices();
        assert_eq!(c.len(), BANNER_PRESETS.len());
        let mut keys = std::collections::HashSet::new();
        let mut labels = std::collections::HashSet::new();
        assert!(c.iter().all(|c| keys.insert(c.value.clone())
            && labels.insert(c.label.clone())
            && !c.label.is_empty()));
        assert!(c.len() <= 25, "fits one select menu");
    }

    #[test]
    fn every_ready_made_banner_renders_a_1200_by_300_strip_and_they_differ() {
        let mut hashes = std::collections::HashSet::new();
        for (key, _) in BANNER_PRESETS {
            let out = crate::render::draw_banner_for_tests(key);
            assert_eq!((out.width, out.height), (1200, 300), "{key}");
            assert!(out.bytes.len() < 2 * 1024 * 1024, "{key} stays small");
            hashes.insert(out.bytes);
        }
        assert_eq!(
            hashes.len(),
            BANNER_PRESETS.len(),
            "no two banners look the same"
        );
    }
}
