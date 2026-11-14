//! GraphQL documents. Adult filtering for media is driven by the `$isAdult` /
//! `$genreNotIn` variables, which the client sets to `false` / `["Hentai"]`
//! unless adult content is explicitly allowed.

macro_rules! with_media_fragment {
    ($body:literal) => {
        concat!(
            $body,
            " fragment M on Media { id title { romaji english native } description(asHtml: false) \
             coverImage { large extraLarge color } bannerImage averageScore popularity episodes chapters \
             duration source status season seasonYear format genres isAdult siteUrl \
             studios(isMain: true) { nodes { name } } nextAiringEpisode { airingAt episode } \
             trailer { id site } staff(perPage: 3, sort: [RELEVANCE]) { edges { role node { name { full } } } } }"
        )
    };
}

/// Up to 50 appearances are fetched so the safety filter sees every popular one.
macro_rules! character_fields {
    () => {
        "id name { full native alternative alternativeSpoiler } image { large } \
         description(asHtml: false) favourites siteUrl age gender dateOfBirth { year month day } bloodType \
         media(perPage: 50, sort: [POPULARITY_DESC]) { pageInfo { hasNextPage } \
         nodes { id title { romaji english native } isAdult genres siteUrl format seasonYear } }"
    };
}

pub const SEARCH_MEDIA: &str = with_media_fragment!(
    "query($search: String, $type: MediaType, $perPage: Int, $isAdult: Boolean, $genreNotIn: [String]) { \
     Page(perPage: $perPage) { media(search: $search, type: $type, isAdult: $isAdult, genre_not_in: $genreNotIn, sort: [SEARCH_MATCH]) { ...M } } }"
);

pub const MEDIA_BY_ID: &str = with_media_fragment!("query($id: Int) { Media(id: $id) { ...M } }");

pub const SEASONAL: &str = with_media_fragment!(
    "query($season: MediaSeason, $year: Int, $page: Int, $perPage: Int, $sort: [MediaSort], $formatIn: [MediaFormat], \
     $isAdult: Boolean, $genreNotIn: [String]) { \
     Page(page: $page, perPage: $perPage) { pageInfo { hasNextPage lastPage } \
     media(season: $season, seasonYear: $year, type: ANIME, format_in: $formatIn, isAdult: $isAdult, \
     genre_not_in: $genreNotIn, sort: $sort) { ...M } } }"
);

pub const AIRING_SCHEDULE: &str = with_media_fragment!(
    "query($from: Int, $to: Int, $page: Int, $perPage: Int) { Page(page: $page, perPage: $perPage) { \
     pageInfo { hasNextPage } \
     airingSchedules(airingAt_greater: $from, airingAt_lesser: $to, sort: [TIME]) { airingAt episode media { ...M } } } }"
);

pub const AIRING_FOR_MEDIA: &str = with_media_fragment!(
    "query($id: Int) { Media(id: $id) { ...M airingSchedule(perPage: 50) { nodes { episode airingAt } } } }"
);

/// Characters of a media. Nested appearances are capped at 25 to stay within AniList's query-complexity limit.
pub const MEDIA_CHARACTERS: &str = "query($id: Int, $page: Int) { Media(id: $id) { \
    characters(page: $page, perPage: 8, sort: [ROLE, RELEVANCE]) { pageInfo { hasNextPage lastPage } edges { role node { \
    id name { full native } image { large } favourites siteUrl \
    media(perPage: 25, sort: [POPULARITY_DESC]) { pageInfo { hasNextPage } nodes { isAdult genres } } } } } } }";

pub const RECOMMENDATIONS: &str = with_media_fragment!(
    "query($id: Int, $page: Int) { Media(id: $id) { \
     recommendations(page: $page, perPage: 5, sort: [RATING_DESC]) { pageInfo { hasNextPage lastPage } nodes { mediaRecommendation { ...M } } } } }"
);

pub const RELATIONS: &str = with_media_fragment!(
    "query($id: Int) { Media(id: $id) { relations { edges { relationType(version: 2) node { ...M } } } } }"
);

pub const RANDOM_MEDIA: &str = with_media_fragment!(
    "query($page: Int, $type: MediaType, $genre: String, $minScore: Int, $isAdult: Boolean, $genreNotIn: [String]) { \
     Page(page: $page, perPage: 20) { pageInfo { lastPage hasNextPage } \
     media(type: $type, genre: $genre, averageScore_greater: $minScore, isAdult: $isAdult, genre_not_in: $genreNotIn, \
     sort: [POPULARITY_DESC]) { ...M } } }"
);

pub const SEARCH_CHARACTER: &str = concat!(
    "query($search: String, $perPage: Int) { Page(perPage: $perPage) { \
     characters(search: $search, sort: [SEARCH_MATCH]) { ",
    character_fields!(),
    " } } }"
);

pub const CHARACTER_BY_ID: &str = concat!(
    "query($id: Int) { Character(id: $id) { ",
    character_fields!(),
    " } }"
);

pub const RANDOM_CHARACTER: &str = concat!(
    "query($page: Int) { Page(page: $page, perPage: 10) { pageInfo { lastPage } \
     characters(sort: [FAVOURITES_DESC]) { ",
    character_fields!(),
    " } } }"
);

pub const GENRES: &str = "query { GenreCollection }";

pub const USER_PROFILE: &str = "query($name: String) { User(name: $name) { id name avatar { large } bannerImage siteUrl \
    statistics { anime { count meanScore episodesWatched genres(limit: 5, sort: [COUNT_DESC]) { genre count } } \
    manga { count meanScore chaptersRead genres(limit: 5, sort: [COUNT_DESC]) { genre count } } } } }";
