use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    #[serde(default)]
    pub has_next_page: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nodes<T> {
    #[serde(default = "Vec::new")]
    pub nodes: Vec<T>,
    #[serde(default)]
    pub page_info: PageInfo,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Title {
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
}

impl Title {
    /// Romaji first, then English, then native.
    pub fn main(&self) -> &str {
        [&self.romaji, &self.english, &self.native]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .find(|s| !s.is_empty())
            .unwrap_or("?")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub large: Option<String>,
    pub extra_large: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextAiring {
    pub airing_at: i64,
    pub episode: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Studio {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Trailer {
    pub id: Option<String>,
    pub site: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StaffName {
    pub full: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StaffNode {
    #[serde(default)]
    pub name: StaffName,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StaffEdge {
    pub role: Option<String>,
    pub node: StaffNode,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StaffConnection {
    #[serde(default)]
    pub edges: Vec<StaffEdge>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: i64,
    #[serde(default)]
    pub title: Title,
    pub description: Option<String>,
    pub cover_image: Option<CoverImage>,
    pub banner_image: Option<String>,
    pub average_score: Option<i32>,
    pub popularity: Option<i64>,
    pub episodes: Option<i32>,
    pub chapters: Option<i32>,
    pub duration: Option<i32>,
    pub source: Option<String>,
    pub status: Option<String>,
    pub season: Option<String>,
    pub season_year: Option<i32>,
    pub format: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub is_adult: bool,
    pub site_url: Option<String>,
    pub studios: Option<Nodes<Studio>>,
    pub next_airing_episode: Option<NextAiring>,
    pub trailer: Option<Trailer>,
    pub staff: Option<StaffConnection>,
    pub country_of_origin: Option<String>,
    #[serde(default)]
    pub external_links: Vec<ExternalLink>,
    pub volumes: Option<i32>,
    pub start_date: Option<StartDate>,
    /// Other names the work is known by ("ORV"): they count in the local index.
    #[serde(default)]
    pub synonyms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExternalLink {
    pub url: Option<String>,
    pub site: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartDate {
    pub year: Option<i32>,
}

impl Media {
    /// First legal streaming site, if AniList lists one.
    pub fn streaming_link(&self) -> Option<&str> {
        self.external_links
            .iter()
            .find(|l| l.kind.as_deref() == Some("STREAMING"))
            .and_then(|l| l.url.as_deref())
    }

    /// Release year: the start date, else the season year.
    pub fn year(&self) -> Option<i32> {
        self.start_date
            .as_ref()
            .and_then(|d| d.year)
            .or(self.season_year)
    }

    pub fn cover(&self) -> Option<&str> {
        let c = self.cover_image.as_ref()?;
        c.large.as_deref().or(c.extra_large.as_deref())
    }

    /// YouTube trailer URL, if the trailer is hosted there.
    pub fn trailer_url(&self) -> Option<String> {
        let t = self.trailer.as_ref()?;
        match (t.site.as_deref(), t.id.as_deref()) {
            (Some(site), Some(id)) if site.eq_ignore_ascii_case("youtube") && !id.is_empty() => {
                Some(format!("https://www.youtube.com/watch?v={id}"))
            }
            _ => None,
        }
    }
}

/// A media a character appears in; only the flags needed for safety are guaranteed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRef {
    pub id: Option<i64>,
    pub title: Option<Title>,
    #[serde(default)]
    pub is_adult: bool,
    #[serde(default)]
    pub genres: Vec<String>,
    pub site_url: Option<String>,
    pub format: Option<String>,
    pub season_year: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterName {
    pub full: Option<String>,
    pub native: Option<String>,
    #[serde(default)]
    pub alternative: Vec<String>,
    #[serde(default)]
    pub alternative_spoiler: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CharacterImage {
    pub large: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FuzzyDate {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: i64,
    #[serde(default)]
    pub name: CharacterName,
    pub image: Option<CharacterImage>,
    pub description: Option<String>,
    pub favourites: Option<i64>,
    pub site_url: Option<String>,
    pub age: Option<String>,
    pub gender: Option<String>,
    pub date_of_birth: Option<FuzzyDate>,
    pub blood_type: Option<String>,
    pub media: Option<Nodes<MediaRef>>,
}

impl Character {
    pub fn display_name(&self) -> &str {
        self.name
            .full
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("?")
    }

    pub fn image_url(&self) -> Option<&str> {
        self.image.as_ref()?.large.as_deref()
    }

    /// Title of the most popular appearance.
    pub fn main_work(&self) -> Option<&str> {
        let r = self.media.as_ref()?.nodes.first()?;
        Some(r.title.as_ref()?.main())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CharacterEdge {
    pub role: Option<String>,
    pub node: Character,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RelationEdge {
    #[serde(rename = "relationType")]
    pub relation_type: Option<String>,
    pub node: Media,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiringNode {
    pub airing_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Avatar {
    pub large: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenreStat {
    pub genre: String,
    pub count: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimeStats {
    pub count: i64,
    pub mean_score: Option<f64>,
    pub episodes_watched: Option<i64>,
    #[serde(default)]
    pub genres: Vec<GenreStat>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangaStats {
    pub count: i64,
    pub mean_score: Option<f64>,
    pub chapters_read: Option<i64>,
    #[serde(default)]
    pub genres: Vec<GenreStat>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserStatistics {
    pub anime: AnimeStats,
    pub manga: MangaStats,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: i64,
    pub name: String,
    pub avatar: Option<Avatar>,
    pub banner_image: Option<String>,
    pub site_url: Option<String>,
    pub statistics: UserStatistics,
}
