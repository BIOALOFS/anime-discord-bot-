pub mod client;
pub mod models;
pub mod queries;
pub mod safety;

#[allow(unused_imports)] // used by commands from milestone 3
pub use client::{AniError, AniListClient, CharacterLookup, MediaKind};
