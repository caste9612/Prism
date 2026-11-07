//! Prism core: catalog model, storage, scanning, hashing, search and jobs.
//!
//! This crate contains no platform-specific and no `unsafe` code. Platform access
//! (volumes, MFT, USN journal, Recycle Bin, SMART) goes through traits that are
//! implemented in `prism-win` and by fakes in tests. See `docs/ARCHITECTURE.md`.

/// Version of the core crate, reported by diagnostics and the development CLI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
