//! Engine integration adapters for deterministic spherical spatial generation.

use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use super::{GeodesicVoronoiBuilder, SphericalSurfaceBuildError};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    DiagnosticContext, DiagnosticSeverity, GraphError, Stage, StageError, StageGraph,
    StageGraphBuilder, StageId, StageInputs, StageRng,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SphericalSurfaceValidationError};
use crate::world::{Meters, SphericalSpaceSpec, SphericalSpecError};

const INVALID_SPEC_CODE: &str = "spherical-spatial.invalid-spec";
const BUILD_FAILED_CODE: &str = "spherical-spatial.build-failed";
const INVALID_SNAPSHOT_CODE: &str = "spherical-spatial.invalid-snapshot";
const RESOLVED_CELL_COUNT_CODE: &str = "spherical-spatial.resolved-cell-count";
const CANCELLED_CODE: &str = "engine.cancelled";

/// Engine transport wrapper for an externally supplied spherical-space specification.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalSpaceArtifact {
    space: SphericalSpaceSpec,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalSpaceArtifactWire {
    space: SphericalSpaceSpecWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalSpaceSpecWire {
    radius: Meters,
    target_cell_count: u32,
}

impl<'de> Deserialize<'de> for SphericalSpaceArtifact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalSpaceArtifactWire::deserialize(deserializer)?;
        let space = SphericalSpaceSpec {
            radius: wire.space.radius,
            target_cell_count: wire.space.target_cell_count,
        };
        space.validate().map_err(D::Error::custom)?;
        Ok(Self::new(space))
    }
}

impl SphericalSpaceArtifact {
    /// Wraps a spherical-space specification for validated engine transport.
    pub fn new(space: SphericalSpaceSpec) -> Self {
        Self { space }
    }

    /// Returns the wrapped spherical-space specification.
    pub fn space(&self) -> &SphericalSpaceSpec {
        &self.space
    }
}

impl Artifact for SphericalSpaceArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("spatial.spherical-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.space
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SPEC_CODE, error.to_string()))
    }
}

/// Engine transport wrapper for a validated closed spherical surface snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalSurfaceArtifact {
    snapshot: SphericalSurfaceSnapshot,
}

impl SphericalSurfaceArtifact {
    /// Wraps a spherical surface snapshot for validated engine transport.
    pub fn new(snapshot: SphericalSurfaceSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the wrapped spherical surface snapshot.
    pub fn snapshot(&self) -> &SphericalSurfaceSnapshot {
        &self.snapshot
    }
}

impl Artifact for SphericalSurfaceArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.spherical-surface");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SNAPSHOT_CODE, error.to_string()))
    }
}

/// Restricted typed dependency bundle supplied to [`SphericalSurfaceStage`].
pub struct SphericalSurfaceStageInputs {
    space: Arc<SphericalSpaceArtifact>,
}

impl StageInputs for SphericalSurfaceStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[SphericalSpaceArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            space: artifacts.get::<SphericalSpaceArtifact>()?,
        })
    }
}

/// Deterministic stage that builds a closed geodesic spherical Voronoi surface.
#[derive(Debug, Clone, Copy, Default)]
pub struct SphericalSurfaceStage;

impl Stage for SphericalSurfaceStage {
    type Inputs = SphericalSurfaceStageInputs;
    type Output = SphericalSurfaceArtifact;

    fn id(&self) -> StageId {
        StageId::new("spatial.spherical-voronoi")
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
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        let space = inputs.space.space();
        space.validate().map_err(invalid_spec)?;
        let snapshot = GeodesicVoronoiBuilder::build_cancellable(space, || rng.is_cancelled())
            .map_err(builder_failure)?;
        snapshot.validate().map_err(invalid_snapshot)?;

        if space.resolved_cell_count() != space.target_cell_count {
            diagnostics.push(
                Diagnostic::with_context(
                    DiagnosticSeverity::Info,
                    RESOLVED_CELL_COUNT_CODE,
                    format!(
                        "resolved spherical cell count {} differs from requested target {}",
                        space.resolved_cell_count(),
                        space.target_cell_count
                    ),
                    DiagnosticContext {
                        stage_id: Some(self.id().as_str().to_owned()),
                        ..DiagnosticContext::default()
                    },
                )
                .expect("stage-owned diagnostic code must satisfy the identifier grammar"),
            );
        }

        Ok(SphericalSurfaceArtifact::new(snapshot))
    }
}

/// Builds a spherical-only foundation graph with one spherical-spec external.
///
/// # Errors
///
/// Returns a graph validation error if the fixed stage declarations cease to
/// satisfy the engine graph contract.
pub fn spherical_foundation_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<SphericalSpaceArtifact>()
        .stage(SphericalSurfaceStage)
        .build()
}

fn invalid_spec(error: SphericalSpecError) -> StageError {
    StageError::new(
        INVALID_SPEC_CODE,
        format!("invalid spherical space specification: {error}"),
    )
}

fn builder_failure(error: SphericalSurfaceBuildError) -> StageError {
    match error {
        SphericalSurfaceBuildError::Cancelled => StageError::new(
            CANCELLED_CODE,
            "spherical surface construction was cancelled",
        ),
        SphericalSurfaceBuildError::InvalidSpec(error) => invalid_spec(error),
        SphericalSurfaceBuildError::InvalidSnapshot(error) => invalid_snapshot(error),
        error => build_failure(error),
    }
}

fn build_failure(error: SphericalSurfaceBuildError) -> StageError {
    StageError::new(
        BUILD_FAILED_CODE,
        format!("spherical Voronoi generation failed: {error}"),
    )
}

fn invalid_snapshot(error: SphericalSurfaceValidationError) -> StageError {
    StageError::new(
        INVALID_SNAPSHOT_CODE,
        format!("generated spherical surface failed validation: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        builder_failure, invalid_spec, BUILD_FAILED_CODE, CANCELLED_CODE, INVALID_SNAPSHOT_CODE,
        INVALID_SPEC_CODE,
    };
    use crate::generators::spatial::SphericalSurfaceBuildError;
    use crate::world::spatial::{SphericalSurfaceValidationError, SPHERICAL_SURFACE_SCHEMA_V1};
    use crate::world::{SphericalSpecError, MIN_SPHERICAL_CELL_COUNT};

    #[test]
    fn spherical_spec_failures_have_a_stable_distinct_stage_error() {
        let source = SphericalSpecError::CellCountOutOfRange {
            found: MIN_SPHERICAL_CELL_COUNT - 1,
            min: MIN_SPHERICAL_CELL_COUNT,
            max: 198_812,
        };

        for error in [
            invalid_spec(source.clone()),
            builder_failure(SphericalSurfaceBuildError::InvalidSpec(source)),
        ] {
            assert_eq!(error.code(), INVALID_SPEC_CODE);
            assert_eq!(
                error.message(),
                "invalid spherical space specification: cell count 41 is outside 42..=198812"
            );
        }
    }

    #[test]
    fn spherical_build_failures_have_a_stable_distinct_stage_error() {
        let error = builder_failure(SphericalSurfaceBuildError::MeshConstruction);

        assert_eq!(error.code(), BUILD_FAILED_CODE);
        assert_eq!(
            error.message(),
            "spherical Voronoi generation failed: geodesic Delaunay mesh construction failed"
        );
    }

    #[test]
    fn spherical_cancellation_uses_the_engine_wide_stable_code() {
        let error = builder_failure(SphericalSurfaceBuildError::Cancelled);

        assert_eq!(error.code(), CANCELLED_CODE);
        assert_eq!(
            error.message(),
            "spherical surface construction was cancelled"
        );
    }

    #[test]
    fn spherical_snapshot_failures_have_a_stable_distinct_stage_error() {
        let source = SphericalSurfaceValidationError::UnsupportedSchema {
            found: SPHERICAL_SURFACE_SCHEMA_V1 + 1,
            supported: SPHERICAL_SURFACE_SCHEMA_V1,
        };
        let error = builder_failure(SphericalSurfaceBuildError::InvalidSnapshot(source));

        assert_eq!(error.code(), INVALID_SNAPSHOT_CODE);
        assert_eq!(
            error.message(),
            "generated spherical surface failed validation: unsupported spherical surface schema version 2; supported version is 1"
        );
    }
}
