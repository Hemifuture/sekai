use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    EvolvedTectonicSnapshot, GeologicSubstrateSnapshot, GlobalCirculationSnapshot,
    NaturalQualityReport, NaturalSurfaceFormationSnapshot, PrimaryReliefSnapshot,
    ResolvedFormationTimeline,
};
use crate::world::spatial::SurfaceRef;

/// Schema for one atomically published natural-formation current state.
pub const NATURAL_FORMATION_BUNDLE_SCHEMA_V1: u16 = 1;

/// Final P2/P3/P4/P5 siblings and their quality evidence for one current state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NaturalFormationBundle {
    schema_version: u16,
    surface_ref: SurfaceRef,
    timeline: ResolvedFormationTimeline,
    tectonics: EvolvedTectonicSnapshot,
    substrate: GeologicSubstrateSnapshot,
    primary_relief: PrimaryReliefSnapshot,
    climate: GlobalCirculationSnapshot,
    surface_formation: NaturalSurfaceFormationSnapshot,
    tectonic_quality: NaturalQualityReport,
    primary_relief_quality: NaturalQualityReport,
    climate_quality: NaturalQualityReport,
    surface_quality: NaturalQualityReport,
}

/// Validated construction payload shared by generation and strict deserialization.
pub(crate) struct NaturalFormationBundleParts {
    pub schema_version: u16,
    pub surface_ref: SurfaceRef,
    pub timeline: ResolvedFormationTimeline,
    pub tectonics: EvolvedTectonicSnapshot,
    pub substrate: GeologicSubstrateSnapshot,
    pub primary_relief: PrimaryReliefSnapshot,
    pub climate: GlobalCirculationSnapshot,
    pub surface_formation: NaturalSurfaceFormationSnapshot,
    pub tectonic_quality: NaturalQualityReport,
    pub primary_relief_quality: NaturalQualityReport,
    pub climate_quality: NaturalQualityReport,
    pub surface_quality: NaturalQualityReport,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NaturalFormationBundleWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    timeline: ResolvedFormationTimeline,
    tectonics: EvolvedTectonicSnapshot,
    substrate: GeologicSubstrateSnapshot,
    primary_relief: PrimaryReliefSnapshot,
    climate: GlobalCirculationSnapshot,
    surface_formation: NaturalSurfaceFormationSnapshot,
    tectonic_quality: NaturalQualityReport,
    primary_relief_quality: NaturalQualityReport,
    climate_quality: NaturalQualityReport,
    surface_quality: NaturalQualityReport,
}

impl NaturalFormationBundle {
    /// Constructs a bundle only after every local and cross-sibling invariant holds.
    pub(crate) fn new(
        parts: NaturalFormationBundleParts,
    ) -> Result<Self, NaturalFormationBundleValidationError> {
        let bundle = Self {
            schema_version: parts.schema_version,
            surface_ref: parts.surface_ref,
            timeline: parts.timeline,
            tectonics: parts.tectonics,
            substrate: parts.substrate,
            primary_relief: parts.primary_relief,
            climate: parts.climate,
            surface_formation: parts.surface_formation,
            tectonic_quality: parts.tectonic_quality,
            primary_relief_quality: parts.primary_relief_quality,
            climate_quality: parts.climate_quality,
            surface_quality: parts.surface_quality,
        };
        bundle.validate()?;
        Ok(bundle)
    }

    /// Revalidates the current-state schema and every cross-sibling identity.
    pub fn validate(&self) -> Result<(), NaturalFormationBundleValidationError> {
        if self.schema_version != NATURAL_FORMATION_BUNDLE_SCHEMA_V1 {
            return Err(NaturalFormationBundleValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: NATURAL_FORMATION_BUNDLE_SCHEMA_V1,
            });
        }
        self.surface_ref.validate().map_err(|error| {
            NaturalFormationBundleValidationError::InvalidNested {
                role: "surface_ref",
                reason: error.to_string(),
            }
        })?;
        if !self.surface_ref.geometry_kind().is_spherical() {
            return Err(NaturalFormationBundleValidationError::InvalidNested {
                role: "surface_ref",
                reason: "natural formation requires a spherical surface".to_owned(),
            });
        }
        self.timeline.validate().map_err(|error| {
            NaturalFormationBundleValidationError::InvalidNested {
                role: "timeline",
                reason: error.to_string(),
            }
        })?;
        self.tectonics.validate().map_err(|error| {
            NaturalFormationBundleValidationError::invalid_domain("tectonics", error)
        })?;
        self.substrate.validate().map_err(|error| {
            NaturalFormationBundleValidationError::invalid_domain("substrate", error)
        })?;
        self.primary_relief.validate().map_err(|error| {
            NaturalFormationBundleValidationError::invalid_domain("primary_relief", error)
        })?;
        self.climate.validate().map_err(|error| {
            NaturalFormationBundleValidationError::invalid_domain("climate", error)
        })?;
        self.surface_formation.validate().map_err(|error| {
            NaturalFormationBundleValidationError::invalid_domain("surface_formation", error)
        })?;

        for (role, found) in [
            ("tectonics", self.tectonics.surface_ref()),
            ("substrate", self.substrate.surface_ref()),
            ("primary_relief", self.primary_relief.surface_ref()),
            ("climate", self.climate.surface_ref()),
            ("surface_formation", self.surface_formation.surface_ref()),
        ] {
            if found != self.surface_ref {
                return Err(NaturalFormationBundleValidationError::SurfaceMismatch {
                    role,
                    found,
                    expected: self.surface_ref,
                });
            }
        }

        let terrain = self.surface_formation.terrain_fields();
        if terrain.elevation_components().primary_elevation_m() != self.primary_relief.elevation_m()
            || terrain.water_inventory_m3().to_bits()
                != self.primary_relief.water_inventory_m3().to_bits()
        {
            return Err(NaturalFormationBundleValidationError::IdentityMismatch {
                field: "primary_relief_to_surface_formation",
            });
        }
        if self
            .surface_formation
            .checkpoint()
            .upstream()
            .formation_climate_checkpoint_fingerprint()
            != self.climate.checkpoint().fingerprint()
        {
            return Err(NaturalFormationBundleValidationError::IdentityMismatch {
                field: "formation_climate_checkpoint_fingerprint",
            });
        }

        for (role, report) in [
            ("tectonic_quality", &self.tectonic_quality),
            ("primary_relief_quality", &self.primary_relief_quality),
            ("climate_quality", &self.climate_quality),
            ("surface_quality", &self.surface_quality),
        ] {
            report.validate().map_err(|error| {
                NaturalFormationBundleValidationError::InvalidNested {
                    role,
                    reason: error.to_string(),
                }
            })?;
            if report.surface_ref() != self.surface_ref {
                return Err(NaturalFormationBundleValidationError::SurfaceMismatch {
                    role,
                    found: report.surface_ref(),
                    expected: self.surface_ref,
                });
            }
        }
        if self.climate_quality.subject_fingerprint()
            != Some(self.climate.checkpoint().fingerprint())
        {
            return Err(NaturalFormationBundleValidationError::IdentityMismatch {
                field: "climate_quality_subject",
            });
        }
        if self.surface_quality.subject_fingerprint()
            != Some(self.surface_formation.checkpoint().fingerprint())
        {
            return Err(NaturalFormationBundleValidationError::IdentityMismatch {
                field: "surface_quality_subject",
            });
        }
        Ok(())
    }

    /// Returns the bundle schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the shared authoritative surface identity.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns the resolved private formation schedule identity.
    pub const fn timeline(&self) -> ResolvedFormationTimeline {
        self.timeline
    }

    /// Returns the final P2 sibling.
    pub const fn tectonics(&self) -> &EvolvedTectonicSnapshot {
        &self.tectonics
    }

    /// Returns the final P3 substrate sibling.
    pub const fn substrate(&self) -> &GeologicSubstrateSnapshot {
        &self.substrate
    }

    /// Returns the final P3 primary-relief sibling.
    pub const fn primary_relief(&self) -> &PrimaryReliefSnapshot {
        &self.primary_relief
    }

    /// Returns the endpoint P4 sibling forced by final P5 terrain.
    pub const fn climate(&self) -> &GlobalCirculationSnapshot {
        &self.climate
    }

    /// Returns the final P5 sibling without a nested climate copy.
    pub const fn surface_formation(&self) -> &NaturalSurfaceFormationSnapshot {
        &self.surface_formation
    }

    /// Returns P2 quality evidence.
    pub const fn tectonic_quality(&self) -> &NaturalQualityReport {
        &self.tectonic_quality
    }

    /// Returns P3 quality evidence.
    pub const fn primary_relief_quality(&self) -> &NaturalQualityReport {
        &self.primary_relief_quality
    }

    /// Returns endpoint P4 quality evidence.
    pub const fn climate_quality(&self) -> &NaturalQualityReport {
        &self.climate_quality
    }

    /// Returns P5 quality evidence.
    pub const fn surface_quality(&self) -> &NaturalQualityReport {
        &self.surface_quality
    }
}

impl<'de> Deserialize<'de> for NaturalFormationBundle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = NaturalFormationBundleWire::deserialize(deserializer)?;
        Self::new(NaturalFormationBundleParts {
            schema_version: wire.schema_version,
            surface_ref: wire.surface_ref,
            timeline: wire.timeline,
            tectonics: wire.tectonics,
            substrate: wire.substrate,
            primary_relief: wire.primary_relief,
            climate: wire.climate,
            surface_formation: wire.surface_formation,
            tectonic_quality: wire.tectonic_quality,
            primary_relief_quality: wire.primary_relief_quality,
            climate_quality: wire.climate_quality,
            surface_quality: wire.surface_quality,
        })
        .map_err(D::Error::custom)
    }
}

/// Failures that reject a partial or contradictory formation bundle.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum NaturalFormationBundleValidationError {
    /// The serialized schema is not the current bundle contract.
    #[error(
        "unsupported natural formation bundle schema {found}; supported schema is {supported}"
    )]
    UnsupportedSchema { found: u16, supported: u16 },
    /// A nested domain or report failed its own invariant.
    #[error("invalid natural formation {role}: {reason}")]
    InvalidNested { role: &'static str, reason: String },
    /// A sibling or report belongs to a different authoritative surface.
    #[error("natural formation {role} references {found:?}; expected {expected:?}")]
    SurfaceMismatch {
        role: &'static str,
        found: SurfaceRef,
        expected: SurfaceRef,
    },
    /// Two final siblings disagree on a required content identity.
    #[error("natural formation identity mismatch for {field}")]
    IdentityMismatch { field: &'static str },
}

impl NaturalFormationBundleValidationError {
    fn invalid_domain(role: &'static str, error: impl ToString) -> Self {
        Self::InvalidNested {
            role,
            reason: error.to_string(),
        }
    }
}
