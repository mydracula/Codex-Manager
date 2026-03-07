use super::{
    clear_storage_cache_for_tests, clear_storage_open_count_for_tests, open_storage_at_path,
    storage_open_count_for_tests,
};
use crate::process_env;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db_path(prefix: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("{prefix}-{nonce}.db"))
        .to_string_lossy()
        .to_string()
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[test]
fn open_storage_reuses_cached_connection_in_same_thread() {
    let db_path = unique_db_path("codexmanager-open-storage-reuse");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);

    let storage = open_storage_at_path(&db_path).expect("open storage 1");
    storage.init().expect("init");
    drop(storage);

    let storage = open_storage_at_path(&db_path).expect("open storage 2");
    drop(storage);

    assert_eq!(storage_open_count_for_tests(&db_path), 1);

    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path);
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn open_storage_reopens_when_db_path_changes() {
    let db_path_1 = unique_db_path("codexmanager-open-storage-path-1");
    let db_path_2 = unique_db_path("codexmanager-open-storage-path-2");
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path_1);
    clear_storage_open_count_for_tests(&db_path_2);

    let storage = open_storage_at_path(&db_path_1).expect("open storage path 1");
    storage.init().expect("init 1");
    drop(storage);

    let storage = open_storage_at_path(&db_path_2).expect("open storage path 2");
    storage.init().expect("init 2");
    drop(storage);

    assert_eq!(storage_open_count_for_tests(&db_path_1), 1);
    assert_eq!(storage_open_count_for_tests(&db_path_2), 1);

    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(&db_path_1);
    clear_storage_open_count_for_tests(&db_path_2);
    let _ = std::fs::remove_file(&db_path_1);
    let _ = std::fs::remove_file(&db_path_2);
}

#[test]
fn open_storage_reopens_when_database_locator_changes() {
    let db_url_1 = "postgres://user:pass@localhost/db1";
    let db_url_2 = "postgres://user:pass@localhost/db2";
    let _driver_guard = EnvGuard::set(process_env::ENV_DB_DRIVER, "postgres");
    let _url_guard = EnvGuard::set(process_env::ENV_DATABASE_URL, db_url_1);
    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(db_url_1);
    clear_storage_open_count_for_tests(db_url_2);

    let first = open_storage_at_path(db_url_1);
    assert!(first.is_none());

    std::env::set_var(process_env::ENV_DATABASE_URL, db_url_2);
    let second = open_storage_at_path(db_url_2);
    assert!(second.is_none());

    assert_eq!(storage_open_count_for_tests(db_url_1), 0);
    assert_eq!(storage_open_count_for_tests(db_url_2), 0);

    clear_storage_cache_for_tests();
    clear_storage_open_count_for_tests(db_url_1);
    clear_storage_open_count_for_tests(db_url_2);
}
