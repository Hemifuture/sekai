use std::sync::atomic::{AtomicBool, Ordering};
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
        self.cancelled.load(Ordering::Acquire)
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

        second.cancel();

        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
        assert_eq!(first.check_cancelled(), Err(BuildCancellationError));
        first.cancel();
        assert!(second.is_cancelled());
    }
}
