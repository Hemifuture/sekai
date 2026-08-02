//! Engine adapters for deterministic current-slice geologic generation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    GeologicGenerationError, GeologicGenerator, MantleGenerationError, MantleGenerator,
    ReliefArtifact, ResolvedGeologicInputArtifact, ResolvedWorldFormationArtifact,
    TectonicArtifact,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SpatialArtifact;
use crate::rules::GeologicModel;
use crate::world::natural::{
    GeologicSnapshot, GeologicSpecError, GeologicValidationError, MantleSnapshot,
    MantleValidationError,
};

const INVALID_SPEC_CODE: &str = "natural.invalid-geologic-spec";
const BUILD_FAILED_CODE: &str = "natural.mantle-build-failed";
const INVALID_MANTLE_CODE: &str = "natural.invalid-mantle";
const INVALID_GEOLOGIC_INPUT_CODE: &str = "natural.invalid-geologic-input";
const GEOLOGIC_BUILD_FAILED_CODE: &str = "natural.geologic-build-failed";
const INVALID_GEOLOGIC_SNAPSHOT_CODE: &str = "natural.invalid-geologic-snapshot";

/// Engine transport wrapper for a complete validated mantle snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MantleArtifact {
    snapshot: MantleSnapshot,
}

impl MantleArtifact {
    /// Wraps a complete validated mantle snapshot for engine transport.
    pub const fn new(snapshot: MantleSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the wrapped mantle snapshot.
    pub const fn snapshot(&self) -> &MantleSnapshot {
        &self.snapshot
    }
}

impl Artifact for MantleArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.mantle");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_MANTLE_CODE, error.to_string()))
    }
}

/// Restricted typed dependencies supplied to [`MantleStage`].
pub struct MantleStageInputs {
    formation: Arc<ResolvedWorldFormationArtifact>,
    resolved_input: Arc<ResolvedGeologicInputArtifact>,
    spatial: Arc<SpatialArtifact>,
}

impl StageInputs for MantleStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedGeologicInputArtifact::KEY,
            ResolvedWorldFormationArtifact::KEY,
            SpatialArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            formation: artifacts.get::<ResolvedWorldFormationArtifact>()?,
            resolved_input: artifacts.get::<ResolvedGeologicInputArtifact>()?,
            spatial: artifacts.get::<SpatialArtifact>()?,
        })
    }
}

/// Deterministic stage that builds independent present-day mantle forcing.
#[derive(Debug, Clone, Copy, Default)]
pub struct MantleStage;

impl Stage for MantleStage {
    type Inputs = MantleStageInputs;
    type Output = MantleArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.mantle")
    }

    fn version(&self) -> u32 {
        2
    }

    fn namespace(&self) -> &'static str {
        "sekai.core"
    }

    fn run(
        &self,
        inputs: Self::Inputs,
        rng: &mut StageRng,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        let resolved_input = inputs.resolved_input.input();
        resolved_input.spec().validate().map_err(invalid_spec)?;
        let snapshot = match resolved_input.model() {
            GeologicModel::CurrentSliceV1 => MantleGenerator::generate(
                inputs.spatial.snapshot(),
                resolved_input.spec(),
                inputs.formation.formation().mantle_bias(),
                rng,
            ),
        }
        .map_err(generation_failure)?;
        snapshot
            .validate_against(inputs.spatial.snapshot())
            .map_err(invalid_mantle)?;
        Ok(MantleArtifact::new(snapshot))
    }
}

fn invalid_spec(error: GeologicSpecError) -> StageError {
    StageError::new(
        INVALID_SPEC_CODE,
        format!("invalid geologic specification: {error}"),
    )
}

fn generation_failure(error: MantleGenerationError) -> StageError {
    match error {
        MantleGenerationError::InvalidSpec(error) => invalid_spec(error),
        MantleGenerationError::HotspotCountExceedsCells { .. } => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
        }
        MantleGenerationError::InvalidSnapshot(error) => invalid_mantle(error),
    }
}

fn invalid_mantle(error: MantleValidationError) -> StageError {
    StageError::new(
        INVALID_MANTLE_CODE,
        format!("generated mantle snapshot failed validation: {error}"),
    )
}

/// Engine transport wrapper for a complete validated surface-geology snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeologicArtifact {
    snapshot: GeologicSnapshot,
}

impl GeologicArtifact {
    /// Wraps a complete validated surface-geology snapshot for engine transport.
    pub const fn new(snapshot: GeologicSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the wrapped geologic snapshot.
    pub const fn snapshot(&self) -> &GeologicSnapshot {
        &self.snapshot
    }
}

impl Artifact for GeologicArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.geology");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_GEOLOGIC_SNAPSHOT_CODE, error.to_string())
        })
    }
}

/// Restricted typed dependencies supplied to [`GeologicStage`].
pub struct GeologicStageInputs {
    resolved_input: Arc<ResolvedGeologicInputArtifact>,
    mantle: Arc<MantleArtifact>,
    relief: Arc<ReliefArtifact>,
    spatial: Arc<SpatialArtifact>,
    tectonic: Arc<TectonicArtifact>,
}

impl StageInputs for GeologicStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedGeologicInputArtifact::KEY,
            MantleArtifact::KEY,
            ReliefArtifact::KEY,
            SpatialArtifact::KEY,
            TectonicArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolved_input: artifacts.get::<ResolvedGeologicInputArtifact>()?,
            mantle: artifacts.get::<MantleArtifact>()?,
            relief: artifacts.get::<ReliefArtifact>()?,
            spatial: artifacts.get::<SpatialArtifact>()?,
            tectonic: artifacts.get::<TectonicArtifact>()?,
        })
    }
}

/// Deterministic stage that publishes current-slice bedrock and geologic potentials.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeologicStage;

impl Stage for GeologicStage {
    type Inputs = GeologicStageInputs;
    type Output = GeologicArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.geology")
    }

    fn version(&self) -> u32 {
        1
    }

    fn namespace(&self) -> &'static str {
        "sekai.core"
    }

    fn run(
        &self,
        inputs: Self::Inputs,
        rng: &mut StageRng,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        let spatial = inputs.spatial.snapshot();
        let tectonic = inputs.tectonic.snapshot();
        let mantle = inputs.mantle.snapshot();
        let relief = inputs.relief.snapshot();
        let resolved = inputs.resolved_input.input();

        resolved
            .spec()
            .validate()
            .map_err(|error| invalid_geologic_input(error.to_string()))?;
        tectonic
            .validate_against(spatial)
            .map_err(|error| invalid_geologic_input(error.to_string()))?;
        mantle
            .validate_against(spatial)
            .map_err(|error| invalid_geologic_input(error.to_string()))?;
        relief
            .validate_against(spatial)
            .map_err(|error| invalid_geologic_input(error.to_string()))?;

        let snapshot = match resolved.model() {
            GeologicModel::CurrentSliceV1 => {
                GeologicGenerator::generate(spatial, tectonic, mantle, relief, resolved.spec(), rng)
            }
        }
        .map_err(geologic_generation_failure)?;
        snapshot
            .validate_against(spatial, tectonic, mantle, relief)
            .map_err(invalid_geologic_snapshot)?;
        Ok(GeologicArtifact::new(snapshot))
    }
}

fn invalid_geologic_input(message: String) -> StageError {
    StageError::new(
        INVALID_GEOLOGIC_INPUT_CODE,
        format!("geologic input failed validation: {message}"),
    )
}

fn geologic_generation_failure(error: GeologicGenerationError) -> StageError {
    match error {
        GeologicGenerationError::InvalidSpec(error) => invalid_geologic_input(error.to_string()),
        GeologicGenerationError::InvalidTectonics(error) => {
            invalid_geologic_input(error.to_string())
        }
        GeologicGenerationError::InvalidMantle(error) => invalid_geologic_input(error.to_string()),
        GeologicGenerationError::InvalidRelief(error) => invalid_geologic_input(error.to_string()),
        GeologicGenerationError::InvalidGeology(error) => StageError::new(
            GEOLOGIC_BUILD_FAILED_CODE,
            format!("geologic synthesis produced invalid fields: {error}"),
        ),
    }
}

fn invalid_geologic_snapshot(error: GeologicValidationError) -> StageError {
    StageError::new(
        INVALID_GEOLOGIC_SNAPSHOT_CODE,
        format!("generated geologic snapshot failed validation: {error}"),
    )
}
