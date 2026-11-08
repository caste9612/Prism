//! Windows implementations of the platform traits defined in `prism-core`:
//! volume detection and identity, MFT reader, USN journal, SMART, Recycle Bin,
//! unelevated shell launch and scheduled-task integration.
//!
//! This is the only crate allowed to use `unsafe` (ADR-0002). Every unsafe block
//! must carry a `// SAFETY:` comment explaining why it is sound.
#![cfg(windows)]
#![allow(unsafe_code)]

pub mod autostart;
pub mod credentials;
pub mod files;
pub mod health;
pub(crate) mod ioctl;
pub mod mft;
pub mod net;
pub mod notify;
pub mod ntfs;
pub mod pipe;
pub mod process;
pub mod shell;
pub mod shell_menu;
pub mod system;
pub mod trash;
pub mod usn;
pub mod video;
pub mod volumes;
pub mod watch;
