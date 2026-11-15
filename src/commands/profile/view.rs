//! The profile browser: one message, three embeds (banner, info, current item) and exactly three component rows:
//! buttons (first, previous, mode toggle, next, last), a dropdown of the items on the current page and a dropdown
//! to switch pages. Nothing else is shown in a profile.

use super::items::{self, Item, Section};
use super::util::{refresh_entries, refresh_works};
use crate::{
    anilist::{ContentPolicy, SafeAniList},
    commands::buttons,
    db::{
        custom::{self, StoredImage},
        game,
        profile::{self as pdb, Profile},
        suggest, works,
    },
    i18n,
    render::{BannerSpec, Renderer},
    ui::{
        embeds::{fit, Emb},
        pagination::{apply_nav, parse_nav, NAV_FIRST, NAV_LAST, NAV_NEXT, NAV_PREV},
        select::{select_row, Choice},
        session::{Outcome, Page, View},
        styles,
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Items per dropdown window (Discord allows 25 options).
pub const WINDOW: usize = 25;
/// Pages shown per page-dropdown group; two slots are kept for "previous / next group".
pub const PAGES_PER_GROUP: usize = 23;

const MODE_ID: &str = "mode";
const ITEM_ID: &str = "item";
const PAGE_ID: &str = "page";
const GROUP_PREV: &str = "grp:prev";
const GROUP_NEXT: &str = "grp:next";
const BANNER_NAME: &str = "banner";

/// The banner picture and the file name it is attached under (a GIF keeps its extension so it animates).
pub struct BannerFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub struct ProfileView {
    pub api: Arc<SafeAniList>,
    pub pool: SqlitePool,
    pub policy: ContentPolicy,
    pub renderer: Arc<Renderer>,
    pub viewer: i64,
    pub owner: i64,
    pub owner_name: String,
    pub owner_avatar: Option<String>,
    pub profile: Profile,
    pub section: Section,
    /// 0-based index of the shown item within the section.
    pub index: usize,
    pub total: i64,
    pub counts: (i64, i64),
    pub suggestions: i64,
    pub badges: Vec<styles::Badge>,
    /// The page (window of 25) that contains `index`.
    pub window: Vec<Item>,
    pub window_start: usize,
    pub custom: Option<StoredImage>,
    pub banner: Option<Arc<BannerFile>>,
    pub notice: Option<String>,
}

/// First index of the window that contains `index`.
pub fn window_start(index: usize) -> usize {
    index / WINDOW * WINDOW
}

pub fn page_count(total: usize) -> usize {
    total.div_ceil(WINDOW).max(1)
}

/// Label of the mode toggle: it names the OTHER mode.
pub fn toggle_label(current: Section) -> &'static str {
    match current {
        Section::Characters => i18n::BTN_SECTION_WORKS,
        Section::Works => i18n::BTN_SECTION_CHARACTERS,
    }
}

/// Options of the page dropdown. Up to 25 pages fit one menu; beyond that the menu shows the group of
/// `PAGES_PER_GROUP` pages that contains `current` plus "previous group" / "next group" entries.
pub fn page_options(current: usize, pages: usize, total: usize) -> Vec<Choice> {
    let range = |p: usize| {
        let (a, b) = (p * WINDOW + 1, ((p + 1) * WINDOW).min(total));
        format!("{a}–{b}")
    };
    let page = |p: usize| Choice {
        label: format!("{} {}", i18n::PAGE_WORD, p + 1),
        description: Some(range(p)),
        value: p.to_string(),
    };
    if pages <= 25 {
        return (0..pages).map(page).collect();
    }
    let group = current / PAGES_PER_GROUP;
    let (start, end) = (
        group * PAGES_PER_GROUP,
        ((group + 1) * PAGES_PER_GROUP).min(pages),
    );
    let mut out = Vec::new();
    if group > 0 {
        out.push(Choice {
            label: i18n::PAGES_PREV_GROUP.to_string(),
            description: None,
            value: GROUP_PREV.to_string(),
        });
    }
    out.extend((start..end).map(page));
    if end < pages {
        out.push(Choice {
            label: i18n::PAGES_NEXT_GROUP.to_string(),
            description: None,
            value: GROUP_NEXT.to_string(),
        });
    }
    out
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

/// The five buttons of row 1: first, previous, mode toggle, next, last.
pub fn button_row(
    section: Section,
    index: usize,
    total: usize,
    enabled: bool,
) -> serenity::CreateActionRow {
    let (at_start, at_end) = (index == 0, index + 1 >= total);
    let nav = |id: &str, label: &str, off: bool| {
        serenity::CreateButton::new(id)
            .label(label)
            .style(serenity::ButtonStyle::Secondary)
            .disabled(!enabled || off)
    };
    buttons(vec![
        nav(NAV_FIRST, "⏮", at_start),
        nav(NAV_PREV, "◀", at_start),
        serenity::CreateButton::new(MODE_ID)
            .label(toggle_label(section))
            .style(serenity::ButtonStyle::Primary)
            .disabled(!enabled),
        nav(NAV_NEXT, "▶", at_end),
        nav(NAV_LAST, "⏭", at_end),
    ])
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
            profile,
            section,
            index: 0,
            total: 0,
            counts: (0, 0),
            suggestions: 0,
            badges: Vec::new(),
            window: Vec::new(),
            window_start: 0,
            custom: None,
            banner: None,
            notice: None,
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

    /// Everything the page needs. The banner is prepared once, on the first load.
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
        self.suggestions = suggest::count_everywhere(&self.pool, self.owner)
            .await
            .unwrap_or(0);
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
            self.banner = self.prepare_banner().await.map(Arc::new);
        }
        Ok(())
    }

    /// An uploaded banner is attached as stored (a GIF stays animated); presets and colours are drawn.
    pub async fn prepare_banner(&self) -> Option<BannerFile> {
        let spec = BannerSpec::parse(self.profile.banner.as_deref());
        if let BannerSpec::Image(id) = &spec {
            if let Ok(Some(img)) = custom::get_image(&self.pool, *id).await {
                let ext = if img.animated { "gif" } else { "png" };
                return Some(BannerFile {
                    name: format!("{BANNER_NAME}.{ext}"),
                    bytes: img.bytes,
                });
            }
        }
        let spec = match spec {
            BannerSpec::Image(_) => BannerSpec::parse(None),
            other => other,
        };
        let r = self.renderer.banner(spec, String::new()).await?;
        Some(BannerFile {
            name: format!("{BANNER_NAME}.{}", r.ext),
            bytes: r.bytes,
        })
    }

    fn banner_embed(&self) -> Emb {
        let mut e = Emb::new();
        e.footer = None;
        e.colour = self.accent();
        e.image = self
            .banner
            .as_ref()
            .map(|b| format!("attachment://{}", b.name));
        e
    }

    /// Name, bio and details. Kept far below Discord's embed limits.
    pub fn info_embed(&self) -> Emb {
        let mut desc = Vec::new();
        if let Some(b) = &self.profile.bio {
            desc.push(fit(b, 300));
        }
        if let Some(q) = &self.profile.quote {
            desc.push(format!("*« {} »*", fit(q, 200)));
        }
        let mut e = Emb::new()
            .title(format!(
                "{} {}",
                i18n::PROFILE_TITLE,
                fit(&self.shown_name(), 60)
            ))
            .author(self.owner_name.clone())
            .author_icon(self.owner_avatar.clone())
            .colour(self.accent())
            .field(i18n::F_PROFILE_CHARACTERS, self.counts.0.to_string(), true)
            .field(i18n::F_PROFILE_WORKS, self.counts.1.to_string(), true)
            .field(
                i18n::F_PROFILE_PRIVACY,
                i18n::privacy_ar(&self.profile.privacy),
                true,
            )
            .field(
                i18n::F_PROFILE_JOINED,
                format!("<t:{}:D>", self.profile.created_at),
                true,
            );
        if self.suggestions > 0 {
            e = e.field(
                i18n::F_PROFILE_SUGGESTIONS,
                self.suggestions.to_string(),
                true,
            );
        }
        if !self.badges.is_empty() {
            e = e.field(
                i18n::F_PROFILE_BADGES,
                self.badges
                    .iter()
                    .map(|b| b.label)
                    .collect::<Vec<_>>()
                    .join(" · "),
                false,
            );
        }
        if !desc.is_empty() {
            e = e.description(desc.join("\n"));
        }
        e.footer = None;
        e
    }

    fn item_embed(&self) -> Emb {
        let footer = format!("{}/{}", self.index + 1, self.total.max(1));
        let mut e = match self.current() {
            Some(i) if i.visible => styles::item_card(i, self.total, self.own()),
            Some(_) => Emb::new().description(i18n::HIDDEN_NOTICE),
            None => Emb::new().description(i18n::PROFILE_EMPTY),
        };
        e.colour = self.accent();
        if let Some(img) = &self.custom {
            e.image = Some(format!("attachment://{}", custom_name(img)));
        }
        if let Some(d) = self
            .current()
            .filter(|i| i.visible)
            .and_then(|i| i.custom_description.as_deref())
        {
            e.description = Some(format!(
                "{}\n📝 {d}",
                e.description.take().unwrap_or_default()
            ));
        }
        e.footer(footer)
    }

    /// The three rows of the profile (pure given the loaded state).
    pub fn rows(&self, enabled: bool) -> Vec<serenity::CreateActionRow> {
        let mut rows = vec![button_row(
            self.section,
            self.index,
            self.total.max(0) as usize,
            enabled,
        )];
        rows.extend(select_row(
            ITEM_ID,
            i18n::SELECT_ITEM_PLACEHOLDER,
            window_choices(&self.window, self.window_start),
            enabled,
        ));
        let pages = page_count(self.total.max(0) as usize);
        rows.extend(select_row(
            PAGE_ID,
            i18n::SELECT_PAGE_PLACEHOLDER,
            page_options(self.index / WINDOW, pages, self.total.max(0) as usize),
            enabled,
        ));
        rows
    }

    fn files(&self) -> Vec<serenity::CreateAttachment> {
        let mut files = Vec::new();
        if let Some(b) = &self.banner {
            files.push(serenity::CreateAttachment::bytes(
                b.bytes.clone(),
                b.name.clone(),
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
            embeds: vec![
                self.banner_embed().build(),
                self.info_embed().build(),
                self.item_embed().build(),
            ],
            components: self.rows(enabled),
            files: self.files(),
        }
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
    fn render(&self, enabled: bool) -> Page {
        self.main_page(enabled)
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        match id {
            MODE_ID => {
                self.section = self.section.other();
                self.index = 0;
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
            PAGE_ID => {
                let Some(v) = values.first() else {
                    return Outcome::Keep;
                };
                let pages = page_count(self.total.max(0) as usize);
                let current = self.index / WINDOW;
                let page = match v.as_str() {
                    GROUP_NEXT => {
                        ((current / PAGES_PER_GROUP + 1) * PAGES_PER_GROUP).min(pages - 1)
                    }
                    GROUP_PREV => (current / PAGES_PER_GROUP).saturating_sub(1) * PAGES_PER_GROUP,
                    n => match n.parse::<usize>() {
                        Ok(p) if p < pages => p,
                        _ => return Outcome::Keep,
                    },
                };
                if page == current {
                    return Outcome::Keep;
                }
                self.index = page * WINDOW;
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

    fn row_json(row: &serenity::CreateActionRow) -> serde_json::Value {
        serde_json::to_value(row).expect("json")
    }

    #[test]
    fn windows_of_25_cover_every_index() {
        assert_eq!(
            (
                window_start(0),
                window_start(24),
                window_start(25),
                window_start(60)
            ),
            (0, 0, 25, 50)
        );
        assert_eq!(
            (page_count(0), page_count(1), page_count(25), page_count(26)),
            (1, 1, 1, 2)
        );
    }

    #[test]
    fn the_first_row_has_five_buttons_and_the_toggle_names_the_other_mode() {
        for (section, label) in [(Section::Characters, "أعمال"), (Section::Works, "شخصيات")]
        {
            let j = row_json(&button_row(section, 3, 12, true));
            let buttons = j["components"].as_array().expect("buttons");
            assert_eq!(buttons.len(), 5);
            let labels: Vec<&str> = buttons
                .iter()
                .map(|b| b["label"].as_str().unwrap_or(""))
                .collect();
            assert_eq!(labels[0], "⏮");
            assert_eq!(labels[1], "◀");
            assert_eq!(labels[2], label, "the toggle shows the other mode");
            assert_eq!(labels[3], "▶");
            assert_eq!(labels[4], "⏭");
        }
        assert_eq!(toggle_label(Section::Characters), "أعمال");
        assert_eq!(toggle_label(Section::Works), "شخصيات");
    }

    #[test]
    fn edge_buttons_are_disabled_at_the_ends() {
        let j = row_json(&button_row(Section::Characters, 0, 12, true));
        let b = j["components"].as_array().expect("buttons");
        assert_eq!(
            (
                b[0]["disabled"].as_bool(),
                b[1]["disabled"].as_bool(),
                b[3]["disabled"].as_bool()
            ),
            (Some(true), Some(true), Some(false))
        );
        let j = row_json(&button_row(Section::Characters, 11, 12, true));
        let b = j["components"].as_array().expect("buttons");
        assert_eq!(
            (b[3]["disabled"].as_bool(), b[4]["disabled"].as_bool()),
            (Some(true), Some(true))
        );
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
    fn item_dropdown_is_capped_numbered_unique_and_short() {
        let window: Vec<Item> = (0..40).map(|n| item(n, &"name ".repeat(80))).collect();
        let c = window_choices(&window, 25);
        assert_eq!(c.len(), WINDOW);
        assert!(c[0].label.starts_with("26. "), "numbered across pages");
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

    #[test]
    fn page_dropdown_lists_every_page_up_to_25() {
        let o = page_options(0, 3, 60);
        assert_eq!(
            o.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(),
            vec!["0", "1", "2"]
        );
        assert_eq!(o[2].description.as_deref(), Some("51–60"));
        assert_eq!(page_options(0, 25, 625).len(), 25);
        assert_eq!(page_options(0, 1, 5).len(), 1);
    }

    #[test]
    fn page_dropdown_groups_ranges_beyond_25_pages_and_stays_within_discords_cap() {
        let pages = 60;
        for current in [0, 22, 23, 45, 59] {
            let o = page_options(current, pages, pages * WINDOW);
            assert!(o.len() <= 25, "{current}: {}", o.len());
            let mut seen = std::collections::HashSet::new();
            assert!(
                o.iter().all(|c| seen.insert(c.value.clone())),
                "unique values"
            );
            let group = current / PAGES_PER_GROUP;
            assert!(
                o.iter().any(|c| c.value == group_first(group)),
                "the current group is shown"
            );
            assert_eq!(o.iter().any(|c| c.value == GROUP_PREV), group > 0);
            assert_eq!(
                o.iter().any(|c| c.value == GROUP_NEXT),
                (group + 1) * PAGES_PER_GROUP < pages
            );
        }
    }

    fn group_first(group: usize) -> String {
        (group * PAGES_PER_GROUP).to_string()
    }
}
