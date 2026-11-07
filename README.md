# Prism

**A catalog of all your disks for Windows — even the disconnected ones.**

Prism indexes every drive you own (internal, external, USB sticks, network shares) and keeps
the catalog available when the drive is unplugged. On top of the catalog it offers instant
search, disk-space analysis, exact and similar duplicates, backup verification and disk
health monitoring.

> **Status:** version 1.0 is being finished on the `rewrite` branch. No release is published
> yet.

## What Prism does

- **Disks** — every disk you connect appears in the catalog and is indexed in the background
  (NTFS volumes are read through the Master File Table: about a million files in a few
  seconds). Name the disk, note where it is kept, give it a colour. Disconnected disks stay in
  the catalog with the data of their last indexing, and Prism recognizes a disk even when its
  drive letter changes.
- **Search** — `Ctrl+Space` from anywhere opens the search window. Results come from all
  disks, connected or not, as you type; filters for type, size, date, disk, connected or not,
  files or folders (e.g. `report size:>1MB ext:pdf`).
- **Space** — a map of what takes space on a disk or folder, the largest files and statistics
  by file type, also for disconnected disks.
- **Cleanup** — space you can recover on the connected disks (temporary files, caches, empty
  folders, large old files), with a confirmation before anything is removed.
- **Duplicates** — identical files across all your disks, found by content. Prism analyses
  the content of the connected disks in the background, at low priority; in *Settings* you can
  pause it, also only while on battery, or leave a disk out.
- **Similar** — photos, pictures and videos that look the same even if resized, re-encoded or
  slightly retouched.
- **Backup** — check that the files of a disk or folder have a copy somewhere else, compare two
  disks or folders, and copy what is missing (every copy is read back and verified).
- **Health** — the SMART data of your disks (temperature, wear, errors), with a notice when a
  disk gets worse.
- **Explorer menu** — right-click a folder or a drive: *Analyze with Prism* and *Find
  duplicates here*.

Prism runs in the notification area: closing the window keeps it working in the background;
*Quit* in the menu of its icon closes it.

## Your files are safe

- Nothing is deleted, moved or overwritten without your explicit confirmation.
- Deleted files go to the Recycle Bin unless you choose otherwise, and Prism checks every file
  again on the disk right before removing it: if it changed since the analysis, it is skipped.
- Copies never overwrite an existing file and are verified by reading them back.
- Prism only reads your disks to build the catalog.

## Installation

Requirements: Windows 11, 64-bit.

1. Run `Prism_<version>_x64-setup.exe`. Windows asks for administrator permission: Prism
   needs it to read the Master File Table and the change journal of NTFS disks, which makes
   indexing and updates fast.
2. If the Microsoft Edge WebView2 runtime is missing, the installer downloads it.
3. Prism starts with Windows, hidden in the notification area, without asking for permission
   each time (a scheduled task with the highest privileges). You can turn this off in
   *Settings*.

The interface is in English and Italian (it follows Windows; you can change it in
*Settings*), with a light and a dark theme.

## Updates

Release builds check for a new version one minute after starting and then once a day. A new
version is downloaded in the background; *Settings › Updates* shows it, and it is installed
only when you choose *Install and restart*. Packages are signed, and a package with a wrong
signature is never installed.

## Privacy

- No telemetry. The only connection Prism makes is the update check.
- The catalog stays on your PC, in `%LOCALAPPDATA%\com.prism.catalog` (the logs, kept for 14
  days, are in its `logs` folder).
- A diagnostic package is created only when you ask for it (*Settings › Diagnostics*); you see
  its content before saving it, and it is up to you whether to send it to anyone.

## Known limitations

- Videos in HEVC (H.265) format are compared only if Windows has the *HEVC Video Extensions*
  from the Microsoft Store.
- Network drives are indexed and searched, but their content is not analysed in the background
  (every byte would cross the network): they take no part in duplicates and similar images, and
  backup checks compare their files by name, size and date.
- Disks connected by USB often do not pass their health data to Windows.

## Uninstall

Use *Settings › Apps › Installed apps › Prism › Uninstall*. The Explorer menu entries and the
start-up task are removed. The uninstaller asks whether to delete the application data: leave
the box empty to keep the catalog, and a new installation will find it again.

## Documentation

| Document | Content |
|---|---|
| [docs/SPEC.md](docs/SPEC.md) | Functional specification with acceptance criteria |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | System architecture |
| [docs/adr/](docs/adr/README.md) | Architecture decision records |
| [docs/TESTING.md](docs/TESTING.md) | Test strategy and data-safety rules |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Milestones |
| [docs/PROGRESS.md](docs/PROGRESS.md) | Development log |
| [docs/REVIEW.md](docs/REVIEW.md) | Decisions taken during development, open items, how to try the app |
| [docs/benchmarks/](docs/benchmarks/README.md) | Performance measurements |

Project documents are written in Italian.

## Development

Requirements: Windows 11 x64, Rust (the version is pinned in `rust-toolchain.toml`), Node.js
for the interface.

```powershell
.\scripts\check.ps1             # format check, clippy, tests (Rust, interface, end-to-end)
.\scripts\build-installer.ps1   # release build and installer (see ARCHITECTURE §8.1)
```
