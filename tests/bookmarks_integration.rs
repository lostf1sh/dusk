use std::sync::{Mutex, OnceLock};

use dusk::config::bookmarks::BookmarkStore;
use tempfile::TempDir;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn bookmark_store_roundtrips_through_public_config_path() {
    let _guard = env_lock().lock().unwrap();
    let tmp = TempDir::new().unwrap();
    let previous = std::env::var_os("XDG_CONFIG_HOME");
    std::env::set_var("XDG_CONFIG_HOME", tmp.path());

    let mut store = BookmarkStore::default();
    store.add("/tmp/a".into(), "alpha".into());
    store.save().unwrap();

    let loaded = BookmarkStore::load().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded.bookmarks[0].label, "alpha");

    if let Some(previous) = previous {
        std::env::set_var("XDG_CONFIG_HOME", previous);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}

#[test]
fn bookmark_store_load_reports_invalid_config() {
    let _guard = env_lock().lock().unwrap();
    let tmp = TempDir::new().unwrap();
    let previous = std::env::var_os("XDG_CONFIG_HOME");
    std::env::set_var("XDG_CONFIG_HOME", tmp.path());

    let config_dir = tmp.path().join("dusk");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("bookmarks.toml"), "not = [valid").unwrap();

    let error = BookmarkStore::load().unwrap_err();
    assert!(error.to_string().contains("failed to parse"));

    if let Some(previous) = previous {
        std::env::set_var("XDG_CONFIG_HOME", previous);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
