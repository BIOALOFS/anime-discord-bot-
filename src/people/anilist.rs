//! AniList staff (voice actors, directors, authors…): requests and pure parsers.

use super::{
    age_from, clean_about, date_string, role_ar, today, Credit, CrewEntry, Hit, PSource, Person,
};
use serde_json::Value;

/// Search by name; the occupations give the role hint of every result.
pub const STAFF_SEARCH: &str = "query($search: String, $perPage: Int) { Page(perPage: $perPage) { \
    staff(search: $search, sort: [SEARCH_MATCH]) { id name { full native } primaryOccupations favourites } } }";

pub const POPULAR_STAFF: &str = "query($perPage: Int) { Page(perPage: $perPage) { \
    staff(sort: [FAVOURITES_DESC]) { id name { full native } primaryOccupations favourites } } }";

/// One person with the characters they voiced and the works they staffed. Lists are kept short to stay within
/// AniList's query-complexity limit.
pub const STAFF_BY_ID: &str = "query($id: Int) { Staff(id: $id) { \
    id name { full native } primaryOccupations age gender homeTown favourites siteUrl description(asHtml: false) \
    image { large } dateOfBirth { year month day } dateOfDeath { year month day } \
    characters(perPage: 20, sort: [FAVOURITES_DESC]) { edges { node { id name { full } } \
      media { id type isAdult genres title { romaji english } startDate { year } } } } \
    staffMedia(perPage: 20, sort: [POPULARITY_DESC]) { edges { staffRole \
      node { id type isAdult genres title { romaji english } startDate { year } } } } } }";

/// The people behind a work: staff, and the Japanese voice actors of its main characters.
pub const MEDIA_CREW: &str = "query($id: Int) { Media(id: $id) { id isAdult genres \
    staff(perPage: 25, sort: [RELEVANCE]) { edges { role node { id name { full native } primaryOccupations favourites } } } \
    characters(perPage: 12, sort: [ROLE, RELEVANCE]) { edges { node { id name { full } } \
      voiceActors(language: JAPANESE) { id name { full native } primaryOccupations favourites } } } } }";

/// Who voices a character (Japanese first, then English).
pub const CHARACTER_PERFORMERS: &str = "query($id: Int) { Character(id: $id) { id \
    media(perPage: 4, sort: [POPULARITY_DESC]) { edges { \
      jp: voiceActors(language: JAPANESE) { id name { full native } primaryOccupations favourites } \
      en: voiceActors(language: ENGLISH) { id name { full native } primaryOccupations favourites } } } } }";

fn s(v: &Value, p: &str) -> Option<String> {
    v.pointer(p)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn occupations(v: &Value) -> Vec<String> {
    v.get("primaryOccupations")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(role_ar).collect())
        .unwrap_or_default()
}

/// One staff node as a list entry.
pub fn hit(node: &Value) -> Option<Hit> {
    let id = node.get("id")?.as_i64()?;
    let name = s(node, "/name/full").or_else(|| s(node, "/name/native"))?;
    Some(Hit {
        key: PSource::AniList.key(id),
        name,
        native: s(node, "/name/native"),
        hint: occupations(node)
            .into_iter()
            .take(2)
            .collect::<Vec<_>>()
            .join(" · "),
        popularity: node.get("favourites").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// The hits of a search document (`/Page/staff`).
pub fn parse_hits(doc: &Value) -> Vec<Hit> {
    doc.pointer("/Page/staff")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(hit).collect())
        .unwrap_or_default()
}

/// A media node that must not be listed: adult flag or an adult genre.
fn media_blocked(m: &Value) -> bool {
    m.get("isAdult").and_then(Value::as_bool).unwrap_or(false)
        || m.get("genres").and_then(Value::as_array).is_some_and(|g| {
            let genres: Vec<String> = g
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            crate::anilist::safety::genres_adult(&genres)
        })
}

fn title_of(m: &Value) -> Option<String> {
    s(m, "/title/romaji").or_else(|| s(m, "/title/english"))
}

fn year_of(m: &Value) -> Option<i32> {
    m.pointer("/startDate/year")
        .and_then(Value::as_i64)
        .map(|y| y as i32)
}

/// The card of a person from a `STAFF_BY_ID` document. `None` when the person has works and every one of them is
/// blocked by the safety rules (such a person is hidden), or the document is empty.
pub fn parse_person(doc: &Value) -> Option<Person> {
    let st = doc.get("Staff").filter(|v| !v.is_null())?;
    let h = hit(st)?;
    let mut credits: Vec<Credit> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut raw = 0;
    for e in st
        .pointer("/characters/edges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let ch = e.get("node");
        let ch_id = ch.and_then(|c| c.get("id")).and_then(Value::as_i64);
        let ch_name = ch.and_then(|c| s(c, "/name/full"));
        for m in e
            .get("media")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            raw += 1;
            if media_blocked(m) {
                continue;
            }
            let (Some(id), Some(title)) = (m.get("id").and_then(Value::as_i64), title_of(m)) else {
                continue;
            };
            let character = ch_id.zip(ch_name.clone()).map(|(i, n)| (i.to_string(), n));
            if seen.insert((id, character.clone())) {
                credits.push(Credit {
                    work_key: Some(format!("anilist:{id}")),
                    title,
                    year: year_of(m),
                    role: None,
                    character,
                });
            }
        }
    }
    for e in st
        .pointer("/staffMedia/edges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(m) = e.get("node") else { continue };
        raw += 1;
        if media_blocked(m) {
            continue;
        }
        let (Some(id), Some(title)) = (m.get("id").and_then(Value::as_i64), title_of(m)) else {
            continue;
        };
        let role = s(e, "/staffRole").map(|r| role_ar(r.split('(').next().unwrap_or(&r).trim()));
        if seen.insert((id, role.clone().map(|r| (r, String::new())))) {
            credits.push(Credit {
                work_key: Some(format!("anilist:{id}")),
                title,
                year: year_of(m),
                role,
                character: None,
            });
        }
    }
    if raw > 0 && credits.is_empty() {
        return None;
    }
    let birth = date_string(
        st.pointer("/dateOfBirth/year").and_then(Value::as_i64),
        st.pointer("/dateOfBirth/month").and_then(Value::as_i64),
        st.pointer("/dateOfBirth/day").and_then(Value::as_i64),
    );
    let deceased = st
        .pointer("/dateOfDeath/year")
        .and_then(Value::as_i64)
        .is_some();
    let age = if deceased {
        None
    } else {
        st.get("age")
            .and_then(Value::as_i64)
            .map(|a| a as i32)
            .or_else(|| birth.as_deref().and_then(|b| age_from(b, today())))
    };
    Some(Person {
        key: h.key.clone(),
        name: h.name,
        native: h.native,
        roles: occupations(st),
        birth,
        age,
        deceased,
        popularity: st.get("favourites").and_then(Value::as_i64),
        image: s(st, "/image/large").filter(|u| u.starts_with("https://s4.anilist.co/")),
        about: s(st, "/description").and_then(|d| clean_about(&d, 400)),
        url: s(st, "/siteUrl"),
        place: s(st, "/homeTown"),
        credits,
    })
}

/// The crew of a work: voice actors of its main characters first, then the staff.
pub fn parse_crew(doc: &Value) -> Vec<CrewEntry> {
    let Some(m) = doc.get("Media").filter(|v| !v.is_null()) else {
        return Vec::new();
    };
    if media_blocked(m) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for e in m
        .pointer("/characters/edges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let character = e
            .pointer("/node/name/full")
            .and_then(Value::as_str)
            .unwrap_or("");
        for va in e
            .get("voiceActors")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(1)
        {
            if let Some(h) = hit(va) {
                if seen.insert(format!("{}|{character}", h.key)) {
                    let role = if character.is_empty() {
                        "مؤدي أصوات".to_string()
                    } else {
                        format!("مؤدي أصوات · {character}")
                    };
                    out.push(CrewEntry { person: h, role });
                }
            }
        }
    }
    for e in m
        .pointer("/staff/edges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(h), role) = (e.get("node").and_then(hit), s(e, "/role")) {
            let role = role
                .map(|r| role_ar(r.split('(').next().unwrap_or(&r).trim()))
                .unwrap_or_else(|| "طاقم".into());
            if seen.insert(format!("{}|{role}", h.key)) {
                out.push(CrewEntry { person: h, role });
            }
        }
    }
    out
}

/// The voice actors of a character, Japanese first.
pub fn parse_performers(doc: &Value) -> Vec<Hit> {
    let mut out: Vec<Hit> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for lang in ["jp", "en"] {
        for e in doc
            .pointer("/Character/media/edges")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for va in e.get(lang).and_then(Value::as_array).into_iter().flatten() {
                if let Some(h) = hit(va) {
                    if seen.insert(h.key.clone()) {
                        out.push(h);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn staff_doc() -> Value {
        json!({"Staff": {
            "id": 95, "name": {"full": "Hiroshi Kamiya", "native": "神谷浩史"},
            "primaryOccupations": ["Voice Actor"], "age": 51, "homeTown": "Chiba, Japan",
            "favourites": 12000, "siteUrl": "https://anilist.co/staff/95",
            "description": "<b>Hiroshi Kamiya</b> is a __Japanese__ voice actor.\n\nBorn in Chiba.",
            "image": {"large": "https://s4.anilist.co/file/staff/95.png"},
            "dateOfBirth": {"year": 1975, "month": 1, "day": 28}, "dateOfDeath": {"year": null, "month": null, "day": null},
            "characters": {"edges": [
                {"node": {"id": 100, "name": {"full": "Levi"}}, "media": [
                    {"id": 16498, "type": "ANIME", "isAdult": false, "genres": ["Action"], "title": {"romaji": "Shingeki no Kyojin", "english": "Attack on Titan"}, "startDate": {"year": 2013}},
                    {"id": 999, "type": "ANIME", "isAdult": true, "genres": ["Hentai"], "title": {"romaji": "Blocked", "english": null}, "startDate": {"year": 2010}}]},
                {"node": {"id": 200, "name": {"full": "Ringo"}}, "media": [
                    {"id": 777, "type": "ANIME", "isAdult": false, "genres": ["Ecchi", "Hentai"], "title": {"romaji": "Genre blocked", "english": null}, "startDate": {"year": 2012}}]}]},
            "staffMedia": {"edges": [
                {"staffRole": "Director (ep 5)", "node": {"id": 11061, "type": "ANIME", "isAdult": false, "genres": ["Drama"], "title": {"romaji": "Hunter x Hunter", "english": null}, "startDate": {"year": 2011}}}]}}})
    }

    #[test]
    fn a_staff_card_has_roles_dates_age_and_only_safe_works() {
        let p = parse_person(&staff_doc()).expect("person");
        assert_eq!(p.key, "anilist-staff:95");
        assert_eq!(p.name, "Hiroshi Kamiya");
        assert_eq!(p.native.as_deref(), Some("神谷浩史"));
        assert_eq!(p.roles, vec!["مؤدي أصوات".to_string()]);
        assert_eq!(p.birth.as_deref(), Some("1975-01-28"));
        assert_eq!(p.age, Some(51));
        assert!(!p.deceased);
        assert_eq!(p.popularity, Some(12000));
        assert_eq!(p.place.as_deref(), Some("Chiba, Japan"));
        assert!(p
            .about
            .as_deref()
            .is_some_and(|a| a.starts_with("Hiroshi Kamiya is a Japanese")));
        let titles: Vec<&str> = p.credits.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Shingeki no Kyojin", "Hunter x Hunter"],
            "adult and Hentai works never appear"
        );
        assert_eq!(p.credits[0].character, Some(("100".into(), "Levi".into())));
        assert_eq!(p.credits[1].role.as_deref(), Some("مخرج"));
        assert_eq!(p.credits[0].work_key.as_deref(), Some("anilist:16498"));
    }

    #[test]
    fn a_person_whose_only_works_are_blocked_is_hidden() {
        let mut d = staff_doc();
        d["Staff"]["characters"]["edges"] = json!([{"node": {"id": 1, "name": {"full": "X"}}, "media": [{"id": 5, "isAdult": true, "genres": [], "title": {"romaji": "A"}}]}]);
        d["Staff"]["staffMedia"]["edges"] = json!([]);
        assert!(parse_person(&d).is_none());
        // a person with no works at all is shown (nothing to hide)
        d["Staff"]["characters"]["edges"] = json!([]);
        assert!(parse_person(&d).is_some());
        assert!(parse_person(&json!({"Staff": null})).is_none());
    }

    #[test]
    fn a_deceased_person_has_no_age() {
        let mut d = staff_doc();
        d["Staff"]["dateOfDeath"] = json!({"year": 2020, "month": 1, "day": 1});
        let p = parse_person(&d).expect("person");
        assert!(p.deceased && p.age.is_none());
    }

    #[test]
    fn search_hits_carry_the_role_hint_and_popularity() {
        let doc = json!({"Page": {"staff": [
            {"id": 95, "name": {"full": "Hiroshi Kamiya", "native": "神谷浩史"}, "primaryOccupations": ["Voice Actor", "Singer"], "favourites": 5},
            {"id": 96, "name": {"full": "No Job", "native": null}, "primaryOccupations": [], "favourites": null}]}});
        let h = parse_hits(&doc);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].label(), "👤 Hiroshi Kamiya · مؤدي أصوات · Singer");
        assert_eq!(h[1].label(), "👤 No Job");
    }

    #[test]
    fn a_work_crew_lists_voice_actors_then_staff() {
        let doc = json!({"Media": {"id": 1, "isAdult": false, "genres": ["Drama"],
            "staff": {"edges": [{"role": "Director", "node": {"id": 7, "name": {"full": "A Director", "native": null}, "primaryOccupations": ["Director"], "favourites": 3}}]},
            "characters": {"edges": [{"node": {"id": 9, "name": {"full": "Okabe"}}, "voiceActors": [{"id": 8, "name": {"full": "Mamoru Miyano", "native": null}, "primaryOccupations": ["Voice Actor"], "favourites": 9}]}]}}});
        let c = parse_crew(&doc);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].role, "مؤدي أصوات · Okabe");
        assert_eq!(c[1].role, "مخرج");
        let mut blocked = doc.clone();
        blocked["Media"]["isAdult"] = json!(true);
        assert!(
            parse_crew(&blocked).is_empty(),
            "the crew of a blocked work is never listed"
        );
    }

    #[test]
    fn performers_put_japanese_first() {
        let doc = json!({"Character": {"id": 1, "media": {"edges": [{
            "jp": [{"id": 1, "name": {"full": "JP Actor", "native": null}, "primaryOccupations": [], "favourites": 1}],
            "en": [{"id": 2, "name": {"full": "EN Actor", "native": null}, "primaryOccupations": [], "favourites": 1},
                   {"id": 1, "name": {"full": "JP Actor", "native": null}, "primaryOccupations": [], "favourites": 1}]}]}}});
        let p = parse_performers(&doc);
        assert_eq!(
            p.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
            vec!["JP Actor", "EN Actor"]
        );
    }
}
