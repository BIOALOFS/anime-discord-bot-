//! One display model for both sections of a profile (characters and works).

use crate::{
    anilist::MediaKind,
    db::{
        profile::{self as pdb, Entry},
        works::{self, WorkEntry},
    },
    ui::cards::{flag, kind_label},
};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Characters,
    Works,
}

impl Section {
    #[allow(dead_code)]
    pub fn key(self) -> &'static str {
        match self {
            Self::Characters => "characters",
            Self::Works => "works",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Characters => Self::Works,
            Self::Works => Self::Characters,
        }
    }
}

#[allow(dead_code)] // tier / image id stay part of the model
#[derive(Debug, Clone)]
pub struct Item {
    pub section: Section,
    /// Character id as text, or the provider-prefixed work id.
    pub key: String,
    pub position: i64,
    pub name: String,
    pub native: Option<String>,
    /// Main work (characters) or the type tag (works).
    pub subtitle: Option<String>,
    pub subtitle_url: Option<String>,
    pub image_url: Option<String>,
    pub tier: Option<String>,
    pub note: Option<String>,
    pub custom_title: Option<String>,
    pub custom_description: Option<String>,
    pub custom_image_id: Option<i64>,
    pub favourites: Option<i64>,
    pub visible: bool,
    /// Watch / read status of a work (works only).
    pub status: Option<String>,
}

impl Item {
    pub fn from_character(e: &Entry) -> Self {
        Self {
            section: Section::Characters,
            key: e.character_id.to_string(),
            position: e.position,
            name: e.name().to_string(),
            native: e.name_native.clone(),
            subtitle: e.main_work.clone(),
            subtitle_url: e.main_work_url.clone(),
            image_url: e.image_url.clone(),
            tier: e.tier.clone(),
            note: e.note.clone(),
            custom_title: e.custom_title.clone(),
            custom_description: e.custom_description.clone(),
            custom_image_id: e.custom_image_id,
            favourites: e.favourites,
            visible: e.visible(),
            status: None,
        }
    }

    pub fn from_work(w: &WorkEntry) -> Self {
        let kind = match w.kind.as_str() {
            "game" => crate::i18n::KIND_GAME,
            "vn" => crate::i18n::KIND_VN,
            "tmdb-tv" => crate::i18n::KIND_SERIES,
            "tmdb-movie" => crate::i18n::KIND_MOVIE,
            "wiki" => crate::i18n::KIND_WIKI,
            other => MediaKind::from_key(other).map_or("", kind_label),
        };
        let tag = format!("{} {}", flag(w.country.as_deref()), kind)
            .trim()
            .to_string();
        let subtitle = match w.year {
            Some(y) => format!("{tag} · {y}"),
            None => tag,
        };
        Self {
            section: Section::Works,
            key: w.work_id.clone(),
            position: w.position,
            name: w.title().to_string(),
            native: None,
            subtitle: Some(subtitle),
            subtitle_url: None,
            image_url: w.cover_url.clone(),
            tier: w.tier.clone(),
            note: w.note.clone(),
            custom_title: None,
            custom_description: None,
            custom_image_id: None,
            favourites: None,
            visible: w.visible(),
            status: None,
        }
    }

    pub fn character_id(&self) -> Option<i64> {
        (self.section == Section::Characters)
            .then(|| self.key.parse().ok())
            .flatten()
    }

    /// The name drawn on grid cells and lists: the personal title, when set, goes under it.
    pub fn display_title(&self) -> Option<&str> {
        self.custom_title.as_deref()
    }
}

pub async fn count(
    pool: &SqlitePool,
    user: i64,
    section: Section,
    group: Option<i64>,
    list: Option<&str>,
) -> Result<i64, pdb::ProfileError> {
    match section {
        Section::Characters => pdb::count_in_group(pool, user, group).await,
        Section::Works => works::count_filtered(pool, user, group, list).await,
    }
}
