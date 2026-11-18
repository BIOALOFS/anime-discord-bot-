//! TMDB data model and the parsers of its JSON documents (pure, tested with fixtures).

use serde_json::Value;
use std::collections::HashMap;

pub const IMAGE_BASE: &str = "https://image.tmdb.org/t/p/";

/// Asian drama origins; everything else is a "series" unless the server's admin moved a country.
pub const ASIAN_DRAMA_COUNTRIES: &[&str] = &[
    "KR", "JP", "CN", "TW", "HK", "TH", "VN", "ID", "PH", "MY", "SG",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tv,
    Movie,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Tv => "tmdb-tv",
            Self::Movie => "tmdb-movie",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Drama,
    Series,
}

/// Which command a show belongs to. An admin override for any of the show's countries wins; otherwise a show with
/// an Asian-drama origin is a drama. A show is in exactly one of the two.
pub fn classify(origin: &[String], overrides: &HashMap<String, Target>) -> Target {
    if let Some(t) = origin.iter().find_map(|c| overrides.get(&c.to_uppercase())) {
        return *t;
    }
    if origin
        .iter()
        .any(|c| ASIAN_DRAMA_COUNTRIES.contains(&c.to_uppercase().as_str()))
    {
        Target::Drama
    } else {
        Target::Series
    }
}

pub fn image(path: &str, size: &str) -> String {
    format!("{IMAGE_BASE}{size}{path}")
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Providers {
    pub flatrate: Vec<String>,
    pub rent: Vec<String>,
    pub buy: Vec<String>,
}

impl Providers {
    pub fn is_empty(&self) -> bool {
        self.flatrate.is_empty() && self.rent.is_empty() && self.buy.is_empty()
    }
}

/// `tmdb-cast:{kind}:{work}:{person}`.
pub fn cast_key(kind: Kind, work: i64, person: i64) -> String {
    format!(
        "tmdb-cast:{}:{work}:{person}",
        if kind == Kind::Tv { "tv" } else { "movie" }
    )
}

/// A cast entry: an actor playing a character of one work.
#[derive(Debug, Clone, PartialEq)]
pub struct Cast {
    pub person_id: i64,
    pub actor: String,
    pub character: String,
    pub photo: Option<String>,
    pub popularity: Option<f64>,
}

impl Cast {
    /// `tmdb-cast:{kind}:{work}:{person}`.
    pub fn key(&self, kind: Kind, work: i64) -> String {
        cast_key(kind, work, self.person_id)
    }

    /// The character name, or the actor's when TMDB has none.
    pub fn display_name(&self) -> &str {
        if self.character.trim().is_empty() {
            &self.actor
        } else {
            &self.character
        }
    }
}

/// Parse `tmdb-cast:tv:123:456` into `(kind, work, person)`.
pub fn parse_cast_key(key: &str) -> Option<(Kind, i64, i64)> {
    let mut p = key.strip_prefix("tmdb-cast:")?.split(':');
    let kind = match p.next()? {
        "tv" => Kind::Tv,
        "movie" => Kind::Movie,
        _ => return None,
    };
    Some((kind, p.next()?.parse().ok()?, p.next()?.parse().ok()?))
}

/// Cast entries live in the integer character tables above this base (VNDB uses 1e12, IGDB 2e12).
pub const CAST_ID_BASE: i64 = 3_000_000_000_000;

/// Pack `(kind, work, person)` into one integer: 25 bits of person, 22 bits of work, 1 bit of kind. `None` when an id
/// does not fit (it is then simply not addable to a profile, never mixed up with another entry).
pub fn encode_cast_id(kind: Kind, work: i64, person: i64) -> Option<i64> {
    if !(0..1 << 25).contains(&person) || !(0..1 << 22).contains(&work) {
        return None;
    }
    let k = i64::from(kind == Kind::Movie);
    Some(CAST_ID_BASE + ((k << 47) | (work << 25) | person))
}

pub fn decode_cast_id(id: i64) -> Option<(Kind, i64, i64)> {
    let n = id
        .checked_sub(CAST_ID_BASE)
        .filter(|n| (0..1 << 48).contains(n))?;
    let kind = if n >> 47 == 1 { Kind::Movie } else { Kind::Tv };
    Some((kind, (n >> 25) & ((1 << 22) - 1), n & ((1 << 25) - 1)))
}

/// Integer work id used by the reminders table (AniList ids stay far below): 3e12 for series, 4e12 for films.
pub fn reminder_id(kind: Kind, id: i64) -> i64 {
    (if kind == Kind::Tv {
        3_000_000_000_000
    } else {
        4_000_000_000_000
    }) + id
}

/// Parse `tmdb-tv:123` / `tmdb-movie:123`.
pub fn parse_work_key(key: &str) -> Option<(Kind, i64)> {
    let (k, id) = key.split_once(':')?;
    let kind = match k {
        "tmdb-tv" => Kind::Tv,
        "tmdb-movie" => Kind::Movie,
        _ => return None,
    };
    Some((kind, id.parse().ok()?))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Work {
    pub id: i64,
    pub kind: Option<Kind>,
    pub title: String,
    pub original_title: String,
    pub year: Option<i32>,
    pub overview: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub rating: Option<f32>,
    pub votes: Option<i64>,
    pub popularity: Option<f64>,
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub seasons: Option<i64>,
    pub episodes: Option<i64>,
    pub runtime: Option<i64>,
    /// `YYYY-MM-DD` of the next episode.
    pub next_air: Option<String>,
    /// The next episode to air: `(date, season, episode)`.
    pub next_ep: Option<(String, i64, i64)>,
    /// The latest episode that aired: `(date, season, episode)`.
    pub last_ep: Option<(String, i64, i64)>,
    pub networks: Vec<String>,
    pub countries: Vec<String>,
    /// Release date of a movie in the requested region (or the global one).
    pub release_date: Option<String>,
    pub adult: bool,
    pub keywords: Vec<String>,
    pub certifications: Vec<String>,
    pub providers: Providers,
    pub cast: Vec<Cast>,
}

impl Work {
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.unwrap_or(Kind::Tv).key(), self.id)
    }

    pub fn url(&self) -> String {
        format!(
            "https://www.themoviedb.org/{}/{}",
            if self.kind == Some(Kind::Movie) {
                "movie"
            } else {
                "tv"
            },
            self.id
        )
    }

    /// Dropped from every output path.
    pub fn blocked(&self) -> bool {
        super::safety::is_blocked(self.adult, &self.keywords, &self.certifications)
    }
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn opt_s(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
}

fn year_of(date: &str) -> Option<i32> {
    date.get(..4)?.parse().ok()
}

fn names(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn base(v: &Value, kind: Kind) -> Work {
    let (title_key, orig_key, date_key) = if kind == Kind::Tv {
        ("name", "original_name", "first_air_date")
    } else {
        ("title", "original_title", "release_date")
    };
    let date = s(v, date_key);
    Work {
        id: v.get("id").and_then(Value::as_i64).unwrap_or(0),
        kind: Some(kind),
        title: s(v, title_key),
        original_title: s(v, orig_key),
        year: year_of(&date),
        overview: s(v, "overview"),
        poster: opt_s(v, "poster_path").map(|p| image(&p, "w500")),
        backdrop: opt_s(v, "backdrop_path").map(|p| image(&p, "w780")),
        rating: v
            .get("vote_average")
            .and_then(Value::as_f64)
            .filter(|r| *r > 0.0)
            .map(|r| r as f32),
        votes: v
            .get("vote_count")
            .and_then(Value::as_i64)
            .filter(|n| *n > 0),
        popularity: v.get("popularity").and_then(Value::as_f64),
        countries: strings(v.get("origin_country")),
        adult: v.get("adult").and_then(Value::as_bool).unwrap_or(false),
        release_date: (kind == Kind::Movie)
            .then_some(date)
            .filter(|d| !d.is_empty()),
        ..Work::default()
    }
}

/// Results of `search/tv`, `search/movie` or a `discover` / list call. Adult-flagged results never come out.
pub fn parse_list(doc: &Value, kind: Kind) -> Vec<Work> {
    doc.get("results")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|r| base(r, kind))
                .filter(|w| w.id > 0 && !w.title.is_empty() && !w.blocked())
                .collect()
        })
        .unwrap_or_default()
}

/// Cast of a `credits` / `aggregate_credits` object (top billing first, capped).
pub fn parse_cast(doc: &Value) -> Vec<Cast> {
    let Some(cast) = doc.get("cast").and_then(Value::as_array) else {
        return Vec::new();
    };
    cast.iter()
        .filter(|c| !c.get("adult").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|c| {
            // `aggregate_credits` lists roles[]; plain credits have `character`.
            let character = c
                .get("character")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    c.get("roles")
                        .and_then(Value::as_array)
                        .and_then(|r| r.first())
                        .and_then(|r| r.get("character"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            Some(Cast {
                person_id: c.get("id")?.as_i64()?,
                actor: c.get("name")?.as_str()?.to_string(),
                character,
                photo: opt_s(c, "profile_path").map(|p| image(&p, "h632")),
                popularity: c.get("popularity").and_then(Value::as_f64),
            })
        })
        .take(60)
        .collect()
}

/// Watch providers of a region from the `watch/providers` object.
pub fn parse_providers(doc: &Value, region: &str) -> Providers {
    let Some(r) = doc.get("results").and_then(|r| r.get(region)) else {
        return Providers::default();
    };
    Providers {
        flatrate: names_by(r, "flatrate"),
        rent: names_by(r, "rent"),
        buy: names_by(r, "buy"),
    }
}

fn names_by(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    p.get("provider_name")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .take(6)
                .collect()
        })
        .unwrap_or_default()
}

/// `{air_date, season_number, episode_number}` of an episode reference.
fn episode_ref(v: Option<&Value>) -> Option<(String, i64, i64)> {
    let v = v?;
    let date = v
        .get("air_date")?
        .as_str()
        .filter(|d| !d.is_empty())?
        .to_string();
    Some((
        date,
        v.get("season_number")?.as_i64()?,
        v.get("episode_number")?.as_i64()?,
    ))
}

/// A TV show from `tv/{id}` with the appended credits, content ratings, keywords and providers.
pub fn parse_tv(doc: &Value, region: &str) -> Work {
    let mut w = base(doc, Kind::Tv);
    w.genres = names(doc.get("genres"));
    w.status = opt_s(doc, "status");
    w.seasons = doc.get("number_of_seasons").and_then(Value::as_i64);
    w.episodes = doc.get("number_of_episodes").and_then(Value::as_i64);
    w.runtime = doc
        .get("episode_run_time")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_i64);
    w.next_air = doc
        .get("next_episode_to_air")
        .and_then(|n| n.get("air_date"))
        .and_then(Value::as_str)
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    w.next_ep = episode_ref(doc.get("next_episode_to_air"));
    w.last_ep = episode_ref(doc.get("last_episode_to_air"));
    w.networks = names(doc.get("networks"));
    w.keywords = names(doc.get("keywords").and_then(|k| k.get("results")));
    w.certifications = doc
        .get("content_ratings")
        .and_then(|c| c.get("results"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| r.get("rating").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    w.providers = parse_providers(doc.get("watch/providers").unwrap_or(&Value::Null), region);
    w.cast = parse_cast(
        doc.get("aggregate_credits")
            .or_else(|| doc.get("credits"))
            .unwrap_or(&Value::Null),
    );
    w
}

/// A movie from `movie/{id}`; the release date is the region's theatrical/digital date when TMDB has one.
pub fn parse_movie(doc: &Value, region: &str) -> Work {
    let mut w = base(doc, Kind::Movie);
    w.genres = names(doc.get("genres"));
    w.status = opt_s(doc, "status");
    w.runtime = doc
        .get("runtime")
        .and_then(Value::as_i64)
        .filter(|r| *r > 0);
    w.keywords = names(doc.get("keywords").and_then(|k| k.get("keywords")));
    let (date, certs) = release_info(doc.get("release_dates").unwrap_or(&Value::Null), region);
    if date.is_some() {
        w.release_date = date;
    }
    w.certifications = certs;
    w.providers = parse_providers(doc.get("watch/providers").unwrap_or(&Value::Null), region);
    w.cast = parse_cast(doc.get("credits").unwrap_or(&Value::Null));
    w
}

/// `(region release date, all certifications)` from the `release_dates` object.
pub fn release_info(doc: &Value, region: &str) -> (Option<String>, Vec<String>) {
    let mut certs = Vec::new();
    let mut date = None;
    for entry in doc
        .get("results")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        let releases = entry
            .get("release_dates")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for r in releases {
            if let Some(c) = r
                .get("certification")
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
            {
                certs.push(c.to_string());
            }
        }
        if entry.get("iso_3166_1").and_then(Value::as_str) == Some(region) {
            date = releases
                .iter()
                .filter_map(|r| r.get("release_date").and_then(Value::as_str))
                .map(|d| d.chars().take(10).collect::<String>())
                .min();
        }
    }
    (date, certs)
}

/// A person from `search/person`: hidden when flagged adult.
pub fn parse_people(doc: &Value) -> Vec<Cast> {
    doc.get("results")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|p| !p.get("adult").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(|p| {
                    Some(Cast {
                        person_id: p.get("id")?.as_i64()?,
                        actor: p.get("name")?.as_str()?.to_string(),
                        character: String::new(),
                        photo: opt_s(p, "profile_path").map(|x| image(&x, "h632")),
                        popularity: p.get("popularity").and_then(Value::as_f64),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
