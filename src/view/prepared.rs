use super::DisplayPrepareError;

/// A non-zero, process-local revision used only for display invalidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayRevision(u64);

impl DisplayRevision {
    /// Creates a non-zero display revision.
    pub fn new(value: u64) -> Result<Self, DisplayPrepareError> {
        if value == 0 {
            return Err(DisplayPrepareError::ZeroRevision);
        }
        Ok(Self(value))
    }

    /// Returns the raw non-zero revision value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Monotonically issues non-zero process-local display revisions.
#[derive(Debug, Clone)]
pub struct DisplayRevisionClock {
    next: u64,
}

impl Default for DisplayRevisionClock {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl DisplayRevisionClock {
    /// Issues the next revision without wrapping.
    pub fn issue(&mut self) -> Result<DisplayRevision, DisplayPrepareError> {
        let revision = DisplayRevision::new(self.next)?;
        let next = self
            .next
            .checked_add(1)
            .ok_or(DisplayPrepareError::RevisionOverflow)?;
        self.next = next;
        Ok(revision)
    }
}
