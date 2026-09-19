//! Product pipeline selection and persisted spherical-world identity.

use crate::world::{Meters, RootSeed};

pub(super) const DEFAULT_TARGET_CELL_COUNT: u32 = 20_000;
/// Root seed used by a newly authored product world.
pub const PRODUCT_DEFAULT_WORLD_SEED: RootSeed = RootSeed::new(42);

/// Which authoritative generation chain the spherical canvas builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WorldPipeline {
    /// The formation product chain (P2v5→P5); the interactive product default.
    Formation,
    /// The spherical natural-foundation chain.
    ///
    /// Kept for arbitrary-resolution worlds: the formation chain is bound to
    /// the fixed quality-profile resolutions, so the 162-cell worlds used by
    /// unit tests can only run here.
    LegacyFoundation,
}

impl Default for WorldPipeline {
    fn default() -> Self {
        // Unit tests author tiny (162-cell) worlds that only the foundation
        // chain accepts; the interactive product always starts on formation.
        #[cfg(test)]
        {
            Self::LegacyFoundation
        }
        #[cfg(not(test))]
        {
            Self::Formation
        }
    }
}

/// Persisted provenance of the currently authored world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PersistedWorldOrigin {
    /// A world authored on the spherical canvas.
    SphericalV1,
}

pub(super) fn missing_world_origin_is_spherical() -> PersistedWorldOrigin {
    PersistedWorldOrigin::SphericalV1
}

/// The graph family selected for runtime initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRuntimeGraph {
    /// The spherical natural foundation graph.
    SphericalNaturalFoundation,
    /// The formation-product graph (P2v5→P5).
    SphericalFormation,
}

/// Returns the spherical space specification used by a newly authored product world.
pub fn default_spherical_space_spec() -> crate::world::SphericalSpaceSpec {
    crate::world::SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).expect("the Earth-like default radius is valid"),
        target_cell_count: DEFAULT_TARGET_CELL_COUNT,
    }
}

pub(super) mod spherical_space_spec_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::world::{Meters, SphericalSpaceSpec};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        radius: f64,
        target_cell_count: u32,
    }

    pub fn serialize<S>(spec: &SphericalSpaceSpec, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Wire {
            radius: spec.radius.get(),
            target_cell_count: spec.target_cell_count,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SphericalSpaceSpec, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        let spec = SphericalSpaceSpec {
            radius: Meters::new(wire.radius).map_err(serde::de::Error::custom)?,
            target_cell_count: wire.target_cell_count,
        };
        spec.validate().map_err(serde::de::Error::custom)?;
        Ok(spec)
    }
}
