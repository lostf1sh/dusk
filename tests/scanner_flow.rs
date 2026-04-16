use std::fs;
use std::sync::mpsc;

use dusk::scanner::walker::{scan, ScanUpdate};
use tempfile::TempDir;

#[test]
fn scan_reports_complete_tree_without_warnings_for_simple_directory() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::write(root.join("a.txt"), "hello").unwrap();
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub").join("b.txt"), "0123456789").unwrap();

    let (tx, rx) = mpsc::channel();
    scan(root.to_path_buf(), tx);

    for update in rx {
        if let ScanUpdate::Complete { root, warnings } = update {
            assert_eq!(root.size, 15);
            assert_eq!(root.total_files(), 2);
            assert_eq!(warnings.skipped_entries, 0);
            return;
        }
    }

    panic!("expected a completion update");
}
