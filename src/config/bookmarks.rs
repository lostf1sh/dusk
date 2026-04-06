use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub path: PathBuf,
    pub label: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BookmarkStore {
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}

impl BookmarkStore {
    /// Load bookmarks from the config file, or return empty store.
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save bookmarks to the config file.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Add a bookmark if not already present.
    pub fn add(&mut self, path: PathBuf, label: String) -> bool {
        if self.bookmarks.iter().any(|b| b.path == path) {
            return false; // already bookmarked
        }
        self.bookmarks.push(Bookmark { path, label });
        true
    }

    /// Remove bookmark at index.
    pub fn remove(&mut self, index: usize) {
        if index < self.bookmarks.len() {
            self.bookmarks.remove(index);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bookmarks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.bookmarks.len()
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        })
        .join("dusk")
        .join("bookmarks.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_duplicate() {
        let mut store = BookmarkStore::default();
        assert!(store.add("/tmp/a".into(), "a".into()));
        assert!(!store.add("/tmp/a".into(), "a".into())); // duplicate
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_remove() {
        let mut store = BookmarkStore::default();
        store.add("/tmp/a".into(), "a".into());
        store.add("/tmp/b".into(), "b".into());
        store.remove(0);
        assert_eq!(store.len(), 1);
        assert_eq!(store.bookmarks[0].label, "b");
    }

    #[test]
    fn test_serialize_roundtrip() {
        let mut store = BookmarkStore::default();
        store.add("/tmp/test".into(), "test dir".into());
        let serialized = toml::to_string_pretty(&store).unwrap();
        let deserialized: BookmarkStore = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.bookmarks.len(), 1);
        assert_eq!(deserialized.bookmarks[0].path, PathBuf::from("/tmp/test"));
    }
}
