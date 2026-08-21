use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::world::{Meters, SphericalSpaceSpec, SphericalSpecError};

/// The only supported serialized natural-resolution plan schema.
pub const NATURAL_RESOLUTION_PLAN_SCHEMA_V1: u16 = 1;

/// A semantic, coordinated natural-world quality level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NaturalQualityProfile {
    /// Fast authoring and deterministic test profile.
    Draft,
    /// Default finished-product target once the complete pipeline meets budget.
    Standard,
    /// Background export and close-inspection profile.
    High,
}

impl NaturalQualityProfile {
    /// Returns the authoritative spherical-cell request for this profile.
    pub const fn authoritative_target_cell_count(self) -> u32 {
        match self {
            Self::Draft => 20_000,
            Self::Standard => 80_000,
            Self::High => 200_000,
        }
    }

    /// Returns the transient tectonic-control spherical-cell request.
    pub const fn tectonic_control_target_cell_count(self) -> u32 {
        match self {
            Self::Draft => 4_842,
            Self::Standard | Self::High => 20_000,
        }
    }

    /// Returns the cubed-sphere face resolution reserved for production climate.
    pub const fn climate_face_resolution(self) -> u16 {
        match self {
            Self::Draft => 24,
            Self::Standard => 32,
            Self::High => 48,
        }
    }

    /// Exact bounded formation horizon for the global circulation solver.
    pub const fn global_circulation_formation_years_max(self) -> u16 {
        match self {
            Self::Draft => 8,
            Self::Standard => 10,
            Self::High => 12,
        }
    }

    /// Resolves this profile against an exact authoritative spherical-space request.
    pub fn resolve(
        self,
        authoritative: &SphericalSpaceSpec,
    ) -> Result<NaturalResolutionPlan, NaturalProfileError> {
        authoritative.validate()?;
        let expected = self.authoritative_target_cell_count();
        if authoritative.target_cell_count != expected {
            return Err(NaturalProfileError::AuthoritativeTargetMismatch {
                profile: self,
                found: authoritative.target_cell_count,
                expected,
            });
        }
        let control = SphericalSpaceSpec {
            radius: authoritative.radius,
            target_cell_count: self.tectonic_control_target_cell_count(),
        };
        control.validate()?;
        Ok(NaturalResolutionPlan {
            schema_version: NATURAL_RESOLUTION_PLAN_SCHEMA_V1,
            profile: self,
            radius: authoritative.radius,
            authoritative_target_cell_count: authoritative.target_cell_count,
            authoritative_resolved_cell_count: authoritative.resolved_cell_count(),
            tectonic_control_target_cell_count: control.target_cell_count,
            tectonic_control_resolved_cell_count: control.resolved_cell_count(),
            climate_face_resolution: self.climate_face_resolution(),
        })
    }
}

/// A versioned record of every work-grid count selected by one quality profile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NaturalResolutionPlan {
    schema_version: u16,
    profile: NaturalQualityProfile,
    radius: Meters,
    authoritative_target_cell_count: u32,
    authoritative_resolved_cell_count: u32,
    tectonic_control_target_cell_count: u32,
    tectonic_control_resolved_cell_count: u32,
    climate_face_resolution: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NaturalResolutionPlanWire {
    schema_version: u16,
    profile: NaturalQualityProfile,
    radius: Meters,
    authoritative_target_cell_count: u32,
    authoritative_resolved_cell_count: u32,
    tectonic_control_target_cell_count: u32,
    tectonic_control_resolved_cell_count: u32,
    climate_face_resolution: u16,
}

impl NaturalResolutionPlan {
    /// Rechecks the schema, profile settings, radius, and requested/resolved counts.
    pub fn validate(&self) -> Result<(), NaturalProfileError> {
        if self.schema_version != NATURAL_RESOLUTION_PLAN_SCHEMA_V1 {
            return Err(NaturalProfileError::UnsupportedSchema {
                found: self.schema_version,
                supported: NATURAL_RESOLUTION_PLAN_SCHEMA_V1,
            });
        }
        let expected = self.profile.resolve(&SphericalSpaceSpec {
            radius: self.radius,
            target_cell_count: self.authoritative_target_cell_count,
        })?;
        for (field, found, expected) in [
            (
                "authoritative_resolved_cell_count",
                self.authoritative_resolved_cell_count,
                expected.authoritative_resolved_cell_count,
            ),
            (
                "tectonic_control_target_cell_count",
                self.tectonic_control_target_cell_count,
                expected.tectonic_control_target_cell_count,
            ),
            (
                "tectonic_control_resolved_cell_count",
                self.tectonic_control_resolved_cell_count,
                expected.tectonic_control_resolved_cell_count,
            ),
            (
                "climate_face_resolution",
                u32::from(self.climate_face_resolution),
                u32::from(expected.climate_face_resolution),
            ),
        ] {
            if found != expected {
                return Err(NaturalProfileError::ResolutionMismatch {
                    field,
                    found,
                    expected,
                });
            }
        }
        Ok(())
    }

    /// Returns the serialized plan schema.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the selected semantic quality profile.
    pub const fn profile(&self) -> NaturalQualityProfile {
        self.profile
    }

    /// Returns the shared planet radius used by all work grids.
    pub const fn radius(&self) -> Meters {
        self.radius
    }

    /// Returns the requested authoritative cell count.
    pub const fn authoritative_target_cell_count(&self) -> u32 {
        self.authoritative_target_cell_count
    }

    /// Returns the exact authoritative geodesic cell count.
    pub const fn authoritative_resolved_cell_count(&self) -> u32 {
        self.authoritative_resolved_cell_count
    }

    /// Returns the requested transient tectonic-control cell count.
    pub const fn tectonic_control_target_cell_count(&self) -> u32 {
        self.tectonic_control_target_cell_count
    }

    /// Returns the exact transient tectonic-control geodesic cell count.
    pub const fn tectonic_control_resolved_cell_count(&self) -> u32 {
        self.tectonic_control_resolved_cell_count
    }

    /// Returns the production cubed-sphere face resolution.
    pub const fn climate_face_resolution(&self) -> u16 {
        self.climate_face_resolution
    }

    /// Reconstructs the authoritative spherical-space specification exactly.
    pub const fn authoritative_space_spec(&self) -> SphericalSpaceSpec {
        SphericalSpaceSpec {
            radius: self.radius,
            target_cell_count: self.authoritative_target_cell_count,
        }
    }

    /// Reconstructs the transient tectonic-control spherical-space specification.
    pub const fn tectonic_control_space_spec(&self) -> SphericalSpaceSpec {
        SphericalSpaceSpec {
            radius: self.radius,
            target_cell_count: self.tectonic_control_target_cell_count,
        }
    }
}

impl<'de> Deserialize<'de> for NaturalResolutionPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = NaturalResolutionPlanWire::deserialize(deserializer)?;
        let plan = Self {
            schema_version: wire.schema_version,
            profile: wire.profile,
            radius: wire.radius,
            authoritative_target_cell_count: wire.authoritative_target_cell_count,
            authoritative_resolved_cell_count: wire.authoritative_resolved_cell_count,
            tectonic_control_target_cell_count: wire.tectonic_control_target_cell_count,
            tectonic_control_resolved_cell_count: wire.tectonic_control_resolved_cell_count,
            climate_face_resolution: wire.climate_face_resolution,
        };
        plan.validate().map_err(D::Error::custom)?;
        Ok(plan)
    }
}

/// Errors returned by strict natural-profile resolution and validation.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum NaturalProfileError {
    /// A serialized plan uses a schema this release cannot interpret.
    #[error(
        "unsupported natural resolution plan schema {found}; supported version is {supported}"
    )]
    UnsupportedSchema { found: u16, supported: u16 },
    /// The authoritative spherical-space specification is invalid.
    #[error("invalid natural profile spherical space: {0}")]
    InvalidSpace(#[from] SphericalSpecError),
    /// The selected profile and geometric source of truth disagree.
    #[error("{profile:?} profile requires authoritative target {expected}, found {found}")]
    AuthoritativeTargetMismatch {
        profile: NaturalQualityProfile,
        found: u32,
        expected: u32,
    },
    /// A stored derived setting no longer matches the selected profile.
    #[error("natural resolution field {field} is {found}; expected {expected}")]
    ResolutionMismatch {
        field: &'static str,
        found: u32,
        expected: u32,
    },
}
