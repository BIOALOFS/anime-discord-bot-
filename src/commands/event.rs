//! `/game mode`: the collective games (see `arena`), versus and the leaderboards.

use super::{
    buttons,
    character::{CharacterView, Mode as CharMode},
    fail, policy, say_ephemeral,
};
use crate::{
    anilist::{models::Character, models::Media, ContentPolicy, SafeAniList},
    db::{custom, game},
    error::{Context, Error},
    i18n,
    ui::{
        autocomplete::to_choices,
        embeds::Emb,
        select::{character_choices, select_row},
        session::{self, Outcome, Page, View, Whisper},
        text::clean_description_max,
    },
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

pub fn modes() -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<(&'static str, &'static str)> = super::arena::MODES
        .iter()
        .map(|(l, k, _)| (*l, *k))
        .collect();
    v.push(("أيهما تفضّل؟", "versus"));
    v.push(("الترتيب", "leaderboard"));
    v
}

// ---- one active game per user per channel -------------------------------------------------------

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

/// `(options, index of the answer)` with the answer in a random position.
pub fn shuffle_options<T>(answer: T, decoys: Vec<T>) -> (Vec<T>, usize) {
    let mut all = decoys;
    let at = rand::random_range(0..=all.len());
    all.insert(at, answer);
    (all, at)
}

// ---- quiz (characters and anime) --------------------------------------------------------------------

pub fn masked_clue(m: &Media) -> String {
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

// ---- versus -------------------------------------------------------------------------------------------------

struct Versus {
    pool: SqlitePool,
    api: std::sync::Arc<SafeAniList>,
    policy: ContentPolicy,
    user: i64,
    a: Character,
    b: Character,
    voted: Option<(bool, u32, u32)>,
    /// A character card opened from the dropdown after voting; Back returns to the result.
    sub: Option<CharacterView>,
}

const VERSUS_PICK: &str = "versus_pick";

impl View for Versus {
    fn take_whisper(&mut self) -> Option<Whisper> {
        self.sub.as_mut().and_then(|s| s.take_whisper())
    }

    fn render(&self, enabled: bool) -> Page {
        if let Some(sub) = &self.sub {
            return sub.render(enabled);
        }
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
        let mut components = vec![row];
        // Both names are listed: once the vote is in, a dropdown opens either character with its buttons.
        if self.voted.is_some() {
            components.extend(select_row(
                VERSUS_PICK,
                i18n::CAST_PLACEHOLDER,
                character_choices(&[self.a.clone(), self.b.clone()]),
                enabled,
            ));
        }
        Page {
            content: notice.or(Some(i18n::VERSUS_PROMPT.to_string())),
            embeds: vec![ea.build(), eb.build()],
            components,
            files: vec![],
            v2: None,
        }
    }

    async fn on_interaction(&mut self, id: &str, values: &[String]) -> Outcome {
        if let Some(sub) = self.sub.as_mut() {
            if sub.wants_to_leave(id) {
                self.sub = None;
                return Outcome::Update;
            }
            return sub.on_interaction(id, values).await;
        }
        if id == VERSUS_PICK {
            let picked = values.first().and_then(|v| v.parse::<i64>().ok());
            let Some(c) = [&self.a, &self.b]
                .into_iter()
                .find(|c| Some(c.id) == picked)
                .cloned()
            else {
                return Outcome::Keep;
            };
            let mut sub = CharacterView::new(
                self.api.clone(),
                self.policy,
                CharMode::Card(c),
                self.pool.clone(),
                self.user,
            )
            .with_back();
            sub.prepare().await;
            self.sub = Some(sub);
            return Outcome::Update;
        }
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
        Outcome::Update
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
                    api: api.clone(),
                    policy: pol,
                    user: ctx.author().id.get() as i64,
                    a,
                    b: decoys.remove(0),
                    voted: None,
                    sub: None,
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
        // This week's points of the collective games first, then the all-time server table.
        let week = game::week_key(chrono::Utc::now().timestamp());
        let weekly = game::weekly_leaderboard(pool, g, &week, 10).await?;
        let weekly = if weekly.is_empty() {
            i18n::EMPTY_LIST.to_string()
        } else {
            weekly
                .iter()
                .enumerate()
                .map(|(i, (u, p, c))| format!("**{}.** <@{u}> — {p} ({c} ✅)", i + 1))
                .collect::<Vec<_>>()
                .join("\n")
        };
        embed = embed.field(i18n::ARENA_WEEKLY, weekly, false);
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

async fn ac_difficulty(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    if !crate::guild::autocomplete_allowed(ctx).await {
        return Vec::new();
    }
    let n = partial.trim().to_lowercase();
    to_choices(
        super::arena::DIFFICULTIES
            .iter()
            .filter(|(l, k)| n.is_empty() || l.contains(&n) || k.contains(&n))
            .map(|(l, k)| ((*l).to_string(), (*k).to_string()))
            .collect(),
    )
}

/// `(Arabic label, key)` of the event categories.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("أنمي", "anime"),
    ("مانجا", "manga"),
    ("رواية", "novel"),
    ("دراما", "drama"),
    ("مسلسل", "series"),
    ("فيلم", "movie"),
];

async fn ac_category(_ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let n = partial.trim().to_lowercase();
    to_choices(
        CATEGORIES
            .iter()
            .filter(|(l, k)| n.is_empty() || l.contains(&n) || k.contains(&n))
            .map(|(l, k)| ((*l).to_string(), (*k).to_string()))
            .collect(),
    )
}

/// The pool of works for a category key (anime by default; unknown keys fall back to it).
pub async fn pool_for(ctx: Context<'_>, category: Option<&str>) -> super::arena::Pool {
    use crate::{anilist::MediaKind, tmdb::find::Cat};
    let key = category.map(str::trim).unwrap_or("anime");
    let key = CATEGORIES
        .iter()
        .find(|(l, k)| *k == key || *l == key)
        .map_or("anime", |(_, k)| *k);
    match key {
        "manga" => {
            super::arena::Pool::Ani(&[MediaKind::Manga, MediaKind::Manhwa, MediaKind::Manhua])
        }
        "novel" => super::arena::Pool::Ani(&[MediaKind::Novel]),
        "drama" | "series" | "movie" => {
            let cat = match key {
                "drama" => Cat::Drama,
                "series" => Cat::Series,
                _ => Cat::Movie,
            };
            let overrides = match ctx.guild_id() {
                Some(g) => crate::db::notify::drama_overrides(&ctx.data().db, g.get() as i64)
                    .await
                    .unwrap_or_default(),
                None => Default::default(),
            };
            super::arena::Pool::Tmdb(Box::new(super::arena::TmdbPool {
                tmdb: ctx.data().tmdb.clone(),
                cat,
                overrides,
            }))
        }
        _ => super::arena::Pool::default_pool(),
    }
}

/// Collective games, versus and the leaderboard.
#[poise::command(
    slash_command,
    name_localized("ar", "فعالية"),
    description_localized(
        "ar",
        "الفعاليات الجماعية: خمّن، صح أم خطأ، الأعلى تقييمًا، الترتيب بالسنة، السباق"
    )
)]
pub async fn event(
    ctx: Context<'_>,
    #[description = "Game mode"]
    #[description_localized("ar", "نوع اللعبة")]
    #[name_localized("ar", "النوع")]
    #[autocomplete = "ac_mode"]
    mode: String,
    #[description = "Difficulty: easy, normal or hard"]
    #[description_localized("ar", "الصعوبة: سهل أو عادي أو صعب")]
    #[name_localized("ar", "الصعوبة")]
    #[autocomplete = "ac_difficulty"]
    difficulty: Option<String>,
    #[description = "Category: anime, manga, novel, drama, series or movie"]
    #[description_localized("ar", "الفئة: أنمي، مانجا، رواية، دراما، مسلسل أو فيلم")]
    #[name_localized("ar", "الفئة")]
    #[autocomplete = "ac_category"]
    category: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    // Starting a game may be limited to some roles; the leaderboard stays open to everyone.
    if mode.trim() != "leaderboard" {
        if let Some(text) = crate::features::perms::denied(ctx, "event").await {
            return say_ephemeral(ctx, &text).await;
        }
    }
    let guild = ctx.guild_id().map(|g| g.get() as i64);
    if let Some(g) = guild {
        if !custom::games_enabled(&ctx.data().db, g).await? {
            return say_ephemeral(ctx, i18n::GAMES_DISABLED).await;
        }
    }
    let level = super::arena::Difficulty::parse(difficulty.as_deref().unwrap_or("normal"));
    if let Some(m) = super::arena::mode_of(&mode) {
        let pool = pool_for(ctx, category.as_deref()).await;
        return super::arena::start(ctx, m, level, pool).await;
    }
    match mode.trim() {
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
    fn the_answer_is_somewhere_among_the_options() {
        for _ in 0..50 {
            let (opts, at) = shuffle_options("answer", vec!["a", "b", "c"]);
            assert_eq!(opts.len(), 4);
            assert_eq!(opts[at], "answer");
        }
    }

    #[test]
    fn every_mode_is_listed() {
        let keys: Vec<_> = modes().into_iter().map(|(_, k)| k).collect();
        assert_eq!(
            keys,
            vec![
                "character-image",
                "work-cover",
                "character-hints",
                "true-false",
                "higher-lower",
                "order-years",
                "race",
                "versus",
                "leaderboard"
            ]
        );
    }
}
