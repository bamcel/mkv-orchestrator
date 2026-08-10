use mkvo_contracts::{WebSettings, WebSettingsRequest};

use super::{MkvoRuntime, apply_web_settings_request, validate_media_server_url, web_settings};
use crate::RuntimeResult;

impl MkvoRuntime {
    pub async fn get_web_settings(&self) -> RuntimeResult<WebSettings> {
        let response = self.settings_service().load().await?;
        self.web_settings_with_secret_aliases(&response).await
    }

    pub async fn save_web_settings(
        &self,
        request: WebSettingsRequest,
    ) -> RuntimeResult<WebSettings> {
        let loaded = self.settings_service().load().await?;
        let mut settings = loaded.settings;
        let mut secrets = Vec::new();
        apply_web_settings_request(&mut settings, request, &mut secrets)?;
        self.validate_configured_roots(&settings).await?;
        for server in &settings.media_servers {
            validate_media_server_url(&server.server_url)?;
        }
        let response = self
            .settings_service()
            .save(mkvo_contracts::SaveSettingsRequest {
                settings,
                secrets,
                expected_revision: Some(loaded.revision),
            })
            .await?;
        self.authorize_configured_roots(&response.settings);
        if let Some(registry) = self.inner.dependencies.process_tools.as_ref() {
            let (explicit, directories) = crate::composition::tool_configuration(
                &response.settings,
                self.inner.config.tool_directories.iter().cloned(),
            );
            registry.reconfigure(explicit, directories);
        }
        self.web_settings_with_secret_aliases(&response).await
    }

    async fn web_settings_with_secret_aliases(
        &self,
        response: &mkvo_contracts::SettingsResponse,
    ) -> RuntimeResult<WebSettings> {
        let mut view = web_settings(&response.settings, &response.secret_status);
        view.has_tvdb_api_key = self
            .secret_alias(&["provider.tvdb.api_key", "tvdbApiKey"])
            .await?
            .is_some();
        view.has_tvdb_pin = self
            .secret_alias(&["provider.tvdb.pin", "tvdbPin"])
            .await?
            .is_some();
        view.has_tmdb_api_key = self
            .secret_alias(&["provider.tmdb.api_key", "tmdbApiKey"])
            .await?
            .is_some();
        view.has_anidb_client = self
            .secret_alias(&["provider.anidb.client", "anidbClient"])
            .await?
            .is_some();
        Ok(view)
    }
}
