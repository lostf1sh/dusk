use std::io::ErrorKind;
use std::path::{Path, PathBuf};

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
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from_path(&config_path())
    }

    fn load_from_path(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => toml::from_str(&content)
                .map_err(anyhow::Error::from)
                .map_err(|error| error.context(format!("failed to parse {}", path.display()))),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).map_err(|error| {
                anyhow::Error::from(error).context(format!("failed to read {}", path.display()))
            }),
        }
    }

    /// Save bookmarks to the config file.
    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to_path(&config_path())
    }

    fn save_to_path(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        let temp_path = path.with_extension("toml.tmp");
        std::fs::write(&temp_path, content)?;
        std::fs::rename(&temp_path, path)?;
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
    use tempfile::TempDir;

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

    #[test]
    fn test_load_invalid_toml_reports_error() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("bookmarks.toml");
        std::fs::write(&path, "not = [valid").unwrap();

        let error = BookmarkStore::load_from_path(&path).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("failed to parse"));
        assert!(message.contains("bookmarks.toml"));
    }

    #[test]
    fn test_save_to_path_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested").join("bookmarks.toml");

        let mut store = BookmarkStore::default();
        store.add("/tmp/test".into(), "test dir".into());
        store.save_to_path(&path).unwrap();

        let loaded = BookmarkStore::load_from_path(&path).unwrap();
        assert_eq!(loaded.bookmarks.len(), 1);
        assert_eq!(loaded.bookmarks[0].label, "test dir");
    }
}
