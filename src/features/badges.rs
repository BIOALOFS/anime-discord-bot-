//! Achievements: a few badges earned from activity, stored once (`badges`), shown as one short line on the profile
//! and announced with a short ephemeral message when earned. Definitions and thresholds live here in code.

use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Characters and works in the profile.
    Items,
    /// Correct answers in events.
    Wins,
    /// Suggestions written.
    Suggestions,
    /// Characters in the profile.
    Characters,
    /// A custom banner (1 when set).
    CustomBanner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Def {
    pub key: &'static str,
    pub label: &'static str,
    pub metric: Metric,
    pub threshold: i64,
}

/// Every badge. Tiers of one metric (10, 50, 100 items) show only their highest earned level on the profile.
pub const DEFS: &[Def] = &[
    Def {
        key: "items_10",
        label: "🏅 أضفتُ 10 عناصر",
        metric: Metric::Items,
        threshold: 10,
    },
    Def {
        key: "items_50",
        label: "🏅 أضفتُ 50 عنصرًا",
        metric: Metric::Items,
        threshold: 50,
    },
    Def {
        key: "items_100",
        label: "🏆 أضفتُ 100 عنصر",
        metric: Metric::Items,
        threshold: 100,
    },
    Def {
        key: "wins_5",
        label: "🎮 فزتُ بـ 5 فعاليات",
        metric: Metric::Wins,
        threshold: 5,
    },
    Def {
        key: "wins_20",
        label: "🎮 فزتُ بـ 20 فعالية",
        metric: Metric::Wins,
        threshold: 20,
    },
    Def {
        key: "suggest_3",
        label: "💡 نشرتُ 3 اقتراحات",
        metric: Metric::Suggestions,
        threshold: 3,
    },
    Def {
        key: "suggest_10",
        label: "💡 نشرتُ 10 اقتراحات",
        metric: Metric::Suggestions,
        threshold: 10,
    },
    Def {
        key: "first_character",
        label: "⭐ أول شخصية",
        metric: Metric::Characters,
        threshold: 1,
    },
    Def {
        key: "custom_banner",
        label: "🎨 أول بنر مخصص",
        metric: Metric::CustomBanner,
        threshold: 1,
    },
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Metrics {
    pub items: i64,
    pub characters: i64,
    pub wins: i64,
    pub suggestions: i64,
    pub custom_banner: bool,
}

impl Metrics {
    fn value(&self, m: Metric) -> i64 {
        match m {
            Metric::Items => self.items,
            Metric::Wins => self.wins,
            Metric::Suggestions => self.suggestions,
            Metric::Characters => self.characters,
            Metric::CustomBanner => i64::from(self.custom_banner),
        }
    }
}

/// The badges these numbers qualify for.
pub fn qualifying(m: &Metrics) -> Vec<&'static Def> {
    DEFS.iter()
        .filter(|d| m.value(d.metric) >= d.threshold)
        .collect()
}

pub async fn metrics(pool: &SqlitePool, user: i64) -> Result<Metrics, sqlx::Error> {
    let characters: i64 =
        sqlx::query_scalar("SELECT count(*) FROM profile_characters WHERE user_id = ?1")
            .bind(user)
            .fetch_one(pool)
            .await?;
    let works: i64 = sqlx::query_scalar("SELECT count(*) FROM profile_works WHERE user_id = ?1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    let wins = crate::db::game::total_correct(pool, user)
        .await
        .unwrap_or(0);
    let suggestions = crate::db::suggest::count_everywhere(pool, user)
        .await
        .unwrap_or(0);
    let banner: Option<String> =
        sqlx::query_scalar("SELECT banner FROM profiles WHERE user_id = ?1")
            .bind(user)
            .fetch_optional(pool)
            .await?
            .flatten();
    Ok(Metrics {
        items: characters + works,
        characters,
        wins,
        suggestions,
        custom_banner: banner.is_some_and(|b| b.starts_with("image:")),
    })
}

/// Award what the member now qualifies for. Returns only the badges earned just now (each is awarded once).
pub async fn award(pool: &SqlitePool, user: i64) -> Vec<&'static Def> {
    let Ok(m) = metrics(pool, user).await else {
        return Vec::new();
    };
    let mut fresh = Vec::new();
    for d in qualifying(&m) {
        let inserted =
            sqlx::query("INSERT OR IGNORE INTO badges (user_id, badge_key) VALUES (?1, ?2)")
                .bind(user)
                .bind(d.key)
                .execute(pool)
                .await
                .map(|r| r.rows_affected() > 0)
                .unwrap_or(false);
        if inserted {
            fresh.push(d);
        }
    }
    fresh
}

/// The short ephemeral celebration for newly earned badges (empty when there is none).
pub fn celebration(fresh: &[&Def]) -> String {
    match fresh {
        [] => String::new(),
        [one] => format!("🎉 مبروك! حصلتَ على شارة جديدة: {}", one.label),
        many => format!(
            "🎉 مبروك! حصلتَ على {} شارات جديدة: {}",
            many.len(),
            many.iter().map(|d| d.label).collect::<Vec<_>>().join("، ")
        ),
    }
}

/// Award what is now earned and return `text` with the celebration appended (used right after an add).
pub async fn announce(pool: &SqlitePool, user: i64, text: String) -> String {
    let fresh = award(pool, user).await;
    with_celebration(text, &fresh)
}

/// Append the celebration to a reply (a new line) when something was earned.
pub fn with_celebration(text: String, fresh: &[&Def]) -> String {
    let c = celebration(fresh);
    if c.is_empty() {
        text
    } else {
        format!("{text}\n{c}")
    }
}

/// The badges to show: of each metric only the highest earned level, at most four, as one short line.
pub async fn line(pool: &SqlitePool, user: i64) -> Vec<&'static str> {
    let earned: Vec<String> = sqlx::query_scalar("SELECT badge_key FROM badges WHERE user_id = ?1")
        .bind(user)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let mut shown: Vec<&'static Def> = Vec::new();
    for d in DEFS.iter().filter(|d| earned.iter().any(|k| k == d.key)) {
        match shown.iter().position(|s| s.metric == d.metric) {
            Some(i) if shown[i].threshold < d.threshold => shown[i] = d,
            Some(_) => {}
            None => shown.push(d),
        }
    }
    shown.into_iter().take(4).map(|d| d.label).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_decide_which_badges_qualify() {
        assert!(qualifying(&Metrics::default()).is_empty());
        let m = Metrics {
            items: 50,
            characters: 3,
            wins: 5,
            suggestions: 2,
            custom_banner: false,
        };
        let keys: Vec<_> = qualifying(&m).iter().map(|d| d.key).collect();
        assert_eq!(keys, ["items_10", "items_50", "wins_5", "first_character"]);
        let all = Metrics {
            items: 100,
            characters: 1,
            wins: 20,
            suggestions: 10,
            custom_banner: true,
        };
        assert_eq!(qualifying(&all).len(), DEFS.len());
        let mut seen = std::collections::HashSet::new();
        assert!(DEFS.iter().all(|d| seen.insert(d.key)), "unique keys");
    }

    #[tokio::test]
    async fn a_badge_is_awarded_exactly_once_and_celebrated_once() {
        let pool = crate::db::connect_memory().await;
        sqlx::query("INSERT INTO profiles (user_id) VALUES (1)")
            .execute(&pool)
            .await
            .expect("profile");
        assert!(award(&pool, 1).await.is_empty(), "nothing yet");
        sqlx::query(
            "INSERT INTO profile_characters (user_id, character_id, position) VALUES (1, 5, 1)",
        )
        .execute(&pool)
        .await
        .expect("c");
        let first = award(&pool, 1).await;
        assert_eq!(
            first.iter().map(|d| d.key).collect::<Vec<_>>(),
            ["first_character"]
        );
        assert!(celebration(&first).contains("⭐ أول شخصية"));
        assert!(award(&pool, 1).await.is_empty(), "never twice");
        for i in 0..9 {
            sqlx::query("INSERT INTO profile_works (user_id, work_id, kind, position) VALUES (1, ?1, 'anime', ?2)").bind(format!("anilist:{i}")).bind(i).execute(&pool).await.expect("w");
        }
        let next = award(&pool, 1).await;
        assert_eq!(next.iter().map(|d| d.key).collect::<Vec<_>>(), ["items_10"]);
        let other = award(&pool, 2).await;
        assert!(other.is_empty(), "badges are per member");
    }

    #[tokio::test]
    async fn the_profile_line_shows_only_the_highest_level_of_each_kind_and_at_most_four() {
        let pool = crate::db::connect_memory().await;
        for k in [
            "items_10",
            "items_50",
            "wins_5",
            "wins_20",
            "suggest_3",
            "first_character",
            "custom_banner",
        ] {
            sqlx::query("INSERT INTO badges (user_id, badge_key) VALUES (1, ?1)")
                .bind(k)
                .execute(&pool)
                .await
                .expect("badge");
        }
        let line = line(&pool, 1).await;
        assert_eq!(line.len(), 4);
        assert!(line.contains(&"🏅 أضفتُ 50 عنصرًا") && !line.contains(&"🏅 أضفتُ 10 عناصر"));
        assert!(line.contains(&"🎮 فزتُ بـ 20 فعالية") && !line.contains(&"🎮 فزتُ بـ 5 فعاليات"));
        assert!(self::line(&pool, 9).await.is_empty());
    }

    #[test]
    fn the_celebration_is_short_and_arabic() {
        assert_eq!(celebration(&[]), "");
        assert!(celebration(&[&DEFS[0]]).starts_with("🎉 مبروك!"));
        assert!(celebration(&[&DEFS[0], &DEFS[3]]).contains("2 شارات"));
        assert_eq!(with_celebration("تم".into(), &[]), "تم");
        assert!(with_celebration("تم".into(), &[&DEFS[0]]).starts_with("تم\n🎉"));
    }

    #[tokio::test]
    async fn a_custom_banner_counts_only_when_it_is_an_uploaded_image() {
        let pool = crate::db::connect_memory().await;
        sqlx::query(
            "INSERT INTO profiles (user_id, banner) VALUES (1, 'preset:ocean'), (2, 'image:7')",
        )
        .execute(&pool)
        .await
        .expect("p");
        assert!(!metrics(&pool, 1).await.expect("m").custom_banner);
        assert!(metrics(&pool, 2).await.expect("m").custom_banner);
        assert_eq!(
            award(&pool, 2)
                .await
                .iter()
                .map(|d| d.key)
                .collect::<Vec<_>>(),
            ["custom_banner"]
        );
    }
}
