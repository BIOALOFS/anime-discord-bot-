//! `/profile compare:@member`: one compact message with the taste match between two profiles, the number of shared
//! items and menus that open each shared item as a new message (with the universal buttons). Private profiles can
//! not be compared (neither yours nor theirs), and profiles the viewer may not see are refused as always.

use super::util::{begin_public, pool, uid};
use crate::{
    components,
    db::profile::{self as pdb, Privacy},
    error::{Context, Error},
    i18n,
    ui::embeds::{fit, Emb},
};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Menus show at most this many shared items each.
const MENU_MAX: usize = 25;

/// One item of a profile: `(category, key)` where the key is a work key or a character id (as text).
pub type Item = (&'static str, String);

fn category_of_kind(kind: &str) -> &'static str {
    match kind {
        "anime" => "anime",
        "manga" | "manhwa" | "manhua" => "manga",
        "novel" => "novel",
        "tmdb-tv" => "tv",
        "tmdb-movie" => "movie",
        "game" | "vn" => "game",
        _ => "other",
    }
}

/// The Arabic name of a category, for the result line.
pub fn category_label(c: &str) -> &'static str {
    match c {
        "anime" => "أنمي",
        "manga" => "مانجا",
        "novel" => "روايات",
        "tv" => "دراما ومسلسلات",
        "movie" => "أفلام",
        "game" => "ألعاب",
        "character" => "شخصيات",
        _ => "أخرى",
    }
}

#[derive(Debug, PartialEq)]
pub struct Match {
    /// 0–100.
    pub percent: u32,
    pub shared: usize,
    /// Shared items per category.
    pub by_category: BTreeMap<&'static str, usize>,
    /// The shared items themselves.
    pub items: Vec<Item>,
}

/// Taste match: the Jaccard similarity of each category (works by kind, and characters), weighted by the size of the
/// category's union so a big shared collection counts more than a tiny one. Two empty profiles match 0%.
pub fn taste_match(a: &[Item], b: &[Item]) -> Match {
    let set = |v: &[Item]| v.iter().cloned().collect::<BTreeSet<Item>>();
    let (a, b) = (set(a), set(b));
    let mut cats: BTreeSet<&'static str> = BTreeSet::new();
    cats.extend(a.iter().chain(b.iter()).map(|(c, _)| *c));
    let (mut weighted, mut total) = (0.0f64, 0.0f64);
    let mut by_category = BTreeMap::new();
    let mut items = Vec::new();
    for c in cats {
        let ca: BTreeSet<&Item> = a.iter().filter(|i| i.0 == c).collect();
        let cb: BTreeSet<&Item> = b.iter().filter(|i| i.0 == c).collect();
        let inter: Vec<&&Item> = ca.intersection(&cb).collect();
        let union = ca.union(&cb).count();
        if union == 0 {
            continue;
        }
        weighted += union as f64 * (inter.len() as f64 / union as f64);
        total += union as f64;
        if !inter.is_empty() {
            by_category.insert(c, inter.len());
            items.extend(inter.into_iter().map(|i| (*i).clone()));
        }
    }
    let percent = if total > 0.0 {
        (weighted / total * 100.0).round() as u32
    } else {
        0
    };
    Match {
        percent,
        shared: items.len(),
        by_category,
        items,
    }
}

/// All items of a member (works and characters).
pub async fn items_of(pool: &SqlitePool, user: i64) -> Result<Vec<Item>, sqlx::Error> {
    let mut out: Vec<Item> = Vec::new();
    let works: Vec<(String, String)> =
        sqlx::query_as("SELECT work_id, kind FROM profile_works WHERE user_id = ?1")
            .bind(user)
            .fetch_all(pool)
            .await?;
    out.extend(
        works
            .into_iter()
            .map(|(id, kind)| (category_of_kind(&kind), id)),
    );
    let chars: Vec<i64> =
        sqlx::query_scalar("SELECT character_id FROM profile_characters WHERE user_id = ?1")
            .bind(user)
            .fetch_all(pool)
            .await?;
    out.extend(
        chars
            .into_iter()
            .map(|c| ("character", crate::commands::top::character_key(c))),
    );
    Ok(out)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Yourself,
    NoProfile,
    /// One of the two profiles is private, or the viewer may not see the other.
    Private,
}

/// May `viewer` compare with `target`? Both profiles must exist and neither may be private.
pub async fn comparable(
    pool: &SqlitePool,
    viewer: i64,
    target: i64,
) -> Result<Result<(), Refusal>, sqlx::Error> {
    if viewer == target {
        return Ok(Err(Refusal::Yourself));
    }
    let (Some(mine), Some(theirs)) = (
        pdb::get_profile(pool, viewer).await.map_err(db_err)?,
        pdb::get_profile(pool, target).await.map_err(db_err)?,
    ) else {
        return Ok(Err(Refusal::NoProfile));
    };
    if mine.privacy() == Privacy::Private
        || theirs.privacy() == Privacy::Private
        || !pdb::can_view(pool, viewer, &theirs).await.map_err(db_err)?
    {
        return Ok(Err(Refusal::Private));
    }
    Ok(Ok(()))
}

fn db_err(e: pdb::ProfileError) -> sqlx::Error {
    sqlx::Error::Protocol(e.to_string())
}

/// Titles of the shared works, from the cache the profile already keeps.
async fn titles(pool: &SqlitePool, keys: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for k in keys {
        if let Ok(Some(t)) = sqlx::query_scalar::<_, String>(
            "SELECT title_romaji FROM media_cache WHERE work_id = ?1",
        )
        .bind(k)
        .fetch_optional(pool)
        .await
        {
            out.insert(k.clone(), t);
        }
    }
    out
}

async fn character_titles(pool: &SqlitePool, items: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for key in items {
        // The key is an AniList id, or a provider key; the cache is keyed by the stored integer id.
        let id = key.parse::<i64>().ok();
        if let Some(id) = id {
            if let Ok(Some(n)) = sqlx::query_scalar::<_, String>(
                "SELECT name_romaji FROM character_cache WHERE character_id = ?1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await
            {
                out.insert(key.clone(), n);
            }
        }
    }
    out
}

/// The message: percent, counts per category, and menus for the shared works and characters.
pub fn message(
    name: &str,
    m: &Match,
    work_items: &[(String, String)],
    char_items: &[(String, String)],
) -> (Emb, Vec<serenity::CreateActionRow>) {
    let mut lines = vec![
        format!(
            "🤝 تطابق الذوق مع **{}**: **{}%**",
            fit(name, 60),
            m.percent
        ),
        format!("العناصر المشتركة: **{}**", m.shared),
    ];
    if !m.by_category.is_empty() {
        lines.push(
            m.by_category
                .iter()
                .map(|(c, n)| format!("{} {n}", category_label(c)))
                .collect::<Vec<_>>()
                .join(" · "),
        );
    }
    if m.shared == 0 {
        lines.push("لا يوجد بينكما عنصر مشترك بعد.".into());
    }
    let mut rows = Vec::new();
    rows.extend(components::work_menu(
        i18n::COMPARE_WORKS_PLACEHOLDER,
        work_items,
    ));
    rows.extend(components::character_menu(
        i18n::COMPARE_CHARS_PLACEHOLDER,
        char_items,
    ));
    (
        Emb::new()
            .title("مقارنة البروفايلات")
            .description(lines.join("\n")),
        rows,
    )
}

pub async fn run(ctx: Context<'_>, other: serenity::User) -> Result<(), Error> {
    begin_public(ctx).await?;
    let pool = pool(ctx);
    let (me, them) = (uid(ctx), other.id.get() as i64);
    if let Err(why) = comparable(&pool, me, them).await? {
        let text = match why {
            Refusal::Yourself => i18n::COMPARE_YOURSELF,
            Refusal::NoProfile => i18n::PROFILE_NONE_OTHER,
            Refusal::Private => i18n::COMPARE_PRIVATE,
        };
        return crate::commands::say_ephemeral(ctx, text).await;
    }
    let (a, b) = (items_of(&pool, me).await?, items_of(&pool, them).await?);
    let m = taste_match(&a, &b);
    let work_keys: Vec<String> = m
        .items
        .iter()
        .filter(|(c, _)| *c != "character")
        .map(|(_, k)| k.clone())
        .take(MENU_MAX)
        .collect();
    let char_keys: Vec<String> = m
        .items
        .iter()
        .filter(|(c, _)| *c == "character")
        .map(|(_, k)| k.clone())
        .take(MENU_MAX)
        .collect();
    let (wt, ct) = (
        titles(&pool, &work_keys).await,
        character_titles(&pool, &char_keys).await,
    );
    let work_items: Vec<(String, String)> = work_keys
        .iter()
        .map(|k| {
            (
                fit(wt.get(k).map_or(k.as_str(), String::as_str), 100),
                k.clone(),
            )
        })
        .collect();
    let char_items: Vec<(String, String)> = char_keys
        .iter()
        .map(|k| {
            (
                fit(ct.get(k).map_or(k.as_str(), String::as_str), 100),
                k.clone(),
            )
        })
        .collect();
    let (embed, rows) = message(other.display_name(), &m, &work_items, &char_items);
    let mut reply = poise::CreateReply::default().embed(embed.build());
    if !rows.is_empty() {
        reply = reply.components(rows);
    }
    ctx.send(reply).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(c: &'static str, k: &str) -> Item {
        (c, k.to_string())
    }

    #[test]
    fn identical_profiles_match_fully_and_disjoint_ones_not_at_all() {
        let a = vec![
            w("anime", "anilist:1"),
            w("anime", "anilist:2"),
            w("character", "5"),
        ];
        assert_eq!(taste_match(&a, &a).percent, 100);
        let b = vec![w("anime", "anilist:9"), w("character", "6")];
        let m = taste_match(&a, &b);
        assert_eq!((m.percent, m.shared), (0, 0));
        assert_eq!(taste_match(&[], &[]).percent, 0, "two empty profiles");
    }

    #[test]
    fn the_match_is_weighted_by_category_size_and_counts_shared_items_per_category() {
        // anime: 3 shared of 4 in the union; characters: 0 of 2. Weighted: (4 * 0.75 + 2 * 0) / 6 = 50%.
        let a = vec![
            w("anime", "1"),
            w("anime", "2"),
            w("anime", "3"),
            w("character", "1"),
        ];
        let b = vec![
            w("anime", "1"),
            w("anime", "2"),
            w("anime", "3"),
            w("anime", "4"),
            w("character", "2"),
        ];
        let m = taste_match(&a, &b);
        assert_eq!(m.percent, 50);
        assert_eq!(m.shared, 3);
        assert_eq!(m.by_category.get("anime"), Some(&3));
        assert!(!m.by_category.contains_key("character"));
        // The same key in two categories is two different items.
        assert_eq!(
            taste_match(&[w("anime", "1")], &[w("manga", "1")]).shared,
            0
        );
        // Duplicates inside one profile do not inflate anything.
        assert_eq!(
            taste_match(&[w("anime", "1"), w("anime", "1")], &[w("anime", "1")]).percent,
            100
        );
    }

    #[tokio::test]
    async fn items_come_from_works_and_characters_with_their_categories() {
        let pool = crate::db::connect_memory().await;
        sqlx::query("INSERT INTO profiles (user_id) VALUES (1)")
            .execute(&pool)
            .await
            .expect("p");
        for (id, kind) in [
            ("anilist:1", "anime"),
            ("anilist:2", "manhwa"),
            ("tmdb-tv:3", "tmdb-tv"),
            ("igdb:4", "game"),
        ] {
            sqlx::query("INSERT INTO profile_works (user_id, work_id, kind, position) VALUES (1, ?1, ?2, 1)").bind(id).bind(kind).execute(&pool).await.expect("w");
        }
        sqlx::query(
            "INSERT INTO profile_characters (user_id, character_id, position) VALUES (1, 5, 1)",
        )
        .execute(&pool)
        .await
        .expect("c");
        let items = items_of(&pool, 1).await.expect("items");
        assert!(
            items.contains(&w("manga", "anilist:2"))
                && items.contains(&w("tv", "tmdb-tv:3"))
                && items.contains(&w("game", "igdb:4"))
                && items.contains(&w("character", "5"))
        );
    }

    #[tokio::test]
    async fn private_profiles_and_missing_ones_cannot_be_compared() {
        let pool = crate::db::connect_memory().await;
        for (u, privacy) in [(1, "public"), (2, "public"), (3, "private"), (5, "servers")] {
            sqlx::query("INSERT INTO profiles (user_id, privacy) VALUES (?1, ?2)")
                .bind(u)
                .bind(privacy)
                .execute(&pool)
                .await
                .expect("p");
        }
        assert_eq!(comparable(&pool, 1, 2).await.expect("c"), Ok(()));
        assert_eq!(
            comparable(&pool, 1, 1).await.expect("c"),
            Err(Refusal::Yourself)
        );
        assert_eq!(
            comparable(&pool, 1, 3).await.expect("c"),
            Err(Refusal::Private),
            "their profile is private"
        );
        assert_eq!(
            comparable(&pool, 3, 1).await.expect("c"),
            Err(Refusal::Private),
            "so is mine"
        );
        assert_eq!(
            comparable(&pool, 1, 4).await.expect("c"),
            Err(Refusal::NoProfile)
        );
        assert_eq!(
            comparable(&pool, 1, 5).await.expect("c"),
            Err(Refusal::Private),
            "'servers' needs a shared server"
        );
        sqlx::query("INSERT INTO user_guilds (guild_id, user_id) VALUES (10, 1), (10, 5)")
            .execute(&pool)
            .await
            .expect("g");
        assert_eq!(
            comparable(&pool, 1, 5).await.expect("c"),
            Ok(()),
            "once they share a server"
        );
    }

    #[test]
    fn the_message_gives_the_percent_the_counts_and_menus_that_open_items() {
        let m = taste_match(
            &[w("anime", "anilist:1"), w("character", "5")],
            &[w("anime", "anilist:1"), w("character", "5")],
        );
        let (e, rows) = message(
            "Sara",
            &m,
            &[("Frieren".into(), "anilist:1".into())],
            &[("Frieren".into(), "5".into())],
        );
        let d = e.description.unwrap_or_default();
        assert!(
            d.contains("**Sara**")
                && d.contains("100%")
                && d.contains("المشتركة: **2**")
                && d.contains("أنمي 1"),
            "{d}"
        );
        let json = serde_json::to_string(&rows).expect("json");
        assert!(
            json.contains("pickwork") && json.contains("pickchar") && rows.len() == 2,
            "{json}"
        );
        let (empty, none) = message("Sara", &taste_match(&[], &[]), &[], &[]);
        assert!(
            empty
                .description
                .unwrap_or_default()
                .contains("لا يوجد بينكما عنصر مشترك")
                && none.is_empty()
        );
    }
}
