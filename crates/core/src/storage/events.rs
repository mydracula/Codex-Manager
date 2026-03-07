use super::{Event, Result, Storage, StorageBackend};

impl Storage {
    pub fn insert_event(&self, event: &Event) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "INSERT INTO events (account_id, type, message, created_at) VALUES (?1, ?2, ?3, ?4)",
                    (
                        &event.account_id,
                        &event.event_type,
                        &event.message,
                        event.created_at,
                    ),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                client.execute(
                    "INSERT INTO events (account_id, type, message, created_at) VALUES ($1, $2, $3, $4)",
                    &[
                        &event.account_id,
                        &event.event_type,
                        &event.message,
                        &event.created_at,
                    ],
                )?;
                Ok(())
            }
        }
    }

    pub fn event_count(&self) -> Result<i64> {
        match &self.backend {
            StorageBackend::Sqlite(_) => self
                .conn()
                .query_row("SELECT COUNT(1) FROM events", [], |row| row.get(0))
                .map_err(Into::into),
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                let row = client.query_one("SELECT COUNT(1) FROM events", &[])?;
                Ok(row.get(0))
            }
        }
    }
}
