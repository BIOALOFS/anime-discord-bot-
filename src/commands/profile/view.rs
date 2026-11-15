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
        self.entries = entries;
        Ok(())
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
            // Image styles are drawn by the image engine; the list is their text fallback.
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
        Page {
            content: notice,
            embeds,
            components: rows,
            files: Vec::new(),
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
    };
    view.load().await?;
    session::run(ctx, view).await
}
