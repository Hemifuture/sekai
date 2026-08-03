//! Shared contracts for closed-sphere atmosphere-ocean circulation experiments.

mod bounded_vec;
mod forcing;
mod snapshot;
mod spec;

pub use forcing::{ForcingError, PlanetForcing};
pub use snapshot::{
    CirculationSnapshot, CirculationSnapshotError, CirculationSolveStats, CirculationSolverId,
};
pub use spec::{
    CirculationSpec, CirculationSpecError, CIRCULATION_SCHEMA_V1, MAX_CUBED_SPHERE_FACE_RESOLUTION,
};

pub(crate) const MAX_CIRCULATION_CELL_COUNT: usize =
    6 * MAX_CUBED_SPHERE_FACE_RESOLUTION as usize * MAX_CUBED_SPHERE_FACE_RESOLUTION as usize;

pub(crate) const MAX_CIRCULATION_MONTHLY_VALUE_COUNT: usize =
    match MAX_CIRCULATION_CELL_COUNT.checked_mul(super::CLIMATE_MONTH_COUNT) {
        Some(count) => count,
        None => panic!("circulation monthly value limit overflowed usize"),
    };
