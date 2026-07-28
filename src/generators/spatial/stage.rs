//! Engine integration adapters for deterministic planar spatial generation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{PlanarVoronoiBuilder, SpatialBuildError};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    GraphError, Stage, StageError, StageGraph, StageGraphBuilder, StageId, StageInputs, StageRng,
};
use crate::world::spatial::{SpatialSnapshot, SpatialValidationError};
use crate::world::{PlanarSpaceSpec, SpecError};

const INVALID_SPEC_CODE: &str = "spatial.invalid-spec";
const BUILD_FAILED_CODE: &str = "spatial.build-failed";
const INVALID_SNAPSHOT_CODE: &str = "spatial.invalid-snapshot";

/// Engine transport wrapper for an externally supplied planar-space specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanarSpaceArtifact {
    space: PlanarSpaceSpec,
}

impl PlanarSpaceArtifact {
    /// Wraps a planar-space specification for validated engine transport.
    pub fn new(space: PlanarSpaceSpec) -> Self {
        Self { space }
    }

    /// Returns the wrapped planar-space specification.
    pub fn space(&self) -> &PlanarSpaceSpec {
        &self.space
    }
}

impl Artifact for PlanarSpaceArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("spatial.planar-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.space
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SPEC_CODE, error.to_string()))
    }
}

/// Engine transport wrapper for a validated planar spatial snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialArtifact {
    snapshot: SpatialSnapshot,
}

impl SpatialArtifact {
    /// Wraps a spatial snapshot for validated engine transport.
    pub fn new(snapshot: SpatialSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the wrapped spatial snapshot.
    pub fn snapshot(&self) -> &SpatialSnapshot {
        &self.snapshot
    }
}

impl Artifact for SpatialArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.spatial");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SNAPSHOT_CODE, error.to_string()))
    }
}

/// Restricted typed dependency bundle supplied to [`SpatialStage`].
pub struct SpatialStageInputs {
    space: Arc<PlanarSpaceArtifact>,
}

impl StageInputs for SpatialStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[PlanarSpaceArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            space: artifacts.get::<PlanarSpaceArtifact>()?,
        })
    }
}

/// Deterministic stage that builds rectangle-clipped planar Voronoi topology.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpatialStage;

impl Stage for SpatialStage {
    type Inputs = SpatialStageInputs;
    type Output = SpatialArtifact;

    fn id(&self) -> StageId {
        StageId::new("spatial.planar-voronoi")
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
        inputs.space.space().validate().map_err(invalid_spec)?;
        let snapshot =
            PlanarVoronoiBuilder::build(inputs.space.space(), rng).map_err(builder_failure)?;
        snapshot.validate().map_err(invalid_snapshot)?;
        Ok(SpatialArtifact::new(snapshot))
    }
}

/// Builds the foundation graph with one planar-spec external and one spatial stage.
///
/// # Errors
///
/// Returns a graph validation error if the fixed stage declarations cease to
/// satisfy the engine graph contract.
pub fn foundation_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<PlanarSpaceArtifact>()
        .stage(SpatialStage)
        .build()
}

fn invalid_spec(error: SpecError) -> StageError {
    StageError::new(
        INVALID_SPEC_CODE,
        format!("invalid planar space specification: {error}"),
    )
}

fn builder_failure(error: SpatialBuildError) -> StageError {
    match error {
        SpatialBuildError::InvalidSpec(error) => invalid_spec(error),
        SpatialBuildError::InvalidSnapshot(error) => invalid_snapshot(error),
        error => build_failure(error),
    }
}

fn build_failure(error: SpatialBuildError) -> StageError {
    StageError::new(
        BUILD_FAILED_CODE,
        format!("planar Voronoi generation failed: {error}"),
    )
}

fn invalid_snapshot(error: SpatialValidationError) -> StageError {
    StageError::new(
        INVALID_SNAPSHOT_CODE,
        format!("generated spatial snapshot failed validation: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        builder_failure, invalid_spec, BUILD_FAILED_CODE, INVALID_SNAPSHOT_CODE, INVALID_SPEC_CODE,
    };
    use crate::generators::spatial::SpatialBuildError;
    use crate::world::spatial::{SpatialValidationError, SPATIAL_SCHEMA_V1};
    use crate::world::{SpecError, MIN_CELL_COUNT};

    #[test]
    fn spec_failures_have_a_stable_distinct_stage_error() {
        let source = SpecError::CellCountOutOfRange {
            found: MIN_CELL_COUNT - 1,
            min: MIN_CELL_COUNT,
            max: 200_000,
        };

        for error in [
            invalid_spec(source.clone()),
            builder_failure(SpatialBuildError::InvalidSpec(source)),
        ] {
            assert_eq!(error.code(), INVALID_SPEC_CODE);
            assert_eq!(
                error.message(),
                "invalid planar space specification: cell count 15 is outside 16..=200000"
            );
        }
    }

    #[test]
    fn build_failures_have_a_stable_distinct_stage_error() {
        let error = builder_failure(SpatialBuildError::EmptyTriangulation);

        assert_eq!(error.code(), BUILD_FAILED_CODE);
        assert_eq!(
            error.message(),
            "planar Voronoi generation failed: site triangulation produced no usable candidate neighbors"
        );
    }

    #[test]
    fn snapshot_failures_have_a_stable_distinct_stage_error() {
        let source = SpatialValidationError::UnsupportedSchema {
            found: SPATIAL_SCHEMA_V1 + 1,
            supported: SPATIAL_SCHEMA_V1,
        };

        let error = builder_failure(SpatialBuildError::InvalidSnapshot(source));

        assert_eq!(error.code(), INVALID_SNAPSHOT_CODE);
        assert_eq!(
            error.message(),
            "generated spatial snapshot failed validation: unsupported spatial schema version 2; supported version is 1"
        );
    }
}
