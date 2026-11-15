//! The profile browser: one display mode, a card per item. A banner strip sits above the card, the buttons move
//! one item at a time and a dropdown jumps to any item of the current window of 25.

use super::{
    items::{self, Item, Section},
    util::{refresh_entries, refresh_works},
};
use crate::{
    anilist::{CharacterLookup, ContentPolicy, SafeAniList},
    commands::{buttons, roster::Roster},
    db::{
        custom::{self, StoredImage},
        game,
        profile::{self as pdb, Profile},
        works,
    },
    i18n,
    render::{BannerSpec, Rendered, Renderer},
    ui::{
        embeds::{fit, Emb},
        pagination::{apply_nav, close_button, nav_row, parse_nav, CLOSE},
        select::{select_row, Choice},
        session::{ModalSpec, Outcome, Page, View, Whisper},
        styles,
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Items per dropdown window (Discord allows 25 options).
pub const WINDOW: usize = 25;

const SECTION_ID: &str = "section";
const ITEM_ID: &str = "item";
const REPORT_ID: &str = "report";
const ROSTER_ID: &str = "open_roster";
const BANNER_FILE: &str = "banner.png";

pub struct ProfileView {
    pub api: Arc<SafeAniList>,
    pub pool: SqlitePool,
    pub policy: ContentPolicy,
    pub renderer: Arc<Renderer>,
    pub viewer: i64,
    pub owner: i64,
    pub owner_name: String,
    pub owner_avatar: Option<String>,
    pub guild: Option<i64>,
    pub profile: Profile,
    pub section: Section,
    /// 0-based index of the shown item within the section.
    pub index: usize,
    pub total: i64,
    pub counts: (i64, i64),
    pub badges: Vec<styles::Badge>,
    /// The dropdown window that contains `index`.
    pub window: Vec<Item>,
    pub window_start: usize,
    pub custom: Option<StoredImage>,
    pub banner: Option<Arc<Rendered>>,
    pub roster: Option<Roster>,
    pub notice: Option<String>,
    pub whisper: Option<Whisper>,
}

/// First index of the window that contains `index`.
pub fn window_start(index: usize) -> usize {
    index / WINDOW * WINDOW
}

/// Dropdown options for a window: numbered labels, unique keys, nothing over Discord's limits.
pub fn window_choices(window: &[Item], window_start: usize) -> Vec<Choice> {
    window
        .iter()
        .enumerate()
        .filter(|(_, i)| i.visible)
        .take(WINDOW)
        .map(|(n, i)| Choice {
            label: fit(&format!("{}. {}", window_start + n + 1, i.name), 100),
            description: i
                .display_title()
                .or(i.subtitle.as_deref())
                .map(|d| fit(d, 100)),
            value: i.key.clone(),
        })
        .collect()
}

impl ProfileView {
    pub fn new(
        ctx: crate::error::Context<'_>,
        profile: Profile,
        owner: i64,
        owner_name: String,
        owner_avatar: Option<String>,
        section: Section,
    ) -> Self {
        Self {
            api: ctx.data().anilist.clone(),
            pool: ctx.data().db.clone(),
            policy: crate::commands::policy(ctx),
            renderer: ctx.data().renderer.clone(),
            viewer: super::util::uid(ctx),
            owner,
            owner_name,
            owner_avatar,
            guild: ctx.guild_id().map(|g| g.get() as i64),
            profile,
            section,
            index: 0,
            total: 0,
            counts: (0, 0),
            badges: Vec::new(),
            window: Vec::new(),
            window_start: 0,
            custom: None,
            banner: None,
            roster: None,
            notice: None,
            whisper: None,
        }
    }

    fn accent(&self) -> Option<u32> {
        self.profile.accent_color.map(|c| c as u32)
    }

    fn own(&self) -> bool {
        self.viewer == self.owner
    }

    /// The name on the card: the personal title when set.
    pub fn shown_name(&self) -> String {
        self.profile
            .display_title
            .clone()
            .unwrap_or_else(|| self.owner_name.clone())
    }

    fn current(&self) -> Option<&Item> {
        self.window
            .get(self.index.saturating_sub(self.window_start))
    }

    /// Everything the page needs. The banner is drawn once, on the first load.
    pub async fn load(&mut self) -> Result<(), pdb::ProfileError> {
        let (chars, works_n) = (
            pdb::count(&self.pool, self.owner).await?,
            works::count(&self.pool, self.owner).await?,
        );
        self.counts = (chars, works_n);
        let watched = works::count_filtered(&self.pool, self.owner, None, Some("watched")).await?;
        let correct = game::total_correct(&self.pool, self.owner)
            .await
            .unwrap_or(0);
        self.badges = styles::badges(chars, works_n, watched, correct);
        self.total = items::count(&self.pool, self.owner, self.section, None, None).await?;
        self.index = self.index.min((self.total as usize).saturating_sub(1));
        self.window_start = window_start(self.index);
        let (offset, limit) = (self.window_start as i64, WINDOW as i64);
        // Re-check stored items against AniList; cached data is used if it is unreachable.
        self.window = match self.section {
            Section::Characters => {
                let raw = pdb::list_page(&self.pool, self.owner, None, offset, limit).await?;
                refresh_entries(&self.api, &self.pool, self.policy, &raw).await;
                let raw = pdb::list_page(&self.pool, self.owner, None, offset, limit).await?;
                raw.iter().map(Item::from_character).collect()
            }
            Section::Works => {
                let raw =
                    works::list_page(&self.pool, self.owner, None, None, offset, limit).await?;
                refresh_works(&self.api, &self.pool, self.policy, &raw).await;
                let raw =
                    works::list_page(&self.pool, self.owner, None, None, offset, limit).await?;
                raw.iter().map(Item::from_work).collect()
            }
        };
        self.custom = match self
            .current()
            .filter(|i| i.section == Section::Characters && i.visible)
        {
            Some(i) => match i.character_id() {
                Some(id) => custom::image_of_character(&self.pool, self.owner, id)
                    .await
                    .ok()
                    .flatten(),
                None => None,
            },
            None => None,
        };
        if self.banner.is_none() {
            self.banner = self.render_banner().await.map(Arc::new);
        }
        Ok(())
    }

    /// The 1200x300 strip: the stored upload, colour or preset (an upload only while its picture exists).
    pub async fn render_banner(&self) -> Option<Rendered> {
        let spec = BannerSpec::parse(self.profile.banner.as_deref());
        let uploaded = match &spec {
            BannerSpec::Image(id) => custom::get_image(&self.pool, *id)
                .await
                .ok()
                .flatten()
                .map(|i| i.bytes),
            _ => None,
        };
        let spec = match (&spec, &uploaded) {
            (BannerSpec::Image(_), None) => BannerSpec::parse(None),
            _ => spec,
        };
        self.renderer
            .banner(spec, uploaded, self.shown_name())
            .await
    }

    fn banner_embed(&self) -> Emb {
        let mut e = styles::header(
            &self.profile,
            &self.shown_name(),
            self.owner_avatar.as_deref(),
            self.counts,
            &self.badges,
        );
        e.title = None;
        e.colour = self.accent();
        e.image = self
            .banner
            .as_ref()
            .map(|_| format!("attachment://{BANNER_FILE}"));
        e
    }

    fn card_embed(&self) -> Emb {
        let footer = format!(
            "{}/{} · {}",
            self.index + 1,
            self.total.max(1),
            i18n::EDIT_HINT
        );
        let mut e = match self.current() {
            Some(i) if i.visible => styles::item_card(i, self.total, self.own()),
            Some(_) => Emb::new().description(i18n::HIDDEN_NOTICE),
            None => Emb::new().description(i18n::PROFILE_EMPTY),
        };
        e.colour = self.accent();
        if let (Some(img), Some(i)) = (&self.custom, self.current()) {
            e.image = Some(format!("attachment://{}", custom_name(img)));
            if let Some(d) = &i.custom_description {
                e.description = Some(format!(
                    "{}\n📝 {d}",
                    e.description.take().unwrap_or_default()
                ));
            }
        } else if let Some(i) = self.current().filter(|i| i.visible) {
            if let Some(d) = &i.custom_description {
                e.description = Some(format!(
                    "{}\n📝 {d}",
                    e.description.take().unwrap_or_default()
                ));
            }
        }
        e.footer(footer)
    }

    /// Components of the main page (pure given the loaded state).
    pub fn rows(&self, enabled: bool) -> Vec<serenity::CreateActionRow> {
        let mut rows: Vec<_> = select_row(
            ITEM_ID,
            i18n::SELECT_ITEM_PLACEHOLDER,
            window_choices(&self.window, self.window_start),
            enabled,
        )
        .into_iter()
        .collect();
        if self.total > 1 {
            rows.push(nav_row(self.index, self.total as usize, enabled));
        }
        let mut actions = vec![serenity::CreateButton::new(SECTION_ID)
            .label(match self.section {
                Section::Characters => i18n::BTN_SECTION_WORKS,
                Section::Works => i18n::BTN_SECTION_CHARACTERS,
            })
            .style(serenity::ButtonStyle::Secondary)
            .disabled(!enabled)];
        if self.current().is_some_and(|i| i.visible) {
            actions.push(
                serenity::CreateButton::new(ROSTER_ID)
                    .label(i18n::BTN_ROSTER)
                    .emoji('👥')
                    .style(serenity::ButtonStyle::Secondary)
                    .disabled(!enabled),
            );
        }
        if self.custom.is_some() && !self.own() {
            actions.push(
                serenity::CreateButton::new(REPORT_ID)
                    .label(i18n::BTN_REPORT)
                    .emoji('🚩')
                    .style(serenity::ButtonStyle::Danger)
                    .disabled(!enabled),
            );
        }
        actions.push(close_button(enabled));
        rows.push(buttons(actions));
        rows
    }

    fn files(&self) -> Vec<serenity::CreateAttachment> {
        let mut files = Vec::new();
        if let Some(b) = &self.banner {
            files.push(serenity::CreateAttachment::bytes(
                b.bytes.clone(),
                BANNER_FILE,
            ));
        }
        if let Some(img) = &self.custom {
            files.push(serenity::CreateAttachment::bytes(
                img.bytes.clone(),
                custom_name(img),
            ));
        }
        files
    }

    pub fn main_page(&self, enabled: bool) -> Page {
        Page {
            content: self.notice.clone(),
            embeds: vec![self.banner_embed().build(), self.card_embed().build()],
            components: self.rows(enabled),
            files: self.files(),
        }
    }

    async fn open_roster(&mut self) -> Outcome {
        let Some(item) = self.current().filter(|i| i.visible).cloned() else {
            return Outcome::Keep;
        };
        let started = match item.section {
            Section::Characters => match item
                .character_id()
                .filter(|id| *id < crate::providers::PROVIDER_CHARACTER_MIN)
            {
                Some(id) => match self.api.character_by_id(id, self.policy).await {
                    Ok(CharacterLookup::Found(c)) => {
                        Roster::start(self.api.clone(), self.policy, &c).await
                    }
                    Ok(_) => Ok(None),
                    Err(e) => Err(e),
                },
                None => Ok(None),
            },
            Section::Works => match works::anilist_id(&item.key) {
                Some(id) => Roster::for_work(self.api.clone(), self.policy, id, &item.name)
                    .await
                    .map(Some),
                None => Ok(None),
            },
        };
        match started {
            Ok(Some(r)) => self.roster = Some(r.with_profile(self.pool.clone(), self.viewer)),
            Ok(None) => self.notice = Some(i18n::ROSTER_NO_WORKS.to_string()),
            Err(e) => self.notice = Some(crate::commands::ani_message(&e).to_string()),
        }
        Outcome::Update
    }
}

fn custom_name(img: &StoredImage) -> String {
    format!(
        "custom-{}.{}",
        img.id,
        if img.animated { "gif" } else { "png" }
    )
}

impl View for ProfileView {
    fn take_whisper(&mut self) -> Option<Whisper> {
        self.whisper.take()
    }

    fn render(&self, enabled: bool) -> Page {
        match &self.roster {
            Some(r) => r.render(enabled),
            None => self.main_page(enabled),
        }
    }

    fn modal_for(&self, id: &str) -> Option<ModalSpec> {
        self.roster.as_ref().and_then(|r| r.modal_for(id))
    }

    async fn on_modal(&mut self, id: &str, input: String) -> Outcome {
        let step = match self.roster.as_mut() {
            Some(r) => r.on_modal(id, input).await,
            None => return Outcome::Keep,
        };
        step.apply(&mut self.roster)
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        if let Some(r) = self.roster.as_mut() {
            let step = r.handle(id, values).await;
            return step.apply(&mut self.roster);
        }
        match id {
            CLOSE => return Outcome::Close,
            ROSTER_ID => return self.open_roster().await,
            REPORT_ID => {
                if let Some(image) = self.custom.as_ref().map(|i| i.id) {
                    let text = match custom::report_image(
                        &self.pool,
                        image,
                        self.viewer,
                        self.guild,
                    )
                    .await
                    {
                        Ok(true) => i18n::REPORT_SENT,
                        Ok(false) => i18n::REPORT_ALREADY,
                        Err(_) => i18n::ERR_GENERIC,
                    };
                    self.whisper = Some(Whisper::text(text));
                }
                return Outcome::Keep;
            }
            ITEM_ID => {
                let Some(key) = values.first() else {
                    return Outcome::Keep;
                };
                let Some(n) = self.window.iter().position(|i| &i.key == key) else {
                    return Outcome::Keep;
                };
                self.index = self.window_start + n;
            }
            SECTION_ID => {
                self.section = self.section.other();
                self.index = 0;
            }
            other => {
                let Some(nav) = parse_nav(other) else {
                    return Outcome::Keep;
                };
                let next = apply_nav(nav, self.index, self.total.max(1) as usize);
                if next == self.index {
                    return Outcome::Keep;
                }
                self.index = next;
            }
        }
        if let Err(e) = self.load().await {
            tracing::warn!("profile view reload failed: {e}");
            self.notice = Some(i18n::ERR_GENERIC.to_string());
        }
        Outcome::Update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: usize, name: &str) -> Item {
        Item {
            section: Section::Characters,
            key: n.to_string(),
            position: n as i64,
            name: name.to_string(),
            native: None,
            subtitle: Some("Work".into()),
            subtitle_url: None,
            image_url: None,
            tier: None,
            note: None,
            custom_title: None,
            custom_description: None,
            custom_image_id: None,
            favourites: None,
            visible: true,
        }
    }

    #[test]
    fn windows_of_25_cover_every_index() {
        assert_eq!(window_start(0), 0);
        assert_eq!(window_start(24), 0);
        assert_eq!(window_start(25), 25);
        assert_eq!(window_start(60), 50);
    }

    #[test]
    fn card_navigation_moves_one_item_and_stops_at_the_ends() {
        use crate::ui::pagination::Nav;
        assert_eq!(apply_nav(Nav::Next, 0, 12), 1);
        assert_eq!(apply_nav(Nav::Prev, 0, 12), 0);
        assert_eq!(apply_nav(Nav::Last, 3, 12), 11);
        assert_eq!(apply_nav(Nav::Next, 11, 12), 11);
        assert_eq!(apply_nav(Nav::First, 7, 12), 0);
    }

    #[test]
    fn dropdown_options_are_capped_numbered_unique_and_short() {
        let window: Vec<Item> = (0..40).map(|n| item(n, &"name ".repeat(80))).collect();
        let c = window_choices(&window, 25);
        assert_eq!(c.len(), WINDOW);
        assert!(c[0].label.starts_with("26. "), "numbered across windows");
        assert!(c.iter().all(|c| c.label.chars().count() <= 100));
        let mut seen = std::collections::HashSet::new();
        assert!(c.iter().all(|c| seen.insert(c.value.clone())));
        let mut hidden = window;
        hidden[0].visible = false;
        assert!(
            !window_choices(&hidden, 0).iter().any(|c| c.value == "0"),
            "hidden items are not offered"
        );
    }
}
