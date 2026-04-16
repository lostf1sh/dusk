use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::SystemTime;

use humansize::{format_size, BINARY};

/// Rich metadata loaded on demand for the file info popup.
pub struct FileMetadata {
    pub size: u64,
    pub permissions: String,
    pub uid: u32,
    pub gid: u32,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub inode: u64,
    pub hard_links: u64,
    pub mime_type: String,
}

impl FileMetadata {
    /// Format metadata as display lines for the info popup.
    pub fn to_lines(&self, name: &str) -> Vec<String> {
        let mut lines = Vec::new();

        lines.push(format!("  Name: {name}"));
        lines.push(format!("  Size: {}", format_size(self.size, BINARY)));
        lines.push(format!("  Permissions: {}", self.permissions));
        lines.push(format!("  Owner: {}:{}", self.uid, self.gid));

        if let Some(modified) = self.modified {
            lines.push(format!("  Modified: {}", format_time(modified)));
        }
        if let Some(created) = self.created {
            lines.push(format!("  Created: {}", format_time(created)));
        }

        lines.push(format!("  Inode: {}", self.inode));
        lines.push(format!("  Hard links: {}", self.hard_links));
        lines.push(format!("  MIME: {}", self.mime_type));

        lines
    }
}

/// Load metadata for a filesystem path.
pub fn load_metadata(path: &Path) -> anyhow::Result<FileMetadata> {
    let meta = std::fs::symlink_metadata(path)?;

    let permissions = format_permissions(meta.mode());
    let mime_type = if meta.is_dir() {
        "inode/directory".to_string()
    } else if meta.is_symlink() {
        "inode/symlink".to_string()
    } else {
        tree_magic_mini::from_filepath(path)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string())
    };

    Ok(FileMetadata {
        size: meta.len(),
        permissions,
        uid: meta.uid(),
        gid: meta.gid(),
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        inode: meta.ino(),
        hard_links: meta.nlink(),
        mime_type,
    })
}

fn format_permissions(mode: u32) -> String {
    let file_type = match mode & 0o170000 {
        0o140000 => 's', // socket
        0o120000 => 'l', // symlink
        0o100000 => '-', // regular file
        0o060000 => 'b', // block device
        0o040000 => 'd', // directory
        0o020000 => 'c', // char device
        0o010000 => 'p', // fifo
        _ => '?',
    };

    let mut s = String::with_capacity(10);
    s.push(file_type);

    let perms = [
        (0o400, 'r', '-'),
        (0o200, 'w', '-'),
        (
            0o100,
            if mode & 0o4000 != 0 { 's' } else { 'x' },
            if mode & 0o4000 != 0 { 'S' } else { '-' },
        ),
        (0o040, 'r', '-'),
        (0o020, 'w', '-'),
        (
            0o010,
            if mode & 0o2000 != 0 { 's' } else { 'x' },
            if mode & 0o2000 != 0 { 'S' } else { '-' },
        ),
        (0o004, 'r', '-'),
        (0o002, 'w', '-'),
        (
            0o001,
            if mode & 0o1000 != 0 { 't' } else { 'x' },
            if mode & 0o1000 != 0 { 'T' } else { '-' },
        ),
    ];

    for (bit, present, absent) in perms {
        s.push(if mode & bit != 0 { present } else { absent });
    }

    s
}

fn format_time(time: SystemTime) -> String {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs();
            // Simple UTC formatting without pulling in chrono
            let days = secs / 86400;
            let remaining = secs % 86400;
            let hours = remaining / 3600;
            let minutes = (remaining % 3600) / 60;

            // Bounds check to prevent overflow in days_to_ymd
            // The algorithm supports dates up to year ~10^9 safely
            if days > i64::MAX as u64 / 2 {
                return "far future".to_string();
            }

            let (year, month, day) = days_to_ymd(days);
            format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02} UTC")
        }
        Err(_) => "unknown".to_string(),
    }
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Civil calendar algorithm (modified from Howard Hinnant)
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_permissions() {
        assert_eq!(format_permissions(0o100755), "-rwxr-xr-x");
        assert_eq!(format_permissions(0o100644), "-rw-r--r--");
        assert_eq!(format_permissions(0o040755), "drwxr-xr-x");
        assert_eq!(format_permissions(0o104755), "-rwsr-xr-x");
        assert_eq!(format_permissions(0o101777), "-rwxrwxrwt");
    }

    #[test]
    fn test_days_to_ymd() {
        // 2024-01-01 is day 19723 since epoch
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }

    #[test]
    fn test_load_metadata_current_dir() {
        let meta = load_metadata(Path::new("."));
        assert!(meta.is_ok());
        let meta = meta.unwrap();
        assert_eq!(meta.mime_type, "inode/directory");
    }
}
