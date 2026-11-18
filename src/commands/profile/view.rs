//! The profile browser: one message, three embeds (banner, info, current item) and exactly three component rows:
//! buttons (first, previous, mode toggle, next, last), a dropdown of the items on the current page and a dropdown
//! to switch pages. Nothing else is shown in a profile.

use super::items::{self, Item, Section};
use super::util::{refresh_entries, refresh_works};
use crate::{
    anilist::{ContentPolicy, SafeAniList},
    commands::buttons,
    db::{
        custom::StoredImage,
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
        v2,
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
/// Container colour when the member has not chosen one.
const DEFAULT_ACCENT: u32 = 0x3DB4F2;

/// The banner picture and the file name it is attached under (a GIF keeps its extension so it animates).
/// Everything attached to one message edit must stay under Discord's upload limit (10 MB on the smallest tier).
pub const MAX_ATTACH_BYTES: usize = 9 * 1024 * 1024;

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
    pub profile: Profile,
    pub section: Section,
    /// 0-based index of the shown item within the section.
    pub index: usize,
    pub total: i64,
    pub counts: (i64, i64),
    pub suggestions: i64,
    pub badges: Vec<&'static str>,
    /// The page (window of 25) that contains `index`.
    pub window: Vec<Item>,
    pub window_start: usize,
    pub custom: Option<Arc<StoredImage>>,
    pub banner: Option<Arc<BannerFile>>,
    pub notice: Option<String>,
    /// For looking up the notable works of a person in the list (people are characters in a profile).
    pub sources: Option<crate::people::fetch::Sources>,
    /// Processed pictures kept in memory, shared by every profile view.
    pub images: super::imgcache::ImageCache,
    /// Up to three notable works of the shown person (empty for anything else).
    pub person_works: Vec<String>,
    /// The wiki an item came from (its snapshot), shown as a source line.
    pub wiki_source: Option<String>,
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
            sources: Some(crate::people::fetch::Sources::of(ctx.data())),
            images: ctx.data().images.clone(),
            wiki_source: None,
            person_works: Vec::new(),
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
        self.badges = crate::features::badges::line(&self.pool, self.owner).await;
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
        // The watch / read status of each work on this window (shown as a text line on its card).
        if self.section == Section::Works {
            for item in &mut self.window {
                item.status = crate::db::status::get(&self.pool, self.owner, &item.key)
                    .await
                    .ok()
                    .flatten();
            }
        }
        self.custom = self.load_custom_image().await;
        self.person_works = self.load_person_works().await;
        self.wiki_source = self.load_wiki_source().await;
        if self.banner.is_none() {
            self.banner = self.prepare_banner().await.map(Arc::new);
        }
        self.prefetch();
        Ok(())
    }

    /// The current character's own picture. A missing row, an unreadable one or one too big for a message all fall
    /// back to the provider picture, with a warning in the log, so that the page itself always renders.
    async fn load_custom_image(&self) -> Option<Arc<StoredImage>> {
        let item = self
            .current()
            .filter(|i| i.section == Section::Characters && i.visible)?;
        let id = item.character_id()?;
        let image_id = item.custom_image_id?;
        let Some(img) = self.images.get(&self.pool, image_id).await else {
            tracing::warn!("custom image of character {id} is missing; using the provider image");
            return None;
        };
        if img.bytes.is_empty() || img.bytes.len() > MAX_ATTACH_BYTES {
            tracing::warn!(
                "custom image {} of character {id} is empty or too large ({} bytes); using the provider image",
                img.id,
                img.bytes.len()
            );
            return None;
        }
        Some(img)
    }

    /// Warm what the neighbours of the shown item need (their pictures, and the card of a person) in the background,
    /// so the next click on ◀ or ▶ finds it in memory.
    pub fn prefetch(&self) {
        let neighbours: Vec<&Item> = [self.index.checked_sub(1), Some(self.index + 1)]
            .into_iter()
            .flatten()
            .filter_map(|i| self.window.get(i.checked_sub(self.window_start)?))
            .filter(|i| i.visible && i.section == Section::Characters)
            .collect();
        let images: Vec<i64> = neighbours
            .iter()
            .filter_map(|i| i.custom_image_id)
            .collect();
        let people: Vec<String> = neighbours
            .iter()
            .filter_map(|i| i.character_id().and_then(crate::people::decode_person_id))
            .collect();
        if images.is_empty() && people.is_empty() {
            return;
        }
        let (cache, pool, sources) = (self.images.clone(), self.pool.clone(), self.sources.clone());
        tokio::spawn(async move {
            for id in images {
                let _ = cache.get(&pool, id).await;
            }
            if let Some(s) = sources {
                for key in people {
                    let _ = crate::people::fetch::person(&s, &key, false).await;
                }
            }
        });
    }

    /// The wiki a shown item came from, by its snapshot (so it is still known when the wiki is gone).
    async fn load_wiki_source(&self) -> Option<String> {
        let item = self.current().filter(|i| i.visible)?;
        let (wiki, page) = crate::wiki::parse_key(&item.key).or_else(|| {
            item.character_id()
                .and_then(crate::wiki::profile::decode_id)
        })?;
        crate::wiki::profile::snapshot(&self.pool, wiki, page)
            .await
            .map(|s| s.source_name)
    }

    /// A person in the list: their three best known works (safety filtered when the card was parsed; cached by the
    /// sources, so this is cheap after the first time).
    async fn load_person_works(&self) -> Vec<String> {
        let (Some(sources), Some(item)) = (&self.sources, self.current()) else {
            return Vec::new();
        };
        let Some(key) = item
            .character_id()
            .and_then(crate::people::decode_person_id)
            .filter(|_| item.visible)
        else {
            return Vec::new();
        };
        match crate::people::fetch::person(sources, &key, false).await {
            crate::people::fetch::Card::Found(p) => {
                p.credits.iter().take(3).map(|c| c.line()).collect()
            }
            _ => Vec::new(),
        }
    }

    /// The banner is dropped from a page only when it and the character picture together would not fit one message.
    fn show_banner(&self) -> bool {
        match (&self.banner, &self.custom) {
            (Some(b), Some(c)) => b.bytes.len() + c.bytes.len() <= MAX_ATTACH_BYTES,
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// An uploaded banner is attached as stored (a GIF stays animated); presets and colours are drawn.
    pub async fn prepare_banner(&self) -> Option<BannerFile> {
        let spec = BannerSpec::parse(self.profile.banner.as_deref());
        if let BannerSpec::Image(id) = &spec {
            if let Some(img) = self.images.get(&self.pool, *id).await {
                let ext = if img.animated { "gif" } else { "png" };
                return Some(BannerFile {
                    name: format!("{BANNER_NAME}.{ext}"),
                    bytes: img.bytes.clone(),
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

    /// The compact details line: "الشخصيات 4 · الأعمال 1 · عام" (+ suggestions). No join date.
    pub fn details_line(&self) -> String {
        let mut parts = vec![
            format!("{} {}", i18n::F_PROFILE_CHARACTERS, self.counts.0),
            format!("{} {}", i18n::F_PROFILE_WORKS, self.counts.1),
            i18n::privacy_ar(&self.profile.privacy).to_string(),
        ];
        if self.suggestions > 0 {
            parts.push(format!(
                "{} {}",
                i18n::F_PROFILE_SUGGESTIONS,
                self.suggestions
            ));
        }
        parts.join(" · ")
    }

    /// Name, bio, quote, details and badges as lines of text.
    pub fn info_text(&self) -> String {
        let mut lines = vec![format!(
            "**{} {}**",
            i18n::PROFILE_TITLE,
            fit(&self.shown_name(), 60)
        )];
        if let Some(b) = &self.profile.bio {
            lines.push(fit(b, 300));
        }
        if let Some(q) = &self.profile.quote {
            lines.push(format!("*« {} »*", fit(q, 200)));
        }
        lines.push(self.details_line());
        if !self.badges.is_empty() {
            lines.push(self.badges.join(" · "));
        }
        lines.join("\n")
    }

    /// The current item as text: name, personal title and description, work, and the counter (shown once).
    pub fn item_text(&self) -> String {
        let counter = format!("`{}/{}`", self.index + 1, self.total.max(1));
        let Some(i) = self.current() else {
            return format!("{}\n{counter}", i18n::PROFILE_EMPTY);
        };
        if !i.visible {
            return format!("{}\n{counter}", i18n::HIDDEN_NOTICE);
        }
        let name = match i.native.as_deref().filter(|n| !n.is_empty()) {
            Some(n) => format!("{} ({n})", i.name),
            None => i.name.clone(),
        };
        let mut lines = vec![format!("### {name}")];
        if let Some(t) = i.display_title() {
            lines.push(format!("✨ **{t}**"));
        }
        let is_person = i
            .character_id()
            .is_some_and(|id| crate::people::decode_person_id(id).is_some());
        if let Some(w) = i.subtitle.as_deref().filter(|w| !w.is_empty()) {
            lines.push(if is_person {
                // "شخص حقيقي · ممثل": a real person among the characters.
                format!("🎙️ {w}")
            } else if i.section == Section::Characters {
                format!("📺 {w}")
            } else {
                w.to_string()
            });
        }
        if let Some(src) = &self.wiki_source {
            lines.push(format!("📄 {}: {src}", i18n::WIKI_SOURCE_LABEL));
        }
        if is_person && !self.person_works.is_empty() {
            lines.push(format!("🎞️ {}", self.person_works.join(" · ")));
        }
        if let Some(d) = &i.custom_description {
            lines.push(format!("📝 {d}"));
        }
        if let Some(st) = i
            .status
            .as_deref()
            .filter(|s| crate::db::status::is_valid(s))
        {
            let written = matches!(i.subtitle.as_deref(), Some(t) if t.contains("مانجا") || t.contains("مانهوا") || t.contains("رواية"));
            lines.push(format!(
                "📌 {}: {}",
                i18n::F_PROFILE_STATUS,
                crate::db::status::label(st, written)
            ));
        }
        if self.own() {
            if let Some(n) = i.note.as_deref() {
                lines.push(format!("🗒️ **{}:** {n}", i18n::NOTE_LABEL));
            }
        }
        if let Some(f) = i.favourites {
            lines.push(format!("❤ {}", crate::ui::format::thousands(f)));
        }
        lines.push(counter);
        lines.join("\n")
    }

    /// Where the current item's picture comes from: the personal picture (attached), or AniList's. Pictures of
    /// other providers are never hotlinked.
    fn item_image(&self) -> Option<String> {
        if let Some(img) = &self.custom {
            return Some(format!("attachment://{}", custom_name(img)));
        }
        self.current().filter(|i| i.visible).and_then(|i| {
            // Pictures of wiki items are kept as saved (a public https image address); the others come from the
            // hosts that allow embedding.
            let wiki = i.key.starts_with("wiki:")
                || i.character_id()
                    .and_then(crate::wiki::profile::decode_id)
                    .is_some();
            i.image_url.clone().filter(|u| {
                crate::people::profile::image_ok(u)
                    || (wiki && crate::wiki::safety::image_url_ok(u))
            })
        })
    }

    fn banner_url(&self) -> Option<String> {
        self.banner
            .as_ref()
            .filter(|_| self.show_banner())
            .map(|b| format!("attachment://{}", b.name))
    }

    /// The whole profile as one Components V2 container: banner, text, item picture, item text, the three rows.
    pub fn v2_body(&self, enabled: bool) -> Vec<serde_json::Value> {
        let mut kids = Vec::new();
        // A V2 message has no `content`: a notice becomes a line of text at the top.
        if let Some(n) = &self.notice {
            kids.push(v2::text(n));
        }
        if let Some(b) = self.banner_url() {
            kids.push(v2::gallery(&[b]));
        }
        kids.push(v2::text(&self.info_text()));
        if let Some(img) = self.item_image() {
            kids.push(v2::gallery(&[img]));
        }
        kids.push(v2::text(&self.item_text()));
        kids.extend(v2::rows(&self.rows(enabled)));
        vec![v2::container(
            kids,
            Some(self.accent().unwrap_or(DEFAULT_ACCENT)),
        )]
    }

    /// The fallback when V2 is refused: two embeds at most. Embed 1 is the banner alone; embed 2 holds the info and
    /// the item with the item picture at the bottom.
    pub fn fallback_embeds(&self) -> Vec<serenity::CreateEmbed> {
        let mut banner = Emb::new();
        banner.footer = None;
        banner.colour = self.accent();
        banner.image = self.banner_url();
        let mut card =
            Emb::new().description(format!("{}\n\n{}", self.info_text(), self.item_text()));
        card.colour = self.accent();
        card.image = self.item_image();
        card.footer = None;
        let mut out = Vec::new();
        if self.banner_url().is_some() {
            out.push(banner.build());
        }
        out.push(card.build());
        out
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
        if let Some(b) = self.banner.as_ref().filter(|_| self.show_banner()) {
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
            embeds: self.fallback_embeds(),
            components: self.rows(enabled),
            files: self.files(),
            v2: Some(self.v2_body(enabled)),
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
    fn class(&self) -> crate::nav::Class {
        crate::nav::Class::Profile
    }

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
    use crate::db::custom;

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
            status: None,
        }
    }

    fn row_json(row: &serenity::CreateActionRow) -> serde_json::Value {
        serde_json::to_value(row).expect("json")
    }

    async fn sample_view() -> ProfileView {
        ProfileView {
            api: Arc::new(SafeAniList::with_fixture(|_, _| {
                Err(crate::anilist::AniError::NotFound)
            })),
            pool: crate::db::connect_memory().await,
            policy: ContentPolicy::SAFE,
            renderer: Arc::new(Renderer::new(reqwest::Client::new())),
            viewer: 1,
            owner: 1,
            owner_name: "Alice".into(),
            profile: Profile {
                user_id: 1,
                bio: Some("bio text".into()),
                accent_color: Some(0xFF8800),
                quote: None,
                banner: None,
                privacy: "public".into(),
                display_title: None,
                language: "ar".into(),
            },
            section: Section::Characters,
            index: 0,
            total: 4,
            counts: (4, 1),
            suggestions: 0,
            badges: Vec::new(),
            window: vec![item(1, "Rem")],
            window_start: 0,
            custom: None,
            banner: Some(Arc::new(BannerFile {
                name: "banner.gif".into(),
                bytes: vec![1],
            })),
            notice: None,
            sources: None,
            images: crate::commands::profile::imgcache::ImageCache::new(8),
            wiki_source: None,
            person_works: Vec::new(),
        }
    }

    #[tokio::test]
    async fn the_profile_is_one_container_with_banner_text_picture_text_and_exactly_three_rows() {
        let v = sample_view().await;
        let body = v.v2_body(true);
        assert_eq!(body.len(), 1, "one visual block");
        assert_eq!(body[0]["type"], 17);
        assert_eq!(
            body[0]["accent_color"], 0xFF8800,
            "the member's colour is the side bar"
        );
        let kinds: Vec<i64> = body[0]["components"]
            .as_array()
            .expect("kids")
            .iter()
            .map(|k| k["type"].as_i64().unwrap_or(0))
            .collect();
        // banner gallery, info text, item text (no picture: this item has none), then button row + two selects
        assert_eq!(kinds, vec![12, 10, 10, 1, 1, 1]);
        assert!(super::v2::validate(&body).is_ok());
        assert_eq!(
            body[0]["components"][0]["items"][0]["media"]["url"],
            "attachment://banner.gif"
        );
    }

    #[tokio::test]
    async fn the_info_is_compact_and_the_counter_appears_once() {
        let v = sample_view().await;
        let details = v.details_line();
        assert!(
            details.starts_with("الشخصيات 4 · الأعمال 1 · عام"),
            "{details}"
        );
        assert!(
            !details.contains("انضم"),
            "the join date is gone: {details}"
        );
        assert!(!details.contains('\n'), "one line, not one field per line");
        let page = v.main_page(true);
        let all: String = v.info_text() + &v.item_text();
        assert_eq!(
            all.matches("1/4").count(),
            1,
            "the position counter is shown once"
        );
        assert!(page.content.is_none(), "no content next to a V2 body");
    }

    #[tokio::test]
    async fn the_fallback_is_at_most_two_embeds_banner_alone_then_info_and_item() {
        let mut v = sample_view().await;
        let e = v.fallback_embeds();
        assert_eq!(e.len(), 2);
        let first = serde_json::to_value(&e[0]).expect("json");
        assert_eq!(first["image"]["url"], "attachment://banner.gif");
        assert!(
            first.get("title").is_none() && first.get("description").is_none(),
            "embed 1 is the banner only"
        );
        v.banner = None;
        assert_eq!(
            v.fallback_embeds().len(),
            1,
            "without a banner there is one embed"
        );
    }

    #[tokio::test]
    async fn the_notice_moves_into_the_container_in_v2() {
        let mut v = sample_view().await;
        v.notice = Some("تنبيه".into());
        let body = v.v2_body(true);
        assert_eq!(body[0]["components"][0]["content"], "تنبيه");
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

    async fn fixture(n: i64) -> ProfileView {
        let mut v = sample_view().await;
        v.banner = None;
        crate::db::profile::create_profile(&v.pool, 1)
            .await
            .expect("profile");
        for id in 1..=n {
            crate::db::profile::add_character(&v.pool, 1, id)
                .await
                .expect("add");
            crate::db::profile::upsert_cache(
                &v.pool,
                &crate::db::profile::CacheRow {
                    character_id: id,
                    name_romaji: format!("Character {id}"),
                    name_native: None,
                    image_url: Some(format!("https://s4.anilist.co/file/{id}.png")),
                    is_safe: true,
                    main_work: Some("Work".into()),
                    main_work_url: None,
                    favourites: None,
                    fetched_at: crate::commands::profile::util::now(),
                },
            )
            .await
            .expect("cache");
        }
        v.load().await.expect("load");
        v
    }

    async fn set_image(v: &ProfileView, character: i64, tag: u8, animated: bool, size: usize) {
        let img = custom::NewImage {
            hash: format!("hash-{character}-{tag}"),
            mime: if animated { "image/gif" } else { "image/png" }.into(),
            bytes: vec![tag; size],
            width: 10,
            height: 10,
            animated,
        };
        let id = match custom::store_image(&v.pool, 1, &img, 50)
            .await
            .expect("store")
        {
            custom::StoreOutcome::Stored(id) | custom::StoreOutcome::Existing(id) => id,
            other => panic!("{other:?}"),
        };
        custom::set_character_image(&v.pool, 1, character, id)
            .await
            .expect("set");
    }

    async fn order(v: &ProfileView) -> Vec<i64> {
        pdb::list_page(&v.pool, 1, None, 0, 100)
            .await
            .expect("list")
            .iter()
            .map(|e| e.character_id)
            .collect()
    }

    async fn goes(v: &mut ProfileView, id: &str, values: &[&str]) -> String {
        let values: Vec<String> = values.iter().map(|s| s.to_string()).collect();
        v.on_interaction(id, &values).await;
        assert!(v.notice.is_none(), "{id}: {:?}", v.notice);
        v.current().map(|i| i.key.clone()).unwrap_or_default()
    }

    #[tokio::test]
    async fn editing_a_picture_keeps_every_item_reachable_from_every_control() {
        let mut v = fixture(30).await;
        let before = order(&v).await;
        set_image(&v, 3, 1, false, 100).await;
        set_image(&v, 4, 2, true, 100).await;
        set_image(&v, 27, 3, false, 100).await;
        // a picture whose row is gone, and one that is empty
        sqlx::query("UPDATE profile_characters SET custom_image_id = 9999 WHERE character_id = 5")
            .execute(&v.pool)
            .await
            .ok();
        set_image(&v, 6, 4, false, 0).await;
        // re-editing the same character replaces the picture without moving it
        set_image(&v, 3, 9, true, 200).await;
        assert_eq!(order(&v).await, before, "an edit never reorders");
        v.index = 0;
        v.load().await.expect("load");

        for step in 1..30 {
            let key = goes(&mut v, crate::ui::pagination::NAV_NEXT, &[]).await;
            assert_eq!(key, (step + 1).to_string(), "▶ reaches item {}", step + 1);
            // the page always renders; at most the banner and the current picture are attached
            let page = v.main_page(true);
            assert!(page.files.len() <= 2, "item {key}: {}", page.files.len());
            assert!(v.v2_body(true).len() == 1);
        }
        assert_eq!(
            goes(&mut v, crate::ui::pagination::NAV_FIRST, &[]).await,
            "1"
        );
        assert_eq!(
            goes(&mut v, crate::ui::pagination::NAV_LAST, &[]).await,
            "30"
        );
        for want in (1..30).rev() {
            assert_eq!(
                goes(&mut v, crate::ui::pagination::NAV_PREV, &[]).await,
                want.to_string()
            );
        }
        // jump select: every item of both pages, including the edited ones
        for target in [3, 4, 5, 6] {
            v.index = 0;
            v.load().await.expect("load");
            assert_eq!(
                goes(&mut v, ITEM_ID, &[&target.to_string()]).await,
                target.to_string()
            );
        }
        assert_eq!(goes(&mut v, PAGE_ID, &["1"]).await, "26");
        assert_eq!(goes(&mut v, ITEM_ID, &["27"]).await, "27");
        assert!(
            v.custom.is_some(),
            "the edited picture is shown on its own item"
        );
        assert_eq!(goes(&mut v, PAGE_ID, &["0"]).await, "1");
        // the mode toggle there and back
        goes(&mut v, MODE_ID, &[]).await;
        assert_eq!(v.section, Section::Works);
        goes(&mut v, MODE_ID, &[]).await;
        assert_eq!(v.section, Section::Characters);
        assert_eq!(order(&v).await, before);
    }

    #[tokio::test]
    async fn a_missing_or_empty_picture_falls_back_to_the_provider_image() {
        let mut v = fixture(3).await;
        sqlx::query("UPDATE profile_characters SET custom_image_id = 9999 WHERE character_id = 1")
            .execute(&v.pool)
            .await
            .ok();
        v.load().await.expect("load");
        assert!(v.custom.is_none());
        assert_eq!(
            v.item_image().as_deref(),
            Some("https://s4.anilist.co/file/1.png")
        );
        set_image(&v, 2, 1, false, 0).await;
        goes(&mut v, crate::ui::pagination::NAV_NEXT, &[]).await;
        assert!(v.custom.is_none(), "an empty file is not used");
    }

    #[tokio::test]
    async fn the_banner_yields_when_it_and_the_picture_do_not_fit_one_message() {
        let mut v = fixture(2).await;
        set_image(&v, 1, 1, true, 3 * 1024 * 1024).await;
        v.banner = Some(Arc::new(BannerFile {
            name: "banner.gif".into(),
            bytes: vec![0; 7 * 1024 * 1024],
        }));
        v.load().await.expect("load");
        let page = v.main_page(true);
        assert_eq!(
            page.files.len(),
            1,
            "only the character picture is attached"
        );
        assert!(v.banner_url().is_none());
        goes(&mut v, crate::ui::pagination::NAV_NEXT, &[]).await;
        assert_eq!(
            v.main_page(true).files.len(),
            1,
            "the banner is back on other items"
        );
        assert!(v.banner_url().is_some());
    }

    #[tokio::test]
    async fn dropdown_values_are_stable_unique_and_within_limits() {
        let mut v = fixture(30).await;
        let before: Vec<String> = window_choices(&v.window, v.window_start)
            .iter()
            .map(|c| c.value.clone())
            .collect();
        set_image(&v, 3, 1, false, 100).await;
        v.load().await.expect("load");
        let after = window_choices(&v.window, v.window_start);
        assert_eq!(
            after.iter().map(|c| c.value.clone()).collect::<Vec<_>>(),
            before
        );
        let mut seen = std::collections::HashSet::new();
        for c in &after {
            assert!(c.label.chars().count() <= 100 && c.value.chars().count() <= 100);
            assert!(seen.insert(c.value.clone()), "unique");
        }
    }

    #[tokio::test]
    async fn people_show_in_the_characters_list_with_a_role_line_and_never_in_a_third_mode() {
        let mut v = fixture(2).await;
        let p = crate::people::Person {
            key: "tmdb-person:17419".into(),
            name: "Bryan Cranston".into(),
            roles: vec!["ممثل".into()],
            image: Some("https://image.tmdb.org/t/p/w500/a.jpg".into()),
            popularity: Some(4250),
            ..crate::people::Person::default()
        };
        crate::people::profile::add(&v.pool, 1, &p, 10)
            .await
            .expect("add");
        v.load().await.expect("load");
        assert_eq!(v.counts.0, 3, "the characters count includes the person");
        assert_eq!(v.total, 3);
        v.index = 2;
        v.load().await.expect("load");
        let text = v.item_text();
        assert!(
            text.contains("Bryan Cranston") && text.contains("🎙️ شخص حقيقي · ممثل"),
            "{text}"
        );
        assert!(
            text.contains("4,250") || text.contains("4250") || text.contains("٤"),
            "the popularity line: {text}"
        );
        assert_eq!(
            v.item_image().as_deref(),
            Some("https://image.tmdb.org/t/p/w500/a.jpg")
        );
        // the toggle stays two-way: characters <-> works
        assert_eq!(Section::Characters.other(), Section::Works);
        assert_eq!(Section::Works.other(), Section::Characters);
        // the jump menu lists it among the characters, by its stable id
        let choices = window_choices(&v.window, v.window_start);
        assert!(choices.iter().any(|c| c.value
            == crate::people::encode_person_id("tmdb-person:17419")
                .expect("id")
                .to_string()));
        // a person is navigable like any character
        v.index = 0;
        v.load().await.expect("load");
        let again = crate::ui::pagination::apply_nav(
            crate::ui::pagination::Nav::Last,
            v.index,
            v.total as usize,
        );
        assert_eq!(again, 2);
    }

    #[tokio::test]
    async fn a_tmdb_cast_entry_goes_from_the_button_to_the_profile_and_can_be_edited() {
        let mut v = fixture(1).await;
        let cid = crate::tmdb::model::encode_cast_id(crate::tmdb::model::Kind::Tv, 1396, 17419)
            .expect("cast id");
        crate::db::profile::upsert_cache(
            &v.pool,
            &crate::db::profile::CacheRow {
                character_id: cid,
                name_romaji: "Walter White".into(),
                name_native: None,
                image_url: Some("https://image.tmdb.org/t/p/w500/w.jpg".into()),
                is_safe: true,
                main_work: Some("Breaking Bad".into()),
                main_work_url: None,
                favourites: None,
                fetched_at: 5,
            },
        )
        .await
        .expect("cache");
        assert_eq!(
            crate::db::profile::add_character(&v.pool, 1, cid)
                .await
                .expect("add"),
            crate::db::profile::AddOutcome::Added(2)
        );
        v.load().await.expect("load");
        v.index = 1;
        v.load().await.expect("load");
        let text = v.item_text();
        assert!(
            text.contains("Walter White") && text.contains("📺 Breaking Bad"),
            "{text}"
        );
        assert_eq!(
            v.item_image().as_deref(),
            Some("https://image.tmdb.org/t/p/w500/w.jpg")
        );
        // editable: a nickname and a custom picture work like on any character
        crate::db::custom::set_title(&v.pool, 1, cid, Some("Heisenberg"))
            .await
            .expect("title");
        set_image(&v, cid, 1, true, 50).await;
        v.load().await.expect("load");
        assert!(v.item_text().contains("Heisenberg"));
        assert!(v.custom.is_some(), "the custom picture (a GIF) is used");
        // it opens from lists by its cast key
        assert_eq!(
            crate::commands::top::character_key(cid),
            "tmdb-cast:tv:1396:17419"
        );
        assert_eq!(
            crate::commands::top::character_key(
                crate::people::encode_person_id("anilist-staff:95").expect("id")
            ),
            "anilist-staff:95",
            "a person opens by its person key"
        );
    }

    #[tokio::test]
    async fn the_neighbours_pictures_are_warmed_in_the_background() {
        let mut v = fixture(6).await;
        set_image(&v, 2, 1, false, 100).await;
        set_image(&v, 4, 2, true, 100).await;
        set_image(&v, 6, 3, false, 100).await;
        v.index = 2; // item 3 on screen: its neighbours are items 2 and 4
        v.load().await.expect("load");
        let image_of = |c: i64| {
            let pool = v.pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>("SELECT custom_image_id FROM profile_characters WHERE user_id = 1 AND character_id = ?1")
                    .bind(c)
                    .fetch_one(&pool)
                    .await
                    .expect("image id")
            }
        };
        let (i2, i4, i6) = (image_of(2).await, image_of(4).await, image_of(6).await);
        let mut warm = false;
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            v.images.settle().await;
            if v.images.contains(i2).await && v.images.contains(i4).await {
                warm = true;
                break;
            }
        }
        assert!(warm, "both neighbours are in memory");
        assert!(
            !v.images.contains(i6).await,
            "an item further away is not prefetched"
        );
    }

    fn wiki_page(wiki: i64, id: i64, title: &str) -> (crate::wiki::Page, crate::wiki::Source) {
        (
            crate::wiki::Page {
                wiki_id: wiki,
                page_id: id,
                title: title.into(),
                kind: crate::wiki::Kind::Character,
                extract: Some("She is the trial captain.".into()),
                image_url: Some("https://static.wikia.nocookie.net/mudae/images/a/a1/Acerola.png/revision/latest".into()),
                aliases: vec![],
                parent_title: Some("Pokémon Sun/Moon".into()),
                categories: vec![],
            },
            crate::wiki::Source {
                id: wiki,
                name: "Mudae Wiki".into(),
                host: "mudae.fandom.com".into(),
                api_url: "https://mudae.fandom.com/api.php".into(),
                lang: "en".into(),
                license_name: Some("CC BY-SA 3.0".into()),
                license_url: None,
                character_category: None,
                work_category: None,
                kind_hint: None,
                enabled: true,
                added_by: None,
                created_at: 0,
                last_checked_at: None,
                last_error: None,
            },
        )
    }

    #[tokio::test]
    async fn a_wiki_item_renders_like_any_other_is_editable_and_survives_the_wiki_and_navigation_keeps_working(
    ) {
        let mut v = fixture(3).await;
        let (page, source) = wiki_page(2, 77, "Acerola");
        crate::wiki::profile::add_character(&v.pool, 1, &page, &source, 10)
            .await
            .expect("add");
        let id = crate::wiki::profile::encode_id(2, 77).expect("id");
        v.load().await.expect("load");
        assert_eq!(v.total, 4, "the wiki character is in the characters list");
        v.index = 3;
        v.load().await.expect("load");
        let text = v.item_text();
        assert!(
            text.contains("Acerola") && text.contains("📺 Pokémon Sun/Moon"),
            "{text}"
        );
        assert!(
            text.contains("المصدر: Mudae Wiki"),
            "the source line: {text}"
        );
        assert_eq!(
            v.item_image().as_deref(),
            Some("https://static.wikia.nocookie.net/mudae/images/a/a1/Acerola.png/revision/latest"),
            "the saved picture is shown"
        );
        // the same edits as any item: a nickname, a description and a picture (GIF)
        crate::db::custom::set_title(&v.pool, 1, id, Some("Captain"))
            .await
            .expect("nickname");
        crate::db::custom::set_description(&v.pool, 1, id, Some("Mine"))
            .await
            .expect("description");
        set_image(&v, id, 1, true, 80).await;
        v.load().await.expect("load");
        assert!(v.item_text().contains("Captain") && v.item_text().contains("Mine"));
        assert!(v.custom.is_some());
        // the wiki is disabled, deleted and its cache cleared: the item and the edits stay
        let wiki_id = match crate::wiki::store::insert(
            &v.pool,
            &crate::wiki::store::NewSource {
                name: "Mudae Wiki".into(),
                host: "mudae.fandom.com".into(),
                api_url: "https://mudae.fandom.com/api.php".into(),
                lang: "en".into(),
                license_name: None,
                license_url: None,
                character_category: None,
                work_category: None,
                kind_hint: None,
                added_by: None,
            },
        )
        .await
        .expect("insert")
        {
            crate::wiki::store::Added::New(i) => i,
            other => panic!("{other:?}"),
        };
        crate::wiki::store::set_enabled(&v.pool, wiki_id, false)
            .await
            .expect("off");
        crate::wiki::store::delete(&v.pool, wiki_id)
            .await
            .expect("delete");
        crate::wiki::store::clear_cache(&v.pool)
            .await
            .expect("clear");
        v.load().await.expect("load");
        let text = v.item_text();
        assert!(
            text.contains("Acerola")
                && text.contains("Captain")
                && text.contains("المصدر: Mudae Wiki"),
            "{text}"
        );
        // navigation reaches the edited wiki item with every control, forward and back
        v.index = 0;
        v.load().await.expect("load");
        assert_eq!(
            goes(&mut v, crate::ui::pagination::NAV_LAST, &[]).await,
            id.to_string()
        );
        assert_eq!(
            goes(&mut v, crate::ui::pagination::NAV_PREV, &[]).await,
            "3"
        );
        assert_eq!(
            goes(&mut v, ITEM_ID, &[&id.to_string()]).await,
            id.to_string()
        );
        assert_eq!(
            goes(&mut v, crate::ui::pagination::NAV_FIRST, &[]).await,
            "1"
        );
    }

    fn group_first(group: usize) -> String {
        (group * PAGES_PER_GROUP).to_string()
    }
}
