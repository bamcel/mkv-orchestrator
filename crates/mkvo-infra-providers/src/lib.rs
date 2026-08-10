//! TVDB, TMDB, AniDB, and AniList metadata clients with normalized,
//! provider-neutral DTOs.

mod adapter;
mod anidb;
mod anilist;
mod common;
mod tmdb;
mod tvdb;

pub use adapter::{
    ConfiguredAniDbProvider, ConfiguredAniListProvider, ConfiguredTmdbProvider,
    ConfiguredTvdbProvider,
};
pub use anidb::AniDbClient;
pub use anilist::AniListClient;
pub use common::{
    EpisodeMetadata, MediaKind, MetadataProviderClient, ProviderCredentials, ProviderError,
    ProviderKind, SearchResult, SecretString, SelectedMedia, normalize_language,
};
pub use tmdb::TmdbClient;
pub use tvdb::TvdbClient;
