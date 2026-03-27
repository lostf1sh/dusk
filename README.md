# dusk

Interactive disk usage analyzer for the terminal. Scan directories, visualize space usage across multiple views, and manage files — all without leaving your shell.

![Rust](https://img.shields.io/badge/rust-2021-orange) ![License](https://img.shields.io/badge/license-MIT-blue)

## Features

- **3 visualization modes** — tree, treemap, horizontal bar chart
- **Parallel scanning** — fast traversal with real-time progress
- **Sorting** — by size, name, modified time, or file count
- **Fuzzy search** — find any file instantly with `/`
- **Filtering** — by extension, size range, or modification date
- **Quick delete** — with trash support (freedesktop spec)
- **Bookmarks** — save and jump to frequently visited directories
- **File info** — permissions, owner, MIME type, inode, timestamps
- **`.gitignore` aware** — respects ignore rules automatically

## Install

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
./target/release/dusk
```

## Usage

```bash
dusk              # scan current directory
dusk /path/to/dir # scan specific directory
dusk --no-trash   # permanent delete instead of trash
```

## Key Bindings

### Navigation

| Key | Action |
|-----|--------|
| `j` / `Down` | Move down |
| `k` / `Up` | Move up |
| `Enter` / `l` / `Right` | Drill into directory |
| `Backspace` / `h` / `Left` | Go back / collapse |
| `Space` | Toggle expand (tree view) |
| `q` / `Esc` | Quit |

### Views

| Key | Action |
|-----|--------|
| `1` | Tree view |
| `2` | Treemap view |
| `3` | Bar chart view |

### Actions

| Key | Action |
|-----|--------|
| `s` | Cycle sort: Size → Name → Modified → Files |
| `S` | Toggle ascending / descending |
| `i` | File info popup |
| `d` | Delete (with confirmation) |
| `/` | Fuzzy search |
| `f` | Filter menu |
| `b` | Bookmark current directory |
| `B` | Open bookmark list |

### Filter Menu (`f`)

| Key | Filter |
|-----|--------|
| `e` | By extension (type and Enter) |
| `1`-`4` | By size: >1M, >10M, >100M, >1G |
| `d` / `w` / `m` / `y` | Modified: 24h / 7d / 30d / 1yr |
| `c` | Clear filter |

### Search (`/`)

Type your query, press **Enter** to search. Navigate results with `j`/`k`, press **Enter** to jump.

### Bookmark List (`B`)

Navigate with `j`/`k`, **Enter** to jump, `d` to remove.

## Layout

```
┌─────────────────────────────┬────────────┐
│                             │            │
│   Visualization (65%)       │   Info     │
│   Tree / Treemap / Bar      │   Panel    │
│                             │            │
├─────────────────────────────┴────────────┤
│ Total: 1.2 GiB │ Files: 4832 │ 0.3s │ [1:Tree] 2:Map 3:Bar │ Sort: Size ▼
└──────────────────────────────────────────┘
```

## Architecture

```
src/
├── main.rs                  # CLI parsing, terminal setup
├── config/
│   └── bookmarks.rs         # Bookmark persistence (~/.config/dusk/bookmarks.toml)
├── model/
│   ├── node.rs              # DiskNode tree, sorting, removal
│   └── metadata.rs          # On-demand file metadata (permissions, MIME, etc.)
├── scanner/
│   ├── walker.rs            # Directory traversal with progress updates
│   └── ignore_rules.rs      # .gitignore / .duskignore support
└── tui/
    ├── mod.rs               # App state machine, key dispatch, rendering
    ├── overlay.rs           # Popups: delete, info, search, filter, bookmarks
    ├── filter.rs            # Filter criteria and matching
    ├── theme.rs             # Color scheme
    ├── views/
    │   ├── tree.rs          # Expandable tree with flat-row model
    │   ├── treemap.rs       # Squarified treemap algorithm
    │   ├── bar.rs           # Horizontal bar chart
    │   └── nav.rs           # Shared navigation for bar/treemap
    └── widgets/
        ├── progress.rs      # Scan progress spinner
        └── text_input.rs    # Text input for search/filter
```

**~4,100 lines of Rust** | **31 tests** | **0 clippy warnings**

### Key Design Decisions

- **Name-based expanded paths** — tree expand state survives sort and delete operations
- **Lazy metadata** — file info loaded on demand, not during scan
- **Overlay system** — all popups share a single rendering/input pipeline
- **Channel-based scan** — scanner thread sends progress via `mpsc`, TUI polls at 100ms

## Dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui` | Terminal UI framework |
| `crossterm` | Cross-platform terminal I/O |
| `clap` | CLI argument parsing |
| `ignore` | .gitignore-aware directory walking |
| `trash` | Freedesktop trash support |
| `fuzzy-matcher` | Skim-based fuzzy search |
| `tree_magic_mini` | MIME type detection |
| `humansize` | Human-readable file sizes |
| `serde` + `toml` | Bookmark serialization |
| `dirs` | XDG config directory paths |
| `anyhow` + `thiserror` | Error handling |

## License

MIT
