use postgres::types::ToSql;
use rusqlite::params;

use super::{Result, Storage, StorageBackend};

impl Storage {
    pub fn list_app_settings(&self) -> Result<Vec<(String, String)>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(
                    "SELECT key, value
                     FROM app_settings
                     ORDER BY key ASC",
                )?;
                let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
                let mut items = Vec::new();
                for row in rows {
                    items.push(row?);
                }
                Ok(items)
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                let rows = client.query(
                    "SELECT key, value FROM app_settings ORDER BY key ASC",
                    &[],
                )?;
                Ok(rows
                    .into_iter()
                    .map(|row| (row.get(0), row.get(1)))
                    .collect())
            }
        }
    }

    pub fn get_app_setting(&self, key: &str) -> Result<Option<String>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(
                    "SELECT value
                     FROM app_settings
                     WHERE key = ?1
                     LIMIT 1",
                )?;
                let mut rows = stmt.query([key])?;
                if let Some(row) = rows.next()? {
                    return Ok(Some(row.get(0)?));
                }
                Ok(None)
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                let row = client.query_opt(
                    "SELECT value FROM app_settings WHERE key = $1 LIMIT 1",
                    &[&key],
                )?;
                Ok(row.map(|row| row.get(0)))
            }
        }
    }

    pub fn set_app_setting(&self, key: &str, value: &str, updated_at: i64) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "INSERT INTO app_settings (key, value, updated_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(key) DO UPDATE SET
                       value = excluded.value,
                       updated_at = excluded.updated_at",
                    params![key, value, updated_at],
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                client.execute(
                    "INSERT INTO app_settings (key, value, updated_at)
                     VALUES ($1, $2, $3)
                     ON CONFLICT(key) DO UPDATE SET
                       value = EXCLUDED.value,
                       updated_at = EXCLUDED.updated_at",
                    &[&key as &(dyn ToSql + Sync), &value, &updated_at],
                )?;
                Ok(())
            }
        }
    }

    pub fn delete_app_setting(&self, key: &str) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn()
                    .execute("DELETE FROM app_settings WHERE key = ?1", [key])?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                client.execute("DELETE FROM app_settings WHERE key = $1", &[&key])?;
                Ok(())
            }
        }
    }
}
