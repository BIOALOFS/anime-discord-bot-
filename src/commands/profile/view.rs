//! `/profile view` and the interactive profile browser.

use super::{
    groups_ac::ac_group,
    util::{begin_public, pool, refresh_entries, uid},
    StyleChoice,
};
use crate::{
    anilist::{models::Character, CharacterLookup, ContentPolicy, SafeAniList},
    commands::{buttons, policy, say_ephemeral},
    db::profile::{self as db, Entry, Group, Profile},
    error::{Context, Error},
    i18n,
    render::{tier_order, CellSpec, HeaderSpec, Rendered, Renderer, TierRow},
    ui::{
        cards,
        embeds::Emb,
        pagination::{apply_nav, close_button, nav_row, page_count, parse_nav, CLOSE},
        select::{select_row, Choice},
        session::{self, Outcome, Page, View},
        styles::{self, Style},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::sync::Arc;

const STYLE_ID: &str = "style";
const ANOTHER_ID: &str = "another";

pub struct ProfileView {
    pub api: Arc<SafeAniList>,
    pub pool: SqlitePool,
    pub policy: ContentPolicy,
    pub viewer: i64,
    pub owner: i64,
    pub owner_name: String,
    pub owner_avatar: Option<String>,
    pub profile: Profile,
    pub group: Option<Group>,
    pub style: Style,
    pub page: usize,
    pub total: i64,
    pub entries: Vec<Entry>,
    /// Full AniList data for the single-character styles (None when AniList is unreachable).
    pub card: Option<Character>,
    pub notice: Option<String>,
    pub renderer: Arc<Renderer>,
    /// Rendered image for the collage / podium / tier styles (`None`: text fallback).
    pub image: Option<Arc<Rendered>>,
    pub image_label: String,
}

impl ProfileView {
    fn group_id(&self) -> Option<i64> {
        self.group.as_ref().map(|g| g.id)
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

    pub async fn load(&mut self) -> Result<(), db::ProfileError> {
        self.total = db::count_in_group(&self.pool, self.owner, self.group_id()).await?;
        self.page = self.page.min(self.pages().saturating_sub(1));
        let offset = match self.style {
            Style::Random if self.total > 0 => rand::random_range(0..self.total),
            _ => (self.page * self.style.per_page()) as i64,
        };
        let mut entries = db::list_page(
            &self.pool,
            self.owner,
            self.group_id(),
            offset,
            self.limit(),
        )
        .await?;
        // Re-check stored characters against AniList; cached data is used if it is unreachable.
        refresh_entries(&self.api, &self.pool, self.policy, &entries).await;
        entries = db::list_page(
            &self.pool,
            self.owner,
            self.group_id(),
            offset,
            self.limit(),
        )
        .await?;
        self.card = None;
        if matches!(self.style, Style::Cards | Style::Gallery | Style::Random) {
            if let Some(e) = entries.iter().find(|e| e.visible()) {
                if let Ok(CharacterLookup::Found(c)) =
                    self.api.character_by_id(e.character_id, self.policy).await
                {
                    self.card = Some(*c);
                }
            }
        }
        self.image = None;
        if matches!(self.style, Style::Collage | Style::Podium | Style::Tier) {
            self.image = self.render_image(&entries).await;
            if let Some(img) = &self.image {
                self.image_label = format!(
                    "profile-{}-p{}.{}",
                    ascii_slug(&self.owner_name),
                    self.page + 1,
                    img.ext
                );
            }
        }
        self.entries = entries;
        Ok(())
    }

    fn cell(e: &Entry) -> CellSpec {
        CellSpec {
            label: e.name().to_string(),
            rank: (e.position <= 3).then_some(e.position as u8),
            image_url: e.image_url.clone(),
        }
    }

    /// Draw the style's image; `None` makes the view fall back to the text list.
    async fn render_image(&self, entries: &[Entry]) -> Option<Arc<Rendered>> {
        let accent = self.profile.accent_color.map(|c| c as u32);
        let header = Some(HeaderSpec {
            name: self.owner_name.clone(),
            bio: self.profile.bio.clone(),
            count: self.total,
            avatar_url: self.owner_avatar.clone(),
        });
        match self.style {
            Style::Collage => {
                let cells: Vec<_> = entries
                    .iter()
                    .filter(|e| e.visible())
                    .map(Self::cell)
                    .collect();
                self.renderer.collage(cells, header, accent, 1.0).await
            }
            Style::Podium => {
                let mut top = db::list_page(&self.pool, self.owner, self.group_id(), 0, 3)
                    .await
                    .ok()?;
                refresh_entries(&self.api, &self.pool, self.policy, &top).await;
                top = db::list_page(&self.pool, self.owner, self.group_id(), 0, 3)
                    .await
                    .ok()?;
                let cells: Vec<_> = top.iter().filter(|e| e.visible()).map(Self::cell).collect();
                self.renderer.podium(cells, header, accent).await
            }
            Style::Tier => {
                let tiered = db::tiered(&self.pool, self.owner).await.ok()?;
                let mut rows: Vec<TierRow> = Vec::new();
                for e in tiered.iter().filter(|e| e.visible()) {
                    let name = e.tier.clone().unwrap_or_default();
                    match rows.iter_mut().find(|r| r.name.eq_ignore_ascii_case(&name)) {
                        Some(r) => r.cells.push(Self::cell(e)),
                        None => rows.push(TierRow {
                            name,
                            cells: vec![Self::cell(e)],
                        }),
                    }
                }
                rows.sort_by_key(|r| tier_order(&r.name));
                self.renderer.tier_list(rows, header, accent).await
            }
            _ => None,
        }
    }

    /// The page without components, for posting a shareable picture.
    pub fn static_page(&self) -> Page {
        let mut page = self.render(false);
        page.components.clear();
        page
    }

    fn image_embed(&self, visible: &[&Entry], pages: usize) -> Emb {
        let mut e = self.header();
        let mut desc = e.description.take().unwrap_or_default();
        let list = match self.style {
            Style::Podium => visible
                .iter()
                .filter(|e| e.position > 3)
                .map(|e| format!("**{}.** {}", e.position, e.name()))
                .collect::<Vec<_>>()
                .join("\n"),
            Style::Collage => visible
                .iter()
                .map(|e| format!("{}. {}", e.position, e.name()))
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

    fn header(&self) -> Emb {
        styles::header(
            &self.profile,
            &self.owner_name,
            self.owner_avatar.as_deref(),
            self.total,
        )
    }

    fn embeds(&self) -> Vec<Emb> {
        let visible: Vec<&Entry> = self.entries.iter().filter(|e| e.visible()).collect();
        let pages = self.pages();
        let own = self.viewer == self.owner;
        match self.style {
            Style::Compact => vec![styles::compact_embed(self.header(), &visible, self.total)],
            Style::Cards | Style::Random => vec![self.single_card(visible.first().copied(), own)],
            Style::Gallery => {
                let mut out = vec![self.single_card(visible.first().copied(), own)];
                out.extend(
                    visible
                        .iter()
                        .skip(1)
                        .take(3)
                        .map(|e| styles::thumb_embed(e)),
                );
                out
            }
            Style::Collage | Style::Podium | Style::Tier if self.image.is_some() => {
                vec![self.image_embed(&visible, pages)]
            }
            // Text list: the style itself, or the fallback when no image could be drawn.
            Style::Names | Style::Collage | Style::Podium | Style::Tier => {
                vec![styles::names_embed(
                    self.header(),
                    &visible,
                    self.page,
                    pages,
                )]
            }
        }
    }

    fn single_card(&self, entry: Option<&Entry>, own: bool) -> Emb {
        let Some(e) = entry else {
            return self.header().description(i18n::PROFILE_EMPTY);
        };
        let note = if own { e.note.as_deref() } else { None };
        let mut emb = match &self.card {
            Some(c) if c.id == e.character_id => {
                let mut card =
                    cards::character_card(c).author(i18n::position_of(e.position, self.total));
                if let Some(n) = note {
                    let d = card.description.take().unwrap_or_default();
                    card.description = Some(format!("{d}\n📝 **{}:** {n}", i18n::NOTE_LABEL));
                }
                card
            }
            _ => styles::cached_card(e, self.total, note),
        };
        emb.colour = self.profile.accent_color.map(|c| c as u32);
        emb
    }
}

impl View for ProfileView {
    fn render(&self, enabled: bool) -> Page {
        let hidden = self.entries.iter().filter(|e| !e.visible()).count();
        let embeds: Vec<_> = self.embeds().into_iter().map(Emb::build).collect();

        let style_choices: Vec<Choice> = Style::ALL
            .iter()
            .map(|s| Choice {
                label: s.label().to_string(),
                description: None,
                value: s.as_str().to_string(),
            })
            .collect();
        let mut rows: Vec<_> = select_row(
            STYLE_ID,
            i18n::SELECT_STYLE_PLACEHOLDER,
            style_choices,
            enabled,
        )
        .into_iter()
        .collect();
        if self.pages() > 1 {
            rows.push(nav_row(self.page, self.pages(), enabled));
        }
        let mut actions = Vec::new();
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
            _ => None,
        };
        let notice = match (notice, self.style, &self.image) {
            (None, Style::Tier, None) => Some(i18n::TIER_NONE.to_string()),
            (n, _, _) => n,
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

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        self.notice = None;
        match id {
            CLOSE => return Outcome::Close,
            STYLE_ID => {
                let Some(style) = values.first().and_then(|v| Style::parse(v)) else {
                    return Outcome::Keep;
                };
                self.style = style;
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

/// Show a profile.
#[poise::command(
    slash_command,
    name_localized("ar", "عرض"),
    description_localized("ar", "اعرض بروفايلك أو بروفايل غيرك")
)]
pub async fn view(
    ctx: Context<'_>,
    #[description = "Whose profile (default: yours)"]
    #[description_localized("ar", "صاحب البروفايل (الافتراضي: أنت)")]
    #[name_localized("ar", "المستخدم")]
    user: Option<serenity::User>,
    #[description = "Display style"]
    #[description_localized("ar", "نمط العرض")]
    #[name_localized("ar", "النمط")]
    style: Option<StyleChoice>,
    #[description = "Only this group"]
    #[description_localized("ar", "مجموعة محددة فقط")]
    #[name_localized("ar", "المجموعة")]
    #[autocomplete = "ac_group"]
    group: Option<String>,
) -> Result<(), Error> {
    begin_public(ctx).await?;
    let pool = pool(ctx);
    let target = user.as_ref().map_or(ctx.author().clone(), Clone::clone);
    let owner = target.id.get() as i64;
    let own = owner == uid(ctx);

    let Some(profile) = db::get_profile(&pool, owner).await? else {
        return say_ephemeral(
            ctx,
            if own {
                i18n::PROFILE_NONE_SELF
            } else {
                i18n::PROFILE_NONE_OTHER
            },
        )
        .await;
    };
    if !db::can_view(&pool, uid(ctx), &profile).await? {
        return say_ephemeral(ctx, i18n::PROFILE_PRIVATE).await;
    }
    let group = match group.as_deref().filter(|g| !g.trim().is_empty()) {
        Some(name) => match db::find_group(&pool, owner, name).await? {
            Some(g) => Some(g),
            None => return say_ephemeral(ctx, i18n::GROUP_NOT_FOUND).await,
        },
        None => None,
    };
    let style = style
        .map(StyleChoice::style)
        .or_else(|| Style::parse(&profile.default_style))
        .unwrap_or(Style::Names);

    let mut view = ProfileView {
        api: ctx.data().anilist.clone(),
        pool,
        policy: policy(ctx),
        viewer: uid(ctx),
        owner,
        owner_name: target.display_name().to_string(),
        owner_avatar: Some(target.face()),
        profile,
        group,
        style,
        page: 0,
        total: 0,
        entries: Vec::new(),
        card: None,
        notice: None,
        renderer: ctx.data().renderer.clone(),
        image: None,
        image_label: String::new(),
    };
    view.load().await?;
    session::run(ctx, view).await
}

/// ASCII-only slug for attachment names (descriptive, safe).
fn ascii_slug(name: &str) -> String {
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

/// A shareable picture of your profile (first page of the collage).
#[poise::command(
    slash_command,
    name_localized("ar", "مشاركة"),
    description_localized("ar", "صورة بروفايلك للنشر")
)]
pub async fn share(
    ctx: Context<'_>,
    #[description = "Page of the collage (default 1)"]
    #[description_localized("ar", "رقم صفحة الصور (الافتراضي 1)")]
    #[name_localized("ar", "الصفحة")]
    #[min = 1]
    page: Option<i64>,
) -> Result<(), Error> {
    begin_public(ctx).await?;
    let pool = pool(ctx);
    let Some(profile) = db::get_profile(&pool, uid(ctx)).await? else {
        return say_ephemeral(ctx, i18n::PROFILE_NONE_SELF).await;
    };
    let author = ctx.author();
    let mut view = ProfileView {
        api: ctx.data().anilist.clone(),
        pool,
        policy: policy(ctx),
        viewer: uid(ctx),
        owner: uid(ctx),
        owner_name: author.display_name().to_string(),
        owner_avatar: Some(author.face()),
        profile,
        group: None,
        style: Style::Collage,
        page: (page.unwrap_or(1) - 1).max(0) as usize,
        total: 0,
        entries: Vec::new(),
        card: None,
        notice: None,
        renderer: ctx.data().renderer.clone(),
        image: None,
        image_label: String::new(),
    };
    view.load().await?;
    let mut shared = view.static_page();
    shared.content = Some(i18n::SHARE_TITLE.to_string());
    ctx.send(shared.into_reply()).await?;
    Ok(())
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
