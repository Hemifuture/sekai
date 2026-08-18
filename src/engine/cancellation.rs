use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use thiserror::Error;

/// The stable result of checking a cancellation signal after it was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("build cancellation requested")]
pub struct BuildCancellationError;

/// A cheap cloneable signal for cooperatively cancelling one build attempt.
#[derive(Debug, Clone, Default)]
pub struct BuildCancellation {
    cancelled: Arc<AtomicBool>,
    observation_count: Arc<AtomicU64>,
}

impl BuildCancellation {
    /// Creates a signal in the running state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Repeated requests are harmless.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.observation_count.fetch_add(1, Ordering::Relaxed);
        self.cancelled.load(Ordering::Acquire)
    }

    /// Number of cooperative observations made by all clones. This supports
    /// progress-synchronized latency evidence without changing cancellation
    /// semantics or exposing algorithm-specific test hooks.
    pub fn observation_count(&self) -> u64 {
        self.observation_count.load(Ordering::Relaxed)
    }

    /// Converts the current signal into a convenient cooperative result.
    pub fn check_cancelled(&self) -> Result<(), BuildCancellationError> {
        if self.is_cancelled() {
            Err(BuildCancellationError)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildCancellation, BuildCancellationError};

    #[test]
    fn clones_observe_the_same_monotonic_signal() {
        let first = BuildCancellation::new();
        let second = first.clone();
        assert!(!first.is_cancelled());
        assert!(!second.is_cancelled());
        assert_eq!(first.check_cancelled(), Ok(()));
        assert_eq!(first.observation_count(), 3);

        second.cancel();

        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
        assert_eq!(first.check_cancelled(), Err(BuildCancellationError));
        assert_eq!(second.observation_count(), 6);
        first.cancel();
        assert!(second.is_cancelled());
    }
}
