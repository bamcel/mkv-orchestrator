use super::*;

impl SqliteStore {
    pub fn set_setting<T: Serialize>(&self, key: &str, version: u32, value: &T) -> StoreResult<()> {
        let value = serde_json::to_string(value)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO settings (key, version, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET version=excluded.version,
             revision=settings.revision + 1, value_json=excluded.value_json,
             updated_at_ms=excluded.updated_at_ms",
            params![key, version, value, now_ms()],
        )?;
        Ok(())
    }

    pub fn get_setting<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
    ) -> StoreResult<Option<(u32, T)>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT version, value_json FROM settings WHERE key = ?1",
                [key],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        row.map(|(version, json)| Ok((version, serde_json::from_str(&json)?)))
            .transpose()
    }

    pub fn get_setting_with_revision<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
    ) -> StoreResult<Option<(u32, u64, T)>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT version, revision, value_json FROM settings WHERE key = ?1",
                [key],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(version, revision, json)| {
            let revision = u64::try_from(revision).map_err(|_| StoreError::NumericOverflow)?;
            Ok((version, revision, serde_json::from_str(&json)?))
        })
        .transpose()
    }

    pub fn save_setting_optimistic<T: Serialize>(
        &self,
        key: &str,
        version: u32,
        value: &T,
        expected_revision: Option<u64>,
    ) -> StoreResult<u64> {
        let value = serde_json::to_string(value)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let actual: Option<i64> = transaction
            .query_row(
                "SELECT revision FROM settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        let actual = actual
            .map(|revision| u64::try_from(revision).map_err(|_| StoreError::NumericOverflow))
            .transpose()?
            .unwrap_or(0);
        if let Some(expected) = expected_revision {
            if expected != actual {
                return Err(StoreError::RevisionConflict {
                    expected: Some(expected),
                    actual,
                });
            }
        } else if actual != 0 {
            return Err(StoreError::RevisionConflict {
                expected: None,
                actual,
            });
        }
        let next = actual.checked_add(1).ok_or(StoreError::NumericOverflow)?;
        let next_sql = i64::try_from(next).map_err(|_| StoreError::NumericOverflow)?;
        transaction.execute(
            "INSERT INTO settings (key, version, revision, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(key) DO UPDATE SET version=excluded.version, revision=excluded.revision,
             value_json=excluded.value_json, updated_at_ms=excluded.updated_at_ms",
            params![key, version, next_sql, value, now_ms()],
        )?;
        transaction.commit()?;
        Ok(next)
    }
}
