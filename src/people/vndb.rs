//! VNDB staff (writers, artists, composers, voice actors): request bodies and pure parsers.

use super::{clean_about, role_ar, Credit, CrewEntry, Hit, PSource, Person};
use crate::providers::Work;
use serde_json::{json, Value};

pub fn search_body(query: &str) -> Value {
    json!({"filters": ["search", "=", query], "fields": "id,aid,ismain,name,original,lang", "results": 20})
}

pub fn staff_body(id: &str) -> Value {
    json!({"filters": ["id", "=", id], "fields": "id,aid,ismain,name,original,lang,gender,description", "results": 1})
}

/// The novels a staff member worked on, with the fields the content verdict needs.
pub fn works_body(id: &str) -> Value {
    json!({"filters": ["staff", "=", ["id", "=", id]], "fields": crate::providers::vndb::VN_FIELDS, "sort": "votecount", "reverse": true, "results": 40})
}

/// Staff and voice actors of one novel.
pub fn crew_body(vn: &str) -> Value {
    json!({"filters": ["id", "=", vn], "fields": "id,staff.id,staff.name,staff.original,staff.role,va.staff.id,va.staff.name,va.staff.original,va.character.id,va.character.name,va.character.original", "results": 1})
}

/// The novels a character appears in (first step of finding who voices them).
pub fn character_vns_body(character: &str) -> Value {
    json!({"filters": ["id", "=", character], "fields": "id,vns.id", "results": 1})
}

fn s(v: &Value, p: &str) -> Option<String> {
    v.pointer(p)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn hit_of(v: &Value) -> Option<Hit> {
    let id = v.get("id")?.as_str()?;
    super::parse_key(&PSource::Vndb.key(id))?;
    Some(Hit {
        key: PSource::Vndb.key(id),
        name: s(v, "/name")?,
        native: s(v, "/original"),
        hint: "طاقم".into(),
        popularity: 0,
    })
}

/// Search results: VNDB lists every alias of a person, so duplicates by id are folded (the main alias wins).
pub fn parse_hits(doc: &Value) -> Vec<Hit> {
    let mut out: Vec<(bool, Hit)> = Vec::new();
    for v in doc
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(h) = hit_of(v) else { continue };
        let main = v.get("ismain").and_then(Value::as_bool).unwrap_or(false);
        match out.iter().position(|(_, o)| o.key == h.key) {
            Some(i) if main && !out[i].0 => out[i] = (true, h),
            Some(_) => {}
            None => out.push((main, h)),
        }
    }
    out.into_iter().map(|(_, h)| h).collect()
}

/// VNDB descriptions use bbcode-like tags (`[url=…]text[/url]`, `[spoiler]`); they are removed, the text stays.
pub fn strip_bbcode(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        match rest[i..].find(']') {
            Some(j) if j <= 200 => rest = &rest[i + j + 1..],
            _ => {
                out.push('[');
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The card from a staff document and the document of the novels they worked on (already judged by the shared
/// verdict through `works`). Hidden when they have novels and none passes.
pub fn parse_person(staff: &Value, raw_works: usize, works: &[Work]) -> Option<Person> {
    let v = staff.pointer("/results/0")?;
    let h = hit_of(v)?;
    if raw_works > 0 && works.is_empty() {
        return None;
    }
    let credits = works
        .iter()
        .map(|w| Credit {
            work_key: Some(w.id.clone()),
            title: w.title.clone(),
            year: w.year,
            role: None,
            character: None,
        })
        .collect();
    let id = v.get("id").and_then(Value::as_str).unwrap_or_default();
    Some(Person {
        key: h.key,
        name: h.name,
        native: h.native,
        roles: vec!["طاقم".into()],
        birth: None,
        age: None,
        deceased: false,
        popularity: None,
        image: None,
        about: s(v, "/description").and_then(|d| clean_about(&strip_bbcode(&d), 400)),
        url: Some(format!("https://vndb.org/{id}")),
        place: s(v, "/lang").map(|l| lang_name(&l)),
        credits,
    })
}

fn lang_name(code: &str) -> String {
    match code {
        "ja" => "اليابان",
        "en" => "إنجليزي",
        "zh" | "zh-Hans" | "zh-Hant" => "الصين",
        "ko" => "كوريا",
        other => other,
    }
    .to_string()
}

/// Voice actors (with their characters) and staff (with their jobs) from `crew_body`.
pub fn parse_crew(doc: &Value) -> Vec<CrewEntry> {
    let Some(v) = doc.pointer("/results/0") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for va in v.get("va").and_then(Value::as_array).into_iter().flatten() {
        let Some(h) = va.get("staff").and_then(hit_of) else {
            continue;
        };
        let ch = s(va, "/character/name");
        let role = match ch {
            Some(c) => format!("مؤدي أصوات · {c}"),
            None => "مؤدي أصوات".into(),
        };
        if seen.insert(format!("{}|{role}", h.key)) {
            out.push(CrewEntry {
                person: Hit {
                    hint: "مؤدي أصوات".into(),
                    ..h
                },
                role,
            });
        }
    }
    for st in v
        .get("staff")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(h) = hit_of(st) else { continue };
        let raw_role = s(st, "/role");
        let role = role_ar(match raw_role.as_deref() {
            Some("chardesign") => "character design",
            Some("director") => "director",
            Some("scenario") => "scenario",
            Some("music") => "music",
            Some("art") => "art",
            Some("songs") => "songs",
            Some("translator") => "مترجم",
            Some("editor") => "محرر",
            Some("qa") => "اختبار الجودة",
            Some(other) => other,
            None => "staff",
        });
        if seen.insert(format!("{}|{role}", h.key)) {
            out.push(CrewEntry {
                person: Hit {
                    hint: role.clone(),
                    ..h
                },
                role,
            });
        }
    }
    out
}

/// The novel ids a character appears in.
pub fn character_vns(doc: &Value) -> Vec<String> {
    doc.pointer("/results/0/vns")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.get("id").and_then(Value::as_str))
                .take(3)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Who voices `character` (`c123`) according to a novel's crew document.
pub fn parse_performers(doc: &Value, character: &str) -> Vec<Hit> {
    let mut out: Vec<Hit> = Vec::new();
    for va in doc
        .pointer("/results/0/va")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if va.pointer("/character/id").and_then(Value::as_str) != Some(character) {
            continue;
        }
        if let Some(h) = va.get("staff").and_then(hit_of) {
            if !out.iter().any(|o| o.key == h.key) {
                out.push(Hit {
                    hint: "مؤدي أصوات".into(),
                    ..h
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_use_only_documented_staff_fields() {
        for (b, what) in [
            (search_body("x"), "search"),
            (staff_body("s1"), "staff"),
            (works_body("s1"), "works"),
            (crew_body("v1"), "crew"),
            (character_vns_body("c1"), "chars"),
        ] {
            assert!(
                b.get("filters").is_some() && b.get("fields").is_some(),
                "{what}"
            );
        }
        assert_eq!(
            works_body("s9")["filters"],
            json!(["staff", "=", ["id", "=", "s9"]])
        );
        assert!(!works_body("s9")["fields"]
            .as_str()
            .unwrap_or("")
            .contains("minage"));
    }

    #[test]
    fn aliases_fold_into_one_hit_and_the_main_alias_wins() {
        let doc = json!({"results": [
            {"id": "s5", "aid": 7, "ismain": false, "name": "Alias Name", "original": null, "lang": "ja"},
            {"id": "s5", "aid": 5, "ismain": true, "name": "Main Name", "original": "本名", "lang": "ja"},
            {"id": "s6", "aid": 9, "ismain": true, "name": "Other", "original": null, "lang": "en"}]});
        let h = parse_hits(&doc);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].name, "Main Name");
        assert_eq!(h[0].native.as_deref(), Some("本名"));
        assert_eq!(h[0].key, "vndb-staff:s5");
    }

    #[test]
    fn a_staff_card_strips_markup_and_lists_only_allowed_novels() {
        let staff = json!({"results": [{"id": "s5", "name": "Writer", "original": "作家", "lang": "ja", "description": "Writes [url=https://x]great[/url] stories. [spoiler]Hi[/spoiler]"}]});
        let w = |id: &str, t: &str| Work {
            id: format!("vndb:{id}"),
            source: crate::providers::Source::Vndb,
            kind: crate::providers::Kind::VisualNovel,
            title: t.into(),
            native: None,
            subtitle: None,
            year: Some(2004),
            status: None,
            rating: None,
            url: None,
            cover_url: None,
            genres: vec![],
            work_key: None,
        };
        let p = parse_person(&staff, 2, &[w("v1", "Clean VN")]).expect("person");
        assert_eq!(p.about.as_deref(), Some("Writes great stories. Hi"));
        assert_eq!(p.url.as_deref(), Some("https://vndb.org/s5"));
        assert_eq!(p.credits.len(), 1);
        assert_eq!(p.credits[0].work_key.as_deref(), Some("vndb:v1"));
        assert!(
            parse_person(&staff, 2, &[]).is_none(),
            "every novel was blocked: hidden"
        );
        assert!(
            parse_person(&staff, 0, &[]).is_some(),
            "no novels at all: nothing to hide"
        );
    }

    #[test]
    fn a_novel_crew_lists_voice_actors_and_jobs_and_performers_match_the_character() {
        let doc = json!({"results": [{"id": "v1",
            "va": [{"staff": {"id": "s10", "name": "VA One", "original": null}, "character": {"id": "c7", "name": "Alice", "original": null}},
                   {"staff": {"id": "s11", "name": "VA Two", "original": null}, "character": {"id": "c8", "name": "Bob", "original": null}}],
            "staff": [{"id": "s20", "name": "Composer", "original": null, "role": "music"}, {"id": "s21", "name": "Artist", "original": null, "role": "chardesign"}]}]});
        let c = parse_crew(&doc);
        assert_eq!(
            c.iter().map(|e| e.role.as_str()).collect::<Vec<_>>(),
            vec![
                "مؤدي أصوات · Alice",
                "مؤدي أصوات · Bob",
                "موسيقى",
                "تصميم الشخصيات"
            ]
        );
        let p = parse_performers(&doc, "c7");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "VA One");
        assert!(parse_performers(&doc, "c99").is_empty());
        assert_eq!(
            character_vns(&json!({"results": [{"vns": [{"id": "v1"}, {"id": "v2"}]}]})),
            vec!["v1", "v2"]
        );
    }

    #[test]
    fn bbcode_stripping_keeps_text_and_stray_brackets() {
        assert_eq!(strip_bbcode("a [b]bold[/b] c"), "a bold c");
        assert_eq!(strip_bbcode("no close [ here"), "no close [ here");
    }
}
