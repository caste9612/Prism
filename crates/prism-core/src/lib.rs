//! Prism core: catalog model, storage, scanning, hashing, search and jobs.
//!
//! This crate contains no platform-specific and no `unsafe` code. Platform access
//! (volumes, MFT, USN journal, Recycle Bin, SMART) goes through traits that are
//! implemented in `prism-win` and by fakes in tests. See `docs/ARCHITECTURE.md`.
//!
//! Main pieces (milestone T1):
//! - [`store`]: SQLite catalog with one writer thread and a pool of readers (ADR-0008);
//! - [`scan`]: enumeration of a folder tree ([`scan::WalkEnumerator`]) into a snapshot;
//! - [`exclusion`]: the single exclusion policy (F04-AC5);
//! - [`reconcile`]: the single reconciler for full indexing and updates (ADR-0006);
//! - [`indexer`]: enumeration + reconciliation with a truthful volume status (F04-AC6);
//! - [`sandbox`]: the guard for destructive file operations in tests (TESTING S2).

pub mod backup;
pub mod cancel;
pub mod changes;
pub mod cleanup;
pub mod diagnostics;
pub mod duplicates;
pub mod exclusion;
pub mod fileops;
pub mod hashing;
pub mod health;
pub mod indexer;
pub mod jobs;
pub mod media;
pub mod model;
pub mod names;
pub mod reconcile;
pub mod refresh;
pub mod sandbox;
pub mod scan;
pub mod search;
pub mod space;
pub mod store;
pub mod volumes;

/// Version of the core crate, reported by diagnostics and the development CLI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
