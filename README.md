# Prism - High-Performance Disk Analyzer

Prism is a fast, modern disk analyzer and duplicate file detector for Windows, built with Rust and Tauri.

![Prism Screenshot](docs/screenshot.png)

## Features

- **Fast Multi-Drive Scanning**: Parallel filesystem traversal with real-time progress per drive
- **Full-Text Search**: SQLite FTS5-powered instant file search with advanced filters
- **Duplicate Detection**: Two-phase BLAKE3 hashing (partial + full) for accurate duplicate finding
- **Similar Image Detection**: Perceptual hashing (dHash) to find visually similar images
- **Interactive Analytics**: Storage explorer with drill-down treemap and file category visualization
- **Quick Search**: Global hotkey (Ctrl+Space) for instant file search overlay
- **Export Results**: Export search results to CSV or JSON
- **System Tray**: Runs in background with system tray integration

## Installation

### Portable (Recommended)

Download `prism.exe` from the [Releases](https://github.com/user/prism/releases) page and run it directly.

Database location: `%APPDATA%\com.prism.diskanalyzer\prism.db`

### From Source

```bash
# Prerequisites: Node.js 18+, Rust 1.70+

# Install dependencies
npm install

# Development
npm run tauri dev

# Production build
npm run tauri build
```

## Usage

### Scanning
- Drives are detected automatically on startup
- Click drive cards to select/deselect drives for scanning
- Click **Scan** to begin indexing selected drives
- Progress is shown per-drive with real-time file counts

### Searching
Use the search bar with optional filters:
- `size:>1MB` - Files larger than 1MB
- `ext:pdf,docx` - Specific extensions
- `type:image` - File type (image, video, audio, document, archive)
- `path:Documents` - Path contains text

### Quick Search (Ctrl+Space)
Press Ctrl+Space anywhere to open the quick search overlay.

### Duplicate Detection
1. Go to **Duplicates** tab
2. Click **Find Duplicates** to analyze
3. Select files to delete (first file in each group is protected as "original")
4. Confirm deletion

### Analytics
- **Storage Explorer**: Click folders to drill down, breadcrumb to navigate up
- **File Categories**: Donut chart with expandable category breakdowns
- **Size Distribution**: See file size distribution across your drives

## Documentation

- [Architecture](docs/ARCHITECTURE.md) - Technical architecture and design
- [Development](docs/DEVELOPMENT.md) - Development setup and guidelines
- [API Reference](docs/API.md) - Tauri command documentation
- [Refactoring Plan](docs/REFACTORING-PLAN.md) - Future improvements roadmap

## Tech Stack

| Layer | Technology |
|-------|------------|
| Frontend | SvelteKit 2.0, TypeScript, TailwindCSS |
| Backend | Rust, Tauri 2.0 |
| Database | SQLite with FTS5 |
| Hashing | BLAKE3 (files), dHash (images) |

## Performance

- **Scanning**: ~50,000+ files/second on SSD, optimized for network drives
- **Search**: <100ms for millions of files using FTS5
- **Memory**: Efficient streaming with batched database inserts

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Space` | Open Quick Search (global) |
| `Escape` | Close Quick Search |
| `Enter` | Open selected file |
| `Ctrl+C` | Copy file path |

## License

MIT License
