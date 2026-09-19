use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::Meters;

/// The supported version of the serialized world specification schema.
pub const WORLD_SPEC_SCHEMA_V1: u16 = 1;
/// The smallest cell count allowed by the V1 numerical-safety budget.
pub const MIN_CELL_COUNT: u32 = 16;
/// The largest cell count allowed by the V1 numerical-safety budget.
pub const MAX_CELL_COUNT: u32 = 200_000;
/// The smallest spatial dimension allowed by the V1 numerical-safety budget, in meters.
pub const MIN_DIMENSION_METERS: f64 = 1.0;
/// The largest spatial dimension allowed by the V1 numerical-safety budget, in meters.
pub const MAX_DIMENSION_METERS: f64 = 100_000_000.0;
/// The smallest supported geodesic subdivision frequency.
pub const MIN_GEODESIC_FREQUENCY: u32 = 2;
/// The largest supported geodesic subdivision frequency.
pub const MAX_GEODESIC_FREQUENCY: u32 = 141;
/// The smallest supported geodesic cell allocation.
pub const MIN_SPHERICAL_CELL_COUNT: u32 = geodesic_cell_count(MIN_GEODESIC_FREQUENCY);
/// The largest supported geodesic cell allocation.
pub const MAX_SPHERICAL_CELL_COUNT: u32 = geodesic_cell_count(MAX_GEODESIC_FREQUENCY);
/// The largest requested spherical cell target; it resolves to the bounded allocation above.
pub const MAX_SPHERICAL_TARGET_CELL_COUNT: u32 = MAX_CELL_COUNT;
/// The largest authoritative spherical-surface vertex allocation in schema V1.
pub const MAX_SPHERICAL_VERTEX_COUNT: u32 = 20 * frequency_squared(MAX_GEODESIC_FREQUENCY);
/// The largest authoritative spherical-surface edge allocation in schema V1.
pub const MAX_SPHERICAL_EDGE_COUNT: u32 = 30 * frequency_squared(MAX_GEODESIC_FREQUENCY);
/// The largest authoritative spherical-surface cell boundary degree in schema V1.
pub const MAX_SPHERICAL_CELL_BOUNDARY_DEGREE: usize = 6;

const MIN_SPHERICAL_RADIUS_METERS: f64 = 1.0;
const MAX_SPHERICAL_RADIUS_METERS: f64 = 100_000_000.0;

const fn frequency_squared(frequency: u32) -> u32 {
    frequency * frequency
}

const fn geodesic_cell_count(frequency: u32) -> u32 {
    10 * frequency_squared(frequency) + 2
}

/// The condition applied at the outer boundary of world space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundaryCondition {
    /// Stops world-space traversal at the outer boundary.
    Closed,
}

/// The validated radius and allocation budget for a geodesic spherical world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SphericalSpaceSpec {
    /// The radius of the spherical world.
    pub radius: Meters,
    /// The requested number of generated surface cells.
    pub target_cell_count: u32,
}

impl SphericalSpaceSpec {
    /// Validates the spherical numerical-safety and allocation budgets.
    pub fn validate(&self) -> Result<(), SphericalSpecError> {
        let radius = self.radius.get();
        if !(MIN_SPHERICAL_RADIUS_METERS..=MAX_SPHERICAL_RADIUS_METERS).contains(&radius) {
            return Err(SphericalSpecError::RadiusOutOfRange {
                found: radius,
                min: MIN_SPHERICAL_RADIUS_METERS,
                max: MAX_SPHERICAL_RADIUS_METERS,
            });
        }

        if !(MIN_SPHERICAL_CELL_COUNT..=MAX_SPHERICAL_TARGET_CELL_COUNT)
            .contains(&self.target_cell_count)
        {
            return Err(SphericalSpecError::CellCountOutOfRange {
                found: self.target_cell_count,
                min: MIN_SPHERICAL_CELL_COUNT,
                max: MAX_SPHERICAL_TARGET_CELL_COUNT,
            });
        }

        Ok(())
    }

    /// Resolves the requested allocation to the nearest available geodesic frequency.
    pub fn resolved_frequency(&self) -> u32 {
        let estimate = ((f64::from(self.target_cell_count) - 2.0) / 10.0).sqrt();
        let lower = (estimate.floor() as u32).clamp(MIN_GEODESIC_FREQUENCY, MAX_GEODESIC_FREQUENCY);
        let upper = lower
            .checked_add(1)
            .expect("bounded geodesic frequency")
            .min(MAX_GEODESIC_FREQUENCY);
        let lower_count = geodesic_cell_count(lower);
        let upper_count = geodesic_cell_count(upper);

        if self.target_cell_count.abs_diff(lower_count)
            <= self.target_cell_count.abs_diff(upper_count)
        {
            lower
        } else {
            upper
        }
    }

    /// Returns the exact generated cell count for the resolved geodesic frequency.
    pub fn resolved_cell_count(&self) -> u32 {
        geodesic_cell_count(self.resolved_frequency())
    }
}

/// Errors returned when a spherical world specification exceeds its safety budget.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalSpecError {
    /// A spherical radius lies outside the numerical-safety budget.
    #[error("radius {found} is outside {min}..={max} meters")]
    RadiusOutOfRange {
        /// The radius that failed validation, in meters.
        found: f64,
        /// The inclusive lower radius limit, in meters.
        min: f64,
        /// The inclusive upper radius limit, in meters.
        max: f64,
    },
    /// The requested cell count lies outside the spherical allocation-safety budget.
    #[error("cell count {found} is outside {min}..={max}")]
    CellCountOutOfRange {
        /// The cell count that failed validation.
        found: u32,
        /// The inclusive lower cell-count limit.
        min: u32,
        /// The inclusive upper cell-count limit.
        max: u32,
    },
}
