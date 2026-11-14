//! GraphQL documents. Adult filtering for media is driven by the `$isAdult` /
//! `$genreNotIn` variables, which the client sets to `false` / `["Hentai"]`
//! unless adult content is explicitly allowed.

macro_rules! with_media_fragment {
    ($body:literal) => {
        concat!(
            $body,
            " fragment M on Media { id title { romaji english native } description(asHtml: false) \
             coverImage { large extraLarge color } bannerImage averageScore popularity episodes chapters \
             status season seasonYear format genres isAdult siteUrl studios(isMain: true) { nodes { name } } \
             nextAiringEpisode { airingAt episode } }"
        )
    };
}

macro_rules! character_fields {
    () => {
        "id name { full native alternative } image { large } description(asHtml: false) favourites siteUrl \
         media(perPage: 25, sort: POPULARITY_DESC) { nodes { id title { romaji english native } isAdult genres siteUrl } }"
    };
}

pub const SEARCH_MEDIA: &str = with_media_fragment!(
    "query($search: String, $type: MediaType, $perPage: Int, $isAdult: Boolean, $genreNotIn: [String]) { \
     Page(perPage: $perPage) { media(search: $search, type: $type, isAdult: $isAdult, genre_not_in: $genreNotIn, sort: SEARCH_MATCH) { ...M } } }"
);

pub const MEDIA_BY_ID: &str = with_media_fragment!("query($id: Int) { Media(id: $id) { ...M } }");

pub const SEASONAL: &str = with_media_fragment!(
    "query($season: MediaSeason, $year: Int, $page: Int, $perPage: Int, $isAdult: Boolean, $genreNotIn: [String]) { \
     Page(page: $page, perPage: $perPage) { media(season: $season, seasonYear: $year, type: ANIME, isAdult: $isAdult, \
     genre_not_in: $genreNotIn, sort: POPULARITY_DESC) { ...M } } }"
);

pub const AIRING_SCHEDULE: &str = with_media_fragment!(
    "query($from: Int, $to: Int, $perPage: Int) { Page(perPage: $perPage) { \
     airingSchedules(airingAt_greater: $from, airingAt_lesser: $to, sort: TIME) { airingAt episode media { ...M } } } }"
);

pub const SEARCH_CHARACTER: &str = concat!(
    "query($search: String, $perPage: Int) { Page(perPage: $perPage) { \
     characters(search: $search, sort: SEARCH_MATCH) { ",
    character_fields!(),
    " } } }"
);

pub const CHARACTER_BY_ID: &str = concat!(
    "query($id: Int) { Character(id: $id) { ",
    character_fields!(),
    " } }"
);

pub const USER_PROFILE: &str = "query($name: String) { User(name: $name) { id name avatar { large } bannerImage siteUrl \
    statistics { anime { count meanScore episodesWatched genres(limit: 5, sort: COUNT_DESC) { genre count } } \
    manga { count meanScore chaptersRead genres(limit: 5, sort: COUNT_DESC) { genre count } } } } }";
