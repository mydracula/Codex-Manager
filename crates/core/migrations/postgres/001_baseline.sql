CREATE TABLE IF NOT EXISTS schema_migrations (
  version TEXT PRIMARY KEY,
  applied_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS accounts (
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  issuer TEXT NOT NULL,
  chatgpt_account_id TEXT,
  workspace_id TEXT,
  group_name TEXT,
  sort BIGINT DEFAULT 0,
  status TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS tokens (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  id_token TEXT NOT NULL,
  access_token TEXT NOT NULL,
  refresh_token TEXT NOT NULL,
  api_key_access_token TEXT,
  last_refresh BIGINT NOT NULL,
  access_token_exp BIGINT,
  next_refresh_at BIGINT,
  last_refresh_attempt_at BIGINT
);

CREATE INDEX IF NOT EXISTS idx_tokens_next_refresh_at
  ON tokens(next_refresh_at);

CREATE TABLE IF NOT EXISTS usage_snapshots (
  id BIGSERIAL PRIMARY KEY,
  account_id TEXT NOT NULL,
  used_percent DOUBLE PRECISION,
  window_minutes BIGINT,
  resets_at BIGINT,
  secondary_used_percent DOUBLE PRECISION,
  secondary_window_minutes BIGINT,
  secondary_resets_at BIGINT,
  credits_json TEXT,
  captured_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_usage_snapshots_account_captured_id
  ON usage_snapshots(account_id, captured_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_usage_snapshots_captured_id
  ON usage_snapshots(captured_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS events (
  id BIGSERIAL PRIMARY KEY,
  account_id TEXT,
  type TEXT NOT NULL,
  message TEXT NOT NULL,
  created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS login_sessions (
  login_id TEXT PRIMARY KEY,
  code_verifier TEXT NOT NULL,
  state TEXT NOT NULL,
  status TEXT NOT NULL,
  error TEXT,
  note TEXT,
  tags TEXT,
  group_name TEXT,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS api_keys (
  id TEXT PRIMARY KEY,
  name TEXT,
  model_slug TEXT,
  reasoning_effort TEXT,
  key_hash TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  last_used_at BIGINT
);

CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash
  ON api_keys(key_hash);

CREATE TABLE IF NOT EXISTS api_key_profiles (
  key_id TEXT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
  client_type TEXT NOT NULL CHECK (client_type IN ('codex', 'claude_code')),
  protocol_type TEXT NOT NULL CHECK (protocol_type IN ('openai_compat', 'anthropic_native', 'azure_openai')),
  auth_scheme TEXT NOT NULL CHECK (auth_scheme IN ('authorization_bearer', 'x_api_key', 'api_key')),
  upstream_base_url TEXT,
  static_headers_json TEXT,
  default_model TEXT,
  reasoning_effort TEXT,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_api_key_profiles_client_protocol
  ON api_key_profiles(client_type, protocol_type);

CREATE TABLE IF NOT EXISTS api_key_secrets (
  key_id TEXT PRIMARY KEY,
  key_value TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_api_key_secrets_updated_at
  ON api_key_secrets(updated_at);

CREATE TABLE IF NOT EXISTS request_logs (
  id BIGSERIAL PRIMARY KEY,
  trace_id TEXT,
  key_id TEXT,
  account_id TEXT,
  request_path TEXT NOT NULL,
  original_path TEXT,
  adapted_path TEXT,
  method TEXT NOT NULL,
  model TEXT,
  reasoning_effort TEXT,
  response_adapter TEXT,
  upstream_url TEXT,
  status_code BIGINT,
  error TEXT,
  created_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_request_logs_created_at
  ON request_logs(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_logs_status_code_created_at
  ON request_logs(status_code, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_logs_method_created_at
  ON request_logs(method, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_logs_key_id_created_at
  ON request_logs(key_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_logs_account_id_created_at
  ON request_logs(account_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_logs_created_at_id
  ON request_logs(created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_request_logs_trace_id_created_at
  ON request_logs(trace_id, created_at DESC);

CREATE TABLE IF NOT EXISTS request_token_stats (
  id BIGSERIAL PRIMARY KEY,
  request_log_id BIGINT NOT NULL,
  key_id TEXT,
  account_id TEXT,
  model TEXT,
  input_tokens BIGINT,
  cached_input_tokens BIGINT,
  output_tokens BIGINT,
  total_tokens BIGINT,
  reasoning_output_tokens BIGINT,
  estimated_cost_usd DOUBLE PRECISION,
  created_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_request_token_stats_request_log_id
  ON request_token_stats(request_log_id);

CREATE INDEX IF NOT EXISTS idx_request_token_stats_created_at
  ON request_token_stats(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_token_stats_account_id_created_at
  ON request_token_stats(account_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_token_stats_key_id_created_at
  ON request_token_stats(key_id, created_at DESC);

CREATE TABLE IF NOT EXISTS model_options_cache (
  scope TEXT PRIMARY KEY,
  items_json TEXT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_accounts_sort_updated_at
  ON accounts(sort ASC, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_accounts_status_sort_updated_at
  ON accounts(status, sort ASC, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_accounts_group_name_sort_updated_at
  ON accounts(group_name, sort ASC, updated_at DESC);
