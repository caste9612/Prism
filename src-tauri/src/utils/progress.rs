//! Progress tracking utilities

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Thread-safe progress tracker
#[derive(Debug)]
pub struct ProgressTracker {
    total: AtomicU64,
    current: AtomicU64,
    start_time: Instant,
}

impl ProgressTracker {
    pub fn new(total: u64) -> Self {
        Self {
            total: AtomicU64::new(total),
            current: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    pub fn increment(&self) {
        self.current.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_by(&self, amount: u64) {
        self.current.fetch_add(amount, Ordering::Relaxed);
    }

    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    pub fn percentage(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        (self.current() as f64 / total as f64) * 100.0
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    pub fn items_per_second(&self) -> f64 {
        let elapsed = self.elapsed_secs();
        if elapsed > 0.0 {
            self.current() as f64 / elapsed
        } else {
            0.0
        }
    }

    pub fn eta_secs(&self) -> Option<f64> {
        let current = self.current();
        let total = self.total();
        let rate = self.items_per_second();

        if current == 0 || rate <= 0.0 || current >= total {
            return None;
        }

        let remaining = total - current;
        Some(remaining as f64 / rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_progress_tracker() {
        let tracker = ProgressTracker::new(100);

        assert_eq!(tracker.current(), 0);
        assert_eq!(tracker.total(), 100);
        assert_eq!(tracker.percentage(), 0.0);

        tracker.increment_by(50);
        assert_eq!(tracker.current(), 50);
        assert_eq!(tracker.percentage(), 50.0);
    }
}
