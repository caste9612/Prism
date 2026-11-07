# Prism

**A catalog of all your disks for Windows — even the disconnected ones.**

Prism indexes every drive you own (internal, external, USB sticks, network shares) and keeps
the catalog available when the drive is unplugged. On top of the catalog it offers instant
search, disk-space analysis, exact and similar duplicates, backup verification and disk
health monitoring.

> **Status:** complete rewrite in progress on the `rewrite` branch. Nothing is released yet.

## Documentation

| Document | Content |
|---|---|
| [docs/SPEC.md](docs/SPEC.md) | Functional specification with acceptance criteria |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | System architecture |
| [docs/adr/](docs/adr/README.md) | Architecture decision records |
| [docs/TESTING.md](docs/TESTING.md) | Test strategy and data-safety rules |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Milestones |
| [docs/PROGRESS.md](docs/PROGRESS.md) | Development log |

Project documents are currently written in Italian.

## Development

Requirements: Windows 10/11 x64, Rust (the version is pinned in `rust-toolchain.toml`).

```powershell
.\scripts\check.ps1     # format check, clippy, tests
```
