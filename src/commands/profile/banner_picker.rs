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
            .banner(BannerSpec::Preset(key.to_string()), String::new())
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

const CONFIRM: &str = "banner_confirm";
const CANCEL: &str = "banner_cancel";

/// The crop of an uploaded picture, with Confirm / Cancel. Nothing is stored before the member confirms.
struct Confirm {
    pool: SqlitePool,
    user: i64,
    image: Option<super::imaging::Processed>,
    preview: (String, Vec<u8>),
    done: Option<String>,
}

impl View for Confirm {
    fn ephemeral(&self) -> bool {
        true
    }

    fn render(&self, enabled: bool) -> Page {
        let mut e = Emb::new().title(i18n::BANNER_CONFIRM_TITLE).description(
            self.done
                .clone()
                .unwrap_or_else(|| i18n::BANNER_CONFIRM_HELP.to_string()),
        );
        e.footer = None;
        let e = e.image(Some(format!("attachment://{}", self.preview.0)));
        let live = enabled && self.done.is_none();
        Page::embed(
            e.build(),
            vec![buttons(vec![
                serenity::CreateButton::new(CONFIRM)
                    .label(i18n::BANNER_CONFIRM_BTN)
                    .style(serenity::ButtonStyle::Success)
                    .disabled(!live),
                serenity::CreateButton::new(CANCEL)
                    .label(i18n::BTN_CANCEL)
                    .style(serenity::ButtonStyle::Secondary)
                    .disabled(!live),
            ])],
        )
        .with_files(vec![serenity::CreateAttachment::bytes(
            self.preview.1.clone(),
            self.preview.0.clone(),
        )])
    }

    async fn on_interaction(&mut self, id: &str, _values: &[String]) -> Outcome {
        match id {
            CANCEL | CLOSE => {
                self.done = Some(i18n::BANNER_CANCELLED.to_string());
                Outcome::Update
            }
            CONFIRM => {
                let Some(image) = self.image.take() else {
                    return Outcome::Keep;
                };
                let result = async {
                    let stored = crate::db::custom::store_image(
                        &self.pool,
                        self.user,
                        &super::actions::new_image(image),
                        crate::db::custom::MAX_IMAGES_PER_USER,
                    )
                    .await?;
                    let img = match stored {
                        crate::db::custom::StoreOutcome::Stored(id)
                        | crate::db::custom::StoreOutcome::Existing(id) => id,
                        crate::db::custom::StoreOutcome::LimitReached => {
                            return Ok::<_, Error>(i18n::IMAGE_LIMIT.to_string())
                        }
                    };
                    pdb::set_banner(
                        &self.pool,
                        self.user,
                        Some(&BannerSpec::Image(img).encode()),
                    )
                    .await?;
                    Ok(crate::features::badges::announce(
                        &self.pool,
                        self.user,
                        i18n::BANNER_SET.to_string(),
                    )
                    .await)
                }
                .await;
                self.done = Some(result.unwrap_or_else(|e| {
                    tracing::warn!("banner confirm failed: {e:#}");
                    i18n::ERR_GENERIC.to_string()
                }));
                Outcome::Update
            }
            _ => Outcome::Keep,
        }
    }
}

/// Show the cropped banner for confirmation.
pub(super) async fn confirm(
    ctx: Context<'_>,
    processed: super::imaging::Processed,
) -> Result<(), Error> {
    let preview = ("banner-preview.png".to_string(), processed.bytes.clone());
    session::run(
        ctx,
        Confirm {
            pool: ctx.data().db.clone(),
            user: uid(ctx),
            image: Some(processed),
            preview,
            done: None,
        },
    )
    .await
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
    fn every_ready_made_banner_renders_a_1200_by_400_strip_and_they_differ() {
        let mut hashes = std::collections::HashSet::new();
        for (key, _) in BANNER_PRESETS {
            let out = crate::render::draw_banner_for_tests(key);
            assert_eq!((out.width, out.height), (1200, 400), "{key}");
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
