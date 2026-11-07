//! Windows implementations of the platform traits defined in `prism-core`:
//! volume detection and identity, MFT reader, USN journal, SMART, Recycle Bin,
//! unelevated shell launch and scheduled-task integration.
//!
//! This is the only crate allowed to use `unsafe` (ADR-0002). Every unsafe block
//! must carry a `// SAFETY:` comment explaining why it is sound.
#![cfg(windows)]
#![allow(unsafe_code)]

pub mod process;
pub mod volumes;
