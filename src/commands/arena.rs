//! Collective games for `/game`: one shared session per channel where every member can answer. The first correct
//! answer wins the round's points, wrong answerers are locked out of that round, timers reveal hints and advance
//! the rounds, and a scoreboard (plus a weekly leaderboard per server) is kept. Only the starter or an admin can skip
//! or end a game. Everything comes from the safe AniList wrapper (no adult works or characters).

use super::{buttons, event::masked_clue, policy, say_ephemeral};
use crate::{
    anilist::{models::Character, models::Media, ContentPolicy, MediaKind, SafeAniList},
    db::game as gdb,
    error::{Context, Error},
    i18n,
    render::Renderer,
    ui::{
        embeds::{fit, Emb},
        session::{self, Outcome, Page, View},
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

const REVEAL_SECS: u64 = 5;
/// Presses closer together than this from one member are ignored (spam).
const PRESS_GAP: Duration = Duration::from_millis(700);
const ANSWER_PREFIX: &str = "ans:";
const SKIP: &str = "arena_skip";
const END: &str = "arena_end";

// ---- modes and difficulty -------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    CharacterImage,
    WorkCover,
    CharacterHints,
    TrueFalse,
    HigherLower,
    OrderYears,
    Race,
}

/// `(Arabic label, key)` of every collective mode.
pub const MODES: &[(&str, &str, Mode)] = &[
    (
        "خمّن الشخصية من صورة",
        "character-image",
        Mode::CharacterImage,
    ),
    ("خمّن العمل من الغلاف", "work-cover", Mode::WorkCover),
    (
        "خمّن الشخصية من الوصف",
        "character-hints",
        Mode::CharacterHints,
    ),
    ("صح أم خطأ", "true-false", Mode::TrueFalse),
    ("الأعلى تقييمًا أو شعبية", "higher-lower", Mode::HigherLower),
    ("رتّب حسب سنة الإصدار", "order-years", Mode::OrderYears),
    ("سباق الجولات", "race", Mode::Race),
];

pub fn mode_of(key: &str) -> Option<Mode> {
    MODES
        .iter()
        .find(|(_, k, _)| *k == key.trim())
        .map(|(_, _, m)| *m)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

/// `(Arabic label, key)`.
pub const DIFFICULTIES: &[(&str, &str)] = &[("سهل", "easy"), ("عادي", "normal"), ("صعب", "hard")];

impl Difficulty {
    pub fn parse(key: &str) -> Self {
        match key.trim() {
            "easy" => Self::Easy,
            "hard" => Self::Hard,
            _ => Self::Normal,
        }
    }

    /// Seconds per round.
    pub fn secs(self) -> u64 {
        match self {
            Self::Easy => 30,
            Self::Normal => 20,
            Self::Hard => 12,
        }
    }

    /// Crop/blur level of the first picture (0 easy .. 2 hard); hints then reveal it step by step down to 0.
    pub fn start_level(self) -> u8 {
        match self {
            Self::Easy => 1,
            Self::Normal | Self::Hard => 2,
        }
    }

    /// Points of a correct answer given at the very start of the round.
    pub fn base_points(self) -> i64 {
        match self {
            Self::Easy => 8,
            Self::Normal => 10,
            Self::Hard => 15,
        }
    }
}

/// Rounds per game.
pub fn rounds_for(mode: Mode) -> u32 {
    if mode == Mode::Race {
        10
    } else {
        5
    }
}

/// Points for a correct answer after `elapsed` of `total` seconds: the base plus up to 5 for being quick.
pub fn points_for(base: i64, elapsed: Duration, total: Duration) -> i64 {
    let left = total.saturating_sub(elapsed).as_secs_f64() / total.as_secs_f64().max(1.0);
    base + (left * 5.0).round() as i64
}

// ---- one active game per channel -------------------------------------------------------------------------

fn active() -> &'static Mutex<HashSet<u64>> {
    static ACTIVE: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Dropping it frees the channel, however the game ends.
pub struct ChannelGuard(u64);

impl Drop for ChannelGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = active().lock() {
            set.remove(&self.0);
        }
    }
}

pub fn try_claim(channel: u64) -> Option<ChannelGuard> {
    let mut set = active().lock().ok()?;
    // Built only on success: a dropped guard of a failed claim would free someone else's game.
    if set.insert(channel) {
        Some(ChannelGuard(channel))
    } else {
        None
    }
}

// ---- rounds ----------------------------------------------------------------------------------------------

/// A hint revealed while the round runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// Show the picture again at this (lower) level.
    Picture(u8),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Round {
    pub title: String,
    pub prompt: String,
    /// The picture to crop/blur: `(url, seed)`; the current level is `level`.
    pub picture: Option<(String, u64)>,
    pub level: u8,
    pub options: Vec<String>,
    pub correct: usize,
    pub hints: Vec<Hint>,
    /// Shown after the round (the right answer with a link when there is one).
    pub reveal: String,
    /// What the answer is, so the universal button ("أظهر العمل" / "أظهر الشخصيات") can be offered after the reveal.
    pub item: Option<(crate::components::ItemKind, String)>,
}

/// Seconds after the round start at which hint `n` (0-based) appears: evenly spread over the first 80% of the time.
pub fn hint_time(n: usize, hints: usize, total: Duration) -> Duration {
    total.mul_f64(0.8 * (n + 1) as f64 / (hints + 1) as f64)
}

/// Four options of a three-work ordering: the right order plus three other distinct permutations, labelled with the
/// letters of the works. Returns `(labels, index of the right one)`.
pub fn order_options(correct: [usize; 3]) -> (Vec<String>, usize) {
    const PERMS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let letters = ["أ", "ب", "ج"];
    let mut others: Vec<[usize; 3]> = PERMS.iter().copied().filter(|p| *p != correct).collect();
    // A fixed shuffle is enough: the answer's position is what is randomised.
    for i in (1..others.len()).rev() {
        others.swap(i, rand::random_range(0..=i));
    }
    others.truncate(3);
    let (all, at) = super::event::shuffle_options(correct, others);
    let label = |p: &[usize; 3]| {
        p.iter()
            .map(|i| letters[*i])
            .collect::<Vec<_>>()
            .join(" ← ")
    };
    (all.iter().map(label).collect(), at)
}

/// A true/false statement about a work, built from facts AniList gave; `lie` shifts the number.
pub fn statement(m: &Media, kind: u8, lie: bool) -> Option<(String, bool)> {
    let shift = |n: i32| {
        if lie {
            n + [-3, -2, -1, 1, 2, 3][rand::random_range(0..6)]
        } else {
            n
        }
    };
    let title = m.title.main();
    let text = match kind {
        0 => format!("عُرض «{title}» لأول مرة عام {}", shift(m.year()?)),
        1 => format!(
            "عدد حلقات «{title}» هو {}",
            shift(m.episodes.filter(|e| *e > 0)?)
        ),
        _ => {
            let studio = m.studios.as_ref()?.nodes.first()?.name.clone();
            format!("«{title}» من إنتاج استوديو {studio}")
        }
    };
    // Studios have no number to shift: a lie there is not offered.
    if kind > 1 && lie {
        return None;
    }
    Some((text, !lie))
}

fn pick<T: Clone>(v: &[T]) -> Option<T> {
    (!v.is_empty()).then(|| v[rand::random_range(0..v.len())].clone())
}

const WORK_KINDS: [MediaKind; 3] = [MediaKind::Anime, MediaKind::Manga, MediaKind::Manhwa];

/// Where the rounds' works come from: AniList kinds, or a TMDB category (dramas, series, films).
#[derive(Clone)]
pub enum Pool {
    Ani(&'static [MediaKind]),
    Tmdb(Box<TmdbPool>),
}

#[derive(Clone)]
pub struct TmdbPool {
    pub tmdb: Arc<crate::tmdb::Tmdb>,
    pub cat: crate::tmdb::find::Cat,
    pub overrides: HashMap<String, crate::tmdb::model::Target>,
}

impl Pool {
    pub fn default_pool() -> Self {
        Self::Ani(&WORK_KINDS)
    }

    /// Kinds for the AniList-based work rounds.
    fn kinds(&self) -> &'static [MediaKind] {
        match self {
            Self::Ani(k) => k,
            Self::Tmdb(_) => &WORK_KINDS,
        }
    }

    /// TMDB has posters, years and ratings but no character pictures or descriptions.
    pub fn supports(&self, mode: Mode) -> bool {
        match self {
            Self::Ani(_) => true,
            Self::Tmdb(_) => !matches!(mode, Mode::CharacterImage | Mode::CharacterHints),
        }
    }
}

async fn media_set(
    api: &SafeAniList,
    policy: ContentPolicy,
    kinds: &[MediaKind],
) -> Option<(Media, Vec<Media>)> {
    api.random_media_set(pick(kinds)?, policy)
        .await
        .ok()
        .flatten()
}

async fn make_character_image(
    api: &SafeAniList,
    policy: ContentPolicy,
    d: Difficulty,
) -> Option<Round> {
    let (answer, decoys) = api.random_character_set(policy).await.ok().flatten()?;
    let url = answer.image_url()?.to_string();
    let names: Vec<String> = decoys
        .iter()
        .map(|c| c.display_name().to_string())
        .collect();
    let (options, correct) =
        super::event::shuffle_options(answer.display_name().to_string(), names);
    let hints = (0..d.start_level()).rev().map(Hint::Picture).collect();
    Some(Round {
        title: i18n::ARENA_CHARACTER_IMAGE.into(),
        prompt: i18n::ARENA_WHO_IS_THIS.into(),
        picture: Some((url, rand::random())),
        level: d.start_level(),
        options,
        correct,
        hints,
        reveal: reveal_character(&answer),
        item: character_item(&answer),
    })
}

/// The character button of a round: shown after the reveal, pointing at the character's first work.
fn character_item(c: &Character) -> Option<(crate::components::ItemKind, String)> {
    let id = c.media.as_ref()?.nodes.first()?.id?;
    Some((
        crate::components::ItemKind::Character,
        format!("anilist:{id}"),
    ))
}

fn work_item(m: &Media) -> Option<(crate::components::ItemKind, String)> {
    Some((
        crate::components::ItemKind::Work,
        format!("anilist:{}", m.id),
    ))
}

fn reveal_character(c: &Character) -> String {
    match c.main_work() {
        Some(w) => format!("✅ **{}** — {w}", c.display_name()),
        None => format!("✅ **{}**", c.display_name()),
    }
}

async fn make_work_cover(
    api: &SafeAniList,
    policy: ContentPolicy,
    d: Difficulty,
    pool: &Pool,
) -> Option<Round> {
    let (answer, decoys) = media_set(api, policy, pool.kinds()).await?;
    let url = answer.cover()?.to_string();
    let names: Vec<String> = decoys.iter().map(|m| m.title.main().to_string()).collect();
    let (options, correct) = super::event::shuffle_options(answer.title.main().to_string(), names);
    let mut hints: Vec<Hint> = (0..d.start_level()).rev().map(Hint::Picture).collect();
    hints.push(Hint::Text(masked_clue(&answer)));
    Some(Round {
        title: i18n::ARENA_WORK_COVER.into(),
        prompt: i18n::ARENA_WHICH_WORK.into(),
        picture: Some((url, rand::random())),
        level: d.start_level(),
        options,
        correct,
        hints,
        reveal: format!("✅ **{}**", answer.title.main()),
        item: work_item(&answer),
    })
}

async fn make_character_hints(
    api: &SafeAniList,
    policy: ContentPolicy,
    _d: Difficulty,
) -> Option<Round> {
    let (answer, decoys) = api.random_character_set(policy).await.ok().flatten()?;
    let desc = answer
        .description
        .as_deref()
        .and_then(|t| crate::ui::text::clean_description_max(t, None, 600))?;
    let masked = super::event::mask_title(&desc, &[answer.display_name()]);
    let chars: Vec<char> = masked.chars().collect();
    if chars.len() < 40 {
        return None;
    }
    let cut = |n: usize| chars.iter().take(n).collect::<String>() + "…";
    let mut hints = vec![Hint::Text(cut(120)), Hint::Text(cut(260))];
    if let Some(w) = answer.main_work() {
        hints.push(Hint::Text(format!("{} {w}", i18n::ARENA_FROM)));
    }
    let names: Vec<String> = decoys
        .iter()
        .map(|c| c.display_name().to_string())
        .collect();
    let (options, correct) =
        super::event::shuffle_options(answer.display_name().to_string(), names);
    Some(Round {
        title: i18n::ARENA_CHARACTER_HINTS.into(),
        prompt: format!("{}\n{}", i18n::ARENA_WHO_IS_THIS, cut(60)),
        picture: None,
        level: 0,
        options,
        correct,
        hints,
        reveal: reveal_character(&answer),
        item: character_item(&answer),
    })
}

async fn make_true_false(
    api: &SafeAniList,
    policy: ContentPolicy,
    _d: Difficulty,
    pool: &Pool,
) -> Option<Round> {
    let (m, _) = media_set(api, policy, &pool.kinds()[..1]).await?;
    for _ in 0..6 {
        let kind = rand::random_range(0..3u8);
        let lie = rand::random::<bool>();
        if let Some((text, truth)) = statement(&m, kind, lie) {
            return Some(Round {
                title: i18n::ARENA_TRUE_FALSE.into(),
                prompt: text,
                picture: None,
                level: 0,
                options: vec![i18n::ARENA_TRUE.into(), i18n::ARENA_FALSE.into()],
                correct: if truth { 0 } else { 1 },
                hints: Vec::new(),
                item: work_item(&m),
                reveal: format!(
                    "{} **{}**",
                    i18n::ARENA_ANSWER_WAS,
                    if truth {
                        i18n::ARENA_TRUE
                    } else {
                        i18n::ARENA_FALSE
                    }
                ),
            });
        }
    }
    None
}

async fn make_higher_lower(
    api: &SafeAniList,
    policy: ContentPolicy,
    _d: Difficulty,
    pool: &Pool,
) -> Option<Round> {
    let (a, mut decoys) = media_set(api, policy, pool.kinds()).await?;
    let b = decoys.pop()?;
    let by_score = rand::random::<bool>();
    let metric = |m: &Media| {
        if by_score {
            m.average_score.map(i64::from)
        } else {
            m.popularity
        }
    };
    let (x, y) = (metric(&a)?, metric(&b)?);
    if x == y {
        return None;
    }
    let (options, correct) =
        super::event::shuffle_options(a.title.main().to_string(), vec![b.title.main().to_string()]);
    // `shuffle_options` put `a` at `correct`; the right answer is the one with the higher number.
    let right = if x > y { correct } else { 1 - correct };
    let what = if by_score {
        i18n::ARENA_HIGHER_RATED
    } else {
        i18n::ARENA_MORE_POPULAR
    };
    Some(Round {
        title: i18n::ARENA_HIGHER_LOWER.into(),
        prompt: what.into(),
        picture: None,
        level: 0,
        options: options.clone(),
        correct: right,
        hints: Vec::new(),
        item: work_item(if x > y { &a } else { &b }),
        reveal: format!(
            "✅ **{}** ({}) · {} ({})",
            options[right],
            x.max(y),
            options[1 - right],
            x.min(y)
        ),
    })
}

async fn make_order_years(
    api: &SafeAniList,
    policy: ContentPolicy,
    _d: Difficulty,
    pool: &Pool,
) -> Option<Round> {
    let (a, mut decoys) = media_set(api, policy, &pool.kinds()[..1]).await?;
    let (b, c) = (decoys.pop()?, decoys.pop()?);
    let mut works = [a, b, c];
    let years: Vec<i32> = works.iter().filter_map(Media::year).collect();
    let distinct: HashSet<i32> = years.iter().copied().collect();
    if years.len() != 3 || distinct.len() != 3 {
        return None;
    }
    // Display order is random; `order` lists indexes from the oldest to the newest.
    for i in (1..3).rev() {
        works.swap(i, rand::random_range(0..=i));
    }
    let mut order = [0usize, 1, 2];
    order.sort_by_key(|i| works[*i].year().unwrap_or(0));
    let (options, correct) = order_options(order);
    let letters = ["أ", "ب", "ج"];
    let list = works
        .iter()
        .enumerate()
        .map(|(i, m)| format!("**{})** {}", letters[i], m.title.main()))
        .collect::<Vec<_>>()
        .join("\n");
    let reveal = order
        .iter()
        .map(|i| {
            format!(
                "{} ({})",
                works[*i].title.main(),
                works[*i].year().unwrap_or(0)
            )
        })
        .collect::<Vec<_>>()
        .join(" ← ");
    Some(Round {
        title: i18n::ARENA_ORDER_YEARS.into(),
        prompt: format!("{}\n{list}", i18n::ARENA_ORDER_PROMPT),
        picture: None,
        level: 0,
        options,
        correct,
        hints: Vec::new(),
        item: None,
        reveal: format!("✅ {reveal}"),
    })
}

/// Build one round of `mode` (race picks a random kind each time). Retries a few times: AniList pages are random.
pub async fn make_round(
    api: &SafeAniList,
    policy: ContentPolicy,
    mode: Mode,
    d: Difficulty,
    pool: &Pool,
) -> Option<Round> {
    if let Pool::Tmdb(t) = pool {
        return super::arena_tmdb::make_round(t, mode, d).await;
    }
    for _ in 0..4 {
        let chosen = if mode == Mode::Race {
            [
                Mode::CharacterImage,
                Mode::WorkCover,
                Mode::CharacterHints,
                Mode::TrueFalse,
                Mode::HigherLower,
                Mode::OrderYears,
            ][rand::random_range(0..6)]
        } else {
            mode
        };
        let round = match chosen {
            Mode::CharacterImage => make_character_image(api, policy, d).await,
            Mode::WorkCover => make_work_cover(api, policy, d, pool).await,
            Mode::CharacterHints => make_character_hints(api, policy, d).await,
            Mode::TrueFalse => make_true_false(api, policy, d, pool).await,
            Mode::HigherLower => make_higher_lower(api, policy, d, pool).await,
            Mode::OrderYears => make_order_years(api, policy, d, pool).await,
            Mode::Race => None,
        };
        if round.is_some() {
            return round;
        }
    }
    None
}

// ---- scoring ---------------------------------------------------------------------------------------------

/// `(user, points, correct)` best first; ties by correct answers, then by id for a stable order.
pub fn standings(
    scores: &HashMap<serenity::UserId, (i64, i64)>,
) -> Vec<(serenity::UserId, i64, i64)> {
    let mut v: Vec<_> = scores.iter().map(|(u, (p, c))| (*u, *p, *c)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
    v
}

/// May this member skip or end the game? Only the starter or an admin.
pub fn may_manage(who: serenity::UserId, starter: serenity::UserId, admin: bool) -> bool {
    who == starter || admin
}

// ---- the shared view -------------------------------------------------------------------------------------

enum Phase {
    Asking {
        started: Instant,
        hints_shown: usize,
    },
    Revealing {
        until: Instant,
        winner: Option<serenity::UserId>,
    },
    Finished,
}

pub struct Arena {
    api: Arc<SafeAniList>,
    source: Pool,
    renderer: Arc<Renderer>,
    policy: ContentPolicy,
    pool: SqlitePool,
    guild: Option<i64>,
    starter: serenity::UserId,
    mode: Mode,
    difficulty: Difficulty,
    total_rounds: u32,
    round_no: u32,
    round: Round,
    picture: Option<(String, Vec<u8>)>,
    hint_lines: Vec<String>,
    phase: Phase,
    scores: HashMap<serenity::UserId, (i64, i64)>,
    locked: HashSet<serenity::UserId>,
    last_press: HashMap<serenity::UserId, Instant>,
    _guard: ChannelGuard,
}

impl Arena {
    fn total(&self) -> Duration {
        Duration::from_secs(self.difficulty.secs())
    }

    async fn draw_picture(&mut self) {
        self.picture = None;
        let Some((url, seed)) = &self.round.picture else {
            return;
        };
        if let Some(img) = self
            .renderer
            .guess_image(url, self.round.level, *seed)
            .await
        {
            self.picture = Some((format!("guess.{}", img.ext), img.bytes));
        }
    }

    async fn begin_round(&mut self, round: Round) {
        self.round = round;
        self.locked.clear();
        self.hint_lines.clear();
        self.round_no += 1;
        self.draw_picture().await;
        self.phase = Phase::Asking {
            started: Instant::now(),
            hints_shown: 0,
        };
    }

    fn reveal(&mut self, winner: Option<serenity::UserId>) {
        self.phase = Phase::Revealing {
            until: Instant::now() + Duration::from_secs(REVEAL_SECS),
            winner,
        };
    }

    async fn finish(&mut self) {
        self.phase = Phase::Finished;
        let Some(guild) = self.guild else { return };
        let week = gdb::week_key(chrono::Utc::now().timestamp());
        for (user, points, correct) in standings(&self.scores) {
            if let Err(e) =
                gdb::add_weekly(&self.pool, guild, &week, user.get() as i64, points, correct).await
            {
                tracing::warn!("weekly score not saved: {e}");
            }
        }
    }

    fn scoreboard(&self) -> String {
        let s = standings(&self.scores);
        if s.is_empty() {
            return i18n::ARENA_NO_SCORES.to_string();
        }
        s.iter()
            .take(5)
            .enumerate()
            .map(|(i, (u, p, c))| format!("**{}.** <@{}> — {p} ({c} ✅)", i + 1, u.get()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn answer_row(&self, enabled: bool) -> serenity::CreateActionRow {
        let asking = matches!(self.phase, Phase::Asking { .. });
        buttons(
            self.round
                .options
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    let style = match (&self.phase, i == self.round.correct) {
                        (Phase::Asking { .. }, _) => serenity::ButtonStyle::Primary,
                        (_, true) => serenity::ButtonStyle::Success,
                        (_, false) => serenity::ButtonStyle::Secondary,
                    };
                    serenity::CreateButton::new(format!("{ANSWER_PREFIX}{i}"))
                        .label(fit(label, 80))
                        .style(style)
                        .disabled(!enabled || !asking)
                })
                .collect(),
        )
    }

    fn manage_row(&self, enabled: bool) -> serenity::CreateActionRow {
        let live = enabled && !matches!(self.phase, Phase::Finished);
        buttons(vec![
            serenity::CreateButton::new(SKIP)
                .label(i18n::ARENA_SKIP)
                .emoji('⏭')
                .style(serenity::ButtonStyle::Secondary)
                .disabled(!live),
            serenity::CreateButton::new(END)
                .label(i18n::ARENA_END)
                .emoji('⏹')
                .style(serenity::ButtonStyle::Danger)
                .disabled(!live),
        ])
    }
}

impl View for Arena {
    fn shared(&self) -> bool {
        true
    }

    fn tick_after(&self) -> Option<Duration> {
        match &self.phase {
            Phase::Asking {
                started,
                hints_shown,
            } => {
                let total = self.total();
                let due = if *hints_shown < self.round.hints.len() {
                    hint_time(*hints_shown, self.round.hints.len(), total)
                } else {
                    total
                };
                Some(
                    due.saturating_sub(started.elapsed())
                        .max(Duration::from_millis(200)),
                )
            }
            Phase::Revealing { until, .. } => Some(
                until
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(200)),
            ),
            Phase::Finished => None,
        }
    }

    async fn on_tick(&mut self) -> Outcome {
        match &self.phase {
            Phase::Asking {
                started,
                hints_shown,
            } => {
                let (elapsed, shown) = (started.elapsed(), *hints_shown);
                if shown < self.round.hints.len()
                    && elapsed >= hint_time(shown, self.round.hints.len(), self.total())
                {
                    match self.round.hints[shown].clone() {
                        Hint::Picture(level) => {
                            self.round.level = level;
                            self.draw_picture().await;
                        }
                        Hint::Text(t) => self.hint_lines.push(t),
                    }
                    if let Phase::Asking { hints_shown, .. } = &mut self.phase {
                        *hints_shown += 1;
                    }
                    return Outcome::Update;
                }
                if elapsed >= self.total() {
                    self.reveal(None);
                    return Outcome::Update;
                }
                Outcome::Keep
            }
            Phase::Revealing { until, .. } => {
                if Instant::now() < *until {
                    return Outcome::Keep;
                }
                if self.round_no >= self.total_rounds {
                    self.finish().await;
                    return Outcome::Close;
                }
                match make_round(
                    &self.api,
                    self.policy,
                    self.mode,
                    self.difficulty,
                    &self.source,
                )
                .await
                {
                    Some(r) => {
                        self.begin_round(r).await;
                        Outcome::Update
                    }
                    None => {
                        // AniList is not answering: end with what was scored.
                        self.finish().await;
                        Outcome::Close
                    }
                }
            }
            Phase::Finished => Outcome::Keep,
        }
    }

    async fn on_interaction(&mut self, _id: &str, _values: &[String]) -> Outcome {
        Outcome::Keep
    }

    async fn on_user_interaction(
        &mut self,
        who: serenity::UserId,
        admin: bool,
        id: &str,
        _values: &[String],
    ) -> Outcome {
        if id == SKIP || id == END {
            if !may_manage(who, self.starter, admin) {
                return Outcome::Whisper(i18n::ARENA_MANAGE_DENIED.to_string());
            }
            if id == END {
                self.finish().await;
                return Outcome::Close;
            }
            if matches!(self.phase, Phase::Asking { .. }) {
                self.reveal(None);
                return Outcome::Update;
            }
            return Outcome::Keep;
        }
        let Some(choice) = id
            .strip_prefix(ANSWER_PREFIX)
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|i| *i < self.round.options.len())
        else {
            return Outcome::Keep;
        };
        let Phase::Asking { started, .. } = &self.phase else {
            return Outcome::Keep;
        };
        let started = *started;
        // Anti-spam: one press per member every 0.7 s.
        let now = Instant::now();
        if self
            .last_press
            .get(&who)
            .is_some_and(|t| now.duration_since(*t) < PRESS_GAP)
        {
            return Outcome::Keep;
        }
        self.last_press.insert(who, now);
        if self.locked.contains(&who) {
            return Outcome::Whisper(i18n::ARENA_ALREADY_ANSWERED.to_string());
        }
        let guild = self.guild;
        if choice != self.round.correct {
            // A wrong answer locks the member out of this round only.
            self.locked.insert(who);
            if let Some(g) = guild {
                let _ =
                    crate::db::game::record_result(&self.pool, g, who.get() as i64, false).await;
            }
            return Outcome::Whisper(i18n::ARENA_WRONG.to_string());
        }
        let pts = points_for(
            self.difficulty.base_points(),
            started.elapsed(),
            self.total(),
        );
        let e = self.scores.entry(who).or_insert((0, 0));
        e.0 += pts;
        e.1 += 1;
        if let Some(g) = guild {
            let _ = crate::db::game::record_result(&self.pool, g, who.get() as i64, true).await;
        }
        self.reveal(Some(who));
        Outcome::Update
    }

    fn render(&self, enabled: bool) -> Page {
        let mut desc = vec![self.round.prompt.clone()];
        desc.extend(self.hint_lines.iter().map(|h| format!("💡 {h}")));
        match &self.phase {
            Phase::Revealing { winner, .. } => {
                desc.push(self.round.reveal.clone());
                desc.push(match winner {
                    Some(w) => i18n::arena_winner(w.get()),
                    None => i18n::ARENA_NOBODY.to_string(),
                });
            }
            Phase::Finished => {
                desc = vec![format!("{}\n{}", i18n::ARENA_FINAL, self.scoreboard())];
            }
            Phase::Asking { .. } => {}
        }
        let mut embed = Emb::new()
            .title(format!(
                "{} · {}/{}",
                self.round.title, self.round_no, self.total_rounds
            ))
            .description(desc.join("\n\n"));
        if !matches!(self.phase, Phase::Finished) {
            embed = embed.field(i18n::ARENA_SCOREBOARD, self.scoreboard(), false);
        }
        let mut files = Vec::new();
        if let Some((name, bytes)) = &self.picture {
            if !matches!(self.phase, Phase::Finished) {
                embed = embed.image(Some(format!("attachment://{name}")));
                files.push(serenity::CreateAttachment::bytes(
                    bytes.clone(),
                    name.clone(),
                ));
            }
        }
        let mut page = Page::embed(
            embed.build(),
            vec![self.answer_row(enabled), self.manage_row(enabled)],
        );
        // The universal buttons appear only once the answer is out (rule 5).
        if let (Phase::Revealing { .. }, Some((kind, key))) = (&self.phase, &self.round.item) {
            let extra = crate::components::standard_actions(*kind, Some(key));
            if !extra.is_empty() {
                page.components.insert(1, buttons(extra));
            }
        }
        if matches!(self.phase, Phase::Finished) {
            page.components.clear();
        }
        page.with_files(files)
    }
}

/// Start a collective game in this channel.
pub async fn start(
    ctx: Context<'_>,
    mode: Mode,
    difficulty: Difficulty,
    source: Pool,
) -> Result<(), Error> {
    if !source.supports(mode) {
        return say_ephemeral(ctx, i18n::ARENA_CATEGORY_UNSUPPORTED).await;
    }
    let Some(guard) = try_claim(ctx.channel_id().get()) else {
        return say_ephemeral(ctx, i18n::ARENA_CHANNEL_BUSY).await;
    };
    let (api, pol) = (ctx.data().anilist.clone(), policy(ctx));
    let Some(round) = make_round(&api, pol, mode, difficulty, &source).await else {
        return say_ephemeral(ctx, i18n::RANDOM_FAILED).await;
    };
    let mut arena = Arena {
        api,
        source,
        renderer: ctx.data().renderer.clone(),
        policy: pol,
        pool: ctx.data().db.clone(),
        guild: ctx.guild_id().map(|g| g.get() as i64),
        starter: ctx.author().id,
        mode,
        difficulty,
        total_rounds: rounds_for(mode),
        round_no: 0,
        round: round.clone(),
        picture: None,
        hint_lines: Vec::new(),
        phase: Phase::Finished,
        scores: HashMap::new(),
        locked: HashSet::new(),
        last_press: HashMap::new(),
        _guard: guard,
    };
    arena.begin_round(round).await;
    session::run(ctx, arena).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn uid(n: u64) -> serenity::UserId {
        serenity::UserId::new(n)
    }

    #[test]
    fn every_mode_has_a_unique_key_and_difficulty_changes_the_timer() {
        let mut keys = HashSet::new();
        assert!(MODES
            .iter()
            .all(|(l, k, _)| keys.insert(*k) && !l.is_empty()));
        assert_eq!(MODES.len(), 7);
        assert_eq!(mode_of("race"), Some(Mode::Race));
        assert_eq!(mode_of("nope"), None);
        assert!(
            Difficulty::Easy.secs() > Difficulty::Normal.secs()
                && Difficulty::Normal.secs() > Difficulty::Hard.secs()
        );
        assert_eq!(Difficulty::parse("hard"), Difficulty::Hard);
        assert_eq!(Difficulty::parse("whatever"), Difficulty::Normal);
        assert_eq!(
            (rounds_for(Mode::Race), rounds_for(Mode::TrueFalse)),
            (10, 5)
        );
    }

    #[test]
    fn quicker_answers_score_more_and_nobody_scores_below_the_base() {
        let total = Duration::from_secs(20);
        assert_eq!(points_for(10, Duration::ZERO, total), 15);
        assert_eq!(points_for(10, Duration::from_secs(10), total), 13);
        assert_eq!(points_for(10, total, total), 10);
        assert_eq!(points_for(10, Duration::from_secs(99), total), 10);
    }

    #[test]
    fn only_the_starter_or_an_admin_may_manage_the_game() {
        assert!(may_manage(uid(1), uid(1), false));
        assert!(may_manage(uid(2), uid(1), true));
        assert!(
            !may_manage(uid(2), uid(1), false),
            "another member can answer but not end the game"
        );
    }

    #[test]
    fn standings_rank_by_points_then_correct_answers() {
        let scores = HashMap::from([(uid(1), (20, 2)), (uid(2), (20, 3)), (uid(3), (5, 1))]);
        let s = standings(&scores);
        assert_eq!(
            s.iter().map(|x| x.0.get()).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
    }

    #[test]
    fn a_channel_hosts_one_game_at_a_time_and_is_freed_afterwards() {
        let a = try_claim(9_000_001).expect("first");
        assert!(
            try_claim(9_000_001).is_none(),
            "second game in the same channel"
        );
        assert!(try_claim(9_000_002).is_some(), "another channel is free");
        drop(a);
        assert!(try_claim(9_000_001).is_some(), "freed when the game ends");
    }

    #[test]
    fn hints_are_spread_over_the_round() {
        let total = Duration::from_secs(20);
        let times: Vec<_> = (0..3).map(|n| hint_time(n, 3, total)).collect();
        assert!(times.windows(2).all(|w| w[0] < w[1]));
        assert!(times[2] < total, "the last hint comes before time runs out");
    }

    #[test]
    fn ordering_options_are_four_distinct_permutations_with_the_right_one_included() {
        for _ in 0..30 {
            let (opts, at) = order_options([1, 2, 0]);
            assert_eq!(opts.len(), 4);
            let set: HashSet<_> = opts.iter().collect();
            assert_eq!(set.len(), 4, "distinct options");
            assert_eq!(opts[at], "ب ← ج ← أ");
        }
    }

    #[test]
    fn true_false_statements_use_known_facts_and_lies_change_the_number() {
        let m: Media = serde_json::from_value(json!({
            "id": 1, "title": {"romaji": "Show"}, "seasonYear": 2013, "episodes": 24,
            "studios": {"nodes": [{"name": "Bones"}]}
        }))
        .expect("media");
        let (t, truth) = statement(&m, 0, false).expect("year");
        assert!(t.contains("2013") && truth);
        let (t, truth) = statement(&m, 0, true).expect("lie");
        assert!(!t.contains("2013") && !truth);
        let (t, truth) = statement(&m, 1, false).expect("episodes");
        assert!(t.contains("24") && truth);
        let (t, _) = statement(&m, 2, false).expect("studio");
        assert!(t.contains("Bones"));
        assert!(
            statement(&m, 2, true).is_none(),
            "a studio cannot be shifted"
        );
        let bare: Media =
            serde_json::from_value(json!({"id": 2, "title": {"romaji": "X"}})).expect("media");
        assert!(
            statement(&bare, 0, false).is_none() && statement(&bare, 1, false).is_none(),
            "no fact, no statement"
        );
    }

    async fn sample_arena(starter: u64, channel: u64) -> Arena {
        let round = Round {
            title: "t".into(),
            prompt: "p".into(),
            picture: None,
            level: 0,
            options: vec!["A".into(), "B".into(), "C".into()],
            correct: 1,
            hints: Vec::new(),
            reveal: "r".into(),
            item: None,
        };
        Arena {
            source: Pool::default_pool(),
            api: Arc::new(SafeAniList::with_fixture(|_, _| {
                Err(crate::anilist::AniError::NotFound)
            })),
            renderer: Arc::new(Renderer::new(reqwest::Client::new())),
            policy: ContentPolicy::SAFE,
            pool: crate::db::connect_memory().await,
            guild: Some(1),
            starter: uid(starter),
            mode: Mode::TrueFalse,
            difficulty: Difficulty::Normal,
            total_rounds: 3,
            round_no: 1,
            round,
            picture: None,
            hint_lines: Vec::new(),
            phase: Phase::Asking {
                started: Instant::now(),
                hints_shown: 0,
            },
            scores: HashMap::new(),
            locked: HashSet::new(),
            last_press: HashMap::new(),
            _guard: try_claim(channel).expect("free channel"),
        }
    }

    #[tokio::test]
    async fn a_member_other_than_the_starter_can_answer_and_the_first_correct_answer_wins_the_point(
    ) {
        let mut g = sample_arena(1, 9_100_001).await;
        assert!(g.shared(), "answers are not restricted to the invoker");
        // Member 2 (not the starter) answers correctly.
        let out = g.on_user_interaction(uid(2), false, "ans:1", &[]).await;
        assert_eq!(out, Outcome::Update);
        assert_eq!(g.scores[&uid(2)].1, 1);
        assert!(
            g.scores[&uid(2)].0 >= 10,
            "base points plus the speed bonus"
        );
        assert!(matches!(g.phase, Phase::Revealing { winner: Some(w), .. } if w == uid(2)));
        // A later correct answer in the same round scores nothing: the round is already decided.
        let late = g.on_user_interaction(uid(3), false, "ans:1", &[]).await;
        assert_eq!(late, Outcome::Keep);
        assert!(!g.scores.contains_key(&uid(3)));
    }

    #[tokio::test]
    async fn a_wrong_answer_locks_that_member_out_of_the_round_only() {
        let mut g = sample_arena(1, 9_100_002).await;
        let wrong = g.on_user_interaction(uid(2), false, "ans:0", &[]).await;
        assert_eq!(wrong, Outcome::Whisper(i18n::ARENA_WRONG.to_string()));
        tokio::time::sleep(PRESS_GAP + Duration::from_millis(50)).await;
        let again = g.on_user_interaction(uid(2), false, "ans:1", &[]).await;
        assert_eq!(
            again,
            Outcome::Whisper(i18n::ARENA_ALREADY_ANSWERED.to_string()),
            "no second try in the same round"
        );
        assert!(g.scores.is_empty());
        // Someone else can still win it.
        assert_eq!(
            g.on_user_interaction(uid(3), false, "ans:1", &[]).await,
            Outcome::Update
        );
    }

    #[tokio::test]
    async fn spamming_the_buttons_is_ignored() {
        let mut g = sample_arena(1, 9_100_003).await;
        g.on_user_interaction(uid(2), false, "ans:0", &[]).await;
        // Immediately again: ignored silently (not even the "already answered" reply).
        assert_eq!(
            g.on_user_interaction(uid(2), false, "ans:1", &[]).await,
            Outcome::Keep
        );
    }

    #[tokio::test]
    async fn only_the_starter_or_an_admin_can_skip_or_end() {
        let mut g = sample_arena(1, 9_100_004).await;
        let denied = Outcome::Whisper(i18n::ARENA_MANAGE_DENIED.to_string());
        assert_eq!(
            g.on_user_interaction(uid(2), false, SKIP, &[]).await,
            denied
        );
        assert_eq!(g.on_user_interaction(uid(2), false, END, &[]).await, denied);
        assert!(
            matches!(g.phase, Phase::Asking { .. }),
            "a refusal changes nothing"
        );
        assert_eq!(
            g.on_user_interaction(uid(1), false, SKIP, &[]).await,
            Outcome::Update,
            "the starter skips"
        );
        assert!(matches!(g.phase, Phase::Revealing { winner: None, .. }));
        assert_eq!(
            g.on_user_interaction(uid(9), true, END, &[]).await,
            Outcome::Close,
            "an admin ends the game"
        );
        assert!(matches!(g.phase, Phase::Finished));
    }

    #[tokio::test]
    async fn finishing_saves_the_weekly_points_per_server() {
        let mut g = sample_arena(1, 9_100_005).await;
        g.on_user_interaction(uid(2), false, "ans:1", &[]).await;
        g.finish().await;
        let week = gdb::week_key(chrono::Utc::now().timestamp());
        let board = gdb::weekly_leaderboard(&g.pool, 1, &week, 10)
            .await
            .expect("board");
        assert_eq!(board.len(), 1);
        assert_eq!((board[0].0, board[0].2), (2, 1));
        assert!(board[0].1 >= 10);
    }

    #[test]
    fn fixed_rounds_never_hand_out_adult_content() {
        // Every round comes from `SafeAniList` (isAdult filtered); the module has no raw client access.
        let src = include_str!("arena.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(!prod.contains("Client::") && !prod.contains("queries::"));
    }
}
