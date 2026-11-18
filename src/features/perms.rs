//! Role based permissions for a few actions. With no role chosen for an action everyone may do it (the default);
//! members with "Manage Server" always may. Chosen in `/admin`, enforced in the handlers.

use crate::{error::Context, guild};
use sqlx::SqlitePool;

/// `(key, Arabic label)` of the actions a server can restrict.
pub const ACTIONS: &[(&str, &str)] = &[
    ("upload", "رفع البنرات والصور المخصصة"),
    ("suggest", "إضافة اقتراحات"),
    ("event", "بدء الفعاليات"),
    ("edit", "تعديل البروفايل"),
];

pub fn label(action: &str) -> &'static str {
    ACTIONS
        .iter()
        .find(|(k, _)| *k == action)
        .map_or("", |(_, l)| *l)
}

/// May a member do an action? `allowed` are the roles the server chose (empty = everyone).
pub fn permits(allowed: &[i64], member_roles: &[i64], is_admin: bool) -> bool {
    is_admin || allowed.is_empty() || member_roles.iter().any(|r| allowed.contains(r))
}

pub async fn allowed_roles(
    pool: &SqlitePool,
    guild: i64,
    action: &str,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT role_id FROM role_permissions WHERE guild_id = ?1 AND action = ?2 ORDER BY role_id",
    )
    .bind(guild)
    .bind(action)
    .fetch_all(pool)
    .await
}

/// Replace the roles allowed for an action (an empty list means everyone again). At most 20 roles.
pub async fn set_roles(
    pool: &SqlitePool,
    guild: i64,
    action: &str,
    roles: &[i64],
) -> Result<(), sqlx::Error> {
    if !ACTIONS.iter().any(|(k, _)| *k == action) {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM role_permissions WHERE guild_id = ?1 AND action = ?2")
        .bind(guild)
        .bind(action)
        .execute(&mut *tx)
        .await?;
    for r in roles.iter().take(20) {
        sqlx::query("INSERT OR IGNORE INTO role_permissions (guild_id, action, role_id) VALUES (?1, ?2, ?3)").bind(guild).bind(action).bind(r).execute(&mut *tx).await?;
    }
    tx.commit().await
}

/// `Some(message)` when this member may not do the action (the caller answers privately in the way that fits how the
/// command already responded); `None` when allowed.
pub async fn denied(ctx: Context<'_>, action: &str) -> Option<String> {
    let g = ctx.guild_id()?;
    let member = ctx.author_member().await?;
    let roles: Vec<i64> = member.roles.iter().map(|r| r.get() as i64).collect();
    let allowed = allowed_roles(&ctx.data().db, g.get() as i64, action)
        .await
        .unwrap_or_default();
    (!permits(&allowed, &roles, guild::is_admin(member.permissions)))
        .then(|| crate::i18n::permission_denied(label(action), &allowed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everyone_may_by_default_and_admins_always_may() {
        assert!(permits(&[], &[], false), "no role chosen: everyone");
        assert!(permits(&[5], &[], true), "Manage Server always");
        assert!(!permits(&[5], &[], false));
        assert!(!permits(&[5], &[6, 7], false));
        assert!(
            permits(&[5, 8], &[6, 8], false),
            "one matching role is enough"
        );
    }

    #[tokio::test]
    async fn roles_are_stored_per_server_and_action_and_replaced_as_a_set() {
        let pool = crate::db::connect_memory().await;
        assert!(allowed_roles(&pool, 1, "upload")
            .await
            .expect("r")
            .is_empty());
        set_roles(&pool, 1, "upload", &[30, 10, 10])
            .await
            .expect("set");
        set_roles(&pool, 1, "suggest", &[11]).await.expect("set");
        set_roles(&pool, 2, "upload", &[99]).await.expect("set");
        assert_eq!(
            allowed_roles(&pool, 1, "upload").await.expect("r"),
            vec![10, 30],
            "deduplicated and sorted"
        );
        assert_eq!(
            allowed_roles(&pool, 1, "suggest").await.expect("r"),
            vec![11]
        );
        assert_eq!(
            allowed_roles(&pool, 2, "upload").await.expect("r"),
            vec![99],
            "another server is separate"
        );
        set_roles(&pool, 1, "upload", &[40]).await.expect("replace");
        assert_eq!(
            allowed_roles(&pool, 1, "upload").await.expect("r"),
            vec![40]
        );
        set_roles(&pool, 1, "upload", &[]).await.expect("clear");
        assert!(
            allowed_roles(&pool, 1, "upload")
                .await
                .expect("r")
                .is_empty(),
            "empty means everyone again"
        );
        set_roles(&pool, 1, "nonsense", &[1])
            .await
            .expect("ignored");
        assert!(allowed_roles(&pool, 1, "nonsense")
            .await
            .expect("r")
            .is_empty());
    }

    #[test]
    fn every_action_has_a_label() {
        assert!(ACTIONS.iter().all(|(k, l)| !l.is_empty() && label(k) == *l));
        assert_eq!(label("x"), "");
    }
}
