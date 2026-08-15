use super::*;

impl SqliteStore {
    pub fn cache_count(&self) -> StoreResult<u64> {
        let connection = self.connection()?;
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM media_cache", [], |row| row.get(0))?;
        u64::try_from(count).map_err(|_| StoreError::NumericOverflow)
    }

    pub fn list_media_under(&self, root: &Path) -> StoreResult<Vec<CachedMedia>> {
        let root = path_text(root).trim_end_matches(['/', '\\']).to_owned();
        let escaped_root = escape_like(&root);
        let slash = format!("{escaped_root}/%");
        let backslash = format!("{escaped_root}\\%");
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT file_path, file_size, modified_at_ns, quick_hash, tool_fingerprint,
                    scanned_at_ms, payload_json
             FROM media_cache
             WHERE file_path = ?1 OR file_path LIKE ?2 ESCAPE '^' OR file_path LIKE ?3 ESCAPE '^'
             ORDER BY file_path",
        )?;
        let rows = statement.query_map(params![root, slash, backslash], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (path, size, modified_at_ns, quick_hash, tool_fingerprint, scanned_at_ms, payload) =
                row?;
            Ok(CachedMedia {
                fingerprint: CacheFingerprint {
                    path: PathBuf::from(path),
                    file_size: u64::try_from(size).map_err(|_| StoreError::NumericOverflow)?,
                    modified_at_ns,
                    quick_hash,
                    tool_fingerprint,
                },
                scanned_at_ms,
                payload: serde_json::from_str(&payload)?,
            })
        })
        .collect()
    }

    pub fn get_valid_media(
        &self,
        fingerprint: &CacheFingerprint,
    ) -> StoreResult<Option<CachedMedia>> {
        let path = path_text(&fingerprint.path);
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT file_size, modified_at_ns, quick_hash, tool_fingerprint, scanned_at_ms, payload_json
                 FROM media_cache WHERE file_path = ?1",
                [&path],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((size, modified_at_ns, quick_hash, tool_fingerprint, scanned_at_ms, payload)) =
            row
        else {
            return Ok(None);
        };
        let size_matches = u64::try_from(size).ok() == Some(fingerprint.file_size);
        let quick_hash_matches = fingerprint.quick_hash.is_none()
            || quick_hash.is_none()
            || quick_hash == fingerprint.quick_hash;
        let matches = size_matches
            && modified_at_ns == fingerprint.modified_at_ns
            && quick_hash_matches
            && tool_fingerprint == fingerprint.tool_fingerprint;
        if !matches {
            connection.execute("DELETE FROM media_cache WHERE file_path = ?1", [&path])?;
            return Ok(None);
        }
        Ok(Some(CachedMedia {
            fingerprint: fingerprint.clone(),
            scanned_at_ms,
            payload: serde_json::from_str(&payload)?,
        }))
    }

    pub fn upsert_media(&self, media: &CachedMedia) -> StoreResult<()> {
        let size =
            i64::try_from(media.fingerprint.file_size).map_err(|_| StoreError::NumericOverflow)?;
        let payload = serde_json::to_string(&media.payload)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO media_cache
             (file_path, file_size, modified_at_ns, quick_hash, tool_fingerprint, scanned_at_ms, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(file_path) DO UPDATE SET
               file_size=excluded.file_size,
               modified_at_ns=excluded.modified_at_ns,
               quick_hash=excluded.quick_hash,
               tool_fingerprint=excluded.tool_fingerprint,
               scanned_at_ms=excluded.scanned_at_ms,
               payload_json=excluded.payload_json",
            params![
                path_text(&media.fingerprint.path),
                size,
                media.fingerprint.modified_at_ns,
                media.fingerprint.quick_hash,
                media.fingerprint.tool_fingerprint,
                media.scanned_at_ms,
                payload,
            ],
        )?;
        Ok(())
    }

    pub fn remove_media(&self, path: &Path) -> StoreResult<bool> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "DELETE FROM media_cache WHERE file_path = ?1",
            [path_text(path)],
        )? > 0)
    }

    pub fn remove_media_under(&self, root: &Path) -> StoreResult<u64> {
        let root = path_text(root).trim_end_matches(['/', '\\']).to_owned();
        let escaped_root = escape_like(&root);
        let slash = format!("{escaped_root}/%");
        let backslash = format!("{escaped_root}\\%");
        let connection = self.connection()?;
        let removed = connection.execute(
            "DELETE FROM media_cache WHERE file_path = ?1 OR file_path LIKE ?2 ESCAPE '^' OR file_path LIKE ?3 ESCAPE '^'",
            params![root, slash, backslash],
        )?;
        u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
    }

    pub fn remove_media_older_than(&self, cutoff_ms: i64) -> StoreResult<u64> {
        let connection = self.connection()?;
        let removed = connection.execute(
            "DELETE FROM media_cache WHERE scanned_at_ms < ?1",
            [cutoff_ms],
        )?;
        u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
    }

    pub fn clear_media_cache(&self) -> StoreResult<u64> {
        let connection = self.connection()?;
        let removed = connection.execute("DELETE FROM media_cache", [])?;
        u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
    }
}
