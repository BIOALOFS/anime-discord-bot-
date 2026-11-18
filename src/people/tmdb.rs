//! TMDB persons (actors, directors, writers…): pure parsers over `search/person`, `person/{id}` with
//! `combined_credits`, and a work's credits.

use super::{age_from, clean_about, role_ar, today, Credit, CrewEntry, Hit, PSource, Person};
use crate::tmdb::model::Kind;
use serde_json::Value;

const IMAGE: &str = "https://image.tmdb.org/t/p/w500";
/// At most this many credits are kept on a card (the most popular first).
const MAX_CREDITS: usize = 60;

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn hint_of(department: Option<&str>) -> String {
    department.map(role_ar).unwrap_or_default()
}

fn hit(v: &Value) -> Option<Hit> {
    let id = v.get("id")?.as_i64()?;
    Some(Hit {
        key: PSource::Tmdb.key(id),
        name: s(v, "name")?,
        native: None,
        hint: hint_of(s(v, "known_for_department").as_deref()),
        popularity: v
            .get("popularity")
            .and_then(Value::as_f64)
            .map_or(0, |p| (p * 100.0) as i64),
    })
}

/// Search results. Persons TMDB flags as adult are never listed.
pub fn parse_hits(doc: &Value) -> Vec<Hit> {
    doc.get("results")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|p| !p.get("adult").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(hit)
                .collect()
        })
        .unwrap_or_default()
}

fn credit_blocked(c: &Value) -> bool {
    if c.get("adult").and_then(Value::as_bool).unwrap_or(false) {
        return true;
    }
    let title = s(c, "title").or_else(|| s(c, "name")).unwrap_or_default();
    crate::anilist::safety::contains_explicit_term(&title)
}

fn kind_of(c: &Value) -> Option<Kind> {
    match c.get("media_type").and_then(Value::as_str)? {
        "tv" => Some(Kind::Tv),
        "movie" => Some(Kind::Movie),
        _ => None,
    }
}

fn work_key(kind: Kind, id: i64) -> String {
    match kind {
        Kind::Tv => format!("tmdb-tv:{id}"),
        Kind::Movie => format!("tmdb-movie:{id}"),
    }
}

fn year(c: &Value) -> Option<i32> {
    s(c, "release_date")
        .or_else(|| s(c, "first_air_date"))
        .and_then(|d| d.get(..4).and_then(|y| y.parse().ok()))
}

/// The card of a person from `person/{id}?append_to_response=combined_credits`. `None` when the person is flagged
/// adult, or when they have credits and every one of them is blocked.
pub fn parse_person(doc: &Value) -> Option<Person> {
    let id = doc.get("id")?.as_i64()?;
    if doc.get("adult").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let name = s(doc, "name")?;
    let mut ranked: Vec<(f64, Credit)> = Vec::new();
    let mut raw = 0;
    let mut seen = std::collections::HashSet::new();
    for (list, is_cast) in [("cast", true), ("crew", false)] {
        for c in doc
            .pointer(&format!("/combined_credits/{list}"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (Some(kind), Some(wid)) = (kind_of(c), c.get("id").and_then(Value::as_i64)) else {
                continue;
            };
            raw += 1;
            if credit_blocked(c) {
                continue;
            }
            let Some(title) = s(c, "title").or_else(|| s(c, "name")) else {
                continue;
            };
            let (role, character) = if is_cast {
                let ch = s(c, "character");
                (
                    None,
                    ch.map(|n| (crate::tmdb::model::cast_key(kind, wid, id), n)),
                )
            } else {
                (s(c, "job").map(|j| role_ar(&j)), None)
            };
            let credit = Credit {
                work_key: Some(work_key(kind, wid)),
                title,
                year: year(c),
                role,
                character,
            };
            if seen.insert((
                credit.work_key.clone(),
                credit.role.clone(),
                credit.character.as_ref().map(|c| c.1.clone()),
            )) {
                let weight = c.get("popularity").and_then(Value::as_f64).unwrap_or(0.0);
                ranked.push((weight, credit));
            }
        }
    }
    if raw > 0 && ranked.is_empty() {
        return None;
    }
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let credits: Vec<Credit> = ranked
        .into_iter()
        .take(MAX_CREDITS)
        .map(|(_, c)| c)
        .collect();
    let birth = s(doc, "birthday");
    let deceased = s(doc, "deathday").is_some();
    let department = s(doc, "known_for_department");
    Some(Person {
        key: PSource::Tmdb.key(id),
        name,
        native: None,
        roles: department.as_deref().map(role_ar).into_iter().collect(),
        age: if deceased {
            None
        } else {
            birth.as_deref().and_then(|b| age_from(b, today()))
        },
        birth,
        deceased,
        popularity: doc
            .get("popularity")
            .and_then(Value::as_f64)
            .map(|p| (p * 100.0) as i64),
        image: s(doc, "profile_path").map(|p| format!("{IMAGE}{p}")),
        about: s(doc, "biography").and_then(|b| clean_about(&b, 400)),
        url: Some(format!("https://www.themoviedb.org/person/{id}")),
        place: s(doc, "place_of_birth").map(|p| short_place(&p)),
        credits,
    })
}

/// "Pittsburgh, Pennsylvania, USA" → "Pittsburgh, USA" (first and last part).
pub fn short_place(p: &str) -> String {
    let parts: Vec<&str> = p
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .collect();
    match parts.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [first, .., last] => format!("{first}, {last}"),
    }
}

/// Cast and the key crew jobs of a work, from `credits` (movies) or `aggregate_credits` (series).
pub fn parse_crew(doc: &Value) -> Vec<CrewEntry> {
    const JOBS: &[&str] = &[
        "Director",
        "Creator",
        "Writer",
        "Screenplay",
        "Novel",
        "Story",
        "Producer",
        "Executive Producer",
        "Original Music Composer",
        "Composer",
        "Director of Photography",
    ];
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in doc
        .get("cast")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(15)
    {
        let Some(h) = hit(c) else { continue };
        let ch = s(c, "character").or_else(|| {
            c.pointer("/roles/0/character")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        let role = match ch.filter(|x| !x.is_empty()) {
            Some(ch) => format!("ممثل · {ch}"),
            None => "ممثل".to_string(),
        };
        if seen.insert(format!("{}|{role}", h.key)) {
            out.push(CrewEntry { person: h, role });
        }
    }
    for c in doc
        .get("crew")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(h) = hit(c) else { continue };
        let job = s(c, "job").or_else(|| {
            c.pointer("/jobs/0/job")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        let Some(job) = job.filter(|j| JOBS.contains(&j.as_str())) else {
            continue;
        };
        let role = role_ar(&job);
        if seen.insert(format!("{}|{role}", h.key)) {
            out.push(CrewEntry { person: h, role });
        }
        if out.len() >= 40 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn person_doc() -> Value {
        json!({"id": 17419, "name": "Bryan Cranston", "adult": false, "known_for_department": "Acting",
            "birthday": "1956-03-07", "deathday": null, "place_of_birth": "Hollywood, Los Angeles, California, USA",
            "popularity": 42.5, "profile_path": "/abc.jpg", "biography": "An American actor.\n\nKnown for Breaking Bad.",
            "combined_credits": {
                "cast": [
                    {"id": 1396, "media_type": "tv", "name": "Breaking Bad", "character": "Walter White", "first_air_date": "2008-01-20", "popularity": 300.0, "adult": false},
                    {"id": 5, "media_type": "movie", "title": "Quiet Film", "character": "Dad", "release_date": "2015-05-01", "popularity": 10.0, "adult": false},
                    {"id": 666, "media_type": "movie", "title": "Adult Thing", "character": "X", "release_date": "2000-01-01", "popularity": 900.0, "adult": true},
                    {"id": 667, "media_type": "movie", "title": "Hardcore Porn Show", "character": "Y", "release_date": "2001-01-01", "popularity": 800.0, "adult": false}],
                "crew": [
                    {"id": 1396, "media_type": "tv", "name": "Breaking Bad", "job": "Director", "first_air_date": "2008-01-20", "popularity": 300.0, "adult": false}]}})
    }

    #[test]
    fn a_person_card_has_department_age_place_image_and_ranked_safe_credits() {
        let p = parse_person(&person_doc()).expect("person");
        assert_eq!(p.key, "tmdb-person:17419");
        assert_eq!(p.roles, vec!["ممثل".to_string()]);
        assert_eq!(p.birth.as_deref(), Some("1956-03-07"));
        assert!(p.age.is_some_and(|a| a >= 69));
        assert_eq!(p.place.as_deref(), Some("Hollywood, USA"));
        assert_eq!(
            p.image.as_deref(),
            Some("https://image.tmdb.org/t/p/w500/abc.jpg")
        );
        assert_eq!(p.popularity, Some(4250));
        let titles: Vec<&str> = p.credits.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Breaking Bad", "Breaking Bad", "Quiet Film"],
            "adult and explicit credits are dropped; popular first"
        );
        assert_eq!(
            p.credits[0].character.as_ref().map(|c| c.1.as_str()),
            Some("Walter White")
        );
        assert!(p.credits[0]
            .character
            .as_ref()
            .is_some_and(|c| c.0.starts_with("tmdb-cast:tv:1396:17419")));
        assert_eq!(p.credits[1].role.as_deref(), Some("مخرج"));
        assert_eq!(p.credits[0].work_key.as_deref(), Some("tmdb-tv:1396"));
    }

    #[test]
    fn adult_people_and_people_with_only_blocked_credits_are_hidden() {
        let mut d = person_doc();
        d["adult"] = json!(true);
        assert!(parse_person(&d).is_none());
        let mut d = person_doc();
        d["combined_credits"] = json!({"cast": [{"id": 1, "media_type": "movie", "title": "X", "adult": true}], "crew": []});
        assert!(parse_person(&d).is_none());
        let mut d = person_doc();
        d["combined_credits"] = json!({"cast": [], "crew": []});
        assert!(
            parse_person(&d).is_some(),
            "no credits at all: nothing to hide"
        );
    }

    #[test]
    fn search_hits_skip_adult_people_and_hint_the_department() {
        let doc = json!({"results": [
            {"id": 1, "name": "Real Actor", "known_for_department": "Acting", "popularity": 5.0, "adult": false},
            {"id": 2, "name": "Hidden", "known_for_department": "Acting", "popularity": 50.0, "adult": true},
            {"id": 3, "name": "Some Director", "known_for_department": "Directing", "popularity": 1.0}]});
        let h = parse_hits(&doc);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].label(), "👤 Real Actor · ممثل");
        assert_eq!(h[1].hint, "مخرج");
    }

    #[test]
    fn a_work_crew_lists_cast_then_key_jobs() {
        let doc = json!({
            "cast": [{"id": 1, "name": "Actor", "character": "Hero", "popularity": 3.0, "known_for_department": "Acting"},
                     {"id": 2, "name": "Series Actor", "roles": [{"character": "Villain"}], "popularity": 2.0}],
            "crew": [{"id": 3, "name": "Boss", "job": "Director", "known_for_department": "Directing"},
                     {"id": 4, "name": "Grip", "job": "Best Boy"}]});
        let c = parse_crew(&doc);
        assert_eq!(
            c.iter().map(|e| e.role.as_str()).collect::<Vec<_>>(),
            vec!["ممثل · Hero", "ممثل · Villain", "مخرج"]
        );
    }

    #[test]
    fn places_are_shortened() {
        assert_eq!(short_place("Paris"), "Paris");
        assert_eq!(short_place("A, B, C, D"), "A, D");
        assert_eq!(short_place(" "), "");
    }
}
