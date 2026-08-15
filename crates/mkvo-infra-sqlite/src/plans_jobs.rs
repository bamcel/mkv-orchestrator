use super::*;

impl SqliteStore {
    pub fn put_plan(&self, plan: &StoredPlan) -> StoreResult<()> {
        let payload = serde_json::to_string(&plan.payload)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO plans (id, kind, version, fingerprint, payload_json, created_at_ms, expires_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, version=excluded.version,
             fingerprint=excluded.fingerprint, payload_json=excluded.payload_json,
             created_at_ms=excluded.created_at_ms, expires_at_ms=excluded.expires_at_ms",
            params![
                plan.id,
                plan.kind,
                plan.version,
                plan.fingerprint,
                payload,
                plan.created_at_ms,
                plan.expires_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn get_plan(&self, id: &str, at_ms: i64) -> StoreResult<Option<StoredPlan>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT kind, version, fingerprint, payload_json, created_at_ms, expires_at_ms
                 FROM plans WHERE id = ?1 AND (expires_at_ms IS NULL OR expires_at_ms > ?2)",
                params![id, at_ms],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(kind, version, fingerprint, payload, created_at_ms, expires_at_ms)| {
                Ok(StoredPlan {
                    id: id.to_owned(),
                    kind,
                    version,
                    fingerprint,
                    payload: serde_json::from_str(&payload)?,
                    created_at_ms,
                    expires_at_ms,
                })
            },
        )
        .transpose()
    }

    pub fn upsert_job(&self, job: &StoredJob) -> StoreResult<()> {
        let request = serde_json::to_string(&job.request)?;
        let result = job.result.as_ref().map(serde_json::to_string).transpose()?;
        let error = job.error.as_ref().map(serde_json::to_string).transpose()?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO jobs
             (id, kind, state, request_json, result_json, idempotency_key, error_json, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, state=excluded.state,
             request_json=excluded.request_json, result_json=excluded.result_json,
             idempotency_key=excluded.idempotency_key, error_json=excluded.error_json,
             updated_at_ms=excluded.updated_at_ms",
            params![
                job.id,
                job.kind,
                job.state,
                request,
                result,
                job.idempotency_key,
                error,
                job.created_at_ms,
                job.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> StoreResult<Option<StoredJob>> {
        self.query_job("id", id)
    }

    pub fn list_recent_jobs(&self, limit: usize) -> StoreResult<Vec<StoredJob>> {
        let limit = i64::try_from(limit).map_err(|_| StoreError::NumericOverflow)?;
        let connection = self.connection()?;
        let mut statement =
            connection.prepare("SELECT id FROM jobs ORDER BY updated_at_ms DESC LIMIT ?1")?;
        let ids = statement
            .query_map([limit], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        drop(connection);
        ids.into_iter()
            .map(|id| {
                self.get_job(&id)?
                    .ok_or_else(|| StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
            })
            .collect()
    }

    pub fn find_job_by_idempotency_key(&self, key: &str) -> StoreResult<Option<StoredJob>> {
        self.query_job("idempotency_key", key)
    }

    fn query_job(&self, column: &str, value: &str) -> StoreResult<Option<StoredJob>> {
        debug_assert!(matches!(column, "id" | "idempotency_key"));
        let sql = format!(
            "SELECT id, kind, state, request_json, result_json, idempotency_key, error_json, created_at_ms, updated_at_ms FROM jobs WHERE {column} = ?1"
        );
        let connection = self.connection()?;
        let row = connection
            .query_row(&sql, [value], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })
            .optional()?;
        row.map(
            |(
                id,
                kind,
                state,
                request,
                result,
                idempotency_key,
                error,
                created_at_ms,
                updated_at_ms,
            )| {
                Ok(StoredJob {
                    id,
                    kind,
                    state,
                    request: serde_json::from_str(&request)?,
                    result: result.map(|json| serde_json::from_str(&json)).transpose()?,
                    idempotency_key,
                    error: error.map(|json| serde_json::from_str(&json)).transpose()?,
                    created_at_ms,
                    updated_at_ms,
                })
            },
        )
        .transpose()
    }
}
