//! `/game mode`: guess the character, guess the anime, versus and the leaderboard.

use super::{buttons, fail, policy, say_ephemeral};
use crate::{
    anilist::{models::Character, models::Media, MediaKind},
    db::{custom, game},
    error::{Context, Error},
    i18n,
    render::Rendered,
    ui::{
        autocomplete::to_choices,
        cards,
        embeds::Emb,
        session::{self, Outcome, Page, View},
        text::clean_description_max,
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const ANSWER_SECONDS: u64 = 20;

pub fn modes() -> Vec<(&'static str, &'static str)> {
    vec![
        ("خمّن الشخصية", "guess-character"),
        ("خمّن الأنمي", "guess-anime"),
        ("أيهما تفضّل؟", "versus"),
        ("الترتيب", "leaderboard"),
    ]
}

// ---- one active game per user per channel -------------------------------------------------------

fn active() -> &'static Mutex<HashSet<(u64, u64)>> {
    static ACTIVE: OnceLock<Mutex<HashSet<(u64, u64)>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Dropping it frees the slot, however the game ends.
pub struct ActiveGuard(u64, u64);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = active().lock() {
            set.remove(&(self.0, self.1));
        }
    }
}

pub fn try_start(user: u64, channel: u64) -> Option<ActiveGuard> {
    let mut set = active().lock().ok()?;
    // `then_some` would build (and immediately drop) a guard even on failure, freeing the other game's slot
    // and deadlocking on the lock held here.
    if set.insert((user, channel)) {
        Some(ActiveGuard(user, channel))
    } else {
        None
    }
}

// ---- pure helpers -------------------------------------------------------------------------------

/// Replace title words (3+ letters) in `text` with blocks so the description does not give the answer away.
pub fn mask_title(text: &str, titles: &[&str]) -> String {
    let mut words: Vec<String> = titles
        .iter()
        .flat_map(|t| t.split(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect();
    words.sort();
    words.dedup();
    text.split_inclusive(|c: char| !c.is_alphanumeric())
        .map(|chunk| {
            let (word, tail): (String, String) = (
                chunk.chars().take_while(|c| c.is_alphanumeric()).collect(),
                chunk.chars().skip_while(|c| c.is_alphanumeric()).collect(),
            );
            if words.contains(&word.to_lowercase()) {
                format!("▇▇▇{tail}")
            } else {
                chunk.to_string()
            }
        })
        .collect()
}

/// `▰▰▰▱▱▱▱▱▱▱ 30%`
pub fn percent_bar(pct: u32) -> String {
    let filled = ((pct.min(100) as f32) / 10.0).round() as usize;
    format!("{}{} {pct}%", "▰".repeat(filled), "▱".repeat(10 - filled))
}

pub fn win_rate(wins: i64, shown: i64) -> u32 {
    if shown <= 0 {
        50
    } else {
        ((wins as f64 / shown as f64) * 100.0).round() as u32
    }
}

/// Difficulty grows with the streak: 0, 1 then 2 (max).
pub fn level_for_streak(streak: i64) -> u8 {
    (streak / 3).clamp(0, 2) as u8
}

/// `(options, index of the answer)` with the answer in a random position.
pub fn shuffle_options<T>(answer: T, decoys: Vec<T>) -> (Vec<T>, usize) {
    let mut all = decoys;
    let at = rand::random_range(0..=all.len());
    all.insert(at, answer);
    (all, at)
}

// ---- quiz (characters and anime) --------------------------------------------------------------------

struct Quiz {
    pool: SqlitePool,
    guild: Option<i64>,
    user: i64,
    labels: Vec<String>,
    correct: usize,
    started: Instant,
    question: Emb,
    reveal: Emb,
    image: Option<Rendered>,
    result: Option<(bool, usize, i64)>,
    _guard: ActiveGuard,
}

impl View for Quiz {
    fn render(&self, enabled: bool) -> Page {
        let style = |i: usize| match &self.result {
            Some(_) if i == self.correct => serenity::ButtonStyle::Success,
            Some((_, chosen, _)) if i == *chosen => serenity::ButtonStyle::Danger,
            _ => serenity::ButtonStyle::Primary,
        };
        let row = buttons(
            self.labels
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    serenity::CreateButton::new(format!("ans:{i}"))
                        .label(crate::ui::embeds::fit(l, 80))
                        .style(style(i))
                        .disabled(!enabled || self.result.is_some())
                })
                .collect(),
        );
        match &self.result {
            None => {
                let page = Page::embed(self.question.clone().build(), vec![row]);
                match &self.image {
                    Some(img) => page.with_files(vec![serenity::CreateAttachment::bytes(
                        img.bytes.clone(),
                        format!("guess.{}", img.ext),
                    )]),
                    None => page,
                }
            }
            Some((correct, _, streak)) => {
                let head = if *correct {
                    i18n::game_correct(*streak)
                } else {
                    i18n::GAME_WRONG.to_string()
                };
                Page::embed(self.reveal.clone().build(), vec![row]).notice(Some(head))
            }
        }
    }

    async fn on_interaction(&mut self, id: &str, _values: &[String]) -> Outcome {
        let Some(i) = id
            .strip_prefix("ans:")
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|i| *i < self.labels.len())
        else {
            return Outcome::Keep;
        };
        // Late answers count as a miss.
        let on_time = self.started.elapsed() <= Duration::from_secs(ANSWER_SECONDS + 2);
        let correct = on_time && i == self.correct;
        let streak = match self.guild {
            Some(g) => game::record_result(&self.pool, g, self.user, correct)
                .await
                .unwrap_or(0),
            None => 0,
        };
        self.result = Some((correct, i, streak));
        Outcome::Close
    }
}

async fn start_quiz_character(ctx: Context<'_>, guild: Option<i64>) -> Result<(), Error> {
    let (api, pol) = (ctx.data().anilist.clone(), policy(ctx));
    let (answer, decoys) = match api.random_character_set(pol).await {
        Ok(Some(v)) => v,
        Ok(None) => return say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
        Err(e) => return fail(ctx, e).await,
    };
    let streak = match guild {
        Some(g) => game::current_streak(&ctx.data().db, g, ctx.author().id.get() as i64)
            .await
            .unwrap_or(0),
        None => 0,
    };
    let level = level_for_streak(streak);
    let Some(url) = answer.image_url().map(str::to_string) else {
        return say_ephemeral(ctx, i18n::RANDOM_FAILED).await;
    };
    let image = ctx.data().renderer.guess_image(&url, level).await;
    let Some(guard) = try_start(ctx.author().id.get(), ctx.channel_id().get()) else {
        return say_ephemeral(ctx, i18n::GAME_ALREADY_ACTIVE).await;
    };
    let names: Vec<String> = decoys
        .iter()
        .map(|c| c.display_name().to_string())
        .collect();
    let (labels, correct) = shuffle_options(answer.display_name().to_string(), names);
    let mut question = Emb::new()
        .title(i18n::GUESS_CHARACTER_TITLE)
        .description(i18n::guess_hint(ANSWER_SECONDS, level, streak));
    if image.is_some() {
        question = question.image(Some("attachment://guess.webp"));
    }
    let quiz = Quiz {
        pool: ctx.data().db.clone(),
        guild,
        user: ctx.author().id.get() as i64,
        labels,
        correct,
        started: Instant::now(),
        question,
        reveal: cards::character_card(&answer),
        image,
        result: None,
        _guard: guard,
    };
    session::run(ctx, quiz).await
}

fn masked_clue(m: &Media) -> String {
    let titles: Vec<&str> = [
        m.title.romaji.as_deref(),
        m.title.english.as_deref(),
        m.title.native.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let desc = m
        .description
        .as_deref()
        .and_then(|d| clean_description_max(d, None, 300))
        .map(|d| mask_title(&d, &titles));
    let mut facts = Vec::new();
    if let Some(y) = m.year() {
        facts.push(y.to_string());
    }
    if let Some(s) = m.studios.as_ref().and_then(|s| s.nodes.first()) {
        facts.push(s.name.clone());
    }
    if !m.genres.is_empty() {
        facts.push(
            m.genres
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(" • "),
        );
    }
    match desc {
        Some(d) => format!("{d}\n\n{}", facts.join(" · ")),
        None => facts.join(" · "),
    }
}

async fn start_quiz_anime(ctx: Context<'_>, guild: Option<i64>) -> Result<(), Error> {
    let (api, pol) = (ctx.data().anilist.clone(), policy(ctx));
    let (answer, decoys) = match api.random_media_set(MediaKind::Anime, pol).await {
        Ok(Some(v)) => v,
        Ok(None) => return say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
        Err(e) => return fail(ctx, e).await,
    };
    let Some(guard) = try_start(ctx.author().id.get(), ctx.channel_id().get()) else {
        return say_ephemeral(ctx, i18n::GAME_ALREADY_ACTIVE).await;
    };
    let names: Vec<String> = decoys.iter().map(|m| m.title.main().to_string()).collect();
    let (labels, correct) = shuffle_options(answer.title.main().to_string(), names);
    let question = Emb::new()
        .title(i18n::GUESS_ANIME_TITLE)
        .description(format!(
            "{}\n\n{}",
            masked_clue(&answer),
            i18n::guess_hint(ANSWER_SECONDS, 0, 0)
        ));
    let quiz = Quiz {
        pool: ctx.data().db.clone(),
        guild,
        user: ctx.author().id.get() as i64,
        labels,
        correct,
        started: Instant::now(),
        question,
        reveal: cards::work_card(&answer),
        image: None,
        result: None,
        _guard: guard,
    };
    session::run(ctx, quiz).await
}

// ---- versus -------------------------------------------------------------------------------------------------

struct Versus {
    pool: SqlitePool,
    a: Character,
    b: Character,
    voted: Option<(bool, u32, u32)>,
}

impl View for Versus {
    fn render(&self, enabled: bool) -> Page {
        let mut ea = Emb::new()
            .title(format!("🅰️ {}", self.a.display_name()))
            .image(self.a.image_url().map(str::to_string));
        let mut eb = Emb::new()
            .title(format!("🅱️ {}", self.b.display_name()))
            .image(self.b.image_url().map(str::to_string));
        ea.footer = None;
        eb.footer = None;
        let mk = |id: &str, label: &str, chosen: bool| {
            serenity::CreateButton::new(id)
                .label(crate::ui::embeds::fit(label, 80))
                .style(if chosen {
                    serenity::ButtonStyle::Success
                } else {
                    serenity::ButtonStyle::Primary
                })
                .disabled(!enabled || self.voted.is_some())
        };
        let (a_won, b_won) = self.voted.map_or((false, false), |(a, _, _)| (a, !a));
        let row = buttons(vec![
            mk("vote:a", self.a.display_name(), a_won),
            mk("vote:b", self.b.display_name(), b_won),
        ]);
        let notice = self.voted.map(|(_, pa, pb)| {
            format!(
                "{}\n🅰️ {}\n🅱️ {}",
                i18n::VERSUS_RESULTS,
                percent_bar(pa),
                percent_bar(pb)
            )
        });
        Page {
            content: notice.or(Some(i18n::VERSUS_PROMPT.to_string())),
            embeds: vec![ea.build(), eb.build()],
            components: vec![row],
            files: vec![],
        }
    }

    async fn on_interaction(&mut self, id: &str, _values: &[String]) -> Outcome {
        let a_wins = match id {
            "vote:a" => true,
            "vote:b" => false,
            _ => return Outcome::Keep,
        };
        let (winner, loser) = if a_wins {
            (self.a.id, self.b.id)
        } else {
            (self.b.id, self.a.id)
        };
        if let Err(e) = game::record_versus(&self.pool, winner, loser).await {
            tracing::warn!("versus vote failed: {e}");
            return Outcome::Keep;
        }
        let (wa, sa) = game::versus_stats(&self.pool, self.a.id)
            .await
            .unwrap_or((0, 0));
        let (wb, sb) = game::versus_stats(&self.pool, self.b.id)
            .await
            .unwrap_or((0, 0));
        let (pa, pb) = (win_rate(wa, sa), win_rate(wb, sb));
        self.voted = Some((a_wins, pa, pb));
        Outcome::Close
    }
}

async fn start_versus(ctx: Context<'_>) -> Result<(), Error> {
    let (api, pol) = (ctx.data().anilist.clone(), policy(ctx));
    match api.random_character_set(pol).await {
        Ok(Some((a, mut decoys))) => {
            session::run(
                ctx,
                Versus {
                    pool: ctx.data().db.clone(),
                    a,
                    b: decoys.remove(0),
                    voted: None,
                },
            )
            .await
        }
        Ok(None) => say_ephemeral(ctx, i18n::RANDOM_FAILED).await,
        Err(e) => fail(ctx, e).await,
    }
}

// ---- leaderboard ------------------------------------------------------------------------------------------

async fn show_leaderboard(ctx: Context<'_>, guild: Option<i64>) -> Result<(), Error> {
    let pool = &ctx.data().db;
    let fmt = |rows: Vec<(i64, i64, i64, i64)>| {
        if rows.is_empty() {
            return i18n::EMPTY_LIST.to_string();
        }
        rows.iter()
            .enumerate()
            .map(|(i, (u, c, p, b))| format!("**{}.** <@{u}> — ✅ {c}/{p} · 🔥 {b}", i + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut embed = Emb::new().title(i18n::LEADERBOARD_TITLE);
    if let Some(g) = guild {
        embed = embed.field(
            i18n::SCOPE_SERVER,
            fmt(game::leaderboard(pool, Some(g), 10).await?),
            false,
        );
    }
    embed = embed.field(
        i18n::SCOPE_GLOBAL,
        fmt(game::leaderboard(pool, None, 10).await?),
        false,
    );
    ctx.send(poise::CreateReply::default().embed(embed.build()))
        .await?;
    Ok(())
}

async fn ac_mode(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let n = partial.trim().to_lowercase();
    to_choices(
        modes()
            .into_iter()
            .filter(|(l, k)| n.is_empty() || l.contains(&n) || k.contains(&n))
            .map(|(l, k)| (l.to_string(), k.to_string()))
            .collect(),
    )
}

/// Guess the character, guess the anime, versus or the leaderboard.
#[poise::command(
    slash_command,
    name_localized("ar", "لعبة"),
    description_localized("ar", "ألعاب: خمّن الشخصية أو الأنمي، أيهما تفضّل، والترتيب")
)]
pub async fn game(
    ctx: Context<'_>,
    #[description = "Game mode"]
    #[description_localized("ar", "نوع اللعبة")]
    #[name_localized("ar", "النوع")]
    #[autocomplete = "ac_mode"]
    mode: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let guild = ctx.guild_id().map(|g| g.get() as i64);
    if let Some(g) = guild {
        if !custom::games_enabled(&ctx.data().db, g).await? {
            return say_ephemeral(ctx, i18n::GAMES_DISABLED).await;
        }
    }
    match mode.trim() {
        "guess-character" => start_quiz_character(ctx, guild).await,
        "guess-anime" => start_quiz_anime(ctx, guild).await,
        "versus" => start_versus(ctx).await,
        "leaderboard" => show_leaderboard(ctx, guild).await,
        _ => say_ephemeral(ctx, i18n::GAME_PICK_MODE).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_masked_in_descriptions() {
        let m = mask_title(
            "Naruto Uzumaki wants to become Hokage. Naruto trains!",
            &["Naruto", "NARUTO: Shippuden"],
        );
        assert!(!m.to_lowercase().contains("naruto"), "{m}");
        assert!(m.contains("Hokage") && m.contains("▇▇▇ Uzumaki"));
        assert_eq!(
            mask_title("no overlap here", &["Bleach"]),
            "no overlap here"
        );
        assert_eq!(
            mask_title("An ox and a cat", &["No"]),
            "An ox and a cat",
            "words shorter than 3 letters are not masked"
        );
        assert_eq!(mask_title("", &["X"]), "");
    }

    #[test]
    fn percent_bars_and_win_rates() {
        assert_eq!(percent_bar(0), "▱▱▱▱▱▱▱▱▱▱ 0%");
        assert_eq!(percent_bar(30), "▰▰▰▱▱▱▱▱▱▱ 30%");
        assert_eq!(percent_bar(100), "▰▰▰▰▰▰▰▰▰▰ 100%");
        assert_eq!(percent_bar(250), "▰▰▰▰▰▰▰▰▰▰ 250%");
        assert_eq!(win_rate(0, 0), 50);
        assert_eq!(win_rate(3, 4), 75);
        assert_eq!(win_rate(0, 5), 0);
    }

    #[test]
    fn difficulty_rises_with_the_streak_up_to_a_cap() {
        let levels: Vec<u8> = [0, 2, 3, 5, 6, 30]
            .iter()
            .map(|s| level_for_streak(*s))
            .collect();
        assert_eq!(levels, vec![0, 0, 1, 1, 2, 2]);
    }

    #[test]
    fn the_answer_is_somewhere_among_the_options() {
        for _ in 0..50 {
            let (opts, at) = shuffle_options("answer", vec!["a", "b", "c"]);
            assert_eq!(opts.len(), 4);
            assert_eq!(opts[at], "answer");
        }
    }

    #[test]
    fn only_one_game_per_user_per_channel() {
        let first = try_start(1, 100).expect("free");
        assert!(try_start(1, 100).is_none(), "same user, same channel");
        assert!(try_start(1, 101).is_some(), "another channel is fine");
        assert!(try_start(2, 100).is_some(), "another user is fine");
        drop(first);
        assert!(
            try_start(1, 100).is_some(),
            "the slot frees when the game ends"
        );
    }

    #[test]
    fn every_mode_is_listed() {
        let keys: Vec<_> = modes().into_iter().map(|(_, k)| k).collect();
        assert_eq!(
            keys,
            vec!["guess-character", "guess-anime", "versus", "leaderboard"]
        );
    }
}
