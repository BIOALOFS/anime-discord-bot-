//! AniList access. `client` (raw transport) is private to this module; callers use `SafeAniList`.

mod client;
pub mod models;
mod queries;
mod safe;
pub mod safety;

pub use client::AniError;
pub use safe::{
    AiringEntry, AiringInfo, CharacterLookup, ContentPolicy, MediaKind, SafeAniList, SeasonalQuery,
    CAST_PAGE_SIZE,
};
