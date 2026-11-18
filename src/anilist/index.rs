//! A local index of the works AniList has answered with (Round 11.3). Autocomplete asks it next to AniList, so a
//! suggestion never depends on one live call: it matches by prefix, by acronym ("orv", "tog", "aot") and through
//! small typos, over every title and synonym. It is a cache, filled by use.

use sqlx::{Row, SqlitePool};

/// One work as the index keeps it.
pub struct Indexed {
    pub id: i64,
    /// `anime`, `manga`, `manhwa`, `manhua` or `novel`.
    pub kind: &'static str,
    pub label: String,
    /// Romaji, English, native and every synonym.
    pub titles: Vec<String>,
    pub popularity: i64,
}

/// A title in its comparable form: lower-case, no accents, apostrophes dropped, other punctuation a space.
pub fn fold(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| !matches!(c, '\'' | '’' | '`' | 'ʼ'))
        .collect();
    crate::wiki::normalize(&cleaned)
}

/// The first letters of the words of a title, articles left out: "Omniscient Reader's Viewpoint" is `orv`,
/// "Tower of God" `tog`, "Attack on Titan" `aot`, "Solo Leveling" `sl`. Empty for a one-word title.
pub fn acronym(title: &str) -> String {
    let folded = fold(title);
    let letters: String = folded
        .split(' ')
        .filter(|w| !w.is_empty() && !matches!(*w, "a" | "an" | "the"))
        .filter_map(|w| w.chars().next())
        .collect();
    if letters.chars().count() >= 2 {
        letters
    } else {
        String::new()
    }
}

/// The index kinds a command kind covers (`/manga` without a format covers every origin).
pub fn kinds_for(kind: super::MediaKind) -> &'static [&'static str] {
    use super::MediaKind;
    match kind {
        MediaKind::Anime => &["anime"],
        MediaKind::Manga => &["manga", "manhwa", "manhua"],
        MediaKind::Manhwa => &["manhwa"],
        MediaKind::Manhua => &["manhua"],
        MediaKind::Novel => &["novel"],
    }
}

/// Remember these works (an existing row is refreshed).
pub async fn record(pool: &SqlitePool, items: &[Indexed]) {
    let now = chrono::Utc::now().timestamp();
    let Ok(mut tx) = pool.begin().await else {
        return;
    };
    for it in items {
        let titles: Vec<String> = it
            .titles
            .iter()
            .map(|t| fold(t))
            .filter(|t| !t.is_empty())
            .collect();
        let mut acros: Vec<String> = it.titles.iter().map(|t| acronym(t)).collect();
        acros.retain(|a| !a.is_empty());
        acros.sort();
        acros.dedup();
        let r = sqlx::query(
            "INSERT INTO media_index (id, kind, label, norm, acro, popularity, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, label = excluded.label, norm = excluded.norm, \
             acro = excluded.acro, popularity = excluded.popularity, updated_at = excluded.updated_at",
        )
        .bind(it.id)
        .bind(it.kind)
        .bind(&it.label)
        .bind(format!("|{}|", titles.join("|")))
        .bind(acros.join(" "))
        .bind(it.popularity)
        .bind(now)
        .execute(&mut *tx)
        .await;
        if let Err(e) = r {
            tracing::debug!("could not index a work: {e}");
        }
    }
    let _ = tx.commit().await;
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Does a query word match a title word: a prefix of it, or within a small typo (one edit from four letters, two from
/// eight), also against the same-length start of a longer word ("omnicient" is close to "omniscient").
fn word_matches(q: &str, w: &str) -> (bool, bool) {
    if w.starts_with(q) {
        return (true, false);
    }
    let n = q.chars().count();
    if n < 4 {
        return (false, false);
    }
    let allowed = if n >= 8 { 2 } else { 1 };
    let head: String = w.chars().take(n).collect();
    let close = distance(q, w) <= allowed || distance(q, &head) <= allowed;
    (close, close)
}

/// How well `q` (folded) matches a row: 0 an acronym or a whole title, 1 a title that starts with it, 2 every word is
/// the start of a title word, 3 the same with small typos; `None` when it does not match.
fn tier(q: &str, norm: &str, acro: &str) -> Option<u8> {
    let compact: String = q.chars().filter(|c| *c != ' ').collect();
    if compact.chars().count() >= 2 && acro.split(' ').any(|a| a == compact) {
        return Some(0);
    }
    let words: Vec<&str> = q.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return None;
    }
    let mut best: Option<u8> = None;
    for title in norm.split('|').filter(|t| !t.is_empty()) {
        let t = if title == q {
            0
        } else if title.starts_with(q) {
            1
        } else {
            let tw: Vec<&str> = title.split(' ').collect();
            let mut typo = false;
            let all = words.iter().all(|qw| {
                tw.iter().any(|w| {
                    let (ok, fuzzy) = word_matches(qw, w);
                    typo |= ok && fuzzy;
                    ok
                })
            });
            if !all {
                continue;
            }
            if typo {
                3
            } else {
                2
            }
        };
        best = Some(best.map_or(t, |b| b.min(t)));
    }
    best
}

/// The indexed works of these kinds that match `q`, best first (then the most popular), at most `limit`:
/// `(id, label)`.
pub async fn lookup(
    pool: &SqlitePool,
    kinds: &[&str],
    q: &str,
    limit: usize,
) -> Vec<(i64, String)> {
    let q = fold(q);
    if q.chars().count() < 2 || kinds.is_empty() {
        return Vec::new();
    }
    // Candidates by the cheap parts of the match: the start of the query anywhere, the acronym, and the first three
    // letters of every word (so that a typo later in a word still finds the row).
    let mut conds = vec!["norm LIKE ?".to_string(), "acro LIKE ?".to_string()];
    let mut binds = vec![format!("%{q}%"), format!("%{}%", q.replace(' ', ""))];
    for w in q.split(' ').filter(|w| w.chars().count() >= 3) {
        conds.push("norm LIKE ?".into());
        binds.push(format!("%{}%", w.chars().take(3).collect::<String>()));
    }
    let marks = vec!["?"; kinds.len()].join(",");
    let sql = format!(
        "SELECT id, label, norm, acro, popularity FROM media_index WHERE kind IN ({marks}) AND ({}) \
         ORDER BY popularity DESC LIMIT 400",
        conds.join(" OR ")
    );
    let mut query = sqlx::query(&sql);
    for k in kinds {
        query = query.bind(*k);
    }
    for b in &binds {
        query = query.bind(b);
    }
    let Ok(rows) = query.fetch_all(pool).await else {
        return Vec::new();
    };
    let mut hits: Vec<(u8, i64, i64, String)> = rows
        .iter()
        .filter_map(|r| {
            let t = tier(&q, &r.get::<String, _>("norm"), &r.get::<String, _>("acro"))?;
            Some((
                t,
                -r.get::<i64, _>("popularity"),
                r.get("id"),
                r.get("label"),
            ))
        })
        .collect();
    hits.sort_by_key(|h| (h.0, h.1));
    hits.into_iter()
        .take(limit)
        .map(|(_, _, id, label)| (id, label))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(id: i64, kind: &'static str, titles: &[&str], pop: i64) -> Indexed {
        Indexed {
            id,
            kind,
            label: format!("{} (x)", titles[0]),
            titles: titles.iter().map(|t| t.to_string()).collect(),
            popularity: pop,
        }
    }

    async fn pool() -> SqlitePool {
        let pool = crate::db::connect_memory().await;
        record(
            &pool,
            &[
                work(
                    105398,
                    "manhwa",
                    &[
                        "Omniscient Reader's Viewpoint",
                        "Jeonjijeok Dokja Sijeom",
                        "ORV",
                    ],
                    50_000,
                ),
                work(
                    2,
                    "manhwa",
                    &["Solo Leveling", "Na Honjaman Rebeleseo", "Only I Level Up"],
                    90_000,
                ),
                work(3, "manhwa", &["Solo Max-Level Newbie"], 8_000),
                work(4, "manhwa", &["Tower of God", "Kami no Tou"], 80_000),
                work(
                    5,
                    "anime",
                    &["Shingeki no Kyojin", "Attack on Titan"],
                    200_000,
                ),
                work(6, "manga", &["One Piece"], 300_000),
                work(7, "manga", &["One Punch-Man", "One Punch Man"], 250_000),
                work(8, "novel", &["Omniscient Reader's Viewpoint"], 10_000),
            ],
        )
        .await;
        pool
    }

    #[test]
    fn acronyms_skip_articles_and_apostrophes() {
        assert_eq!(acronym("Omniscient Reader's Viewpoint"), "orv");
        assert_eq!(acronym("Tower of God"), "tog");
        assert_eq!(acronym("Attack on Titan"), "aot");
        assert_eq!(acronym("Solo Leveling"), "sl");
        assert_eq!(acronym("The Beginning After The End"), "bae");
        assert_eq!(acronym("Naruto"), "", "a one-word title has none");
    }

    #[tokio::test]
    async fn abbreviations_partial_words_and_typos_each_give_a_list_best_match_first() {
        let p = pool().await;
        let manga = super::kinds_for(crate::anilist::MediaKind::Manga);
        let ids = |v: Vec<(i64, String)>| v.into_iter().map(|(i, _)| i).collect::<Vec<_>>();
        // an acronym that AniList's own text search may not match
        assert_eq!(ids(lookup(&p, manga, "orv", 25).await)[0], 105398);
        assert_eq!(ids(lookup(&p, manga, "tog", 25).await), vec![4]);
        assert_eq!(
            ids(lookup(&p, &["anime"], "aot", 25).await),
            vec![5],
            "aot is Attack on Titan"
        );
        // a partial word lists several, the most popular first within the same match
        let solo = ids(lookup(&p, manga, "solo", 25).await);
        assert!(solo.len() >= 2 && solo[0] == 2, "{solo:?}");
        assert!(solo.contains(&3));
        let one = ids(lookup(&p, manga, "one", 25).await);
        assert!(
            one.len() >= 2 && one.contains(&6) && one.contains(&7),
            "{one:?}"
        );
        assert!(!ids(lookup(&p, manga, "omni", 25).await).is_empty());
        // typos, case and punctuation
        assert_eq!(ids(lookup(&p, manga, "solo levling", 25).await)[0], 2);
        assert_eq!(ids(lookup(&p, manga, "OMNICIENT", 25).await)[0], 105398);
        assert_eq!(ids(lookup(&p, manga, "tower  of-god", 25).await)[0], 4);
        // the synonym counts
        assert_eq!(ids(lookup(&p, manga, "only i level", 25).await)[0], 2);
        // the kind is respected, and nothing matches nonsense
        assert!(ids(lookup(&p, &["novel"], "tog", 25).await).is_empty());
        assert!(lookup(&p, manga, "zzzzqq", 25).await.is_empty());
        assert!(lookup(&p, manga, "a", 25).await.is_empty());
    }

    #[test]
    fn small_typos_are_tolerated_only_from_four_letters() {
        assert!(word_matches("levling", "leveling").0);
        assert!(word_matches("omnicient", "omniscient").0);
        assert!(!word_matches("sol", "soup").0, "three letters: prefix only");
        assert!(word_matches("sol", "solo").0);
    }
}
