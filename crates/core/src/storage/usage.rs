use rusqlite::Row;

use super::{Result, Storage, StorageBackend, UsageSnapshotRecord};

impl Storage {
    pub fn insert_usage_snapshot(&self, snap: &UsageSnapshotRecord) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "INSERT INTO usage_snapshots (account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    (
                        &snap.account_id,
                        snap.used_percent,
                        snap.window_minutes,
                        snap.resets_at,
                        snap.secondary_used_percent,
                        snap.secondary_window_minutes,
                        snap.secondary_resets_at,
                        &snap.credits_json,
                        snap.captured_at,
                    ),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                client.execute(
                    "INSERT INTO usage_snapshots (account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                    &[
                        &snap.account_id,
                        &snap.used_percent,
                        &snap.window_minutes,
                        &snap.resets_at,
                        &snap.secondary_used_percent,
                        &snap.secondary_window_minutes,
                        &snap.secondary_resets_at,
                        &snap.credits_json,
                        &snap.captured_at,
                    ],
                )?;
                Ok(())
            }
        }
    }

    pub fn prune_usage_snapshots_for_account(
        &self,
        account_id: &str,
        retain: usize,
    ) -> Result<usize> {
        if retain == 0 {
            return Ok(0);
        }
        match &self.backend {
            StorageBackend::Sqlite(_) => self
                .conn()
                .execute(
                    "DELETE FROM usage_snapshots
                     WHERE account_id = ?1
                       AND id NOT IN (
                         SELECT id
                         FROM usage_snapshots
                         WHERE account_id = ?1
                         ORDER BY captured_at DESC, id DESC
                         LIMIT ?2
                       )",
                    (account_id, retain as i64),
                )
                .map_err(Into::into),
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                let deleted = client.execute(
                    "DELETE FROM usage_snapshots
                     WHERE account_id = $1
                       AND id NOT IN (
                         SELECT id
                         FROM usage_snapshots
                         WHERE account_id = $1
                         ORDER BY captured_at DESC, id DESC
                         LIMIT $2
                       )",
                    &[&account_id, &(retain as i64)],
                )?;
                Ok(deleted as usize)
            }
        }
    }

    pub fn usage_snapshot_count_for_account(&self, account_id: &str) -> Result<i64> {
        match &self.backend {
            StorageBackend::Sqlite(_) => self
                .conn()
                .query_row(
                    "SELECT COUNT(1) FROM usage_snapshots WHERE account_id = ?1",
                    [account_id],
                    |row| row.get(0),
                )
                .map_err(Into::into),
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                let row = client.query_one(
                    "SELECT COUNT(1) FROM usage_snapshots WHERE account_id = $1",
                    &[&account_id],
                )?;
                Ok(row.get(0))
            }
        }
    }

    pub fn latest_usage_snapshot(&self) -> Result<Option<UsageSnapshotRecord>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(
                    "SELECT account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at FROM usage_snapshots ORDER BY captured_at DESC, id DESC LIMIT 1",
                )?;
                let mut rows = stmt.query([])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(map_usage_snapshot_row(row)?))
                } else {
                    Ok(None)
                }
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                let row = client.query_opt(
                    "SELECT account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at
                     FROM usage_snapshots
                     ORDER BY captured_at DESC, id DESC
                     LIMIT 1",
                    &[],
                )?;
                Ok(row.map(map_usage_snapshot_row_pg))
            }
        }
    }

    pub fn latest_usage_snapshot_for_account(
        &self,
        account_id: &str,
    ) -> Result<Option<UsageSnapshotRecord>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(
                    "SELECT account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at
                     FROM usage_snapshots
                     WHERE account_id = ?1
                     ORDER BY captured_at DESC, id DESC
                     LIMIT 1",
                )?;
                let mut rows = stmt.query([account_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(map_usage_snapshot_row(row)?))
                } else {
                    Ok(None)
                }
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                let row = client.query_opt(
                    "SELECT account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at
                     FROM usage_snapshots
                     WHERE account_id = $1
                     ORDER BY captured_at DESC, id DESC
                     LIMIT 1",
                    &[&account_id],
                )?;
                Ok(row.map(map_usage_snapshot_row_pg))
            }
        }
    }

    pub fn latest_usage_snapshots_by_account(&self) -> Result<Vec<UsageSnapshotRecord>> {
        let sql = "WITH ranked AS (
                SELECT
                    id,
                    account_id,
                    used_percent,
                    window_minutes,
                    resets_at,
                    secondary_used_percent,
                    secondary_window_minutes,
                    secondary_resets_at,
                    credits_json,
                    captured_at,
                    ROW_NUMBER() OVER (
                        PARTITION BY account_id
                        ORDER BY captured_at DESC, id DESC
                    ) AS rn
                FROM usage_snapshots
            )
            SELECT
                account_id,
                used_percent,
                window_minutes,
                resets_at,
                secondary_used_percent,
                secondary_window_minutes,
                secondary_resets_at,
                credits_json,
                captured_at
            FROM ranked
            WHERE rn = 1
            ORDER BY captured_at DESC, id DESC";
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(sql)?;
                let mut rows = stmt.query([])?;
                let mut out = Vec::new();
                while let Some(row) = rows.next()? {
                    out.push(map_usage_snapshot_row(row)?);
                }
                Ok(out)
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = postgres::Client::connect(url, postgres::NoTls)?;
                let rows = client.query(sql, &[])?;
                Ok(rows.into_iter().map(map_usage_snapshot_row_pg).collect())
            }
        }
    }

    pub(super) fn ensure_usage_secondary_columns(&self) -> Result<()> {
        self.ensure_column("usage_snapshots", "secondary_used_percent", "REAL")?;
        self.ensure_column("usage_snapshots", "secondary_window_minutes", "INTEGER")?;
        self.ensure_column("usage_snapshots", "secondary_resets_at", "INTEGER")?;
        Ok(())
    }
}

fn map_usage_snapshot_row(row: &Row<'_>) -> Result<UsageSnapshotRecord> {
    Ok(UsageSnapshotRecord {
        account_id: row.get(0)?,
        used_percent: row.get(1)?,
        window_minutes: row.get(2)?,
        resets_at: row.get(3)?,
        secondary_used_percent: row.get(4)?,
        secondary_window_minutes: row.get(5)?,
        secondary_resets_at: row.get(6)?,
        credits_json: row.get(7)?,
        captured_at: row.get(8)?,
    })
}

fn map_usage_snapshot_row_pg(row: postgres::Row) -> UsageSnapshotRecord {
    UsageSnapshotRecord {
        account_id: row.get(0),
        used_percent: row.get(1),
        window_minutes: row.get(2),
        resets_at: row.get(3),
        secondary_used_percent: row.get(4),
        secondary_window_minutes: row.get(5),
        secondary_resets_at: row.get(6),
        credits_json: row.get(7),
        captured_at: row.get(8),
    }
}
