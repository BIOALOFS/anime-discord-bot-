//! "👥 شخصيات العمل": the full cast of a character's work with numbered pages, a page-jump modal and
//! range selects. A component embedded by the views that show a character card.

use crate::{
    anilist::{models::Character, safety, AniError, ContentPolicy, SafeAniList, CAST_PAGE_SIZE},
    commands::buttons,
    i18n,
    ui::{
        cards,
        embeds::Emb,
        format::role_ar,
        pagination::{
            apply_nav, back_button, close_button, page_count, parse_nav, BACK, CLOSE, NAV_FIRST,
            NAV_LAST, NAV_NEXT, NAV_PREV,
        },
        select::{select_row, Choice},
        session::{ModalSpec, Outcome, Page},
    },
};
use poise::serenity_prelude as serenity;
use std::{ops::RangeInclusive, sync::Arc};

pub const PER_PAGE: usize = 10;
const RANGES_PER_MENU: usize = 25;
const MAX_WORKS: usize = 10;

const ID_WORK: &str = "rwork";
const ID_GROUP: &str = "rgroup";
const ID_RANGE: &str = "rrange";
const ID_CHAR: &str = "rchar";
pub const ID_JUMP: &str = "rjump";

// ---- pure helpers -------------------------------------------------------------------------

pub fn pages_of(total: usize) -> usize {
    page_count(total, PER_PAGE)
}

/// AniList pages (25 each) holding the cast entries of roster page `page` (10 each).
pub fn anilist_pages(page: usize) -> RangeInclusive<usize> {
    let start = page * PER_PAGE;
    (start / CAST_PAGE_SIZE + 1)..=((start + PER_PAGE - 1) / CAST_PAGE_SIZE + 1)
}

/// "11–20"
pub fn range_label(page: usize, total: usize) -> String {
    format!(
        "{}–{}",
        page * PER_PAGE + 1,
        ((page + 1) * PER_PAGE).min(total.max(1))
    )
}

/// Roster pages are split into groups of 25 so one select menu never exceeds Discord's 25 options.
pub fn group_count(pages: usize) -> usize {
    pages.div_ceil(RANGES_PER_MENU).max(1)
}

pub fn group_of_page(page: usize) -> usize {
    page / RANGES_PER_MENU
}

pub fn pages_in_group(group: usize, pages: usize) -> std::ops::Range<usize> {
    let start = group * RANGES_PER_MENU;
    start.min(pages)..(start + RANGES_PER_MENU).min(pages)
}

pub fn group_label(group: usize, total: usize) -> String {
    let per = RANGES_PER_MENU * PER_PAGE;
    format!(
        "{}–{}",
        group * per + 1,
        ((group + 1) * per).min(total.max(1))
    )
}

/// 1-based page typed by the user (Latin or Arabic-Indic digits) -> 0-based page.
pub fn parse_page_input(input: &str, pages: usize) -> Option<usize> {
    let digits: String = input
        .trim()
        .chars()
        .filter_map(|c| match c {
            '0'..='9' => Some(c),
            '٠'..='٩' => char::from_digit(c as u32 - '٠' as u32, 10),
            '۰'..='۹' => char::from_digit(c as u32 - '۰' as u32, 10),
            _ => None,
        })
        .collect();
    let n: usize = digits.parse().ok()?;
    (1..=pages).contains(&n).then(|| n - 1)
}

// ---- component ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Update,
    Keep,
    /// Leave the roster and return to the host view.
    Exit,
    Close,
}

impl Step {
    /// Translate into the host view's outcome; `Exit` drops the roster.
    pub fn apply(self, slot: &mut Option<Roster>) -> Outcome {
        match self {
            Step::Update => Outcome::Update,
            Step::Keep => Outcome::Keep,
            Step::Close => Outcome::Close,
            Step::Exit => {
                *slot = None;
                Outcome::Update
            }
        }
    }
}

enum Mode {
    WorkPick,
    List,
    Card(Box<Character>),
}

pub struct Roster {
    api: Arc<SafeAniList>,
    policy: ContentPolicy,
    works: Vec<(i64, String)>,
    work: Option<(i64, String)>,
    origin: i64,
    total: usize,
    page: usize,
    entries: Vec<(usize, Character, String)>,
    mode: Mode,
    notice: Option<String>,
}

fn works_of(c: &Character, policy: ContentPolicy) -> Vec<(i64, String)> {
    c.media
        .as_ref()
        .map(|m| {
            m.nodes
                .iter()
                .filter(|r| policy.allow_adult || !safety::appearance_adult(r))
                .filter_map(|r| Some((r.id?, r.title.as_ref()?.main().to_string())))
                .take(MAX_WORKS)
                .collect()
        })
        .unwrap_or_default()
}

impl Roster {
    /// `Ok(None)` when the character has no known work.
    pub async fn start(
        api: Arc<SafeAniList>,
        policy: ContentPolicy,
        character: &Character,
    ) -> Result<Option<Self>, AniError> {
        let works = works_of(character, policy);
        if works.is_empty() {
            return Ok(None);
        }
        let mut r = Self {
            api,
            policy,
            work: (works.len() == 1).then(|| works[0].clone()),
            works,
            origin: character.id,
            total: 0,
            page: 0,
            entries: Vec::new(),
            mode: Mode::WorkPick,
            notice: None,
        };
        if r.work.is_some() {
            r.mode = Mode::List;
            r.load().await?;
        }
        Ok(Some(r))
    }

    async fn load(&mut self) -> Result<(), AniError> {
        let Some((id, _)) = self.work.clone() else {
            return Ok(());
        };
        let start = self.page * PER_PAGE;
        let mut entries = Vec::new();
        let mut total = self.total;
        for ap in anilist_pages(self.page) {
            let page = self.api.work_cast_page(id, ap, self.policy).await?;
            total = page.total;
            entries.extend(page.entries);
        }
        self.total = total;
        self.entries = entries
            .into_iter()
            .filter(|(i, ..)| (start..start + PER_PAGE).contains(i))
            .collect();
        Ok(())
    }

    fn fail(&mut self, e: AniError) -> Step {
        tracing::warn!("anilist error in roster: {e}");
        self.notice = Some(crate::commands::ani_message(&e).to_string());
        Step::Update
    }

    fn pages(&self) -> usize {
        pages_of(self.total)
    }

    pub fn modal_for(&self, id: &str) -> Option<ModalSpec> {
        (id == ID_JUMP && matches!(self.mode, Mode::List)).then(|| ModalSpec {
            title: i18n::ROSTER_JUMP_TITLE.to_string(),
            label: i18n::roster_jump_label(self.pages()),
        })
    }

    pub async fn on_modal(&mut self, id: &str, input: String) -> Step {
        self.notice = None;
        if id != ID_JUMP {
            return Step::Keep;
        }
        match parse_page_input(&input, self.pages()) {
            Some(p) => self.goto(p).await,
            None => {
                self.notice = Some(i18n::ROSTER_BAD_PAGE.to_string());
                Step::Update
            }
        }
    }

    async fn goto(&mut self, page: usize) -> Step {
        if page == self.page {
            return Step::Keep;
        }
        self.page = page;
        match self.load().await {
            Ok(()) => Step::Update,
            Err(e) => self.fail(e),
        }
    }

    pub async fn handle(&mut self, id: &str, values: &[String]) -> Step {
        self.notice = None;
        let first = values.first().map(String::as_str);
        match id {
            CLOSE => Step::Close,
            BACK => match self.mode {
                Mode::Card(_) => {
                    self.mode = Mode::List;
                    Step::Update
                }
                _ => Step::Exit,
            },
            ID_WORK => {
                let Some(picked) = first.and_then(|v| v.parse::<i64>().ok()) else {
                    return Step::Keep;
                };
                let Some(work) = self.works.iter().find(|(i, _)| *i == picked).cloned() else {
                    return Step::Keep;
                };
                self.work = Some(work);
                self.mode = Mode::List;
                self.page = 0;
                self.total = 0;
                match self.load().await {
                    Ok(()) => Step::Update,
                    Err(e) => self.fail(e),
                }
            }
            ID_GROUP => match first.and_then(|v| v.parse::<usize>().ok()) {
                Some(g) => self.goto(g * RANGES_PER_MENU).await,
                None => Step::Keep,
            },
            ID_RANGE => match first.and_then(|v| v.parse::<usize>().ok()) {
                Some(p) if p < self.pages() => self.goto(p).await,
                _ => Step::Keep,
            },
            ID_CHAR => {
                let Some(cid) = first.and_then(|v| v.parse::<i64>().ok()) else {
                    return Step::Keep;
                };
                match self.api.character_by_id(cid, self.policy).await {
                    Ok(crate::anilist::CharacterLookup::Found(c)) => {
                        self.mode = Mode::Card(c);
                        Step::Update
                    }
                    Ok(_) => {
                        self.notice = Some(i18n::NOT_FOUND.to_string());
                        Step::Update
                    }
                    Err(e) => self.fail(e),
                }
            }
            other => match (parse_nav(other), &self.mode) {
                (Some(nav), Mode::List) => {
                    let next = apply_nav(nav, self.page, self.pages());
                    self.goto(next).await
                }
                _ => Step::Keep,
            },
        }
    }

    // ---- rendering ----

    fn title(&self) -> &str {
        self.work.as_ref().map_or("", |(_, t)| t.as_str())
    }

    fn actions(&self, enabled: bool) -> serenity::CreateActionRow {
        buttons(vec![back_button(enabled), close_button(enabled)])
    }

    fn list_embed(&self) -> Emb {
        let lines: Vec<String> = self
            .entries
            .iter()
            .map(|(i, c, role)| {
                let name = match &c.site_url {
                    Some(u) => format!("[{}]({u})", c.display_name()),
                    None => c.display_name().to_string(),
                };
                let tag = role_ar(role);
                if c.id == self.origin {
                    format!("⭐ **{}. {name}** — {tag}", i + 1)
                } else {
                    format!("**{}.** {name} — {tag}", i + 1)
                }
            })
            .collect();
        Emb::new()
            .title(format!("{} — {}", i18n::ROSTER_TITLE, self.title()))
            .description(if lines.is_empty() {
                i18n::EMPTY_LIST.to_string()
            } else {
                lines.join("\n")
            })
            .footer(i18n::roster_footer(self.page + 1, self.pages(), self.total))
    }

    /// ⏮ ◀ [🔢 n/m] ▶ ⏭: the middle button opens the jump modal.
    fn nav_row(&self, enabled: bool) -> serenity::CreateActionRow {
        let pages = self.pages();
        let (at_start, at_end) = (self.page == 0, self.page + 1 >= pages);
        let btn = |id: &str, label: &str, off: bool| {
            serenity::CreateButton::new(id)
                .label(label)
                .style(serenity::ButtonStyle::Secondary)
                .disabled(!enabled || off)
        };
        buttons(vec![
            btn(NAV_FIRST, "⏮", at_start),
            btn(NAV_PREV, "◀", at_start),
            serenity::CreateButton::new(ID_JUMP)
                .label(format!("🔢 {}/{pages}", self.page + 1))
                .style(serenity::ButtonStyle::Primary)
                .disabled(!enabled || pages <= 1),
            btn(NAV_NEXT, "▶", at_end),
            btn(NAV_LAST, "⏭", at_end),
        ])
    }

    pub fn render(&self, enabled: bool) -> Page {
        let (embed, rows) = match &self.mode {
            Mode::WorkPick => {
                let body = self
                    .works
                    .iter()
                    .enumerate()
                    .map(|(i, (_, t))| format!("**{}.** {t}", i + 1))
                    .collect::<Vec<_>>()
                    .join("\n");
                let choices = self
                    .works
                    .iter()
                    .map(|(id, t)| Choice {
                        label: crate::ui::embeds::fit(t, 100),
                        description: None,
                        value: id.to_string(),
                    })
                    .collect();
                let mut rows: Vec<_> =
                    select_row(ID_WORK, i18n::ROSTER_WORK_PLACEHOLDER, choices, enabled)
                        .into_iter()
                        .collect();
                rows.push(self.actions(enabled));
                (
                    Emb::new().title(i18n::ROSTER_PICK_WORK).description(body),
                    rows,
                )
            }
            Mode::List => {
                let pages = self.pages();
                let mut rows = Vec::new();
                if group_count(pages) > 1 {
                    let groups = (0..group_count(pages))
                        .map(|g| Choice {
                            label: group_label(g, self.total),
                            description: None,
                            value: g.to_string(),
                        })
                        .collect();
                    rows.extend(select_row(
                        ID_GROUP,
                        i18n::ROSTER_GROUP_PLACEHOLDER,
                        groups,
                        enabled,
                    ));
                }
                let ranges = pages_in_group(group_of_page(self.page), pages)
                    .map(|p| Choice {
                        label: range_label(p, self.total),
                        description: None,
                        value: p.to_string(),
                    })
                    .collect();
                rows.extend(select_row(
                    ID_RANGE,
                    i18n::ROSTER_RANGE_PLACEHOLDER,
                    ranges,
                    enabled,
                ));
                rows.push(self.nav_row(enabled));
                let chars = self
                    .entries
                    .iter()
                    .map(|(i, c, _)| Choice {
                        label: crate::ui::embeds::fit(
                            &format!("{}. {}", i + 1, c.display_name()),
                            100,
                        ),
                        description: None,
                        value: c.id.to_string(),
                    })
                    .collect();
                rows.extend(select_row(
                    ID_CHAR,
                    i18n::ROSTER_CHAR_PLACEHOLDER,
                    chars,
                    enabled,
                ));
                rows.push(self.actions(enabled));
                (self.list_embed(), rows)
            }
            Mode::Card(c) => {
                let back = serenity::CreateButton::new(BACK)
                    .label(i18n::BTN_BACK_TO_LIST)
                    .emoji('↩')
                    .style(serenity::ButtonStyle::Secondary)
                    .disabled(!enabled);
                (
                    cards::character_card(c),
                    vec![buttons(vec![back, close_button(enabled)])],
                )
            }
        };
        Page::embed(embed.build(), rows).notice(self.notice.clone())
    }
}

#[cfg(test)]
mod tests;
