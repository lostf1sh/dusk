use std::sync::{Mutex, OnceLock};

use dusk::config::bookmarks::BookmarkStore;
use tempfile::TempDir;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct ConfigDirGuard {
    previous: Option<std::ffi::OsString>,
}

impl ConfigDirGuard {
    fn new(path: &std::path::Path) -> Self {
        let previous = std::env::var_os("DUSK_CONFIG_DIR");
        std::env::set_var("DUSK_CONFIG_DIR", path);
        Self { previous }
    }
}

impl Drop for ConfigDirGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var("DUSK_CONFIG_DIR", previous);
        } else {
            std::env::remove_var("DUSK_CONFIG_DIR");
        }
    }
}

#[test]
fn bookmark_store_roundtrips_through_public_config_path() {
    let _guard = env_lock().lock().unwrap();
    let tmp = TempDir::new().unwrap();
    let _config_guard = ConfigDirGuard::new(tmp.path());

    let mut store = BookmarkStore::default();
    store.add("/tmp/a".into(), "alpha".into());
    store.save().unwrap();

    let loaded = BookmarkStore::load().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded.bookmarks[0].label, "alpha");
}

#[test]
fn bookmark_store_load_reports_invalid_config() {
    let _guard = env_lock().lock().unwrap();
    let tmp = TempDir::new().unwrap();
    let _config_guard = ConfigDirGuard::new(tmp.path());

    std::fs::write(tmp.path().join("bookmarks.toml"), "not = [valid").unwrap();

    let error = BookmarkStore::load().unwrap_err();
    assert!(error.to_string().contains("failed to parse"));
}
