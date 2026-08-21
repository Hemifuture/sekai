use thiserror::Error;

/// Checked arithmetic failure while measuring owned presentation allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("integer overflow while accounting resident bytes for {context}")]
pub struct ResidentBytesError {
    pub(crate) context: &'static str,
}

pub(crate) fn capacity_bytes<T>(
    capacity: usize,
    context: &'static str,
) -> Result<usize, ResidentBytesError> {
    capacity
        .checked_mul(std::mem::size_of::<T>())
        .ok_or(ResidentBytesError { context })
}

pub(crate) fn add_bytes(
    total: usize,
    bytes: usize,
    context: &'static str,
) -> Result<usize, ResidentBytesError> {
    total
        .checked_add(bytes)
        .ok_or(ResidentBytesError { context })
}

pub(crate) fn add_capacity<T>(
    total: usize,
    capacity: usize,
    context: &'static str,
) -> Result<usize, ResidentBytesError> {
    add_bytes(total, capacity_bytes::<T>(capacity, context)?, context)
}
