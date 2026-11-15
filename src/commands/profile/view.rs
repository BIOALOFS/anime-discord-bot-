//! The profile browser: image-first pages, section switch, item dropdown -> detail view, style dropdown.

use super::{
    items::{self, Item, Section},
    util::{refresh_entries, refresh_works},
};
use crate::{
    anilist::{models::Character, models::Media, CharacterLookup, ContentPolicy, SafeAniList},
    commands::{buttons, roster::Roster},
    db::{
        custom::{self, StoredImage},
        game,
        profile::{self as pdb, Group, Profile},
        works,
    },
    i18n,
    render::{self, CellSpec, HeaderSpec, Images, Rendered, Renderer, Theme, TierRow},
    ui::{
        cards,
        embeds::Emb,
        pagination::{
            apply_nav, back_button, close_button, nav_row, page_count, parse_nav, BACK, CLOSE,
        },
        select::{select_row, Choice},
        session::{ModalSpec, Outcome, Page, View, Whisper},
        styles::{self, Style},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

const STYLE_ID: &str = "style";
const SECTION_ID: &str = "section";
const ANOTHER_ID: &str = "another";
const ITEM_ID: &str = "item";
const REPORT_ID: &str = "report";
const ROSTER_ID: &str = "open_roster";

pub enum Detail {
    Character {
        item: Item,
        card: Option<Box<Character>>,
        custom: Option<StoredImage>,
    },
    Work {
        item: Item,
        media: Option<Box<Media>>,
    },
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
    pub guild: Option<i64>,
    pub profile: Profile,
    pub group: Option<Group>,
    pub section: Section,
    pub style: Style,
    pub page: usize,
    /// Items in the current section/group.
    pub total: i64,
    pub counts: (i64, i64),
    pub badges: Vec<styles::Badge>,
    pub items: Vec<Item>,
    pub image: Option<Arc<Rendered>>,
    pub image_label: String,
    pub detail: Option<Detail>,
    pub roster: Option<Roster>,
    pub notice: Option<String>,
    pub whisper: Option<Whisper>,
}

impl ProfileView {
    fn group_id(&self) -> Option<i64> {
        self.group.as_ref().map(|g| g.id)
    }

    fn theme(&self) -> Theme {
        Theme::by_name(&self.profile.template)
    }

    fn accent(&self) -> Option<u32> {
        self.profile.accent_color.map(|c| c as u32)
    }

    fn pages(&self) -> usize {
        if self.style.single_page() {
            1
        } else {
            page_count(self.total as usize, self.style.per_page())
        }
    }

    /// Rows to fetch for the current style.
    fn limit(&self) -> i64 {
        match self.style {
            Style::Gallery => 4,
            Style::Random => 1,
            s => s.per_page() as i64,
        }
    }

    fn offset(&self) -> i64 {
        match self.style {
            Style::Random if self.total > 0 => rand::random_range(0..self.total),
            _ => (self.page * self.style.per_page()) as i64,
        }
    }

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
        self.total =
            items::count(&self.pool, self.owner, self.section, self.group_id(), None).await?;
        self.page = self.page.min(self.pages().saturating_sub(1));
        let (offset, limit, gid) = (self.offset(), self.limit(), self.group_id());
        // Re-check stored items against AniList; cached data is used if it is unreachable.
        self.items = match self.section {
            Section::Characters => {
                let raw = pdb::list_page(&self.pool, self.owner, gid, offset, limit).await?;
                refresh_entries(&self.api, &self.pool, self.policy, &raw).await;
                let raw = pdb::list_page(&self.pool, self.owner, gid, offset, limit).await?;
                raw.iter().map(Item::from_character).collect()
            }
            Section::Works => {
                let raw =
                    works::list_page(&self.pool, self.owner, gid, None, offset, limit).await?;
                refresh_works(&self.api, &self.pool, self.policy, &raw).await;
                let raw =
                    works::list_page(&self.pool, self.owner, gid, None, offset, limit).await?;
                raw.iter().map(Item::from_work).collect()
            }
        };
        self.image = None;
        if matches!(self.style, Style::Collage | Style::Podium | Style::Tier) {
            self.image = self.render_image().await;
            if let Some(img) = &self.image {
                self.image_label = format!(
                    "profile-{}-{}-p{}.{}",
                    ascii_slug(&self.owner_name),
                    self.section.key(),
                    self.page + 1,
                    img.ext
                );
            }
        }
        Ok(())
    }

    fn header_spec(&self) -> HeaderSpec {
        HeaderSpec {
            name: self.owner_name.clone(),
            badges: self.badges.iter().map(|b| b.letter).collect(),
            bio: self.profile.bio.clone(),
            count: self.counts.0 + self.counts.1,
            avatar_url: self.owner_avatar.clone(),
        }
    }

    /// Grid cells; items with a personal image use its first frame (stored under a `custom:` key).
    async fn cells(&self, items: &[Item], extra: &mut Images) -> Vec<CellSpec> {
        let mut out = Vec::new();
        for i in items.iter().filter(|i| i.visible) {
            let mut url = i.image_url.clone();
            if let Some(id) = i.custom_image_id {
                if let Ok(Some(img)) = custom::get_image(&self.pool, id).await {
                    let key = format!("custom:{}", img.hash);
                    let bytes = img.bytes.clone();
                    if let Some(frame) =
                        tokio::task::spawn_blocking(move || render::first_frame(&bytes))
                            .await
                            .ok()
                            .flatten()
                    {
                        extra.insert(key.clone(), Arc::new(frame));
                        url = Some(key);
                    }
                }
            }
            out.push(CellSpec {
                label: i.name.clone(),
                subtitle: i.display_title().map(str::to_string),
                rank: (i.position <= 3).then_some(i.position as u8),
                image_url: url,
            });
        }
        out
    }

    /// Draw the style's image; `None` makes the view fall back to the text list.
    async fn render_image(&self) -> Option<Arc<Rendered>> {
        let header = Some(self.header_spec());
        let (accent, theme) = (self.accent(), self.theme());
        let mut extra = Images::new();
        match self.style {
            Style::Collage => {
                let cells = self.cells(&self.items, &mut extra).await;
                self.renderer
                    .collage(cells, header, accent, theme, 1.0, extra)
                    .await
            }
            Style::Podium => {
                let top = items::load_page(
                    &self.pool,
                    self.owner,
                    self.section,
                    self.group_id(),
                    None,
                    0,
                    3,
                )
                .await
                .ok()?;
                let cells = self.cells(&top, &mut extra).await;
                self.renderer
                    .podium(cells, header, accent, theme, extra)
                    .await
            }
            Style::Tier => {
                let tiered: Vec<Item> = match self.section {
                    Section::Characters => {
                        let mut v = Vec::new();
                        for e in pdb::tiered(&self.pool, self.owner).await.ok()? {
                            v.push(Item::from_character(&e));
                        }
                        v
                    }
                    Section::Works => works::tiered(&self.pool, self.owner)
                        .await
                        .ok()?
                        .iter()
                        .map(Item::from_work)
                        .collect(),
                };
                let mut rows: Vec<TierRow> = Vec::new();
                for i in tiered.iter().filter(|i| i.visible) {
                    let name = i.tier.clone().unwrap_or_default();
                    let cell = self
                        .cells(std::slice::from_ref(i), &mut extra)
                        .await
                        .into_iter()
                        .next()?;
                    match rows.iter_mut().find(|r| r.name.eq_ignore_ascii_case(&name)) {
                        Some(r) => r.cells.push(cell),
                        None => rows.push(TierRow {
                            name,
                            cells: vec![cell],
                        }),
                    }
                }
                rows.sort_by_key(|r| render::tier_order(&r.name));
                self.renderer
                    .tier_list(rows, header, accent, theme, extra)
                    .await
            }
            _ => None,
        }
    }

    fn header(&self) -> Emb {
        styles::header(
            &self.profile,
            &self.owner_name,
            self.owner_avatar.as_deref(),
            self.counts,
            &self.badges,
        )
    }

    fn image_embed(&self, visible: &[&Item], pages: usize) -> Emb {
        let mut e = self.header();
        let mut desc = e.description.take().unwrap_or_default();
        let list = match self.style {
            Style::Podium => visible
                .iter()
                .filter(|i| i.position > 3)
                .map(|i| format!("**{}.** {}", i.position, i.name))
                .collect::<Vec<_>>()
                .join("\n"),
            Style::Collage => visible
                .iter()
                .map(|i| format!("{}. {}", i.position, i.name))
                .collect::<Vec<_>>()
                .join(" · "),
            _ => String::new(),
        };
        if !list.is_empty() {
            desc.push_str(&format!("\n\n{list}"));
        }
        e.description = Some(desc);
        e.image = Some(format!("attachment://{}", self.image_label));
        e.footer(format!(
            "{} · {}/{}",
            i18n::POWERED_BY,
            self.page + 1,
            pages
        ))
    }

    fn embeds(&self) -> Vec<Emb> {
        let visible: Vec<&Item> = self.items.iter().filter(|i| i.visible).collect();
        let pages = self.pages();
        let own = self.viewer == self.owner;
        let card = |i: Option<&Item>| match i {
            Some(i) => {
                let mut e = styles::item_card(i, self.total, own);
                e.colour = self.accent();
                e
            }
            None => self.header().description(i18n::PROFILE_EMPTY),
        };
        match self.style {
            Style::Compact => vec![styles::compact_embed(self.header(), &visible, self.total)],
            Style::Cards | Style::Random => vec![card(visible.first().copied())],
            Style::Gallery => {
                let mut out = vec![card(visible.first().copied())];
                out.extend(
                    visible
                        .iter()
                        .skip(1)
                        .take(3)
                        .map(|i| styles::thumb_embed(i)),
                );
                out
            }
            Style::Collage | Style::Podium | Style::Tier if self.image.is_some() => {
                vec![self.image_embed(&visible, pages)]
            }
            // Text list: the style itself, or the fallback when no image could be drawn.
            _ => vec![styles::names_embed(
                self.header(),
                &visible,
                self.page,
                pages,
            )],
        }
    }

    // ---- detail ------------------------------------------------------------------------

    async fn open_detail(&mut self, key: &str) -> Outcome {
        let Some(item) = self
            .items
            .iter()
            .find(|i| i.key == key && i.visible)
            .cloned()
        else {
            return Outcome::Keep;
        };
        self.detail = Some(match item.section {
            Section::Characters => {
                let id = item.character_id().unwrap_or_default();
                let card = match self.api.character_by_id(id, self.policy).await {
                    Ok(CharacterLookup::Found(c)) => Some(c),
                    _ => None,
                };
                let custom = custom::image_of_character(&self.pool, self.owner, id)
                    .await
                    .ok()
                    .flatten();
                Detail::Character { item, card, custom }
            }
            Section::Works => {
                let media = match works::anilist_id(&item.key) {
                    Some(id) => self
                        .api
                        .media_by_id(id, self.policy)
                        .await
                        .ok()
                        .flatten()
                        .map(Box::new),
                    None => None,
                };
                Detail::Work { item, media }
            }
        });
        Outcome::Update
    }

    fn detail_page(&self, d: &Detail, enabled: bool) -> Page {
        let own = self.viewer == self.owner;
        let mut files = Vec::new();
        let (mut embed, link, has_custom) = match d {
            Detail::Character { item, card, custom } => {
                let mut e = match card {
                    Some(c) => cards::character_card(c),
                    None => styles::item_card(item, self.total, own),
                };
                let mut lines: Vec<String> = e.description.take().into_iter().collect();
                if let Some(t) = &item.custom_title {
                    lines.insert(0, format!("✨ **{t}**"));
                }
                if let Some(desc) = &item.custom_description {
                    lines.push(format!("📝 {desc}"));
                }
                if own {
                    if let Some(n) = &item.note {
                        lines.push(format!("🗒️ **{}:** {n}", i18n::NOTE_LABEL));
                    }
                }
                if !lines.is_empty() {
                    e.description = Some(lines.join("\n"));
                }
                if let Some(img) = custom {
                    let name = format!(
                        "custom-{}.{}",
                        img.id,
                        if img.animated { "gif" } else { "png" }
                    );
                    e.image = Some(format!("attachment://{name}"));
                    files.push(serenity::CreateAttachment::bytes(img.bytes.clone(), name));
                }
                let link = card.as_ref().and_then(|c| c.site_url.clone());
                (e, link, custom.is_some())
            }
            Detail::Work { item, media } => {
                let e = match media {
                    Some(m) => cards::work_card(m),
                    None => styles::item_card(item, self.total, own),
                };
                (e, media.as_ref().and_then(|m| m.site_url.clone()), false)
            }
        };
        embed.colour = embed.colour.or(self.accent());
        let mut row = vec![
            back_button(enabled),
            serenity::CreateButton::new(ROSTER_ID)
                .label(i18n::BTN_ROSTER)
                .emoji('👥')
                .style(serenity::ButtonStyle::Secondary)
                .disabled(!enabled),
        ];
        if has_custom && !own {
            row.push(
                serenity::CreateButton::new(REPORT_ID)
                    .label(i18n::BTN_REPORT)
                    .emoji('🚩')
                    .style(serenity::ButtonStyle::Danger)
                    .disabled(!enabled),
            );
        }
        if let Some(url) = link {
            row.push(
                serenity::CreateButton::new_link(url).label(format!("🔗 {}", i18n::BTN_ANILIST)),
            );
        }
        row.push(close_button(enabled));
        Page::embed(embed.build(), vec![buttons(row)])
            .notice(self.notice.clone())
            .with_files(files)
    }

    async fn open_roster(&mut self) -> Outcome {
        let started = match &self.detail {
            Some(Detail::Character { card: Some(c), .. }) => {
                Roster::start(self.api.clone(), self.policy, c).await
            }
            Some(Detail::Work { item, .. }) => match works::anilist_id(&item.key) {
                Some(id) => Roster::for_work(self.api.clone(), self.policy, id, &item.name)
                    .await
                    .map(Some),
                None => Ok(None),
            },
            _ => Ok(None),
        };
        match started {
            Ok(Some(r)) => self.roster = Some(r),
            Ok(None) => self.notice = Some(i18n::ROSTER_NO_WORKS.to_string()),
            Err(e) => self.notice = Some(crate::commands::ani_message(&e).to_string()),
        }
        Outcome::Update
    }

    // ---- main page ---------------------------------------------------------------------

    fn main_page(&self, enabled: bool) -> Page {
        let hidden = self.items.iter().filter(|i| !i.visible).count();
        let embeds: Vec<_> = self.embeds().into_iter().map(Emb::build).collect();

        let item_choices: Vec<Choice> = self
            .items
            .iter()
            .filter(|i| i.visible)
            .take(25)
            .map(|i| Choice {
                label: crate::ui::embeds::fit(&format!("{}. {}", i.position, i.name), 100),
                description: i
                    .display_title()
                    .or(i.subtitle.as_deref())
                    .map(|d| crate::ui::embeds::fit(d, 100)),
                value: i.key.clone(),
            })
            .collect();
        let style_choices: Vec<Choice> = Style::ALL
            .iter()
            .map(|s| Choice {
                label: s.label().to_string(),
                description: None,
                value: s.as_str().to_string(),
            })
            .collect();
        let mut rows: Vec<_> = select_row(
            ITEM_ID,
            i18n::SELECT_ITEM_PLACEHOLDER,
            item_choices,
            enabled,
        )
        .into_iter()
        .collect();
        rows.extend(select_row(
            STYLE_ID,
            i18n::SELECT_STYLE_PLACEHOLDER,
            style_choices,
            enabled,
        ));
        if self.pages() > 1 {
            rows.push(nav_row(self.page, self.pages(), enabled));
        }
        let mut actions = vec![serenity::CreateButton::new(SECTION_ID)
            .label(match self.section {
                Section::Characters => i18n::BTN_SECTION_WORKS,
                Section::Works => i18n::BTN_SECTION_CHARACTERS,
            })
            .style(serenity::ButtonStyle::Secondary)
            .disabled(!enabled)];
        if self.style == Style::Random {
            actions.push(
                serenity::CreateButton::new(ANOTHER_ID)
                    .label(i18n::BTN_ANOTHER)
                    .emoji('🔀')
                    .style(serenity::ButtonStyle::Primary)
                    .disabled(!enabled),
            );
        }
        actions.push(close_button(enabled));
        rows.push(buttons(actions));

        let notice = match (&self.notice, hidden) {
            (Some(n), _) => Some(n.clone()),
            (None, h) if h > 0 => Some(i18n::HIDDEN_NOTICE.to_string()),
            (None, _) if self.style == Style::Tier && self.image.is_none() => {
                Some(i18n::TIER_NONE.to_string())
            }
            _ => None,
        };
        let files = self
            .image
            .iter()
            .map(|img| {
                serenity::CreateAttachment::bytes(img.bytes.clone(), self.image_label.clone())
            })
            .collect();
        Page {
            content: notice,
            embeds,
            components: rows,
            files,
        }
    }

    /// The page without components, for posting a shareable picture.
    pub fn static_page(&self) -> Page {
        let mut page = self.main_page(false);
        page.components.clear();
        page
    }
}

impl View for ProfileView {
    fn take_whisper(&mut self) -> Option<Whisper> {
        self.whisper.take()
    }

    fn render(&self, enabled: bool) -> Page {
        if let Some(r) = &self.roster {
            return r.render(enabled);
        }
        match &self.detail {
            Some(d) => self.detail_page(d, enabled),
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
        if id == CLOSE {
            return Outcome::Close;
        }
        if self.detail.is_some() {
            return match id {
                BACK => {
                    self.detail = None;
                    Outcome::Update
                }
                ROSTER_ID => self.open_roster().await,
                REPORT_ID => {
                    let image = match &self.detail {
                        Some(Detail::Character {
                            custom: Some(img), ..
                        }) => Some(img.id),
                        _ => None,
                    };
                    if let Some(image) = image {
                        let text =
                            match custom::report_image(&self.pool, image, self.viewer, self.guild)
                                .await
                            {
                                Ok(true) => i18n::REPORT_SENT,
                                Ok(false) => i18n::REPORT_ALREADY,
                                Err(_) => i18n::ERR_GENERIC,
                            };
                        self.whisper = Some(Whisper::text(text));
                    }
                    Outcome::Keep
                }
                _ => Outcome::Keep,
            };
        }
        match id {
            ITEM_ID => {
                let Some(key) = values.first() else {
                    return Outcome::Keep;
                };
                return self.open_detail(key).await;
            }
            STYLE_ID => {
                let Some(style) = values.first().and_then(|v| Style::parse(v)) else {
                    return Outcome::Keep;
                };
                self.style = style;
                self.page = 0;
            }
            SECTION_ID => {
                self.section = self.section.other();
                self.page = 0;
            }
            ANOTHER_ID => {}
            other => {
                let Some(nav) = parse_nav(other) else {
                    return Outcome::Keep;
                };
                let next = apply_nav(nav, self.page, self.pages());
                if next == self.page {
                    return Outcome::Keep;
                }
                self.page = next;
            }
        }
        if let Err(e) = self.load().await {
            tracing::warn!("profile view reload failed: {e}");
            self.notice = Some(i18n::ERR_GENERIC.to_string());
        }
        Outcome::Update
    }
}

/// ASCII-only slug for attachment names (descriptive, safe).
pub fn ascii_slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(20)
        .collect::<String>()
        .to_lowercase();
    if slug.is_empty() {
        "user".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::ascii_slug;

    #[test]
    fn slugs_are_ascii_and_never_empty() {
        assert_eq!(ascii_slug("Alice Smith!"), "alicesmith");
        assert_eq!(ascii_slug("مستخدم"), "user");
        assert_eq!(ascii_slug(&"x".repeat(50)).len(), 20);
    }
}
