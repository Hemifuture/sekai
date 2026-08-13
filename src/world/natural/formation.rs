use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// The supported version of a requested world-formation specification.
pub const WORLD_FORMATION_SPEC_SCHEMA_V1: u16 = 1;
/// The supported version of a resolved world-formation selection.
pub const RESOLVED_WORLD_FORMATION_SCHEMA_V1: u16 = 1;

/// An author-facing macro formation choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WorldFormationPreset {
    /// Selects one concrete preset deterministically from the root seed.
    Random,
    /// Favors several separated major continents.
    Continents,
    /// Favors many separated small landmasses.
    Archipelago,
    /// Favors one dominant continental mass.
    Supercontinent,
    /// Favors one large island with smaller satellites.
    GreatIsland,
    /// Favors small landmasses with stronger independent mantle forcing.
    VolcanicIslands,
}

/// A concrete macro formation choice consumed by generators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ResolvedWorldFormationPreset {
    /// Several separated major continents.
    Continents,
    /// Many separated small landmasses.
    Archipelago,
    /// One dominant continental mass.
    Supercontinent,
    /// One large island with smaller satellites.
    GreatIsland,
    /// Small landmasses with stronger independent mantle forcing.
    VolcanicIslands,
}

impl ResolvedWorldFormationPreset {
    /// Returns the initial continental-crust recommendation paired with this morphology.
    pub const fn recommended_continental_crust_fraction(self) -> f32 {
        match self {
            Self::Continents => 0.38,
            Self::Archipelago => 0.26,
            Self::Supercontinent => 0.42,
            Self::GreatIsland => 0.28,
            Self::VolcanicIslands => 0.16,
        }
    }
}

/// The narrow mantle-facing projection of a resolved formation choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MantleFormationBias {
    /// Leaves the resolved geologic specification unchanged.
    Neutral,
    /// Adds a bounded active-hotspot prior without reading tectonic state.
    VolcanicIslands,
}

/// A versioned request for the world's macro formation prior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorldFormationSpec {
    /// The schema version used to interpret this specification.
    pub schema_version: u16,
    /// The requested named or deterministic-random preset.
    pub preset: WorldFormationPreset,
}

impl Default for WorldFormationSpec {
    fn default() -> Self {
        Self {
            schema_version: WORLD_FORMATION_SPEC_SCHEMA_V1,
            preset: WorldFormationPreset::Continents,
        }
    }
}

impl WorldFormationSpec {
    /// Validates the serialized specification schema.
    pub fn validate(&self) -> Result<(), WorldFormationSpecError> {
        if self.schema_version != WORLD_FORMATION_SPEC_SCHEMA_V1 {
            return Err(WorldFormationSpecError::UnsupportedSpecSchema {
                found: self.schema_version,
                supported: WORLD_FORMATION_SPEC_SCHEMA_V1,
            });
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct WorldFormationSpecWire {
    schema_version: u16,
    preset: WorldFormationPreset,
}

impl<'de> Deserialize<'de> for WorldFormationSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorldFormationSpecWire::deserialize(deserializer)?;
        let spec = Self {
            schema_version: wire.schema_version,
            preset: wire.preset,
        };
        spec.validate().map_err(serde::de::Error::custom)?;
        Ok(spec)
    }
}

/// A validated concrete formation selection and its requested provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedWorldFormation {
    schema_version: u16,
    requested: WorldFormationPreset,
    resolved: ResolvedWorldFormationPreset,
}

impl ResolvedWorldFormation {
    /// Constructs a resolved selection only for a supported schema.
    pub fn new(
        schema_version: u16,
        requested: WorldFormationPreset,
        resolved: ResolvedWorldFormationPreset,
    ) -> Result<Self, WorldFormationSpecError> {
        let formation = Self {
            schema_version,
            requested,
            resolved,
        };
        formation.validate()?;
        Ok(formation)
    }

    /// Validates the serialized resolved-selection schema.
    pub fn validate(&self) -> Result<(), WorldFormationSpecError> {
        if self.schema_version != RESOLVED_WORLD_FORMATION_SCHEMA_V1 {
            return Err(WorldFormationSpecError::UnsupportedResolvedSchema {
                found: self.schema_version,
                supported: RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            });
        }
        Ok(())
    }

    /// Returns the author-facing requested preset.
    pub const fn requested(&self) -> WorldFormationPreset {
        self.requested
    }

    /// Returns the concrete preset consumed by generators.
    pub const fn resolved(&self) -> ResolvedWorldFormationPreset {
        self.resolved
    }

    /// Returns the narrow mantle-facing formation projection.
    pub const fn mantle_bias(&self) -> MantleFormationBias {
        match self.resolved {
            ResolvedWorldFormationPreset::VolcanicIslands => MantleFormationBias::VolcanicIslands,
            ResolvedWorldFormationPreset::Continents
            | ResolvedWorldFormationPreset::Archipelago
            | ResolvedWorldFormationPreset::Supercontinent
            | ResolvedWorldFormationPreset::GreatIsland => MantleFormationBias::Neutral,
        }
    }

    /// Returns the visible initial continental-crust recommendation.
    pub const fn recommended_continental_crust_fraction(&self) -> f32 {
        self.resolved.recommended_continental_crust_fraction()
    }
}

#[derive(Deserialize)]
struct ResolvedWorldFormationWire {
    schema_version: u16,
    requested: WorldFormationPreset,
    resolved: ResolvedWorldFormationPreset,
}

impl<'de> Deserialize<'de> for ResolvedWorldFormation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ResolvedWorldFormationWire::deserialize(deserializer)?;
        Self::new(wire.schema_version, wire.requested, wire.resolved)
            .map_err(serde::de::Error::custom)
    }
}

/// Errors returned by world-formation specification validation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldFormationSpecError {
    /// A requested specification uses an unsupported schema.
    #[error("unsupported world-formation spec schema {found}; supported version is {supported}")]
    UnsupportedSpecSchema {
        /// The encountered schema version.
        found: u16,
        /// The only supported schema version.
        supported: u16,
    },
    /// A resolved selection uses an unsupported schema.
    #[error(
        "unsupported resolved world-formation schema {found}; supported version is {supported}"
    )]
    UnsupportedResolvedSchema {
        /// The encountered schema version.
        found: u16,
        /// The only supported schema version.
        supported: u16,
    },
}
