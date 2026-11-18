//! IGDB companies (developers and publishers, labelled "شركة"): request bodies and pure parsers.

use super::{clean_about, Credit, CrewEntry, Hit, PSource, Person};
use crate::providers::Work;
use serde_json::Value;

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace(['\\', '"'], ""))
}

pub fn search_body(query: &str) -> String {
    format!(
        "search {}; fields name,country,description; limit 15;",
        quote(query)
    )
}

/// A company with the games it developed and published, expanded with the fields the content verdict needs.
pub fn company_body(id: i64) -> String {
    let nested = |prefix: &str| {
        crate::providers::igdb::GAME_FIELDS
            .split(',')
            .map(|f| format!("{prefix}.{f}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "fields name,country,description,url,start_date,logo.image_id,{},{}; where id = {id}; limit 1;",
        nested("developed"),
        nested("published")
    )
}

/// The companies involved in one game.
pub fn crew_body(game: i64) -> String {
    format!("fields involved_companies.company.id,involved_companies.company.name,involved_companies.developer,involved_companies.publisher,involved_companies.porting,involved_companies.supporting; where id = {game}; limit 1;")
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn hit_of(v: &Value) -> Option<Hit> {
    let id = v.get("id")?.as_i64()?;
    Some(Hit {
        key: PSource::Igdb.key(id),
        name: s(v, "name")?,
        native: None,
        hint: "شركة".into(),
        popularity: 0,
    })
}

pub fn parse_hits(doc: &Value) -> Vec<Hit> {
    doc.as_array()
        .map(|a| a.iter().filter_map(hit_of).collect())
        .unwrap_or_default()
}

/// ISO 3166-1 numeric code → Arabic country name (the ones that matter for games).
pub fn country_name(code: i64) -> Option<&'static str> {
    Some(match code {
        392 => "اليابان",
        840 => "الولايات المتحدة",
        826 => "المملكة المتحدة",
        250 => "فرنسا",
        276 => "ألمانيا",
        124 => "كندا",
        410 => "كوريا الجنوبية",
        156 => "الصين",
        752 => "السويد",
        380 => "إيطاليا",
        724 => "إسبانيا",
        643 => "روسيا",
        528 => "هولندا",
        616 => "بولندا",
        208 => "الدنمارك",
        246 => "فنلندا",
        _ => return None,
    })
}

/// The company card. `games` is the combined list of developed and published games as IGDB returned them.
/// Hidden when it has games and none passes the content verdict.
pub fn parse_company(doc: &Value, mature: bool) -> Option<Person> {
    let v = doc.as_array()?.first()?;
    let h = hit_of(v)?;
    let mut raw = 0;
    let mut credits: Vec<Credit> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (list, role) in [("developed", "تطوير"), ("published", "نشر")] {
        let games: Vec<Value> = v
            .get(list)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        // Nested ids without fields (IGDB sends plain numbers when a field was not expanded) carry no verdict.
        let games: Vec<Value> = games.into_iter().filter(|g| g.is_object()).collect();
        raw += games.len();
        let mut works: Vec<(Work, u64)> =
            crate::providers::igdb::parse_games(&Value::Array(games), mature);
        works.sort_by_key(|(_, votes)| std::cmp::Reverse(*votes));
        for (w, _) in works {
            if seen.insert((w.id.clone(), role)) {
                credits.push(Credit {
                    work_key: Some(w.id),
                    title: w.title,
                    year: w.year,
                    role: Some(role.into()),
                    character: None,
                });
            }
        }
    }
    if raw > 0 && credits.is_empty() {
        return None;
    }
    let logo = v
        .pointer("/logo/image_id")
        .and_then(Value::as_str)
        .map(|i| format!("https://images.igdb.com/igdb/image/upload/t_logo_med/{i}.png"));
    let started = v
        .get("start_date")
        .and_then(Value::as_i64)
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|d| d.format("%Y-%m-%d").to_string());
    Some(Person {
        key: h.key,
        name: h.name,
        native: None,
        roles: vec!["شركة".into()],
        birth: started,
        age: None,
        deceased: false,
        popularity: None,
        image: logo,
        about: s(v, "description").and_then(|d| clean_about(&d, 400)),
        url: s(v, "url").filter(|u| u.starts_with("https://www.igdb.com/")),
        place: v
            .get("country")
            .and_then(Value::as_i64)
            .and_then(country_name)
            .map(str::to_string),
        credits,
    })
}

/// The developer / publisher companies of a game.
pub fn parse_crew(doc: &Value) -> Vec<CrewEntry> {
    let mut out = Vec::new();
    let Some(game) = doc.as_array().and_then(|a| a.first()) else {
        return out;
    };
    for ic in game
        .get("involved_companies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(h) = ic.get("company").and_then(hit_of) else {
            continue;
        };
        let flag = |k: &str| ic.get(k).and_then(Value::as_bool).unwrap_or(false);
        let role = if flag("developer") {
            "مطوّر"
        } else if flag("publisher") {
            "ناشر"
        } else if flag("porting") {
            "نقل اللعبة"
        } else if flag("supporting") {
            "دعم"
        } else {
            "شركة"
        };
        out.push(CrewEntry {
            person: h,
            role: role.into(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn game(id: i64, name: &str, themes: Value) -> Value {
        json!({"id": id, "name": name, "first_release_date": 1_000_000_000, "total_rating_count": 100 + id,
            "themes": themes, "genres": [{"name": "Adventure"}], "keywords": [{"name": "survival"}], "url": "https://www.igdb.com/games/x"})
    }

    #[test]
    fn bodies_ask_for_the_verdict_fields_and_escape_quotes() {
        let b = company_body(7);
        assert!(
            b.contains("developed.themes.id")
                && b.contains("published.keywords.name")
                && b.contains("where id = 7")
        );
        assert!(search_body("a\"b").contains("search \"ab\""));
        assert!(crew_body(3).contains("involved_companies.company.name"));
    }

    #[test]
    fn a_company_card_has_country_description_logo_and_only_allowed_games() {
        let doc = json!([{"id": 70, "name": "Konami", "country": 392, "description": "A Japanese company.", "start_date": 123_456_789,
            "url": "https://www.igdb.com/companies/konami", "logo": {"image_id": "abc"},
            "developed": [game(1, "Silent Hill 2", json!([{"id": 19, "name": "Horror"}])), game(2, "Erotic One", json!([{"id": 42, "name": "Erotic"}]))],
            "published": [game(3, "Other Game", json!([]))]}]);
        let p = parse_company(&doc, true).expect("company");
        assert_eq!(p.name, "Konami");
        assert_eq!(p.roles, vec!["شركة".to_string()]);
        assert_eq!(p.place.as_deref(), Some("اليابان"));
        assert_eq!(
            p.image.as_deref(),
            Some("https://images.igdb.com/igdb/image/upload/t_logo_med/abc.png")
        );
        let titles: Vec<&str> = p.credits.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Silent Hill 2", "Other Game"],
            "the erotic game is never listed"
        );
        assert_eq!(p.credits[0].role.as_deref(), Some("تطوير"));
        assert_eq!(p.credits[1].role.as_deref(), Some("نشر"));
        // mature-themes off also hides the horror game
        let q = parse_company(&doc, false).expect("company");
        assert_eq!(q.credits.len(), 1);
    }

    #[test]
    fn a_company_whose_games_are_all_blocked_is_hidden() {
        let doc = json!([{"id": 1, "name": "Adult Studio", "developed": [game(9, "Bad", json!([{"id": 42, "name": "Erotic"}]))], "published": []}]);
        assert!(parse_company(&doc, true).is_none());
        let none = json!([{"id": 2, "name": "No Games Yet", "developed": [], "published": []}]);
        assert!(parse_company(&none, true).is_some());
    }

    #[test]
    fn hits_are_labelled_company_and_crew_names_the_job() {
        let h = parse_hits(&json!([{"id": 5, "name": "Capcom", "country": 392}]));
        assert_eq!(h[0].label(), "👤 Capcom · شركة");
        let crew = parse_crew(&json!([{"id": 1, "involved_companies": [
            {"company": {"id": 5, "name": "Capcom"}, "developer": true, "publisher": false},
            {"company": {"id": 6, "name": "Pub"}, "developer": false, "publisher": true}]}]));
        assert_eq!(
            crew.iter().map(|c| c.role.as_str()).collect::<Vec<_>>(),
            vec!["مطوّر", "ناشر"]
        );
    }
}
