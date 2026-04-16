use std::path::PathBuf;
use std::sync::mpsc;

use clap::Parser;
use dusk::{scanner, tui};

#[derive(Parser)]
#[command(
    name = "dusk",
    version,
    about = "Interactive disk usage analyzer with visualization"
)]
struct Cli {
    /// Directory to scan (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Permanently delete files instead of moving to trash
    #[arg(long)]
    no_trash: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let root = cli
        .path
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Cannot access '{}': {}", cli.path.display(), e))?;

    if !root.is_dir() {
        anyhow::bail!("'{}' is not a directory", root.display());
    }

    let (tx, rx) = mpsc::channel();

    let scan_root = root.clone();
    std::thread::spawn(move || {
        scanner::walker::scan(scan_root, tx);
    });

    let mut terminal = ratatui::init();
    let _restore_terminal = RestoreTerminalOnDrop;
    let mut app = tui::App::new(root, rx, !cli.no_trash);
    app.run(&mut terminal)
}

struct RestoreTerminalOnDrop;

impl Drop for RestoreTerminalOnDrop {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
