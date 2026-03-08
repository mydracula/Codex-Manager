use postgres::types::ToSql;
use rusqlite::{params_from_iter, types::Value, Row};

use super::{connect_postgres, now_ts, Account, Result, Storage, StorageBackend, Token};

#[derive(Clone, Copy)]
enum AccountUsageQueryMode {
    ActiveAvailable,
    LowQuota,
}

impl Storage {
    pub fn insert_account(&self, account: &Account) -> Result<()> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "INSERT INTO accounts (id, label, issuer, chatgpt_account_id, workspace_id, group_name, sort, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(id) DO UPDATE SET
                       label = excluded.label,
                       issuer = excluded.issuer,
                       chatgpt_account_id = excluded.chatgpt_account_id,
                       workspace_id = excluded.workspace_id,
                       group_name = excluded.group_name,
                       sort = excluded.sort,
                       status = excluded.status,
                       created_at = excluded.created_at,
                       updated_at = excluded.updated_at",
                    (
                        &account.id,
                        &account.label,
                        &account.issuer,
                        &account.chatgpt_account_id,
                        &account.workspace_id,
                        &account.group_name,
                        account.sort,
                        &account.status,
                        account.created_at,
                        account.updated_at,
                    ),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "INSERT INTO accounts (id, label, issuer, chatgpt_account_id, workspace_id, group_name, sort, status, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                     ON CONFLICT(id) DO UPDATE SET
                       label = EXCLUDED.label,
                       issuer = EXCLUDED.issuer,
                       chatgpt_account_id = EXCLUDED.chatgpt_account_id,
                       workspace_id = EXCLUDED.workspace_id,
                       group_name = EXCLUDED.group_name,
                       sort = EXCLUDED.sort,
                       status = EXCLUDED.status,
                       created_at = EXCLUDED.created_at,
                       updated_at = EXCLUDED.updated_at",
                    &[
                        &account.id,
                        &account.label,
                        &account.issuer,
                        &account.chatgpt_account_id,
                        &account.workspace_id,
                        &account.group_name,
                        &account.sort,
                        &account.status,
                        &account.created_at,
                        &account.updated_at,
                    ],
                )?;
                Ok(())
            }
        }
    }

    pub fn account_count(&self) -> Result<i64> {
        match &self.backend {
            StorageBackend::Sqlite(_) => self
                .conn()
                .query_row("SELECT COUNT(1) FROM accounts", [], |row| row.get(0))
                .map_err(Into::into),
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let row = client.query_one("SELECT COUNT(1) FROM accounts", &[])?;
                Ok(row.get(0))
            }
        }
    }

    pub fn account_count_filtered(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
    ) -> Result<i64> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut params = Vec::new();
                let where_clause =
                    build_account_where_clause(query, group_name, &mut params, "accounts");
                let sql = format!("SELECT COUNT(1) FROM accounts{where_clause}");
                self.conn()
                    .query_row(&sql, params_from_iter(params), |row| row.get(0))
                    .map_err(Into::into)
            }
            StorageBackend::PostgresUrl(url) => {
                let (where_clause, params) =
                    build_account_where_clause_pg(query, group_name, "accounts", 1);
                let sql = format!("SELECT COUNT(1) FROM accounts{where_clause}");
                let refs = pg_param_refs(&params);
                let mut client = connect_postgres(url)?;
                let row = client.query_one(&sql, &refs)?;
                Ok(row.get(0))
            }
        }
    }

    pub fn account_count_active_available(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
    ) -> Result<i64> {
        self.count_accounts_with_usage_mode(
            query,
            group_name,
            AccountUsageQueryMode::ActiveAvailable,
        )
    }

    pub fn account_count_low_quota(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
    ) -> Result<i64> {
        self.count_accounts_with_usage_mode(query, group_name, AccountUsageQueryMode::LowQuota)
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        self.list_accounts_filtered(None, None)
    }

    pub fn list_accounts_filtered(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
    ) -> Result<Vec<Account>> {
        self.query_accounts(query, group_name, None)
    }

    pub fn list_accounts_paginated(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Account>> {
        self.query_accounts(query, group_name, Some((offset, limit)))
    }

    pub fn list_accounts_active_available(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
        pagination: Option<(i64, i64)>,
    ) -> Result<Vec<Account>> {
        self.query_accounts_with_usage_mode(
            query,
            group_name,
            AccountUsageQueryMode::ActiveAvailable,
            pagination,
        )
    }

    pub fn list_accounts_low_quota(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
        pagination: Option<(i64, i64)>,
    ) -> Result<Vec<Account>> {
        self.query_accounts_with_usage_mode(
            query,
            group_name,
            AccountUsageQueryMode::LowQuota,
            pagination,
        )
    }

    pub fn list_gateway_candidates(&self) -> Result<Vec<(Account, Token)>> {
        let sql = format!(
            "{latest_usage_cte}
             SELECT
               {account_select},
               {token_select}
             FROM accounts a
             JOIN tokens t
               ON t.account_id = a.id
             LEFT JOIN latest_usage lu
               ON lu.account_id = a.id
              AND lu.rn = 1
             WHERE LOWER(TRIM(COALESCE(a.status, ''))) = 'active'
               AND ({gateway_available_clause})
             ORDER BY a.sort ASC, a.updated_at DESC",
            latest_usage_cte = latest_usage_cte_sql(),
            account_select = account_select_columns("a"),
            token_select = token_select_columns("t"),
            gateway_available_clause = gateway_available_usage_clause("lu"),
        );

        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(&sql)?;
                let mut rows = stmt.query([])?;
                let mut out = Vec::new();
                while let Some(row) = rows.next()? {
                    out.push(map_gateway_candidate_row(row)?);
                }
                Ok(out)
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let rows = client.query(&sql, &[])?;
                Ok(rows.into_iter().map(map_gateway_candidate_row_pg).collect())
            }
        }
    }

    pub fn find_account_by_id(&self, account_id: &str) -> Result<Option<Account>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut stmt = self.conn().prepare(
                    "SELECT id, label, issuer, chatgpt_account_id, workspace_id, group_name, sort, status, created_at, updated_at
                     FROM accounts
                     WHERE id = ?1
                     LIMIT 1",
                )?;
                let mut rows = stmt.query([account_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(map_account_row(row)?))
                } else {
                    Ok(None)
                }
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let row = client.query_opt(
                    "SELECT id, label, issuer, chatgpt_account_id, workspace_id, group_name, sort, status, created_at, updated_at
                     FROM accounts
                     WHERE id = $1
                     LIMIT 1",
                    &[&account_id],
                )?;
                Ok(row.map(map_account_row_pg))
            }
        }
    }

    pub fn update_account_sort(&self, account_id: &str, sort: i64) -> Result<()> {
        let updated_at = now_ts();
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "UPDATE accounts SET sort = ?1, updated_at = ?2 WHERE id = ?3",
                    (sort, updated_at, account_id),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "UPDATE accounts SET sort = $1, updated_at = $2 WHERE id = $3",
                    &[&sort, &updated_at, &account_id],
                )?;
                Ok(())
            }
        }
    }

    pub fn update_account_status(&self, account_id: &str, status: &str) -> Result<()> {
        let updated_at = now_ts();
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                self.conn().execute(
                    "UPDATE accounts SET status = ?1, updated_at = ?2 WHERE id = ?3",
                    (status, updated_at, account_id),
                )?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                client.execute(
                    "UPDATE accounts SET status = $1, updated_at = $2 WHERE id = $3",
                    &[&status, &updated_at, &account_id],
                )?;
                Ok(())
            }
        }
    }

    pub fn update_account_status_if_changed(&self, account_id: &str, status: &str) -> Result<bool> {
        let updated_at = now_ts();
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let updated = self.conn().execute(
                    "UPDATE accounts SET status = ?1, updated_at = ?2 WHERE id = ?3 AND status != ?1",
                    (status, updated_at, account_id),
                )?;
                Ok(updated > 0)
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let updated = client.execute(
                    "UPDATE accounts SET status = $1, updated_at = $2 WHERE id = $3 AND status != $1",
                    &[&status, &updated_at, &account_id],
                )?;
                Ok(updated > 0)
            }
        }
    }

    pub fn delete_account(&mut self, account_id: &str) -> Result<()> {
        match &mut self.backend {
            StorageBackend::Sqlite(conn) => {
                let tx = conn.transaction()?;
                tx.execute("DELETE FROM tokens WHERE account_id = ?1", [account_id])?;
                tx.execute(
                    "DELETE FROM usage_snapshots WHERE account_id = ?1",
                    [account_id],
                )?;
                tx.execute("DELETE FROM events WHERE account_id = ?1", [account_id])?;
                tx.execute("DELETE FROM accounts WHERE id = ?1", [account_id])?;
                tx.commit()?;
                Ok(())
            }
            StorageBackend::PostgresUrl(url) => {
                let mut client = connect_postgres(url)?;
                let mut tx = client.transaction()?;
                tx.execute("DELETE FROM tokens WHERE account_id = $1", &[&account_id])?;
                tx.execute("DELETE FROM usage_snapshots WHERE account_id = $1", &[&account_id])?;
                tx.execute("DELETE FROM events WHERE account_id = $1", &[&account_id])?;
                tx.execute("DELETE FROM accounts WHERE id = $1", &[&account_id])?;
                tx.commit()?;
                Ok(())
            }
        }
    }

    pub(super) fn ensure_account_meta_columns(&self) -> Result<()> {
        self.ensure_column("accounts", "chatgpt_account_id", "TEXT")?;
        self.ensure_column("accounts", "group_name", "TEXT")?;
        self.ensure_column("accounts", "sort", "INTEGER DEFAULT 0")?;
        self.ensure_column("login_sessions", "note", "TEXT")?;
        self.ensure_column("login_sessions", "tags", "TEXT")?;
        self.ensure_column("login_sessions", "group_name", "TEXT")?;
        Ok(())
    }

    fn query_accounts(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
        pagination: Option<(i64, i64)>,
    ) -> Result<Vec<Account>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut params = Vec::new();
                let where_clause = build_account_where_clause(query, group_name, &mut params, "a");
                let mut sql = format!(
                    "SELECT {} FROM accounts a{where_clause} ORDER BY a.sort ASC, a.updated_at DESC",
                    account_select_columns("a"),
                );

                if let Some((offset, limit)) = pagination {
                    sql.push_str(" LIMIT ? OFFSET ?");
                    params.push(Value::Integer(limit.max(1)));
                    params.push(Value::Integer(offset.max(0)));
                }

                let mut stmt = self.conn().prepare(&sql)?;
                let mut rows = stmt.query(params_from_iter(params))?;
                let mut out = Vec::new();
                while let Some(row) = rows.next()? {
                    out.push(map_account_row(row)?);
                }
                Ok(out)
            }
            StorageBackend::PostgresUrl(url) => {
                let (where_clause, mut params) =
                    build_account_where_clause_pg(query, group_name, "a", 1);
                let mut sql = format!(
                    "SELECT {} FROM accounts a{where_clause} ORDER BY a.sort ASC, a.updated_at DESC",
                    account_select_columns("a"),
                );

                if let Some((offset, limit)) = pagination {
                    sql.push_str(&format!(
                        " LIMIT ${} OFFSET ${}",
                        params.len() + 1,
                        params.len() + 2
                    ));
                    params.push(Box::new(limit.max(1)));
                    params.push(Box::new(offset.max(0)));
                }

                let refs = pg_param_refs(&params);
                let mut client = connect_postgres(url)?;
                let rows = client.query(&sql, &refs)?;
                Ok(rows.into_iter().map(map_account_row_pg).collect())
            }
        }
    }

    fn query_accounts_with_usage_mode(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
        mode: AccountUsageQueryMode,
        pagination: Option<(i64, i64)>,
    ) -> Result<Vec<Account>> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut params = Vec::new();
                let mut where_clause =
                    build_account_where_clause(query, group_name, &mut params, "a");
                append_where_clause(
                    &mut where_clause,
                    account_usage_filter_clause(mode, "a", "lu").as_str(),
                );
                let mut sql = format!(
                    "{latest_usage_cte}
                     SELECT {account_select}
                     FROM accounts a
                     LEFT JOIN latest_usage lu
                       ON lu.account_id = a.id
                      AND lu.rn = 1
                     {where_clause}
                     ORDER BY a.sort ASC, a.updated_at DESC",
                    latest_usage_cte = latest_usage_cte_sql(),
                    account_select = account_select_columns("a"),
                );

                if let Some((offset, limit)) = pagination {
                    sql.push_str(" LIMIT ? OFFSET ?");
                    params.push(Value::Integer(limit.max(1)));
                    params.push(Value::Integer(offset.max(0)));
                }

                let mut stmt = self.conn().prepare(&sql)?;
                let mut rows = stmt.query(params_from_iter(params))?;
                let mut out = Vec::new();
                while let Some(row) = rows.next()? {
                    out.push(map_account_row(row)?);
                }
                Ok(out)
            }
            StorageBackend::PostgresUrl(url) => {
                let (mut where_clause, mut params) =
                    build_account_where_clause_pg(query, group_name, "a", 1);
                append_where_clause(
                    &mut where_clause,
                    account_usage_filter_clause(mode, "a", "lu").as_str(),
                );
                let mut sql = format!(
                    "{latest_usage_cte}
                     SELECT {account_select}
                     FROM accounts a
                     LEFT JOIN latest_usage lu
                       ON lu.account_id = a.id
                      AND lu.rn = 1
                     {where_clause}
                     ORDER BY a.sort ASC, a.updated_at DESC",
                    latest_usage_cte = latest_usage_cte_sql(),
                    account_select = account_select_columns("a"),
                );

                if let Some((offset, limit)) = pagination {
                    sql.push_str(&format!(
                        " LIMIT ${} OFFSET ${}",
                        params.len() + 1,
                        params.len() + 2
                    ));
                    params.push(Box::new(limit.max(1)));
                    params.push(Box::new(offset.max(0)));
                }

                let refs = pg_param_refs(&params);
                let mut client = connect_postgres(url)?;
                let rows = client.query(&sql, &refs)?;
                Ok(rows.into_iter().map(map_account_row_pg).collect())
            }
        }
    }

    fn count_accounts_with_usage_mode(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
        mode: AccountUsageQueryMode,
    ) -> Result<i64> {
        match &self.backend {
            StorageBackend::Sqlite(_) => {
                let mut params = Vec::new();
                let mut where_clause =
                    build_account_where_clause(query, group_name, &mut params, "a");
                append_where_clause(
                    &mut where_clause,
                    account_usage_filter_clause(mode, "a", "lu").as_str(),
                );
                let sql = format!(
                    "{latest_usage_cte}
                     SELECT COUNT(1)
                     FROM accounts a
                     LEFT JOIN latest_usage lu
                       ON lu.account_id = a.id
                      AND lu.rn = 1
                     {where_clause}",
                    latest_usage_cte = latest_usage_cte_sql(),
                );
                self.conn()
                    .query_row(&sql, params_from_iter(params), |row| row.get(0))
                    .map_err(Into::into)
            }
            StorageBackend::PostgresUrl(url) => {
                let (mut where_clause, params) =
                    build_account_where_clause_pg(query, group_name, "a", 1);
                append_where_clause(
                    &mut where_clause,
                    account_usage_filter_clause(mode, "a", "lu").as_str(),
                );
                let sql = format!(
                    "{latest_usage_cte}
                     SELECT COUNT(1)
                     FROM accounts a
                     LEFT JOIN latest_usage lu
                       ON lu.account_id = a.id
                      AND lu.rn = 1
                     {where_clause}",
                    latest_usage_cte = latest_usage_cte_sql(),
                );
                let refs = pg_param_refs(&params);
                let mut client = connect_postgres(url)?;
                let row = client.query_one(&sql, &refs)?;
                Ok(row.get(0))
            }
        }
    }
}

fn normalize_optional_filter(value: Option<&str>) -> Option<String> {
    let trimmed = value.map(str::trim).unwrap_or_default();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn build_account_where_clause(
    query: Option<&str>,
    group_name: Option<&str>,
    params: &mut Vec<Value>,
    table_name: &str,
) -> String {
    let mut clauses = Vec::new();

    if let Some(keyword) = normalize_optional_filter(query) {
        let pattern = format!("%{keyword}%");
        let label_column = qualified_column(table_name, "label");
        let id_column = qualified_column(table_name, "id");
        clauses.push(format!(
            "(LOWER({label_column}) LIKE LOWER(?) OR LOWER({id_column}) LIKE LOWER(?))"
        ));
        params.push(Value::Text(pattern.clone()));
        params.push(Value::Text(pattern));
    }

    if let Some(group) = normalize_optional_filter(group_name) {
        clauses.push(format!(
            "{} = ?",
            qualified_column(table_name, "group_name")
        ));
        params.push(Value::Text(group));
    }

    if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    }
}

fn append_where_clause(where_clause: &mut String, clause: &str) {
    if clause.trim().is_empty() {
        return;
    }
    if where_clause.is_empty() {
        where_clause.push_str(" WHERE ");
    } else {
        where_clause.push_str(" AND ");
    }
    where_clause.push_str(clause);
}

fn build_account_where_clause_pg(
    query: Option<&str>,
    group_name: Option<&str>,
    table_name: &str,
    start_index: usize,
) -> (String, Vec<Box<dyn ToSql + Sync>>) {
    let mut clauses = Vec::new();
    let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
    let mut next_index = start_index;

    if let Some(keyword) = normalize_optional_filter(query) {
        let pattern = format!("%{keyword}%");
        let label_column = qualified_column(table_name, "label");
        let id_column = qualified_column(table_name, "id");
        clauses.push(format!(
            "(LOWER({label_column}) LIKE LOWER(${}) OR LOWER({id_column}) LIKE LOWER(${}))",
            next_index,
            next_index + 1
        ));
        params.push(Box::new(pattern.clone()));
        params.push(Box::new(pattern));
        next_index += 2;
    }

    if let Some(group) = normalize_optional_filter(group_name) {
        clauses.push(format!(
            "{} = ${next_index}",
            qualified_column(table_name, "group_name")
        ));
        params.push(Box::new(group));
    }

    if clauses.is_empty() {
        (String::new(), params)
    } else {
        (format!(" WHERE {}", clauses.join(" AND ")), params)
    }
}

fn pg_param_refs(params: &[Box<dyn ToSql + Sync>]) -> Vec<&(dyn ToSql + Sync)> {
    params
        .iter()
        .map(|param| param.as_ref() as &(dyn ToSql + Sync))
        .collect()
}

fn qualified_column(table_name: &str, column: &str) -> String {
    format!("{table_name}.{column}")
}

fn latest_usage_cte_sql() -> &'static str {
    "WITH latest_usage AS (
        SELECT
            account_id,
            used_percent,
            window_minutes,
            secondary_used_percent,
            secondary_window_minutes,
            ROW_NUMBER() OVER (
                PARTITION BY account_id
                ORDER BY captured_at DESC, id DESC
            ) AS rn
        FROM usage_snapshots
    )"
}

fn available_usage_clause(usage_alias: &str) -> String {
    format!(
        "{usage_alias}.used_percent IS NOT NULL
         AND {usage_alias}.window_minutes IS NOT NULL
         AND (
            ({usage_alias}.secondary_used_percent IS NULL AND {usage_alias}.secondary_window_minutes IS NULL)
            OR ({usage_alias}.secondary_used_percent IS NOT NULL AND {usage_alias}.secondary_window_minutes IS NOT NULL)
         )
         AND {usage_alias}.used_percent < 100
         AND ({usage_alias}.secondary_used_percent IS NULL OR {usage_alias}.secondary_used_percent < 100)"
    )
}

fn gateway_available_usage_clause(usage_alias: &str) -> String {
    format!(
        "{usage_alias}.account_id IS NULL OR ({})",
        available_usage_clause(usage_alias)
    )
}

fn account_usage_filter_clause(
    mode: AccountUsageQueryMode,
    account_alias: &str,
    usage_alias: &str,
) -> String {
    match mode {
        AccountUsageQueryMode::ActiveAvailable => format!(
            "LOWER(TRIM(COALESCE({account_alias}.status, ''))) != 'inactive'
             AND {usage_alias}.account_id IS NOT NULL
             AND ({})",
            available_usage_clause(usage_alias)
        ),
        AccountUsageQueryMode::LowQuota => format!(
            "{usage_alias}.account_id IS NOT NULL
             AND ({usage_alias}.used_percent >= 80 OR {usage_alias}.secondary_used_percent >= 80)"
        ),
    }
}

fn account_select_columns(table_name: &str) -> String {
    [
        "id",
        "label",
        "issuer",
        "chatgpt_account_id",
        "workspace_id",
        "group_name",
        "sort",
        "status",
        "created_at",
        "updated_at",
    ]
    .into_iter()
    .map(|column| qualified_column(table_name, column))
    .collect::<Vec<_>>()
    .join(", ")
}

fn token_select_columns(table_name: &str) -> String {
    [
        "account_id",
        "id_token",
        "access_token",
        "refresh_token",
        "api_key_access_token",
        "last_refresh",
    ]
    .into_iter()
    .map(|column| qualified_column(table_name, column))
    .collect::<Vec<_>>()
    .join(", ")
}

fn map_account_row(row: &Row<'_>) -> Result<Account> {
    map_account_row_from_offset(row, 0)
}

fn map_account_row_from_offset(row: &Row<'_>, offset: usize) -> Result<Account> {
    Ok(Account {
        id: row.get(offset)?,
        label: row.get(offset + 1)?,
        issuer: row.get(offset + 2)?,
        chatgpt_account_id: row.get(offset + 3)?,
        workspace_id: row.get(offset + 4)?,
        group_name: row.get(offset + 5)?,
        sort: row.get(offset + 6)?,
        status: row.get(offset + 7)?,
        created_at: row.get(offset + 8)?,
        updated_at: row.get(offset + 9)?,
    })
}

fn map_account_row_pg(row: postgres::Row) -> Account {
    map_account_row_pg_from_offset(&row, 0)
}

fn map_account_row_pg_from_offset(row: &postgres::Row, offset: usize) -> Account {
    Account {
        id: row.get(offset),
        label: row.get(offset + 1),
        issuer: row.get(offset + 2),
        chatgpt_account_id: row.get(offset + 3),
        workspace_id: row.get(offset + 4),
        group_name: row.get(offset + 5),
        sort: row.get(offset + 6),
        status: row.get(offset + 7),
        created_at: row.get(offset + 8),
        updated_at: row.get(offset + 9),
    }
}

fn map_token_row_from_offset(row: &Row<'_>, offset: usize) -> Result<Token> {
    Ok(Token {
        account_id: row.get(offset)?,
        id_token: row.get(offset + 1)?,
        access_token: row.get(offset + 2)?,
        refresh_token: row.get(offset + 3)?,
        api_key_access_token: row.get(offset + 4)?,
        last_refresh: row.get(offset + 5)?,
    })
}

fn map_token_row_pg_from_offset(row: &postgres::Row, offset: usize) -> Token {
    Token {
        account_id: row.get(offset),
        id_token: row.get(offset + 1),
        access_token: row.get(offset + 2),
        refresh_token: row.get(offset + 3),
        api_key_access_token: row.get(offset + 4),
        last_refresh: row.get(offset + 5),
    }
}

fn map_gateway_candidate_row(row: &Row<'_>) -> Result<(Account, Token)> {
    let account = map_account_row_from_offset(row, 0)?;
    let token = map_token_row_from_offset(row, 10)?;
    Ok((account, token))
}

fn map_gateway_candidate_row_pg(row: postgres::Row) -> (Account, Token) {
    let account = map_account_row_pg_from_offset(&row, 0);
    let token = map_token_row_pg_from_offset(&row, 10);
    (account, token)
}
