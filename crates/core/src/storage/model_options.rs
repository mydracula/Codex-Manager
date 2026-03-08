use rusqlite::params;

use super::{connect_postgres, ModelOptionsCacheRecord, Result, Storage, StorageBackend};

impl Storage {
    pub fn upsert_model_options_cache(
        &self,
        scope: &str,
        items_json: &str,
        updated_at: i64,
    ) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "INSERT INTO model_options_cache (scope, items_json, updated_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(scope) DO UPDATE SET
                       items_json = excluded.items_json,
                       updated_at = excluded.updated_at",
                    params![scope, items_json, updated_at],
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "INSERT INTO model_options_cache (scope, items_json, updated_at)
                     VALUES ($1, $2, $3)
                     ON CONFLICT(scope) DO UPDATE SET
                       items_json = EXCLUDED.items_json,
                       updated_at = EXCLUDED.updated_at",
                    &[&scope, &items_json, &updated_at],
                )?;
                Ok(())
            }
        }
    }

    pub fn get_model_options_cache(
        &self,
        scope: &str,
    ) -> Result<Option<ModelOptionsCacheRecord>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(
                    "SELECT scope, items_json, updated_at
                     FROM model_options_cache
                     WHERE scope = ?1
                     LIMIT 1",
                )?;
                let mut rows = stmt.query([scope])?;
                if let Some(row) = rows.next()? {
                    return Ok(Some(ModelOptionsCacheRecord {
                        scope: row.get(0)?,
                        items_json: row.get(1)?,
                        updated_at: row.get(2)?,
                    }));
                }
                Ok(None)
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let row = client.query_opt(
                    "SELECT scope, items_json, updated_at
                     FROM model_options_cache
                     WHERE scope = $1
                     LIMIT 1",
                    &[&scope],
                )?;
                Ok(row.map(|row| ModelOptionsCacheRecord {
                    scope: row.get(0),
                    items_json: row.get(1),
                    updated_at: row.get(2),
                }))
            }
        }
    }
}
