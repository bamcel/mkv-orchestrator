use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};

impl MkvoRuntime {
    pub async fn get_library_catalog(
        &self,
        request: LibraryCatalogRequest,
    ) -> RuntimeResult<LibraryCatalogResponse> {
        let loaded = self.settings_service().load().await?;
        let server = loaded
            .settings
            .media_servers
            .iter()
            .find(|server| {
                server
                    .id
                    .to_string()
                    .eq_ignore_ascii_case(&request.server_id)
            })
            .ok_or_else(|| {
                RuntimeError::not_found(format!("media server {}", request.server_id))
            })?;
        let server_paths = server
            .libraries
            .iter()
            .filter(|library| {
                library.enabled && library.name.eq_ignore_ascii_case(&request.library_name)
            })
            .map(|library| library.server_path.clone())
            .collect::<Vec<_>>();
        if server_paths.is_empty() {
            return Err(RuntimeError::not_found(format!(
                "library {:?} on media server {}",
                request.library_name, request.server_id
            )));
        }
        let connection = self.media_server_connection(server).await?;
        let client = media_server_client(server.kind, &loaded.settings);
        let items = client
            .discover_items(
                &connection,
                &request.library_name,
                &server_paths,
                CancellationToken::new(),
            )
            .await?
            .into_iter()
            .map(|item| LibraryCatalogItem {
                id: item.id,
                title: item.title,
                year: item.year,
                media_type: item.media_type,
                has_poster: item.has_poster,
            })
            .collect();
        Ok(LibraryCatalogResponse { items })
    }

    pub async fn get_library_artwork(
        &self,
        request: LibraryArtworkRequest,
    ) -> RuntimeResult<LibraryArtworkResponse> {
        if request.item_id.is_empty()
            || request.item_id.len() > 128
            || !request
                .item_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(RuntimeError::invalid("invalid media-server item id"));
        }
        let loaded = self.settings_service().load().await?;
        let server = loaded
            .settings
            .media_servers
            .iter()
            .find(|server| {
                server
                    .id
                    .to_string()
                    .eq_ignore_ascii_case(&request.server_id)
            })
            .ok_or_else(|| {
                RuntimeError::not_found(format!("media server {}", request.server_id))
            })?;
        let connection = self.media_server_connection(server).await?;
        let artwork = media_server_client(server.kind, &loaded.settings)
            .fetch_artwork(&connection, &request.item_id, CancellationToken::new())
            .await?;
        if !artwork
            .content_type
            .to_ascii_lowercase()
            .starts_with("image/")
        {
            return Err(RuntimeError::invalid(
                "media server returned a non-image poster response",
            ));
        }
        Ok(LibraryArtworkResponse {
            content_type: artwork.content_type,
            data_base64: STANDARD.encode(artwork.bytes),
        })
    }

    pub async fn get_library_local_artwork(
        &self,
        request: LibraryLocalArtworkRequest,
    ) -> RuntimeResult<LibraryArtworkResponse> {
        if request.folder_paths.is_empty() || request.folder_paths.len() > 256 {
            return Err(RuntimeError::invalid(
                "local artwork requires between 1 and 256 folder paths",
            ));
        }

        let mut directories = Vec::<PathBuf>::new();
        for folder_path in request.folder_paths {
            let Ok(folder) = self
                .inner
                .dependencies
                .paths
                .authorize_read(Path::new(&folder_path))
                .await
            else {
                continue;
            };
            if !folder.is_dir() {
                continue;
            }

            // A series audit is grouped by season folder, while poster.jpg is
            // normally stored beside those seasons in the series directory.
            // Prefer that shared series poster, then allow a season poster.
            if is_season_directory(&folder)
                && let Some(parent) = folder.parent()
                && let Ok(parent) = self.inner.dependencies.paths.authorize_read(parent).await
            {
                push_unique_path(&mut directories, parent);
            }
            push_unique_path(&mut directories, folder);
        }

        for directory in directories {
            let Some(candidate) = find_local_poster(&directory).await? else {
                continue;
            };
            let poster = self
                .inner
                .dependencies
                .paths
                .authorize_read(&candidate)
                .await?;
            let metadata = tokio::fs::metadata(&poster).await?;
            if metadata.len() == 0 || metadata.len() > 10 * 1024 * 1024 {
                continue;
            }
            let bytes = tokio::fs::read(&poster).await?;
            return Ok(LibraryArtworkResponse {
                content_type: local_poster_content_type(&poster).to_owned(),
                data_base64: STANDARD.encode(bytes),
            });
        }

        Err(RuntimeError::not_found(
            "no local poster artwork was found for this library title",
        ))
    }

    async fn media_server_connection(
        &self,
        server: &MediaServerSettings,
    ) -> RuntimeResult<MediaServerConnection> {
        let key = server
            .credential
            .secret_reference
            .as_deref()
            .unwrap_or("mediaServer:missing");
        let legacy_key = format!("mediaServer:{}", server.id);
        let credential = self
            .secret_alias(&[key, &legacy_key])
            .await?
            .unwrap_or_default();
        Ok(MediaServerConnection {
            kind: server.kind,
            base_url: server.server_url.clone(),
            credential,
        })
    }

    pub async fn test_media_server_connection(
        &self,
        request: MediaServerConnectionRequest,
    ) -> RuntimeResult<MediaServerTestResponse> {
        let loaded = self.settings_service().load().await?;
        let stored = request.id.as_deref().and_then(|id| {
            loaded
                .settings
                .media_servers
                .iter()
                .find(|server| server.id.to_string().eq_ignore_ascii_case(id))
        });
        let kind = request
            .server_type
            .as_deref()
            .map(parse_server_kind)
            .transpose()?
            .or_else(|| stored.map(|server| server.kind))
            .unwrap_or(MediaServerKind::Jellyfin);
        let requested_url = request
            .server_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if request.api_key.is_none()
            && let (Some(requested_url), Some(stored)) = (requested_url, stored)
            && !media_server_urls_equivalent(requested_url, &stored.server_url)
        {
            return Err(RuntimeError::invalid(
                "Changing a media server URL requires its API key to be entered again.",
            ));
        }
        let url = requested_url
            .map(str::to_owned)
            .or_else(|| stored.map(|server| server.server_url.clone()))
            .ok_or_else(|| RuntimeError::invalid("Enter a server URL."))?;
        validate_media_server_url(&url)?;
        let credential = match request.api_key {
            Some(value) => value,
            None => {
                let key = stored
                    .and_then(|server| server.credential.secret_reference.as_deref())
                    .unwrap_or("media_server.temporary.api_key");
                self.secret_alias(&[key]).await?.unwrap_or_default()
            }
        };
        let client = media_server_client(kind, &loaded.settings);
        let connection = MediaServerConnection {
            kind,
            base_url: url,
            credential,
        };
        match client
            .discover_libraries(&connection, CancellationToken::new())
            .await
        {
            Ok(libraries) => Ok(MediaServerTestResponse {
                success: true,
                status: if libraries.is_empty() {
                    "Connection succeeded, but no library paths were returned.".to_owned()
                } else {
                    format!(
                        "Connection succeeded. Found {} library path(s).",
                        libraries.len()
                    )
                },
                library_count: libraries.len(),
            }),
            Err(error) => Ok(MediaServerTestResponse {
                success: false,
                status: error.to_string(),
                library_count: 0,
            }),
        }
    }

    pub async fn sync_media_server_libraries(
        &self,
        id: &str,
    ) -> RuntimeResult<MediaServerSyncResponse> {
        let loaded = self.settings_service().load().await?;
        let mut settings = loaded.settings;
        let index = settings
            .media_servers
            .iter()
            .position(|server| server.id.to_string().eq_ignore_ascii_case(id))
            .ok_or_else(|| RuntimeError::not_found(format!("media server {id}")))?;
        let server = settings.media_servers[index].clone();
        let key = server
            .credential
            .secret_reference
            .as_deref()
            .unwrap_or("mediaServer:missing");
        let legacy_key = format!("mediaServer:{}", server.id);
        let credential = self
            .secret_alias(&[key, &legacy_key])
            .await?
            .unwrap_or_default();
        let connection = MediaServerConnection {
            kind: server.kind,
            base_url: server.server_url.clone(),
            credential,
        };
        let mut libraries = media_server_client(server.kind, &settings)
            .discover_libraries(&connection, CancellationToken::new())
            .await?;
        resolve_media_server_local_paths(&mut libraries, &self.inner.config.media_root);
        settings.media_servers[index]
            .libraries
            .clone_from(&libraries);
        settings.media_servers[index].last_synced_at = Some(Utc::now());
        let saved = self
            .settings_service()
            .save(mkvo_contracts::SaveSettingsRequest {
                settings,
                secrets: Vec::new(),
                expected_revision: Some(loaded.revision),
            })
            .await?;
        let server = web_media_server(&saved.settings.media_servers[index]);
        let status = format!(
            "Sync complete for {}: {} library path(s).",
            server.name,
            libraries.len()
        );
        Ok(MediaServerSyncResponse {
            libraries: server.libraries.clone(),
            server,
            status,
        })
    }

    /// Render the operation log as plain text for download.
    ///
    /// The host writes the file, so this returns the content and a suggested
    /// name rather than touching the filesystem itself — the browser and the
    /// desktop save it in different ways.
    pub(super) async fn export_logs_impl(&self) -> RuntimeResult<LogExport> {
        let entries = self
            .inner
            .dependencies
            .logs
            .query(&LogQuery::default())
            .await?;
        let mut content = String::new();
        for entry in &entries {
            content.push_str(&format!(
                "{timestamp}\t{level:?}\t{area}\t{correlation}\t{message}",
                timestamp = entry.timestamp_utc.to_rfc3339(),
                level = entry.level,
                area = entry.area,
                correlation = entry.correlation_id,
                message = entry.message,
            ));
            if !entry.detail.trim().is_empty() {
                // Keep one entry on one line so the export stays greppable.
                content.push('\t');
                content.push_str(&entry.detail.replace(['\r', '\n'], " "));
            }
            content.push('\n');
        }
        Ok(LogExport {
            file_name: format!("mkvo-logs-{}.txt", Utc::now().format("%Y%m%d-%H%M%S")),
            entry_count: entries.len(),
            content,
        })
    }

    /// Recent jobs across kinds, newest first, for the Logs and Jobs views.
    pub(super) async fn list_recent_jobs_impl(
        &self,
        limit: Option<usize>,
    ) -> RuntimeResult<RecentJobsResponse> {
        let limit = limit.unwrap_or(50).clamp(1, 500);
        let jobs = self.inner.dependencies.jobs.list_recent(limit).await?;
        Ok(RecentJobsResponse { jobs })
    }
}

const LOCAL_POSTER_NAMES: [&str; 9] = [
    "poster.jpg",
    "poster.jpeg",
    "poster.png",
    "folder.jpg",
    "folder.jpeg",
    "folder.png",
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
];

fn is_season_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let normalized = name.trim().to_ascii_lowercase();
    normalized.strip_prefix("season").is_some_and(|suffix| {
        suffix
            .trim()
            .chars()
            .all(|character| character.is_ascii_digit())
    }) || normalized.strip_prefix('s').is_some_and(|suffix| {
        !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
    })
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|existing| same_path(existing, &candidate)) {
        paths.push(candidate);
    }
}

async fn find_local_poster(directory: &Path) -> RuntimeResult<Option<PathBuf>> {
    let mut matches = HashMap::<String, PathBuf>::new();
    let mut entries = tokio::fs::read_dir(directory).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() {
            matches.insert(
                entry.file_name().to_string_lossy().to_ascii_lowercase(),
                entry.path(),
            );
        }
    }
    Ok(LOCAL_POSTER_NAMES
        .iter()
        .find_map(|name| matches.get(*name).cloned()))
}

fn local_poster_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        _ => "image/jpeg",
    }
}

#[cfg(test)]
mod local_artwork_tests {
    use super::*;

    #[test]
    fn recognizes_common_season_directory_names() {
        assert!(is_season_directory(Path::new("/shows/Example/Season 01")));
        assert!(is_season_directory(Path::new("/shows/Example/S2")));
        assert!(!is_season_directory(Path::new("/shows/Example")));
        assert!(!is_season_directory(Path::new("/movies/Seven")));
    }

    #[tokio::test]
    async fn finds_poster_names_case_insensitively() {
        let directory = tempfile::tempdir().expect("temporary directory");
        tokio::fs::write(directory.path().join("Poster.JPG"), b"poster")
            .await
            .expect("write poster");
        let found = find_local_poster(directory.path())
            .await
            .expect("search")
            .expect("poster");
        assert_eq!(
            found.file_name().and_then(|value| value.to_str()),
            Some("Poster.JPG")
        );
    }
}
