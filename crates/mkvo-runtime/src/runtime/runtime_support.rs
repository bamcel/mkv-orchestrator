use super::*;

impl MkvoRuntime {
    pub(crate) fn settings_service(&self) -> Arc<SettingsService> {
        Arc::clone(&self.inner.settings_service)
    }

    pub(crate) async fn provider_client(
        &self,
        requested: Option<&str>,
        requested_language: Option<&str>,
    ) -> RuntimeResult<Arc<dyn MetadataProviderClient>> {
        let loaded = self.settings_service().load().await?;
        let provider = requested
            .map(parse_provider)
            .transpose()?
            .unwrap_or(loaded.settings.rename.provider);
        let language = requested_language
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&loaded.settings.rename.language)
            .to_owned();
        match provider {
            MetadataProvider::Tvdb => {
                let api_key = self
                    .secret_alias(&["provider.tvdb.api_key", "tvdbApiKey"])
                    .await?
                    .ok_or_else(|| RuntimeError::invalid("TVDB API key is not configured"))?;
                let pin = self
                    .secret_alias(&["provider.tvdb.pin", "tvdbPin"])
                    .await?
                    .map(SecretString::new);
                Ok(Arc::new(ConfiguredTvdbProvider::new(
                    TvdbClient::new(),
                    ProviderCredentials {
                        api_key: SecretString::new(api_key),
                        pin,
                    },
                    language,
                )))
            }
            MetadataProvider::Tmdb => {
                let api_key = self
                    .secret_alias(&["provider.tmdb.api_key", "tmdbApiKey"])
                    .await?
                    .ok_or_else(|| RuntimeError::invalid("TMDB API key is not configured"))?;
                Ok(Arc::new(ConfiguredTmdbProvider::new(
                    TmdbClient::new(),
                    ProviderCredentials::api_key(api_key),
                    language,
                )))
            }
            // AniList is a public GraphQL API and needs no credentials.
            MetadataProvider::AniList => Ok(Arc::new(ConfiguredAniListProvider::new(
                AniListClient::new(),
                language,
            ))),
            // AniDB identifies callers by a registered client name rather than a
            // key, stored as `name/version`. Searching only reads the public
            // title dump, so a missing client name is reported by the episode
            // call rather than blocking search.
            MetadataProvider::AniDb => {
                let client = self
                    .secret_alias(&["provider.anidb.client", "anidbClient"])
                    .await?
                    .unwrap_or_default();
                Ok(Arc::new(ConfiguredAniDbProvider::new(
                    AniDbClient::new(),
                    ProviderCredentials::api_key(client),
                    language,
                )))
            }
        }
    }

    pub(crate) async fn current_domain_files(&self) -> Vec<MediaFile> {
        self.inner.current_scan.read().await.files.clone()
    }

    pub(crate) fn dependencies(&self) -> &RuntimeDependencies {
        &self.inner.dependencies
    }

    pub(crate) fn jobs(&self) -> &Arc<JobSupervisor> {
        &self.inner.jobs
    }

    pub(crate) fn config(&self) -> &RuntimeConfig {
        &self.inner.config
    }

    /// Authorize a folder the user picked in the browser.
    ///
    /// Browsing may range wider than the authorized roots, so a folder found
    /// that way is not yet usable as a scan source. Choosing it is the user's
    /// own explicit act and carries the same weight as the native folder
    /// picker, but it still goes through the same canonicalization and
    /// validation as any other grant — the path is not trusted for having come
    /// from the browser.
    ///
    /// Hosts that confine browsing refuse this: on a network service the caller
    /// is not necessarily the machine's owner.
    pub fn authorize_browsed_root(&self, path: &str) -> RuntimeResult<AuthorizedRootGrant> {
        if self.inner.config.browse_scope != BrowseScope::Unrestricted {
            return Err(RuntimeError::invalid(
                "this host only allows sources inside its configured roots",
            ));
        }
        self.grant_authorized_root(Path::new(path), true)
    }

    /// Grant the roots a saved settings revision just made authoritative.
    ///
    /// Composition grants persisted roots at startup, so without this a scan
    /// root or watch folder chosen in Settings stays unauthorized until the next
    /// restart. Choosing a root in Settings is the user's own explicit act, so
    /// it carries the same authority as the startup grant; a path that cannot be
    /// canonicalized is reported and skipped rather than failing the save, which
    /// has already been committed.
    /// A library folder must exist, and on a confined host it must sit inside a
    /// root the operator already granted.
    ///
    /// Without that second rule the setting would be an authorization bypass: a
    /// web client could name `/etc`, save it, and have the runtime grant it --
    /// which is precisely what the authorized roots exist to prevent. An
    /// unrestricted desktop has no such boundary to breach, since the user
    /// already reaches every one of these paths through their file manager.
    pub(super) async fn validate_configured_roots(
        &self,
        settings: &AppSettings,
    ) -> RuntimeResult<()> {
        for root in &settings.scan.library_roots {
            if root.name.trim().is_empty() {
                return Err(RuntimeError::invalid("a library folder needs a name"));
            }
            if root.path.as_os_str().is_empty() {
                return Err(RuntimeError::invalid(format!(
                    "library folder `{}` needs a path",
                    root.name.trim()
                )));
            }
        }

        let configured = settings
            .scan
            .default_root
            .iter()
            .chain(settings.scan.library_roots.iter().map(|root| &root.path))
            .chain(settings.watch.roots.iter());
        for path in configured {
            let validated = if self.inner.config.browse_scope == BrowseScope::AuthorizedRootsOnly {
                self.inner.dependencies.paths.authorize_read(path).await?
            } else {
                path.clone()
            };
            if !validated.is_dir() {
                return Err(RuntimeError::invalid(format!(
                    "library folder is not a directory: {}",
                    display_path(path)
                )));
            }
        }
        Ok(())
    }

    pub(super) fn authorize_configured_roots(&self, settings: &AppSettings) {
        // A remote host's operator owns its authorization boundary. Settings
        // may select descendants of those roots, but must never expand it.
        if self.inner.config.browse_scope == BrowseScope::AuthorizedRootsOnly {
            return;
        }
        for path in settings
            .scan
            .default_root
            .iter()
            .chain(settings.scan.library_roots.iter().map(|root| &root.path))
            .chain(settings.watch.roots.iter())
        {
            if let Err(error) = self.grant_authorized_root(path, true) {
                tracing::warn!(
                    path = %path.display(),
                    error = %error,
                    "configured root could not be authorized"
                );
            }
        }
    }

    /// Host-only capability grant intended for a native folder picker or
    /// trusted server bootstrap. Transports should not expose this directly.
    pub fn grant_authorized_root(
        &self,
        path: impl AsRef<Path>,
        writable: bool,
    ) -> RuntimeResult<AuthorizedRootGrant> {
        let roots = self
            .inner
            .dependencies
            .authorized_roots
            .as_ref()
            .ok_or_else(|| {
                RuntimeError::internal(
                    "dynamic path grants are unavailable for custom dependencies",
                )
            })?;
        let root = roots
            .grant(path, writable)
            .map_err(|error| RuntimeError::invalid(error.to_string()))?;
        Ok(AuthorizedRootGrant {
            path: display_path(root.path()),
            writable: root.writable(),
        })
    }

    #[must_use]
    pub fn list_authorized_roots(&self) -> Vec<AuthorizedRootGrant> {
        self.inner
            .dependencies
            .authorized_roots
            .as_ref()
            .map(|roots| {
                roots
                    .roots()
                    .into_iter()
                    .map(|root| AuthorizedRootGrant {
                        path: display_path(root.path()),
                        writable: root.writable(),
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                self.inner
                    .config
                    .authorized_roots
                    .iter()
                    .map(|(path, writable)| AuthorizedRootGrant {
                        path: display_path(path),
                        writable: *writable,
                    })
                    .collect()
            })
    }

    pub(crate) fn legacy_rename_history(&self) -> &[LegacyRenameBatchRecord] {
        &self.inner.legacy_rename_history
    }

    pub(super) async fn secret_alias(&self, keys: &[&str]) -> RuntimeResult<Option<String>> {
        for key in keys {
            if let Some(value) = self.inner.dependencies.secrets.get(key).await?
                && !value.is_empty()
            {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
}

/// Render a path for the UI.
///
/// Authorized roots are canonicalized, and on Windows `fs::canonicalize` returns
/// the extended-length `\\?\` form. That prefix is correct to keep for process
/// arguments, where it is what lifts the `MAX_PATH` limit, but showing it to the
/// user is wrong: it is not what they typed and not what they can paste back.
/// It is only removed when doing so still yields a valid path.
pub(crate) fn display_path(path: &Path) -> String {
    mkvo_domain::normalized_path_text(path)
}
