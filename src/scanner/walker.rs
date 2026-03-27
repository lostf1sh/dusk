use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::model::node::{DiskNode, NodeType};
use crate::scanner::ignore_rules::build_walk;

/// Messages sent from the scanner thread to the TUI.
pub enum ScanUpdate {
    Progress { files_found: u64, bytes_found: u64 },
    Complete(DiskNode),
    Error(String),
}

struct RawEntry {
    path: PathBuf,
    size: u64,
    node_type: NodeType,
}

/// Run a full directory scan. Meant to be called on a spawned thread.
/// Sends progress updates and a final Complete/Error via `tx`.
pub fn scan(root: PathBuf, tx: Sender<ScanUpdate>) {
    match scan_inner(&root, &tx) {
        Ok(tree) => {
            let _ = tx.send(ScanUpdate::Complete(tree));
        }
        Err(e) => {
            let _ = tx.send(ScanUpdate::Error(format!("{e:#}")));
        }
    }
}

fn scan_inner(root: &Path, tx: &Sender<ScanUpdate>) -> anyhow::Result<DiskNode> {
    let walker = build_walk(root).build();

    let mut entries: Vec<RawEntry> = Vec::new();
    let mut files_found: u64 = 0;
    let mut bytes_found: u64 = 0;

    for result in walker {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue, // permission errors, broken symlinks, etc.
        };

        let path = entry.path().to_path_buf();

        // Skip the root entry itself — we'll create the root node manually
        if path == root {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue, // can't stat — skip
        };

        let node_type = if metadata.is_symlink() {
            NodeType::Symlink
        } else if metadata.is_dir() {
            NodeType::Dir
        } else {
            NodeType::File
        };

        let size = if node_type == NodeType::Dir {
            0 // directories get their size from children
        } else {
            metadata.len()
        };

        files_found += 1;
        bytes_found += size;

        entries.push(RawEntry {
            path,
            size,
            node_type,
        });

        // Send progress every 500 entries
        if files_found.is_multiple_of(500) {
            let _ = tx.send(ScanUpdate::Progress {
                files_found,
                bytes_found,
            });
        }
    }

    // Send final progress before building tree
    let _ = tx.send(ScanUpdate::Progress {
        files_found,
        bytes_found,
    });

    // Build tree from flat entries
    let mut tree = build_tree(root, &entries);
    tree.sort_children_by_size();
    Ok(tree)
}

/// Build a DiskNode tree from a flat list of entries by grouping children under their parent paths.
fn build_tree(root: &Path, entries: &[RawEntry]) -> DiskNode {
    // Map: parent_path -> list of child entries
    let mut children_map: HashMap<PathBuf, Vec<usize>> = HashMap::new();

    for (i, entry) in entries.iter().enumerate() {
        if let Some(parent) = entry.path.parent() {
            children_map
                .entry(parent.to_path_buf())
                .or_default()
                .push(i);
        }
    }

    fn build_node(
        path: &Path,
        name: String,
        node_type: NodeType,
        own_size: u64,
        depth: usize,
        entries: &[RawEntry],
        children_map: &HashMap<PathBuf, Vec<usize>>,
    ) -> DiskNode {
        let mut node = DiskNode::new(name, own_size, node_type, depth);

        if let Some(child_indices) = children_map.get(path) {
            for &idx in child_indices {
                let child_entry = &entries[idx];
                let child_name = child_entry
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| child_entry.path.to_string_lossy().into_owned());

                let child = build_node(
                    &child_entry.path,
                    child_name,
                    child_entry.node_type.clone(),
                    child_entry.size,
                    depth + 1,
                    entries,
                    children_map,
                );
                node.children.push(child);
            }
        }

        // Directory size = sum of children sizes
        if node.node_type == NodeType::Dir {
            node.size = node.children.iter().map(|c| c.size).sum();
        }

        node
    }

    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());

    build_node(
        root,
        root_name,
        NodeType::Dir,
        0,
        0,
        entries,
        &children_map,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_scan_temp_dir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create structure:
        // root/
        //   a.txt (5 bytes)
        //   sub/
        //     b.txt (10 bytes)
        fs::write(root.join("a.txt"), "hello").unwrap();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("b.txt"), "0123456789").unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        scan(root.to_path_buf(), tx);

        // Drain until Complete
        let mut tree = None;
        for msg in rx {
            if let ScanUpdate::Complete(t) = msg {
                tree = Some(t);
                break;
            }
        }

        let tree = tree.expect("should receive Complete");
        assert_eq!(tree.size, 15); // 5 + 10
        assert_eq!(tree.total_files(), 2);
        assert_eq!(tree.total_dirs(), 1); // sub/
        assert_eq!(tree.children.len(), 2); // a.txt and sub/
    }

    #[test]
    fn test_duskignore() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(root.join(".duskignore"), "ignored.txt\n").unwrap();
        fs::write(root.join("kept.txt"), "keep").unwrap();
        fs::write(root.join("ignored.txt"), "drop").unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        scan(root.to_path_buf(), tx);

        let mut tree = None;
        for msg in rx {
            if let ScanUpdate::Complete(t) = msg {
                tree = Some(t);
                break;
            }
        }

        let tree = tree.expect("should receive Complete");
        // Should only contain .duskignore and kept.txt (ignored.txt filtered out)
        let names: Vec<&str> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"kept.txt"));
        assert!(!names.contains(&"ignored.txt"));
    }
}
