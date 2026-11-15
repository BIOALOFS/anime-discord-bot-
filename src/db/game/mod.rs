//! Game scores (per server), versus votes and hidden recommendations.

use sqlx::{Row, SqlitePool};

pub type Result<T> = std::result::Result<T, sqlx::Error>;

/// Record one finished round and return the new streak.
pub async fn record_result(pool: &SqlitePool, guild: i64, user: i64, correct: bool) -> Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO game_scores (guild_id, user_id) VALUES (?1, ?2)")
        .bind(guild)
        .bind(user)
        .execute(pool)
        .await?;
    if correct {
        sqlx::query(
            "UPDATE game_scores SET correct = correct + 1, played = played + 1, streak = streak + 1, \
             best_streak = MAX(best_streak, streak + 1) WHERE guild_id = ?1 AND user_id = ?2",
        )
        .bind(guild)
        .bind(user)
        .execute(pool)
        .await?;
    } else {
        sqlx::query("UPDATE game_scores SET played = played + 1, streak = 0 WHERE guild_id = ?1 AND user_id = ?2")
            .bind(guild)
            .bind(user)
            .execute(pool)
            .await?;
    }
    sqlx::query_scalar("SELECT streak FROM game_scores WHERE guild_id = ?1 AND user_id = ?2")
        .bind(guild)
        .bind(user)
        .fetch_one(pool)
        .await
}

pub async fn current_streak(pool: &SqlitePool, guild: i64, user: i64) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT streak FROM game_scores WHERE guild_id = ?1 AND user_id = ?2")
            .bind(guild)
            .bind(user)
            .fetch_optional(pool)
            .await?
            .unwrap_or(0),
    )
}

/// `(user_id, correct, played, best_streak)` best first; `guild = None` sums every server.
pub async fn leaderboard(
    pool: &SqlitePool,
    guild: Option<i64>,
    limit: i64,
) -> Result<Vec<(i64, i64, i64, i64)>> {
    let rows = sqlx::query(
        "SELECT user_id, SUM(correct) AS c, SUM(played) AS p, MAX(best_streak) AS b FROM game_scores \
         WHERE (?1 IS NULL OR guild_id = ?1) GROUP BY user_id ORDER BY c DESC, p ASC LIMIT ?2",
    )
    .bind(guild)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get("user_id"), r.get("c"), r.get("p"), r.get("b")))
        .collect())
}

pub async fn total_correct(pool: &SqlitePool, user: i64) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, Option<i64>>(
        "SELECT SUM(correct) FROM game_scores WHERE user_id = ?1",
    )
    .bind(user)
    .fetch_one(pool)
    .await?
    .unwrap_or(0))
}

/// One versus round: the winner gains a win, both gain an appearance.
pub async fn record_versus(pool: &SqlitePool, winner: i64, loser: i64) -> Result<()> {
    for (id, win) in [(winner, 1), (loser, 0)] {
        sqlx::query(
            "INSERT INTO versus_votes (character_id, wins, shown) VALUES (?1, ?2, 1) \
             ON CONFLICT(character_id) DO UPDATE SET wins = wins + ?2, shown = shown + 1",
        )
        .bind(id)
        .bind(win)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// `(wins, shown)` for a character.
pub async fn versus_stats(pool: &SqlitePool, character: i64) -> Result<(i64, i64)> {
    let r = sqlx::query("SELECT wins, shown FROM versus_votes WHERE character_id = ?1")
        .bind(character)
        .fetch_optional(pool)
        .await?;
    Ok(r.map_or((0, 0), |r| (r.get("wins"), r.get("shown"))))
}

pub async fn hide_recommendation(pool: &SqlitePool, user: i64, work: i64) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO hidden_recommendations (user_id, work_id) VALUES (?1, ?2)")
        .bind(user)
        .bind(work)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn hidden_recommendations(pool: &SqlitePool, user: i64) -> Result<Vec<i64>> {
    sqlx::query_scalar("SELECT work_id FROM hidden_recommendations WHERE user_id = ?1")
        .bind(user)
        .fetch_all(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_memory;

    #[tokio::test]
    async fn streaks_and_leaderboards() {
        let pool = connect_memory().await;
        assert_eq!(record_result(&pool, 1, 10, true).await.expect("r"), 1);
        assert_eq!(record_result(&pool, 1, 10, true).await.expect("r"), 2);
        assert_eq!(
            record_result(&pool, 1, 10, false).await.expect("r"),
            0,
            "a miss resets the streak"
        );
        assert_eq!(record_result(&pool, 1, 10, true).await.expect("r"), 1);
        record_result(&pool, 1, 20, true).await.expect("r");
        record_result(&pool, 2, 20, true).await.expect("r");
        record_result(&pool, 2, 20, true).await.expect("r");
        let server1 = leaderboard(&pool, Some(1), 10).await.expect("l");
        assert_eq!(
            server1[0],
            (10, 3, 4, 2),
            "user 10: 3 correct of 4, best streak 2"
        );
        let global = leaderboard(&pool, None, 10).await.expect("l");
        assert_eq!(
            global[0].0, 20,
            "user 20 has 3 correct across servers and fewer plays"
        );
        assert_eq!(total_correct(&pool, 20).await.expect("t"), 3);
        assert_eq!(total_correct(&pool, 99).await.expect("t"), 0);
    }

    #[tokio::test]
    async fn versus_and_hidden_recommendations() {
        let pool = connect_memory().await;
        record_versus(&pool, 1, 2).await.expect("v");
        record_versus(&pool, 1, 3).await.expect("v");
        assert_eq!(versus_stats(&pool, 1).await.expect("s"), (2, 2));
        assert_eq!(versus_stats(&pool, 2).await.expect("s"), (0, 1));
        assert_eq!(versus_stats(&pool, 9).await.expect("s"), (0, 0));
        hide_recommendation(&pool, 5, 100).await.expect("h");
        hide_recommendation(&pool, 5, 100)
            .await
            .expect("idempotent");
        assert_eq!(
            hidden_recommendations(&pool, 5).await.expect("h"),
            vec![100]
        );
    }
}
