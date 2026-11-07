//! Cooperative cancellation for long-running work (scans, reconciliation, jobs).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A cheap, cloneable cancellation flag shared between the requester and the worker.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

/// Returned by work that stopped because its [`CancelToken`] was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("operation cancelled")]
pub struct Cancelled;

impl CancelToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Idempotent.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Returns `Err(Cancelled)` once cancellation has been requested.
    ///
    /// # Errors
    /// [`Cancelled`] if [`CancelToken::cancel`] was called.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_the_flag() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(b.check().is_ok());
        a.cancel();
        assert!(b.is_cancelled());
        assert_eq!(b.check(), Err(Cancelled));
    }
}
