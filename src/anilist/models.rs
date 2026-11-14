use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Nodes<T> {
    #[serde(default = "Vec::new")]
    pub nodes: Vec<T>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Title {
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
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
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CharacterName {
    pub full: Option<String>,
    pub native: Option<String>,
    #[serde(default)]
    pub alternative: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CharacterImage {
    pub large: Option<String>,
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
    pub media: Option<Nodes<MediaRef>>,
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
