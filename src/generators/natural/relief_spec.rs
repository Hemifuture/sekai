use serde::{Deserialize, Serialize};

use crate::engine::{Artifact, ArtifactKey, ArtifactValidationError};
use crate::world::natural::ReliefSpec;

const INVALID_RELIEF_SPEC_CODE: &str = "natural.invalid-relief-spec";

/// Engine transport for one externally authored relief specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReliefSpecArtifact {
    spec: ReliefSpec,
}

impl ReliefSpecArtifact {
    /// Wraps one relief specification for validated engine transport.
    pub const fn new(spec: ReliefSpec) -> Self {
        Self { spec }
    }

    /// Borrows the wrapped author request.
    pub const fn spec(&self) -> &ReliefSpec {
        &self.spec
    }
}

impl Artifact for ReliefSpecArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.relief-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.spec.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_RELIEF_SPEC_CODE, error.to_string())
        })
    }
}
