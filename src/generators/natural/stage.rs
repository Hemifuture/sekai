//! Engine transport for an externally supplied tectonic specification.

use serde::{Deserialize, Serialize};

use crate::engine::{Artifact, ArtifactKey, ArtifactValidationError};
use crate::world::natural::TectonicSpec;

const INVALID_SPEC_CODE: &str = "natural.invalid-tectonic-spec";

/// Engine transport wrapper for an externally supplied tectonic specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TectonicSpecArtifact {
    spec: TectonicSpec,
}

impl TectonicSpecArtifact {
    /// Wraps a tectonic specification for validated engine transport.
    pub const fn new(spec: TectonicSpec) -> Self {
        Self { spec }
    }

    /// Returns the wrapped tectonic specification.
    pub const fn spec(&self) -> &TectonicSpec {
        &self.spec
    }
}

impl Artifact for TectonicSpecArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.tectonic-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.spec
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SPEC_CODE, error.to_string()))
    }
}
