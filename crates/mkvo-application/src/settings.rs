use std::collections::BTreeSet;
use std::sync::Arc;

use mkvo_contracts::{SaveSettingsRequest, SecretStatus, SettingsResponse};
use mkvo_domain::{AppSettings, WatchSettings};

use crate::{ApplicationError, ApplicationResult, SecretStore, SettingsRepository, WatchBackend};

const KNOWN_SECRET_KEYS: [&str; 3] = ["tvdbApiKey", "tvdbPin", "tmdbApiKey"];

pub struct SettingsService {
    repository: Arc<dyn SettingsRepository>,
    secrets: Arc<dyn SecretStore>,
    watcher: Option<Arc<dyn WatchBackend>>,
}

impl SettingsService {
    #[must_use]
    pub fn new(
        repository: Arc<dyn SettingsRepository>,
        secrets: Arc<dyn SecretStore>,
        watcher: Option<Arc<dyn WatchBackend>>,
    ) -> Self {
        Self {
            repository,
            secrets,
            watcher,
        }
    }

    pub async fn load(&self) -> ApplicationResult<SettingsResponse> {
        let (settings, revision) = self.repository.load().await?;
        Ok(SettingsResponse {
            settings,
            revision,
            secret_status: self.secret_statuses().await?,
        })
    }

    pub async fn save(&self, request: SaveSettingsRequest) -> ApplicationResult<SettingsResponse> {
        let (old_settings, old_revision) = self.repository.load().await?;
        let settings = request.settings.normalized();
        validate_settings(&settings)?;
        validate_secret_updates(&request.secrets)?;

        let expected_revision = request.expected_revision.or(Some(old_revision));
        let revision = self.repository.save(&settings, expected_revision).await?;
        for update in request.secrets {
            if update.clear {
                self.secrets.remove(&update.key).await?;
            } else if let Some(value) = update.value.as_deref()
                && !value.is_empty()
            {
                self.secrets.set(&update.key, value).await?;
            }
        }

        if old_settings.watch != settings.watch
            && let Some(watcher) = &self.watcher
        {
            watcher.stop().await?;
            if settings.watch.enabled
                && let Err(error) = watcher
                    .start(&settings.watch.roots, settings.watch.force_polling)
                    .await
            {
                // Settings and watcher state move together. Restore both on
                // watcher failure; secrets are intentionally independent.
                let _ = self.repository.save(&old_settings, Some(revision)).await;
                let _ = restart_watcher(watcher, &old_settings.watch).await;
                return Err(error.into());
            }
        }

        Ok(SettingsResponse {
            settings,
            revision,
            secret_status: self.secret_statuses().await?,
        })
    }

    async fn secret_statuses(&self) -> ApplicationResult<Vec<SecretStatus>> {
        let mut statuses = Vec::with_capacity(KNOWN_SECRET_KEYS.len());
        for key in KNOWN_SECRET_KEYS {
            let secret = self.secrets.get(key).await?;
            statuses.push(SecretStatus {
                key: key.to_owned(),
                configured: secret.is_some(),
                masked_hint: secret.as_deref().map(masked_hint),
            });
        }
        Ok(statuses)
    }
}

async fn restart_watcher(
    watcher: &Arc<dyn WatchBackend>,
    settings: &WatchSettings,
) -> Result<(), crate::PortError> {
    watcher.stop().await?;
    if settings.enabled {
        watcher
            .start(&settings.roots, settings.force_polling)
            .await?;
    }
    Ok(())
}

fn validate_settings(settings: &AppSettings) -> ApplicationResult<()> {
    if settings.schema_version == 0 {
        return Err(ApplicationError::InvalidRequest(
            "settings schema version must be positive".to_owned(),
        ));
    }
    if settings.rename.template.trim().is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "rename template must not be empty".to_owned(),
        ));
    }
    if settings
        .watch
        .roots
        .iter()
        .any(|root| root.as_os_str().is_empty())
    {
        return Err(ApplicationError::InvalidRequest(
            "watch roots must not be blank".to_owned(),
        ));
    }
    if settings
        .media_servers
        .iter()
        .filter(|server| server.is_default)
        .count()
        > 1
    {
        return Err(ApplicationError::InvalidRequest(
            "only one media server can be the default".to_owned(),
        ));
    }
    if settings.media_servers.iter().any(|server| {
        server.name.trim().is_empty()
            || !(server.server_url.starts_with("http://")
                || server.server_url.starts_with("https://"))
    }) {
        return Err(ApplicationError::InvalidRequest(
            "media servers require a name and HTTP(S) URL".to_owned(),
        ));
    }
    let mut theme_names = BTreeSet::new();
    for theme in &settings.appearance.custom_themes {
        let name = theme.name.trim().to_lowercase();
        if name.is_empty() || !theme_names.insert(name) {
            return Err(ApplicationError::InvalidRequest(
                "custom theme names must be non-empty and unique".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_secret_updates(updates: &[mkvo_contracts::SecretUpdate]) -> ApplicationResult<()> {
    let mut keys = BTreeSet::new();
    for update in updates {
        if update.key.trim().is_empty() || update.key.len() > 100 {
            return Err(ApplicationError::InvalidRequest(
                "secret key must contain 1-100 characters".to_owned(),
            ));
        }
        if !keys.insert(update.key.as_str()) {
            return Err(ApplicationError::InvalidRequest(format!(
                "secret '{}' was updated more than once",
                update.key
            )));
        }
        if update.clear && update.value.is_some() {
            return Err(ApplicationError::InvalidRequest(format!(
                "secret '{}' cannot be set and cleared together",
                update.key
            )));
        }
    }
    Ok(())
}

fn masked_hint(secret: &str) -> String {
    let characters: Vec<_> = secret.chars().collect();
    let visible = characters.len().min(4);
    let suffix: String = characters[characters.len() - visible..].iter().collect();
    format!("••••{suffix}")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use async_trait::async_trait;
    use tokio::sync::RwLock;

    use super::*;
    use crate::PortError;

    struct Repository(RwLock<(AppSettings, u64)>);
    #[async_trait]
    impl SettingsRepository for Repository {
        async fn load(&self) -> Result<(AppSettings, u64), PortError> {
            Ok(self.0.read().await.clone())
        }
        async fn save(
            &self,
            settings: &AppSettings,
            expected_revision: Option<u64>,
        ) -> Result<u64, PortError> {
            let mut value = self.0.write().await;
            if expected_revision.is_some_and(|expected| expected != value.1) {
                return Err(PortError::Conflict("revision".to_owned()));
            }
            value.1 += 1;
            value.0 = settings.clone();
            Ok(value.1)
        }
    }

    #[derive(Default)]
    struct Secrets(RwLock<BTreeMap<String, String>>);
    #[async_trait]
    impl SecretStore for Secrets {
        async fn get(&self, key: &str) -> Result<Option<String>, PortError> {
            Ok(self.0.read().await.get(key).cloned())
        }
        async fn set(&self, key: &str, value: &str) -> Result<(), PortError> {
            self.0
                .write()
                .await
                .insert(key.to_owned(), value.to_owned());
            Ok(())
        }
        async fn remove(&self, key: &str) -> Result<(), PortError> {
            self.0.write().await.remove(key);
            Ok(())
        }
    }

    #[tokio::test]
    async fn response_masks_secrets_and_normalizes_workers() {
        let repository = Arc::new(Repository(RwLock::new((AppSettings::default(), 1))));
        let secrets = Arc::new(Secrets::default());
        let service = SettingsService::new(repository, secrets, None);
        let mut settings = AppSettings::default();
        settings.workers.max_scan_workers = 99;
        let response = service
            .save(SaveSettingsRequest {
                settings,
                secrets: vec![mkvo_contracts::SecretUpdate {
                    key: "tvdbApiKey".to_owned(),
                    value: Some("abcdefgh".to_owned()),
                    clear: false,
                }],
                expected_revision: Some(1),
            })
            .await
            .unwrap();
        assert_eq!(response.settings.workers.max_scan_workers, 8);
        let tvdb = response
            .secret_status
            .iter()
            .find(|status| status.key == "tvdbApiKey")
            .unwrap();
        assert_eq!(tvdb.masked_hint.as_deref(), Some("••••efgh"));
    }
}
