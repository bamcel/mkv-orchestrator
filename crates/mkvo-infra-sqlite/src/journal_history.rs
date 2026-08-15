use super::*;

impl SqliteStore {
    pub fn put_journal_entry(&self, entry: &JournalEntry) -> StoreResult<()> {
        let payload = serde_json::to_string(&entry.payload)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO operation_journal
             (job_id, step_index, step_kind, state, payload_json, started_at_ms, completed_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(job_id, step_index) DO UPDATE SET step_kind=excluded.step_kind,
             state=excluded.state, payload_json=excluded.payload_json,
             started_at_ms=excluded.started_at_ms, completed_at_ms=excluded.completed_at_ms",
            params![
                entry.job_id,
                entry.step_index,
                entry.step_kind,
                entry.state,
                payload,
                entry.started_at_ms,
                entry.completed_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn journal_for_job(&self, job_id: &str) -> StoreResult<Vec<JournalEntry>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT step_index, step_kind, state, payload_json, started_at_ms, completed_at_ms
             FROM operation_journal WHERE job_id = ?1 ORDER BY step_index",
        )?;
        let rows = statement.query_map([job_id], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (step_index, step_kind, state, payload, started_at_ms, completed_at_ms) = row?;
            Ok(JournalEntry {
                job_id: job_id.to_owned(),
                step_index,
                step_kind,
                state,
                payload: serde_json::from_str(&payload)?,
                started_at_ms,
                completed_at_ms,
            })
        })
        .collect()
    }

    pub fn record_rename_batch(&self, batch: &RenameBatch) -> StoreResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO rename_batches (id, created_at_ms, undone_at_ms, provider, template, total_files)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET created_at_ms=excluded.created_at_ms,
             undone_at_ms=excluded.undone_at_ms, provider=excluded.provider,
             template=excluded.template, total_files=excluded.total_files",
            params![
                batch.id,
                batch.created_at_ms,
                batch.undone_at_ms,
                batch.provider,
                batch.template,
                batch.entries.len(),
            ],
        )?;
        transaction.execute(
            "DELETE FROM rename_entries WHERE batch_id = ?1",
            [&batch.id],
        )?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO rename_entries (batch_id, position, original_path, renamed_path) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (position, entry) in batch.entries.iter().enumerate() {
                insert.execute(params![
                    batch.id,
                    position,
                    path_text(&entry.original_path),
                    path_text(&entry.renamed_path),
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_rename_batches(&self, limit: usize) -> StoreResult<Vec<RenameBatch>> {
        let limit = i64::try_from(limit).map_err(|_| StoreError::NumericOverflow)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, created_at_ms, undone_at_ms, provider, template
             FROM rename_batches ORDER BY created_at_ms DESC LIMIT ?1",
        )?;
        let batch_rows = statement.query_map([limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut batches = Vec::new();
        for row in batch_rows {
            let (id, created_at_ms, undone_at_ms, provider, template) = row?;
            let entries = load_rename_entries(&connection, &id)?;
            batches.push(RenameBatch {
                id,
                created_at_ms,
                undone_at_ms,
                provider,
                template,
                entries,
            });
        }
        Ok(batches)
    }

    pub fn mark_rename_batch_undone(&self, id: &str, undone_at_ms: i64) -> StoreResult<bool> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "UPDATE rename_batches SET undone_at_ms = ?2 WHERE id = ?1 AND undone_at_ms IS NULL",
            params![id, undone_at_ms],
        )? > 0)
    }

    pub fn clear_rename_batches(&self) -> StoreResult<u64> {
        let connection = self.connection()?;
        let removed = connection.execute("DELETE FROM rename_batches", [])?;
        u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
    }
}
