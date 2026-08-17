use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
}

#[cfg(test)]
mod tests {
    use super::BuildCancellation;

    #[test]
    fn clones_observe_the_same_monotonic_signal() {
        let first = BuildCancellation::new();
        let second = first.clone();
        assert!(!first.is_cancelled());
        assert!(!second.is_cancelled());

        second.cancel();

        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
        first.cancel();
        assert!(second.is_cancelled());
    }
}
