//! TVDB and TMDB metadata clients with normalized, provider-neutral DTOs.

mod adapter;
mod common;
mod tmdb;
mod tvdb;

pub use adapter::{ConfiguredTmdbProvider, ConfiguredTvdbProvider};
pub use common::{
    EpisodeMetadata, MediaKind, MetadataProviderClient, ProviderCredentials, ProviderError,
    ProviderKind, SearchResult, SecretString, SelectedMedia, normalize_language,
};
pub use tmdb::TmdbClient;
pub use tvdb::TvdbClient;
