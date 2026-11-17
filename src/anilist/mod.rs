//! AniList access. `client` (raw transport) is private to this module; callers use `SafeAniList`.

mod client;
pub mod models;
mod queries;
mod safe;
pub mod safety;

pub use client::AniError;
pub use safe::{
    kind_of, AiringEntry, AiringInfo, CharacterLookup, ContentPolicy, MediaFilter, MediaKind,
    SafeAniList, CAST_PAGE_SIZE,
};
