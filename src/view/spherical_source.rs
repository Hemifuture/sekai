//! Immutable identity for presentation data derived from one spherical build.

use crate::engine::BuildResultHash;
use crate::world::spatial::SurfaceRef;
use crate::world::RootSeed;

/// The validated build identity shared by every spherical presentation derivative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SphericalPresentationSource {
    root_seed: RootSeed,
    surface_ref: SurfaceRef,
    build_result_hash: BuildResultHash,
    graph_contract_version: u16,
}

impl SphericalPresentationSource {
    /// Constructs an identity from values already validated at the app boundary.
    pub(crate) const fn new(
        root_seed: RootSeed,
        surface_ref: SurfaceRef,
        build_result_hash: BuildResultHash,
        graph_contract_version: u16,
    ) -> Self {
        Self {
            root_seed,
            surface_ref,
            build_result_hash,
            graph_contract_version,
        }
    }

    /// Returns the root seed of the authoritative natural build.
    pub const fn root_seed(&self) -> RootSeed {
        self.root_seed
    }

    /// Returns the exact spherical surface used by the authoritative natural build.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns the semantic hash of the authoritative natural build outputs.
    pub const fn build_result_hash(&self) -> &BuildResultHash {
        &self.build_result_hash
    }

    /// Returns the graph contract version required to interpret the source.
    pub const fn graph_contract_version(&self) -> u16 {
        self.graph_contract_version
    }
}
