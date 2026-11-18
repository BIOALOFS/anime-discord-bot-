//! Optional daily posts in the notifications channel, off by default and set per server in `/admin`: character
//! birthdays (only characters that live in a member's profile there), "شخصية اليوم" and "عمل اليوم" (popular, safe, not
//! repeated for 30 days). Each is claimed per server and day, so a restart never posts twice.

use super::settings;
use crate::{
    anilist::{kind_of, models::Character, ContentPolicy, MediaKind},
    components::{self, ItemKind},
    db::feed as fdb,
    feed::Feed,
    ui::{cards, embeds::Emb},
};
use chrono::{Datelike, Timelike};
use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

pub const BIRTHDAYS: &str = "birthdays";
pub const FEATURED_CHARACTER: &str = "featured_character";
pub const FEATURED_WORK: &str = "featured_work";
/// Featured picks are not repeated for this many days.
pub const NO_REPEAT_DAYS: i64 = 30;

/// Has the server's chosen hour been reached today (server time zone)?
pub fn hour_reached(now: i64, tz: chrono_tz::Tz, hour: i64) -> bool {
    chrono::DateTime::from_timestamp(now, 0)
        .map(|d| d.with_timezone(&tz))
        .is_some_and(|d| i64::from(d.hour()) >= hour)
}

/// The server's local date, `YYYY-MM-DD`.
pub fn local_day(now: i64, tz: chrono_tz::Tz) -> String {
    chrono::DateTime::from_timestamp(now, 0)
        .map(|d| d.with_timezone(&tz))
        .map_or_else(String::new, |d| {
            format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
        })
}

/// The day `NO_REPEAT_DAYS` ago.
pub fn no_repeat_since(today: &str) -> String {
    chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .map(|d| {
            (d - chrono::Duration::days(NO_REPEAT_DAYS))
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_default()
}

/// Birthday characters that are in at least one member's profile of the server (nobody else's would be noise).
pub fn birthday_matches<'a>(
    birthdays: &'a [Character],
    in_profiles: &HashMap<i64, Vec<i64>>,
) -> Vec<&'a Character> {
    birthdays
        .iter()
        .filter(|c| in_profiles.contains_key(&c.id))
        .collect()
}

/// Which members of a server have which of these characters: `character -> [members]` (public profiles only).
pub async fn members_with(
    pool: &SqlitePool,
    guild: i64,
    characters: &[i64],
) -> Result<HashMap<i64, Vec<i64>>, sqlx::Error> {
    let mut out: HashMap<i64, Vec<i64>> = HashMap::new();
    if characters.is_empty() {
        return Ok(out);
    }
    let marks = (0..characters.len())
        .map(|i| format!("?{}", i + 2))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT pc.character_id, pc.user_id FROM profile_characters pc \
         JOIN profiles p ON p.user_id = pc.user_id AND p.privacy != 'private' \
         WHERE pc.user_id IN (SELECT user_id FROM user_guilds WHERE guild_id = ?1) AND pc.character_id IN ({marks}) ORDER BY pc.user_id"
    );
    let mut q = sqlx::query_as::<_, (i64, i64)>(&sql).bind(guild);
    for c in characters {
        q = q.bind(c);
    }
    for (c, u) in q.fetch_all(pool).await? {
        out.entry(c).or_default().push(u);
    }
    Ok(out)
}

fn character_rows(c: &Character) -> Vec<serenity::CreateActionRow> {
    let work = c
        .media
        .as_ref()
        .and_then(|m| m.nodes.first())
        .and_then(|n| n.id)
        .map(|id| format!("anilist:{id}"));
    let mut row = components::standard_actions(ItemKind::Character, work.as_deref());
    row.push(components::add_button(&format!("anilist-char:{}", c.id)));
    vec![serenity::CreateActionRow::Buttons(row)]
}

fn birthday_embed(c: &Character, owners: &[i64]) -> Emb {
    let mut e = cards::character_card(c);
    let who = owners
        .iter()
        .take(5)
        .map(|u| format!("<@{u}>"))
        .collect::<Vec<_>>()
        .join("، ");
    let line = format!(
        "🎂 اليوم عيد ميلاد **{}**! هذه الشخصية في بروفايل: {who}",
        c.display_name()
    );
    e.description = Some(match e.description.take() {
        Some(d) => format!("{line}\n\n{d}"),
        None => line,
    });
    e
}

async fn send(
    f: &Feed,
    channel: i64,
    content: Option<String>,
    embed: Emb,
    rows: Vec<serenity::CreateActionRow>,
) {
    let mut msg = serenity::CreateMessage::new().embed(embed.build());
    if let Some(c) = content {
        msg = msg
            .content(c)
            .allowed_mentions(serenity::CreateAllowedMentions::new());
    }
    if !rows.is_empty() {
        msg = msg.components(rows);
    }
    match serenity::ChannelId::new(channel as u64)
        .send_message(&f.http, msg)
        .await
    {
        Ok(sent) => crate::nav::register_notification(&f.pool, &sent).await,
        Err(e) => tracing::debug!("could not post a daily message: {e}"),
    }
}

async fn guild_tz(f: &Feed, guild: i64) -> chrono_tz::Tz {
    crate::db::admin::timezone(&f.pool, guild)
        .await
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(chrono_tz::Asia::Muscat)
}

/// The notifications channel of a server, when set and on.
async fn channel(f: &Feed, guild: i64) -> Option<i64> {
    let cfg = fdb::get_config(&f.pool, guild).await.ok()?;
    cfg.enabled.then_some(cfg.channel_id).flatten()
}

async fn birthdays(f: &Feed, guild: i64, ch: i64, today: &str) -> anyhow::Result<()> {
    let mut all = Vec::new();
    for page in 1..=5 {
        let (list, more) = f.api.birthdays_page(page, ContentPolicy::SAFE).await?;
        all.extend(list);
        if !more {
            break;
        }
    }
    let ids: Vec<i64> = all.iter().map(|c| c.id).collect();
    let owners = members_with(&f.pool, guild, &ids).await?;
    for c in birthday_matches(&all, &owners) {
        if settings::claim(
            &f.pool,
            guild,
            "birthday",
            &format!("anilist-char:{}", c.id),
            today,
        )
        .await?
        {
            send(
                f,
                ch,
                None,
                birthday_embed(c, &owners[&c.id]),
                character_rows(c),
            )
            .await;
        }
    }
    Ok(())
}

async fn featured_character(
    f: &Feed,
    guild: i64,
    ch: i64,
    today: &str,
    now: i64,
) -> anyhow::Result<()> {
    let recent: HashSet<String> = settings::posted_since(
        &f.pool,
        guild,
        "featured-character",
        &no_repeat_since(today),
    )
    .await?
    .into_iter()
    .collect();
    for attempt in 0..8u64 {
        let seed = (now as u64)
            .wrapping_add(guild as u64)
            .wrapping_mul(31)
            .wrapping_add(attempt * 7919);
        let Some(c) = f.api.daily_character(seed, ContentPolicy::SAFE).await? else {
            continue;
        };
        if recent.contains(&c.id.to_string()) {
            continue;
        }
        if settings::claim(
            &f.pool,
            guild,
            "featured-character",
            &c.id.to_string(),
            today,
        )
        .await?
        {
            let mut e = cards::character_card(&c);
            e.description = Some(match e.description.take() {
                Some(d) => format!("⭐ شخصية اليوم\n\n{d}"),
                None => "⭐ شخصية اليوم".into(),
            });
            send(f, ch, None, e, character_rows(&c)).await;
        }
        return Ok(());
    }
    Ok(())
}

async fn featured_work(f: &Feed, guild: i64, ch: i64, today: &str) -> anyhow::Result<()> {
    let recent: HashSet<String> =
        settings::posted_since(&f.pool, guild, "featured-work", &no_repeat_since(today))
            .await?
            .into_iter()
            .collect();
    for _ in 0..8 {
        let kind = [MediaKind::Anime, MediaKind::Manga][rand::random_range(0..2)];
        let Some(m) = f
            .api
            .random_media(kind, None, None, ContentPolicy::SAFE)
            .await?
        else {
            continue;
        };
        if m.popularity.unwrap_or(0) < 2_000 || recent.contains(&m.id.to_string()) {
            continue;
        }
        if settings::claim(&f.pool, guild, "featured-work", &m.id.to_string(), today).await? {
            let mut e = cards::work_card(&m);
            e.description = Some(match e.description.take() {
                Some(d) => format!("🌟 عمل اليوم\n\n{d}"),
                None => "🌟 عمل اليوم".into(),
            });
            let key = format!("anilist:{}", m.id);
            let mut row = components::standard_actions(ItemKind::Work, Some(&key));
            row.push(components::add_button(&key));
            if kind_of(&m) != MediaKind::Novel {
                row.push(components::remind_button(&key));
            }
            send(
                f,
                ch,
                None,
                e,
                vec![serenity::CreateActionRow::Buttons(row)],
            )
            .await;
        }
        return Ok(());
    }
    Ok(())
}

/// One pass over the servers that switched a daily post on.
pub async fn tick(f: &Feed, now: i64) -> anyhow::Result<()> {
    for (feature, kind) in [(BIRTHDAYS, 0), (FEATURED_CHARACTER, 1), (FEATURED_WORK, 2)] {
        for (guild, cfg) in settings::enabled_guilds(&f.pool, feature).await? {
            let tz = guild_tz(f, guild).await;
            if !hour_reached(now, tz, cfg.hour) {
                continue;
            }
            let Some(ch) = channel(f, guild).await else {
                continue;
            };
            let today = local_day(now, tz);
            // The whole day's post of this kind is claimed first: a failure later is not retried until tomorrow.
            if !settings::claim(&f.pool, guild, &format!("run-{feature}"), "day", &today).await? {
                continue;
            }
            let r = match kind {
                0 => birthdays(f, guild, ch, &today).await,
                1 => featured_character(f, guild, ch, &today, now).await,
                _ => featured_work(f, guild, ch, &today).await,
            };
            if let Err(e) = r {
                tracing::warn!(guild, feature, "daily post failed: {e:#}");
            }
        }
    }
    settings::prune_posts(&f.pool, &no_repeat_since(&local_day(now, chrono_tz::UTC))).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn character(id: i64) -> Character {
        serde_json::from_value(serde_json::json!({"id": id, "name": {"full": format!("C{id}")}}))
            .expect("character")
    }

    #[test]
    fn the_hour_and_the_day_follow_the_servers_zone() {
        let tz = chrono_tz::Asia::Muscat;
        let nine = tz
            .with_ymd_and_hms(2026, 11, 17, 9, 0, 0)
            .single()
            .expect("t")
            .timestamp();
        assert!(hour_reached(nine, tz, 9) && !hour_reached(nine - 60, tz, 9));
        assert!(
            hour_reached(nine + 3600, tz, 9),
            "later the same day still counts"
        );
        assert_eq!(local_day(nine, tz), "2026-11-17");
        // 21:30 UTC on the 17th is already the 18th in Muscat.
        let late = chrono::Utc
            .with_ymd_and_hms(2026, 11, 17, 21, 30, 0)
            .single()
            .expect("t")
            .timestamp();
        assert_eq!(local_day(late, tz), "2026-11-18");
        assert_eq!(no_repeat_since("2026-11-17"), "2026-10-18");
    }

    #[test]
    fn only_birthday_characters_that_a_member_has_are_posted() {
        let all = vec![character(1), character(2), character(3)];
        let owners = HashMap::from([(2, vec![10]), (3, vec![11, 12])]);
        let ids: Vec<i64> = birthday_matches(&all, &owners)
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec![2, 3]);
        assert!(
            birthday_matches(&all, &HashMap::new()).is_empty(),
            "nobody has them: no post"
        );
    }

    #[tokio::test]
    async fn members_are_found_per_character_for_this_server_and_public_profiles_only() {
        let pool = crate::db::connect_memory().await;
        for (u, privacy) in [(1, "public"), (2, "private"), (3, "public"), (4, "public")] {
            sqlx::query("INSERT INTO profiles (user_id, privacy) VALUES (?1, ?2)")
                .bind(u)
                .bind(privacy)
                .execute(&pool)
                .await
                .expect("p");
        }
        for (g, u) in [(10, 1), (10, 2), (10, 3), (11, 4)] {
            sqlx::query("INSERT INTO user_guilds (guild_id, user_id) VALUES (?1, ?2)")
                .bind(g)
                .bind(u)
                .execute(&pool)
                .await
                .expect("g");
        }
        for (u, c) in [(1, 5), (2, 5), (3, 5), (3, 6), (4, 5)] {
            sqlx::query("INSERT INTO profile_characters (user_id, character_id, position) VALUES (?1, ?2, 1)").bind(u).bind(c).execute(&pool).await.expect("c");
        }
        let m = members_with(&pool, 10, &[5, 6, 7]).await.expect("m");
        assert_eq!(
            m.get(&5),
            Some(&vec![1, 3]),
            "the private profile and the other server's member are excluded"
        );
        assert_eq!(m.get(&6), Some(&vec![3]));
        assert!(!m.contains_key(&7));
        assert!(members_with(&pool, 10, &[]).await.expect("m").is_empty());
    }

    #[tokio::test]
    async fn a_pick_is_remembered_for_thirty_days_and_forgotten_after() {
        let pool = crate::db::connect_memory().await;
        assert!(
            settings::claim(&pool, 1, "featured-work", "77", "2026-11-01")
                .await
                .expect("c")
        );
        let recent: HashSet<String> =
            settings::posted_since(&pool, 1, "featured-work", &no_repeat_since("2026-11-17"))
                .await
                .expect("p")
                .into_iter()
                .collect();
        assert!(recent.contains("77"), "16 days ago is inside the window");
        let later: HashSet<String> =
            settings::posted_since(&pool, 1, "featured-work", &no_repeat_since("2026-12-05"))
                .await
                .expect("p")
                .into_iter()
                .collect();
        assert!(!later.contains("77"), "35 days ago is outside it");
    }

    #[test]
    fn the_birthday_message_names_the_character_and_its_owners_and_has_the_universal_button() {
        let e = birthday_embed(&character(9), &[10, 11]);
        let d = e.description.unwrap_or_default();
        assert!(
            d.contains("🎂") && d.contains("C9") && d.contains("<@10>") && d.contains("<@11>"),
            "{d}"
        );
        let mut c = character(9);
        c.media = serde_json::from_value(
            serde_json::json!({"nodes": [{"id": 42, "title": {"romaji": "W"}}]}),
        )
        .ok();
        let json = serde_json::to_string(&character_rows(&c)).expect("json");
        assert!(
            json.contains("showwork:anilist:42") && json.contains("أظهر العمل"),
            "{json}"
        );
    }
}
