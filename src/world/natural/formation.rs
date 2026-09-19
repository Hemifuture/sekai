use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// The supported version of a requested world-formation specification.
pub const WORLD_FORMATION_SPEC_SCHEMA_V1: u16 = 1;
/// The supported version of a resolved world-formation selection.
pub const RESOLVED_WORLD_FORMATION_SCHEMA_V1: u16 = 1;
/// Number of finite evolution steps in the user-approved Sekai reference
/// formation horizon. This is a product parameter, not a literature constant
/// or an Earth-age claim.
pub const SEKAI_REFERENCE_FORMATION_STEP_COUNT: u16 = 128;
/// Duration of one reference step, stored as integer kyr for identity-stable
/// serde; the 2 Myr step follows Cortial et al. (2019), DOI
/// 10.1111/cgf.13614.
pub const CORTIAL_FORMATION_STEP_DURATION_KYR: u32 = 2_000;

/// The validated finite schedule used to derive the current formation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResolvedFormationTimeline {
    step_count: u16,
    step_duration_kyr: u32,
}

impl ResolvedFormationTimeline {
    /// Returns Sekai's authored horizon with Cortial's sourced step duration.
    pub const fn sekai_reference() -> Self {
        Self {
            step_count: SEKAI_REFERENCE_FORMATION_STEP_COUNT,
            step_duration_kyr: CORTIAL_FORMATION_STEP_DURATION_KYR,
        }
    }

    /// Returns a production-step prefix for bounded unit tests.
    #[cfg(test)]
    pub(crate) fn test_prefix(step_count: u16) -> Self {
        assert!(
            (1..=SEKAI_REFERENCE_FORMATION_STEP_COUNT).contains(&step_count),
            "a test prefix must contain at least one production formation step"
        );
        Self {
            step_count,
            step_duration_kyr: CORTIAL_FORMATION_STEP_DURATION_KYR,
        }
    }

    /// Returns the finite number of formation steps.
    pub const fn step_count(self) -> u16 {
        self.step_count
    }

    /// Returns one step duration in identity-stable integer kyr.
    pub const fn step_duration_kyr(self) -> u32 {
        self.step_duration_kyr
    }

    /// Returns one step duration in Myr for the numerical process kernels.
    pub fn step_duration_myr(self) -> f64 {
        f64::from(self.step_duration_kyr) / 1_000.0
    }

    /// Returns the complete private formation horizon in Myr.
    pub fn total_duration_myr(self) -> f64 {
        f64::from(self.step_count) * self.step_duration_myr()
    }

    /// Rejects timelines outside the currently supported product identity.
    pub fn validate(self) -> Result<(), WorldFormationSpecError> {
        #[cfg(test)]
        if self.step_duration_kyr == CORTIAL_FORMATION_STEP_DURATION_KYR
            && (1..=SEKAI_REFERENCE_FORMATION_STEP_COUNT).contains(&self.step_count)
        {
            return Ok(());
        }
        if self != Self::sekai_reference() {
            return Err(WorldFormationSpecError::UnsupportedTimeline {
                step_count: self.step_count,
                step_duration_kyr: self.step_duration_kyr,
            });
        }
        Ok(())
    }
}

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

    /// Returns the measured median emergent-land recommendation for this morphology.
    ///
    /// Values are the frozen medians from the T0b 17-seed WaterInventory probe
    /// recorded in the land-fraction-driver design §2.4 and §8.
    pub const fn recommended_land_fraction(self) -> f32 {
        match self {
            Self::Continents => 0.20,
            Self::Archipelago => 0.22,
            Self::Supercontinent => 0.17,
            Self::GreatIsland => 0.23,
            Self::VolcanicIslands => 0.16,
        }
    }

    /// Returns the opening continental-nucleus count before clamping to plate count.
    ///
    /// Continents uses the six geological continents of Mortimer et al. (2017).
    /// Archipelago uses Cogley (1984) fourteen continents. Supercontinent is one
    /// assembled mass (Wilson 1966). VolcanicIslands uses Cogley's four named
    /// microcontinents. GreatIsland's primary nucleus is one; satellites are
    /// [`Self::satellite_nucleus_count`].
    pub const fn continental_nucleus_count(self) -> u16 {
        match self {
            Self::Continents => 6,
            Self::Archipelago => 14,
            Self::Supercontinent | Self::GreatIsland => 1,
            Self::VolcanicIslands => 4,
        }
    }

    /// Returns whether this morphology is published mid-way through the
    /// dispersal half of a Wilson cycle (Wilson 1966), so its opening state
    /// must be the assembled supercontinent it disperses from: continental
    /// nuclei clustered on one hemisphere with an oceanic hemisphere opposite
    /// (Wegener 1915; Seton et al. 2012 Pangea/Panthalassa). Archipelago is
    /// the late snapshot in which the fragments have already spread around the
    /// globe, as Cogley's (1984) fourteen continents do today, so it opens
    /// from dispersed nuclei on plate representatives like the assembled and
    /// single-mass morphologies.
    pub const fn opens_in_dispersal_phase(self) -> bool {
        match self {
            Self::Continents => true,
            Self::Archipelago
            | Self::Supercontinent
            | Self::GreatIsland
            | Self::VolcanicIslands => false,
        }
    }

    /// Returns extra satellite nuclei grown after the primary GreatIsland mass.
    ///
    /// Cogley (1984) lists four named microcontinents (Rockall, Seychelles,
    /// Agulhas, Jan Mayen). Other presets do not place satellites.
    pub const fn satellite_nucleus_count(self) -> u16 {
        match self {
            Self::GreatIsland => 4,
            Self::Continents | Self::Archipelago | Self::Supercontinent | Self::VolcanicIslands => {
                0
            }
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
    timeline: ResolvedFormationTimeline,
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
            timeline: ResolvedFormationTimeline::sekai_reference(),
        };
        formation.validate()?;
        Ok(formation)
    }

    /// Replaces the production timeline with a bounded prefix in unit tests.
    #[cfg(test)]
    pub(crate) fn with_test_timeline(mut self, timeline: ResolvedFormationTimeline) -> Self {
        timeline
            .validate()
            .expect("the test-only timeline constructor returns a valid prefix");
        self.timeline = timeline;
        self
    }

    /// Validates the serialized resolved-selection schema.
    pub fn validate(&self) -> Result<(), WorldFormationSpecError> {
        if self.schema_version != RESOLVED_WORLD_FORMATION_SCHEMA_V1 {
            return Err(WorldFormationSpecError::UnsupportedResolvedSchema {
                found: self.schema_version,
                supported: RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            });
        }
        self.timeline.validate()?;
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

    /// Returns the finite schedule whose endpoint this resolved state denotes.
    pub const fn timeline(&self) -> ResolvedFormationTimeline {
        self.timeline
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

    /// Returns the visible emergent-land recommendation.
    pub const fn recommended_land_fraction(&self) -> f32 {
        self.resolved.recommended_land_fraction()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolvedFormationTimelineWire {
    step_count: u16,
    step_duration_kyr: u32,
}

impl ResolvedFormationTimelineWire {
    fn resolve(self) -> Result<ResolvedFormationTimeline, WorldFormationSpecError> {
        let timeline = ResolvedFormationTimeline {
            step_count: self.step_count,
            step_duration_kyr: self.step_duration_kyr,
        };
        timeline.validate()?;
        Ok(timeline)
    }
}

impl<'de> Deserialize<'de> for ResolvedFormationTimeline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ResolvedFormationTimelineWire::deserialize(deserializer)?
            .resolve()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolvedWorldFormationWire {
    schema_version: u16,
    requested: WorldFormationPreset,
    resolved: ResolvedWorldFormationPreset,
    timeline: ResolvedFormationTimelineWire,
}

impl<'de> Deserialize<'de> for ResolvedWorldFormation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ResolvedWorldFormationWire::deserialize(deserializer)?;
        let formation = Self {
            schema_version: wire.schema_version,
            requested: wire.requested,
            resolved: wire.resolved,
            timeline: wire.timeline.resolve().map_err(serde::de::Error::custom)?,
        };
        formation.validate().map_err(serde::de::Error::custom)?;
        Ok(formation)
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
    /// A resolved formation carries a schedule outside the supported product identity.
    #[error("unsupported formation timeline with {step_count} steps of {step_duration_kyr} kyr")]
    UnsupportedTimeline {
        /// The encountered number of evolution steps.
        step_count: u16,
        /// The encountered duration of one step in kyr.
        step_duration_kyr: u32,
    },
}
