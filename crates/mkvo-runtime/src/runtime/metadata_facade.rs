use mkvo_contracts::{RenameProviderTestResponse, RenameScopeRow};
use tokio_util::sync::CancellationToken;

use super::{MkvoRuntime, rename_search_result};
use crate::compat::{
    RenameProviderTestRequest, RenameScopesRequest, RenameSearchRequest, RenameSearchResult,
};
use crate::{RuntimeError, RuntimeResult};

impl MkvoRuntime {
    pub async fn search_rename_metadata(
        &self,
        request: RenameSearchRequest,
    ) -> RuntimeResult<Vec<RenameSearchResult>> {
        if request.query.trim().is_empty() {
            return Err(RuntimeError::invalid("Enter a title to search."));
        }
        let provider = self
            .provider_client(request.provider.as_deref(), request.language.as_deref())
            .await?;
        let results = provider
            .search(
                request.query.trim(),
                request.language.as_deref(),
                CancellationToken::new(),
            )
            .await?;
        Ok(results.into_iter().map(rename_search_result).collect())
    }

    pub async fn load_rename_scopes(
        &self,
        request: RenameScopesRequest,
    ) -> RuntimeResult<Vec<RenameScopeRow>> {
        let provider = self
            .provider_client(request.provider.as_deref(), request.language.as_deref())
            .await?;
        let episodes = provider
            .episodes(
                &request.selected_result.media_id(),
                request.language.as_deref(),
                CancellationToken::new(),
            )
            .await?;
        let mut seasons: Vec<_> = episodes.iter().map(|episode| episode.season).collect();
        seasons.sort_unstable();
        seasons.dedup();
        let mut scopes = vec![RenameScopeRow {
            key: "all".to_owned(),
            label: format!("All episodes ({})", episodes.len()),
            is_selected: true,
        }];
        scopes.extend(seasons.into_iter().map(|season| RenameScopeRow {
            key: format!("season:{season}"),
            label: format!("Season {season}"),
            is_selected: false,
        }));
        Ok(scopes)
    }

    pub async fn test_rename_provider(
        &self,
        request: RenameProviderTestRequest,
    ) -> RuntimeResult<RenameProviderTestResponse> {
        let provider = self
            .provider_client(request.provider.as_deref(), request.language.as_deref())
            .await?;
        match provider.test(CancellationToken::new()).await {
            Ok(()) => Ok(RenameProviderTestResponse {
                success: true,
                status: "Metadata provider connection successful.".to_owned(),
            }),
            Err(error) => Ok(RenameProviderTestResponse {
                success: false,
                status: format!("Metadata provider connection failed: {error}"),
            }),
        }
    }
}
