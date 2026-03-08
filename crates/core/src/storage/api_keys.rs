use rusqlite::Row;

use super::{connect_postgres, now_ts, ApiKey, Result, Storage, StorageBackend};

const API_KEY_SELECT_SQL: &str = "SELECT
    k.id,
    k.name,
    COALESCE(p.default_model, k.model_slug) AS model_slug,
    COALESCE(p.reasoning_effort, k.reasoning_effort) AS reasoning_effort,
    COALESCE(p.client_type, 'codex') AS client_type,
    COALESCE(p.protocol_type, 'openai_compat') AS protocol_type,
    COALESCE(p.auth_scheme, 'authorization_bearer') AS auth_scheme,
    p.upstream_base_url,
    p.static_headers_json,
    k.key_hash,
    k.status,
    k.created_at,
    k.last_used_at
 FROM api_keys k
 LEFT JOIN api_key_profiles p ON p.key_id = k.id";

impl Storage {
    pub fn insert_api_key(&self, key: &ApiKey) -> Result<()> {
        let updated_at = now_ts();
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "INSERT INTO api_keys (id, name, model_slug, reasoning_effort, key_hash, status, created_at, last_used_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(id) DO UPDATE SET
                       name = excluded.name,
                       model_slug = excluded.model_slug,
                       reasoning_effort = excluded.reasoning_effort,
                       key_hash = excluded.key_hash,
                       status = excluded.status,
                       created_at = excluded.created_at,
                       last_used_at = excluded.last_used_at",
                    (
                        &key.id,
                        &key.name,
                        &key.model_slug,
                        &key.reasoning_effort,
                        &key.key_hash,
                        &key.status,
                        key.created_at,
                        &key.last_used_at,
                    ),
                )?;
                self.conn().execute(
                    "INSERT INTO api_key_profiles (key_id, client_type, protocol_type, auth_scheme, upstream_base_url, static_headers_json, default_model, reasoning_effort, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(key_id) DO UPDATE SET
                       client_type = excluded.client_type,
                       protocol_type = excluded.protocol_type,
                       auth_scheme = excluded.auth_scheme,
                       upstream_base_url = excluded.upstream_base_url,
                       static_headers_json = excluded.static_headers_json,
                       default_model = excluded.default_model,
                       reasoning_effort = excluded.reasoning_effort,
                       updated_at = excluded.updated_at",
                    (
                        &key.id,
                        &key.client_type,
                        &key.protocol_type,
                        &key.auth_scheme,
                        &key.upstream_base_url,
                        &key.static_headers_json,
                        &key.model_slug,
                        &key.reasoning_effort,
                        key.created_at,
                        updated_at,
                    ),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "INSERT INTO api_keys (id, name, model_slug, reasoning_effort, key_hash, status, created_at, last_used_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                     ON CONFLICT(id) DO UPDATE SET
                       name = EXCLUDED.name,
                       model_slug = EXCLUDED.model_slug,
                       reasoning_effort = EXCLUDED.reasoning_effort,
                       key_hash = EXCLUDED.key_hash,
                       status = EXCLUDED.status,
                       created_at = EXCLUDED.created_at,
                       last_used_at = EXCLUDED.last_used_at",
                    &[
                        &key.id,
                        &key.name,
                        &key.model_slug,
                        &key.reasoning_effort,
                        &key.key_hash,
                        &key.status,
                        &key.created_at,
                        &key.last_used_at,
                    ],
                )?;
                client.execute(
                    "INSERT INTO api_key_profiles (key_id, client_type, protocol_type, auth_scheme, upstream_base_url, static_headers_json, default_model, reasoning_effort, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                     ON CONFLICT(key_id) DO UPDATE SET
                       client_type = EXCLUDED.client_type,
                       protocol_type = EXCLUDED.protocol_type,
                       auth_scheme = EXCLUDED.auth_scheme,
                       upstream_base_url = EXCLUDED.upstream_base_url,
                       static_headers_json = EXCLUDED.static_headers_json,
                       default_model = EXCLUDED.default_model,
                       reasoning_effort = EXCLUDED.reasoning_effort,
                       updated_at = EXCLUDED.updated_at",
                    &[
                        &key.id,
                        &key.client_type,
                        &key.protocol_type,
                        &key.auth_scheme,
                        &key.upstream_base_url,
                        &key.static_headers_json,
                        &key.model_slug,
                        &key.reasoning_effort,
                        &key.created_at,
                        &updated_at,
                    ],
                )?;
                Ok(())
            }
        }
    }

    pub fn list_api_keys(&self) -> Result<Vec<ApiKey>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self
                    .conn()
                    .prepare(&format!("{API_KEY_SELECT_SQL} ORDER BY k.created_at DESC"))?;
                let mut rows = stmt.query([])?;
                let mut out = Vec::new();
                while let Some(row) = rows.next()? {
                    out.push(map_api_key_row(row)?);
                }
                Ok(out)
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let rows = client.query(
                    &format!("{API_KEY_SELECT_SQL} ORDER BY k.created_at DESC"),
                    &[],
                )?;
                Ok(rows.into_iter().map(map_api_key_row_pg).collect())
            }
        }
    }

    pub fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(&format!(
                    "{API_KEY_SELECT_SQL}
                     WHERE k.key_hash = ?1
                     LIMIT 1"
                ))?;
                let mut rows = stmt.query([key_hash])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(map_api_key_row(row)?))
                } else {
                    Ok(None)
                }
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let row = client.query_opt(
                    &format!(
                        "{API_KEY_SELECT_SQL}
                         WHERE k.key_hash = $1
                         LIMIT 1"
                    ),
                    &[&key_hash],
                )?;
                Ok(row.map(map_api_key_row_pg))
            }
        }
    }

    pub fn find_api_key_by_id(&self, key_id: &str) -> Result<Option<ApiKey>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(&format!(
                    "{API_KEY_SELECT_SQL}
                     WHERE k.id = ?1
                     LIMIT 1"
                ))?;
                let mut rows = stmt.query([key_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(map_api_key_row(row)?))
                } else {
                    Ok(None)
                }
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let row = client.query_opt(
                    &format!(
                        "{API_KEY_SELECT_SQL}
                         WHERE k.id = $1
                         LIMIT 1"
                    ),
                    &[&key_id],
                )?;
                Ok(row.map(map_api_key_row_pg))
            }
        }
    }

    pub fn update_api_key_last_used(&self, key_hash: &str) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "UPDATE api_keys SET last_used_at = ?1 WHERE key_hash = ?2",
                    (now_ts(), key_hash),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "UPDATE api_keys SET last_used_at = $1 WHERE key_hash = $2",
                    &[&now_ts(), &key_hash],
                )?;
                Ok(())
            }
        }
    }

    pub fn update_api_key_status(&self, key_id: &str, status: &str) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "UPDATE api_keys SET status = ?1 WHERE id = ?2",
                    (status, key_id),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "UPDATE api_keys SET status = $1 WHERE id = $2",
                    &[&status, &key_id],
                )?;
                Ok(())
            }
        }
    }

    pub fn update_api_key_model_slug(&self, key_id: &str, model_slug: Option<&str>) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "UPDATE api_keys SET model_slug = ?1 WHERE id = ?2",
                    (model_slug, key_id),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "UPDATE api_keys SET model_slug = $1 WHERE id = $2",
                    &[&model_slug, &key_id],
                )?;
                Ok(())
            }
        }
    }

    pub fn update_api_key_model_config(
        &self,
        key_id: &str,
        model_slug: Option<&str>,
        reasoning_effort: Option<&str>,
    ) -> Result<()> {
        let now = now_ts();
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "UPDATE api_keys SET model_slug = ?1, reasoning_effort = ?2 WHERE id = ?3",
                    (model_slug, reasoning_effort, key_id),
                )?;
                self.conn().execute(
                    "INSERT INTO api_key_profiles (
                        key_id,
                        client_type,
                        protocol_type,
                        auth_scheme,
                        upstream_base_url,
                        static_headers_json,
                        default_model,
                        reasoning_effort,
                        created_at,
                        updated_at
                    )
                    SELECT
                        id,
                        'codex',
                        'openai_compat',
                        'authorization_bearer',
                        NULL,
                        NULL,
                        ?2,
                        ?3,
                        ?4,
                        ?4
                    FROM api_keys
                    WHERE id = ?1
                    ON CONFLICT(key_id) DO UPDATE SET
                        default_model = excluded.default_model,
                        reasoning_effort = excluded.reasoning_effort,
                        updated_at = excluded.updated_at",
                    (key_id, model_slug, reasoning_effort, now),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "UPDATE api_keys SET model_slug = $1, reasoning_effort = $2 WHERE id = $3",
                    &[&model_slug, &reasoning_effort, &key_id],
                )?;
                client.execute(
                    "INSERT INTO api_key_profiles (
                        key_id,
                        client_type,
                        protocol_type,
                        auth_scheme,
                        upstream_base_url,
                        static_headers_json,
                        default_model,
                        reasoning_effort,
                        created_at,
                        updated_at
                    )
                    SELECT
                        id,
                        'codex',
                        'openai_compat',
                        'authorization_bearer',
                        NULL,
                        NULL,
                        $2,
                        $3,
                        $4,
                        $4
                    FROM api_keys
                    WHERE id = $1
                    ON CONFLICT(key_id) DO UPDATE SET
                        default_model = EXCLUDED.default_model,
                        reasoning_effort = EXCLUDED.reasoning_effort,
                        updated_at = EXCLUDED.updated_at",
                    &[&key_id, &model_slug, &reasoning_effort, &now],
                )?;
                Ok(())
            }
        }
    }

    pub fn update_api_key_profile_config(
        &self,
        key_id: &str,
        client_type: &str,
        protocol_type: &str,
        auth_scheme: &str,
        upstream_base_url: Option<&str>,
        static_headers_json: Option<&str>,
    ) -> Result<()> {
        let now = now_ts();
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "INSERT INTO api_key_profiles (
                        key_id,
                        client_type,
                        protocol_type,
                        auth_scheme,
                        upstream_base_url,
                        static_headers_json,
                        default_model,
                        reasoning_effort,
                        created_at,
                        updated_at
                    )
                    SELECT
                        id,
                        ?2,
                        ?3,
                        ?4,
                        ?5,
                        ?6,
                        model_slug,
                        reasoning_effort,
                        created_at,
                        ?7
                    FROM api_keys
                    WHERE id = ?1
                    ON CONFLICT(key_id) DO UPDATE SET
                        client_type = excluded.client_type,
                        protocol_type = excluded.protocol_type,
                        auth_scheme = excluded.auth_scheme,
                        upstream_base_url = excluded.upstream_base_url,
                        static_headers_json = excluded.static_headers_json,
                        updated_at = excluded.updated_at",
                    (
                        key_id,
                        client_type,
                        protocol_type,
                        auth_scheme,
                        upstream_base_url,
                        static_headers_json,
                        now,
                    ),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "INSERT INTO api_key_profiles (
                        key_id,
                        client_type,
                        protocol_type,
                        auth_scheme,
                        upstream_base_url,
                        static_headers_json,
                        default_model,
                        reasoning_effort,
                        created_at,
                        updated_at
                    )
                    SELECT
                        id,
                        $2,
                        $3,
                        $4,
                        $5,
                        $6,
                        model_slug,
                        reasoning_effort,
                        created_at,
                        $7
                    FROM api_keys
                    WHERE id = $1
                    ON CONFLICT(key_id) DO UPDATE SET
                        client_type = EXCLUDED.client_type,
                        protocol_type = EXCLUDED.protocol_type,
                        auth_scheme = EXCLUDED.auth_scheme,
                        upstream_base_url = EXCLUDED.upstream_base_url,
                        static_headers_json = EXCLUDED.static_headers_json,
                        updated_at = EXCLUDED.updated_at",
                    &[
                        &key_id,
                        &client_type,
                        &protocol_type,
                        &auth_scheme,
                        &upstream_base_url,
                        &static_headers_json,
                        &now,
                    ],
                )?;
                Ok(())
            }
        }
    }

    pub fn delete_api_key(&self, key_id: &str) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn()
                    .execute("DELETE FROM api_key_secrets WHERE key_id = ?1", [key_id])?;
                self.conn()
                    .execute("DELETE FROM api_keys WHERE id = ?1", [key_id])?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute("DELETE FROM api_key_secrets WHERE key_id = $1", &[&key_id])?;
                client.execute("DELETE FROM api_keys WHERE id = $1", &[&key_id])?;
                Ok(())
            }
        }
    }

    pub fn upsert_api_key_secret(&self, key_id: &str, key_value: &str) -> Result<()> {
        let now = now_ts();
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "INSERT INTO api_key_secrets (key_id, key_value, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?3)
                     ON CONFLICT(key_id) DO UPDATE SET
                       key_value = excluded.key_value,
                       updated_at = excluded.updated_at",
                    (key_id, key_value, now),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "INSERT INTO api_key_secrets (key_id, key_value, created_at, updated_at)
                     VALUES ($1, $2, $3, $3)
                     ON CONFLICT(key_id) DO UPDATE SET
                       key_value = EXCLUDED.key_value,
                       updated_at = EXCLUDED.updated_at",
                    &[&key_id, &key_value, &now],
                )?;
                Ok(())
            }
        }
    }

    pub fn find_api_key_secret_by_id(&self, key_id: &str) -> Result<Option<String>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self
                    .conn()
                    .prepare("SELECT key_value FROM api_key_secrets WHERE key_id = ?1 LIMIT 1")?;
                let mut rows = stmt.query([key_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row.get(0)?))
                } else {
                    Ok(None)
                }
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let row = client.query_opt(
                    "SELECT key_value FROM api_key_secrets WHERE key_id = $1 LIMIT 1",
                    &[&key_id],
                )?;
                Ok(row.map(|row| row.get(0)))
            }
        }
    }

    pub(super) fn ensure_api_key_model_column(&self) -> Result<()> {
        self.ensure_column("api_keys", "model_slug", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_api_key_reasoning_column(&self) -> Result<()> {
        self.ensure_column("api_keys", "reasoning_effort", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_api_key_profiles_table(&self) -> Result<()> {
        self.conn().execute(
            "CREATE TABLE IF NOT EXISTS api_key_profiles (
                key_id TEXT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
                client_type TEXT NOT NULL CHECK (client_type IN ('codex', 'claude_code')),
                protocol_type TEXT NOT NULL CHECK (protocol_type IN ('openai_compat', 'anthropic_native', 'azure_openai')),
                auth_scheme TEXT NOT NULL CHECK (auth_scheme IN ('authorization_bearer', 'x_api_key', 'api_key')),
                upstream_base_url TEXT,
                static_headers_json TEXT,
                default_model TEXT,
                reasoning_effort TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn().execute(
            "CREATE INDEX IF NOT EXISTS idx_api_key_profiles_client_protocol ON api_key_profiles(client_type, protocol_type)",
            [],
        )?;
        self.backfill_api_key_profiles()
    }

    pub(super) fn ensure_api_key_secrets_table(&self) -> Result<()> {
        self.conn().execute(
            "CREATE TABLE IF NOT EXISTS api_key_secrets (
                key_id TEXT PRIMARY KEY,
                key_value TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn().execute(
            "CREATE INDEX IF NOT EXISTS idx_api_key_secrets_updated_at ON api_key_secrets(updated_at)",
            [],
        )?;
        Ok(())
    }

    fn backfill_api_key_profiles(&self) -> Result<()> {
        self.conn().execute(
            "INSERT INTO api_key_profiles (
                key_id,
                client_type,
                protocol_type,
                auth_scheme,
                upstream_base_url,
                static_headers_json,
                default_model,
                reasoning_effort,
                created_at,
                updated_at
            )
            SELECT
                id,
                'codex',
                'openai_compat',
                'authorization_bearer',
                NULL,
                NULL,
                model_slug,
                reasoning_effort,
                created_at,
                created_at
            FROM api_keys
            ON CONFLICT(key_id) DO NOTHING",
            [],
        )?;
        Ok(())
    }
}

fn map_api_key_row(row: &Row<'_>) -> Result<ApiKey> {
    Ok(ApiKey {
        id: row.get(0)?,
        name: row.get(1)?,
        model_slug: row.get(2)?,
        reasoning_effort: row.get(3)?,
        client_type: row.get(4)?,
        protocol_type: row.get(5)?,
        auth_scheme: row.get(6)?,
        upstream_base_url: row.get(7)?,
        static_headers_json: row.get(8)?,
        key_hash: row.get(9)?,
        status: row.get(10)?,
        created_at: row.get(11)?,
        last_used_at: row.get(12)?,
    })
}

fn map_api_key_row_pg(row: postgres::Row) -> ApiKey {
    ApiKey {
        id: row.get(0),
        name: row.get(1),
        model_slug: row.get(2),
        reasoning_effort: row.get(3),
        client_type: row.get(4),
        protocol_type: row.get(5),
        auth_scheme: row.get(6),
        upstream_base_url: row.get(7),
        static_headers_json: row.get(8),
        key_hash: row.get(9),
        status: row.get(10),
        created_at: row.get(11),
        last_used_at: row.get(12),
    }
}
