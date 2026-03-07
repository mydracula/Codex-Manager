use crate::process_env;
use std::sync::Mutex;

static PROCESS_ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

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

    fn unset(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::remove_var(key);
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
fn database_locator_uses_database_url_for_non_sqlite_driver() {
    let _guard = PROCESS_ENV_TEST_LOCK.lock().expect("lock");
    let _driver_guard = EnvGuard::set(process_env::ENV_DB_DRIVER, "postgres");
    let _url_guard = EnvGuard::set(
        process_env::ENV_DATABASE_URL,
        "postgres://user:pass@localhost/codexmanager",
    );
    let _path_guard = EnvGuard::unset(process_env::ENV_DB_PATH);

    let locator = process_env::database_locator_for_open().expect("database locator");
    assert_eq!(locator, "postgres://user:pass@localhost/codexmanager");
    assert_eq!(process_env::storage_identity(), "postgres:postgres://user:pass@localhost/codexmanager");
}

#[test]
fn storage_base_dir_falls_back_to_exe_dir_for_non_sqlite_driver() {
    let _guard = PROCESS_ENV_TEST_LOCK.lock().expect("lock");
    let _driver_guard = EnvGuard::set(process_env::ENV_DB_DRIVER, "mysql");
    let _url_guard = EnvGuard::set(
        process_env::ENV_DATABASE_URL,
        "mysql://user:pass@localhost/codexmanager",
    );
    let _path_guard = EnvGuard::set(process_env::ENV_DB_PATH, "relative/ignored.db");

    assert_eq!(process_env::storage_base_dir(), process_env::exe_dir());
}

#[test]
fn supported_runtime_driver_is_accepted() {
    let _guard = PROCESS_ENV_TEST_LOCK.lock().expect("lock");
    let _driver_guard = EnvGuard::set(process_env::ENV_DB_DRIVER, "postgres");
    let _url_guard = EnvGuard::set(
        process_env::ENV_DATABASE_URL,
        "postgres://user:pass@localhost/codexmanager",
    );

    process_env::ensure_supported_driver_for_runtime().expect("postgres accepted");
}
