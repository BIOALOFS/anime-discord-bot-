//! Cards for works that only a fallback provider (MangaDex, MangaUpdates) has. Never reached unless
//! `ENABLE_FALLBACK_PROVIDERS=true`. The source is credited and the cover is attached, not hotlinked.

use super::buttons;
use crate::{
    error::{Context, Error},
    i18n,
    providers::{Providers, Work},
    ui::{
        embeds::Emb,
        pagination::{back_button, close_button, BACK, CLOSE},
        select::{select_row, Choice, PICK_ID},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use std::sync::Arc;

pub fn cover_name(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "cover.png"
    } else if bytes.starts_with(b"GIF8") {
        "cover.gif"
    } else if bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        "cover.webp"
    } else {
        "cover.jpg"
    }
}

pub fn card(w: &Work, has_cover: bool, cover_name: &str) -> Emb {
    let mut e = Emb::new().title(&w.title).url(w.url.clone());
    e = e.author(w.source.credit());
    e.footer = Some(w.source.credit().to_string());
    if has_cover {
        e = e.image(Some(format!("attachment://{cover_name}")));
    }
    if !w.genres.is_empty() {
        e = e.description(
            w.genres
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(" • "),
        );
    }
    if let Some(y) = w.year {
        e = e.field(i18n::F_YEAR, y.to_string(), true);
    }
    if let Some(s) = &w.status {
        e = e.field(i18n::F_STATUS, s, true);
    }
    if let Some(r) = w.rating {
        e = e.field(i18n::F_SCORE, format!("{r:.1}/10"), true);
    }
    e
}

struct FallbackView {
    providers: Arc<Providers>,
    works: Vec<Work>,
    chosen: Option<usize>,
    cover: Option<Arc<Vec<u8>>>,
    from_list: bool,
}

impl FallbackView {
    async fn open(&mut self, i: usize) {
        self.chosen = Some(i);
        self.cover = self.providers.cover(&self.works[i]).await;
    }
}

impl View for FallbackView {
    fn render(&self, enabled: bool) -> Page {
        match self.chosen {
            Some(i) => {
                let w = &self.works[i];
                let name = self.cover.as_ref().map_or("cover.jpg", |c| cover_name(c));
                let mut row = Vec::new();
                if self.from_list {
                    row.push(back_button(enabled));
                }
                if let Some(u) = &w.url {
                    row.push(
                        serenity::CreateButton::new_link(u)
                            .label(format!("🔗 {}", w.source.credit())),
                    );
                }
                row.push(close_button(enabled));
                let page = Page::embed(
                    card(w, self.cover.is_some(), name).build(),
                    vec![buttons(row)],
                );
                match &self.cover {
                    Some(c) => page.with_files(vec![serenity::CreateAttachment::bytes(
                        c.to_vec(),
                        name.to_string(),
                    )]),
                    None => page,
                }
            }
            None => {
                let body = self
                    .works
                    .iter()
                    .enumerate()
                    .map(|(i, w)| format!("**{}.** {} — {}", i + 1, w.title, w.source.credit()))
                    .collect::<Vec<_>>()
                    .join("\n");
                let choices = self
                    .works
                    .iter()
                    .enumerate()
                    .map(|(i, w)| Choice {
                        label: crate::ui::embeds::fit(&w.title, 100),
                        description: Some(w.source.credit().to_string()),
                        value: i.to_string(),
                    })
                    .collect();
                let mut rows: Vec<_> =
                    select_row(PICK_ID, i18n::PICK_PLACEHOLDER, choices, enabled)
                        .into_iter()
                        .collect();
                rows.push(buttons(vec![close_button(enabled)]));
                Page::embed(
                    Emb::new()
                        .title(i18n::PICK_PROMPT)
                        .description(body)
                        .build(),
                    rows,
                )
            }
        }
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        match id {
            CLOSE => Outcome::Close,
            BACK => {
                self.chosen = None;
                self.cover = None;
                Outcome::Update
            }
            PICK_ID => match values
                .first()
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|i| *i < self.works.len())
            {
                Some(i) => {
                    self.open(i).await;
                    Outcome::Update
                }
                None => Outcome::Keep,
            },
            _ => Outcome::Keep,
        }
    }
}

/// Search the fallback providers; `Ok(false)` when disabled or nothing was found.
pub async fn run(ctx: Context<'_>, query: &str) -> Result<bool, Error> {
    let providers = ctx.data().providers.clone();
    if !providers.enabled() {
        return Ok(false);
    }
    let works = providers.search(query).await;
    if works.is_empty() {
        return Ok(false);
    }
    let mut view = FallbackView {
        providers,
        from_list: works.len() > 1,
        works,
        chosen: None,
        cover: None,
    };
    if view.works.len() == 1 {
        view.open(0).await;
    }
    session::run(ctx, view).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Source;

    fn work() -> Work {
        Work {
            id: "mdex:a".into(),
            source: Source::MangaDex,
            title: "Some Series".into(),
            year: Some(2021),
            status: Some("ongoing".into()),
            rating: Some(8.25),
            url: Some("https://mangadex.org/title/a".into()),
            cover_url: Some("https://uploads.mangadex.org/covers/a/c.jpg".into()),
            genres: vec!["Action".into(), "Drama".into()],
        }
    }

    #[test]
    fn the_card_credits_the_source_and_attaches_the_cover() {
        let e = card(&work(), true, "cover.jpg");
        assert_eq!(e.footer.as_deref(), Some("Source: MangaDex"));
        assert_eq!(e.author.as_deref(), Some("Source: MangaDex"));
        assert_eq!(
            e.image.as_deref(),
            Some("attachment://cover.jpg"),
            "an attachment, never the provider's URL"
        );
        assert!(!e.image.as_deref().unwrap_or("").contains("mangadex.org"));
        let no_cover = card(&work(), false, "cover.jpg");
        assert!(no_cover.image.is_none());
        assert!(e.fields.iter().any(|f| f.1 == "2021"));
    }

    #[test]
    fn cover_file_names_follow_the_bytes() {
        assert_eq!(cover_name(b"\x89PNG..."), "cover.png");
        assert_eq!(cover_name(b"GIF89a"), "cover.gif");
        assert_eq!(cover_name(b"RIFF\0\0\0\0WEBPVP8"), "cover.webp");
        assert_eq!(cover_name(&[0xFF, 0xD8, 0xFF]), "cover.jpg");
    }
}
