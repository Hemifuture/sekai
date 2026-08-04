use std::collections::BTreeSet;
use std::f64::consts::PI;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    Hotspot, MantleValidationError, HEAT_FLOW_MAX_MW_M2, HEAT_FLOW_MIN_MW_M2, MAX_HOTSPOT_COUNT,
};
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceGeometryKind, SurfaceRef,
    SurfaceRefError,
};
use crate::world::{CellId, HotspotId};

/// The supported schema for surface-bound spherical mantle snapshots.
pub const MANTLE_SNAPSHOT_SCHEMA_V2: u16 = 2;

/// Immutable present-day mantle forcing bound to one authoritative spherical surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalMantleSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    hotspots: Vec<Hotspot>,
    heat_flow_mw_m2: Vec<f32>,
    volcanic_influence: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalMantleSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    hotspots: Vec<Hotspot>,
    heat_flow_mw_m2: Vec<f32>,
    volcanic_influence: Vec<f32>,
}

impl SphericalMantleSnapshot {
    /// Canonicalizes hotspot order and validates every self-contained invariant.
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        mut hotspots: Vec<Hotspot>,
        heat_flow_mw_m2: Vec<f32>,
        volcanic_influence: Vec<f32>,
    ) -> Result<Self, SphericalMantleValidationError> {
        hotspots.sort_by_key(Hotspot::id);
        let snapshot = Self {
            schema_version,
            surface_ref,
            hotspots,
            heat_flow_mw_m2,
            volcanic_influence,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks invariants that do not require authoritative surface records.
    pub fn validate(&self) -> Result<(), SphericalMantleValidationError> {
        if self.schema_version != MANTLE_SNAPSHOT_SCHEMA_V2 {
            return Err(SphericalMantleValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: MANTLE_SNAPSHOT_SCHEMA_V2,
            });
        }
        self.surface_ref.validate()?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(SphericalMantleValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        if self.hotspots.len() > usize::from(MAX_HOTSPOT_COUNT) {
            return Err(SphericalMantleValidationError::TooManyHotspots {
                found: self.hotspots.len(),
                max: MAX_HOTSPOT_COUNT,
            });
        }

        let cell_count = self.surface_ref.cell_count() as usize;
        let mut source_cells = BTreeSet::new();
        for (index, hotspot) in self.hotspots.iter().enumerate() {
            hotspot
                .validate()
                .map_err(SphericalMantleValidationError::InvalidHotspot)?;
            let expected = HotspotId::from_raw(index as u32);
            if hotspot.id() != expected {
                return Err(SphericalMantleValidationError::NonContiguousHotspotId {
                    expected,
                    found: hotspot.id(),
                });
            }
            if hotspot.source_cell().raw() as usize >= cell_count {
                return Err(
                    SphericalMantleValidationError::HotspotSourceCellOutOfRange {
                        hotspot_id: hotspot.id(),
                        source_cell: hotspot.source_cell(),
                        cell_count,
                    },
                );
            }
            if !source_cells.insert(hotspot.source_cell()) {
                return Err(SphericalMantleValidationError::DuplicateHotspotSourceCell {
                    source_cell: hotspot.source_cell(),
                });
            }
        }

        validate_length("heat_flow_mw_m2", self.heat_flow_mw_m2.len(), cell_count)?;
        validate_length(
            "volcanic_influence",
            self.volcanic_influence.len(),
            cell_count,
        )?;
        for (index, &value) in self.heat_flow_mw_m2.iter().enumerate() {
            if !value.is_finite() || !(HEAT_FLOW_MIN_MW_M2..=HEAT_FLOW_MAX_MW_M2).contains(&value) {
                return Err(SphericalMantleValidationError::HeatFlowOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: value,
                    min: HEAT_FLOW_MIN_MW_M2,
                    max: HEAT_FLOW_MAX_MW_M2,
                });
            }
        }
        for (index, &value) in self.volcanic_influence.iter().enumerate() {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(
                    SphericalMantleValidationError::VolcanicInfluenceOutOfRange {
                        cell: CellId::from_raw(index as u32),
                        found: value,
                    },
                );
            }
        }
        Ok(())
    }

    /// Rechecks exact surface identity and spherical support-distance bounds.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), SphericalMantleValidationError> {
        self.validate()?;
        surface.validate()?;
        let authoritative = SurfaceRef::try_for_spherical(surface)?;
        if self.surface_ref != authoritative {
            return Err(SphericalMantleValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        let maximum_m = PI * surface.radius().get();
        for hotspot in &self.hotspots {
            if hotspot.support_radius_m().get() > maximum_m {
                return Err(
                    SphericalMantleValidationError::SupportRadiusExceedsHemisphere {
                        hotspot_id: hotspot.id(),
                        found_m: hotspot.support_radius_m().get(),
                        maximum_m,
                    },
                );
            }
        }
        Ok(())
    }

    /// Returns the V2 schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact authoritative surface identity.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns current mantle hotspots in stable identifier order.
    pub fn hotspots(&self) -> &[Hotspot] {
        &self.hotspots
    }

    /// Returns surface heat flow in milliwatts per square meter.
    pub fn heat_flow_mw_m2(&self) -> &[f32] {
        &self.heat_flow_mw_m2
    }

    /// Returns normalized present-day volcanic influence.
    pub fn volcanic_influence(&self) -> &[f32] {
        &self.volcanic_influence
    }
}

impl<'de> Deserialize<'de> for SphericalMantleSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalMantleSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.hotspots,
            wire.heat_flow_mw_m2,
            wire.volcanic_influence,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_length(
    field: &'static str,
    found: usize,
    expected: usize,
) -> Result<(), SphericalMantleValidationError> {
    if found != expected {
        return Err(SphericalMantleValidationError::FieldLengthMismatch {
            field,
            expected,
            found,
        });
    }
    Ok(())
}

/// Failures in surface-bound spherical mantle contracts.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalMantleValidationError {
    /// The snapshot uses an unsupported schema version.
    #[error(
        "unsupported spherical mantle schema version {found}; supported version is {supported}"
    )]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
        /// The supported schema version.
        supported: u16,
    },
    /// The stored surface identity is malformed.
    #[error("invalid surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    /// The stored identity does not describe spherical V1 geometry.
    #[error("spherical mantle requires spherical_v1 geometry, found {found:?}")]
    InvalidSurfaceKind {
        /// The rejected geometry kind.
        found: SurfaceGeometryKind,
    },
    /// The snapshot exceeds the hotspot allocation budget.
    #[error("hotspot count {found} exceeds the maximum {max}")]
    TooManyHotspots {
        /// The rejected hotspot count.
        found: usize,
        /// The inclusive maximum.
        max: u16,
    },
    /// A reused hotspot primitive is invalid.
    #[error("invalid hotspot: {0}")]
    InvalidHotspot(MantleValidationError),
    /// Hotspot identifiers are not the exact contiguous range from zero.
    #[error("expected hotspot ID {expected:?}, found {found:?}")]
    NonContiguousHotspotId {
        /// The required identifier.
        expected: HotspotId,
        /// The stored identifier.
        found: HotspotId,
    },
    /// Several hotspots use the same source cell.
    #[error("several hotspots use source cell {source_cell:?}")]
    DuplicateHotspotSourceCell {
        /// The duplicated source cell.
        source_cell: CellId,
    },
    /// A hotspot source lies outside the surface allocation.
    #[error("hotspot {hotspot_id:?} source {source_cell:?} lies outside cell count {cell_count}")]
    HotspotSourceCellOutOfRange {
        /// The hotspot with the invalid source.
        hotspot_id: HotspotId,
        /// The rejected source cell.
        source_cell: CellId,
        /// The authoritative dense cell count.
        cell_count: usize,
    },
    /// A dense field length differs from the surface identity.
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        /// The stable field name.
        field: &'static str,
        /// The required dense length.
        expected: usize,
        /// The actual dense length.
        found: usize,
    },
    /// A heat-flow value is non-finite or outside its physical envelope.
    #[error("heat flow at {cell:?} is {found}; expected finite {min}..={max}")]
    HeatFlowOutOfRange {
        /// The cell containing the rejected value.
        cell: CellId,
        /// The rejected value.
        found: f32,
        /// The inclusive lower bound.
        min: f32,
        /// The inclusive upper bound.
        max: f32,
    },
    /// A volcanic-influence value is non-finite or outside zero to one.
    #[error("volcanic influence at {cell:?} is {found}; expected finite 0..=1")]
    VolcanicInfluenceOutOfRange {
        /// The cell containing the rejected value.
        cell: CellId,
        /// The rejected value.
        found: f32,
    },
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The exact surface identity differs from the authoritative snapshot.
    #[error("mantle surface identity {snapshot:?} does not match {authoritative:?}")]
    SurfaceMismatch {
        /// Identity stored by the mantle snapshot.
        snapshot: SurfaceRef,
        /// Identity derived from the authoritative surface.
        authoritative: SurfaceRef,
    },
    /// A support radius exceeds the longest useful geodesic distance.
    #[error(
        "hotspot {hotspot_id:?} support radius {found_m} m exceeds half-circumference {maximum_m} m"
    )]
    SupportRadiusExceedsHemisphere {
        /// The hotspot with excessive support.
        hotspot_id: HotspotId,
        /// The rejected support radius in meters.
        found_m: f64,
        /// The spherical half-circumference in meters.
        maximum_m: f64,
    },
}
