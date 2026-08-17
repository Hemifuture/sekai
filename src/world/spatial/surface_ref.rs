use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    SpatialSnapshot, SpatialValidationError, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SPATIAL_SCHEMA_V1, SPHERICAL_SURFACE_SCHEMA_V1,
    SPHERICAL_SURFACE_SCHEMA_V2,
};

/// The authoritative geometry family addressed by a natural-field snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceGeometryKind {
    /// A legacy rectangle-clipped planar Voronoi surface.
    PlanarV1,
    /// A closed geodesic spherical Voronoi surface.
    SphericalV1,
    /// A closed generic geodesic-polygon spherical mesh.
    SphericalGeodesicV2,
}

impl SurfaceGeometryKind {
    const fn supported_schema(self) -> u16 {
        match self {
            Self::PlanarV1 => SPATIAL_SCHEMA_V1,
            Self::SphericalV1 => SPHERICAL_SURFACE_SCHEMA_V1,
            Self::SphericalGeodesicV2 => SPHERICAL_SURFACE_SCHEMA_V2,
        }
    }

    /// Returns whether this kind belongs to the closed spherical family.
    pub const fn is_spherical(self) -> bool {
        matches!(self, Self::SphericalV1 | Self::SphericalGeodesicV2)
    }
}

/// A stable identity for the exact authoritative surface behind derived fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRef {
    geometry_kind: SurfaceGeometryKind,
    geometry_schema: u16,
    cell_count: u32,
    edge_count: u32,
    fingerprint: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceRefWire {
    geometry_kind: SurfaceGeometryKind,
    geometry_schema: u16,
    cell_count: u32,
    edge_count: u32,
    fingerprint: [u8; 32],
}

impl SurfaceRef {
    /// Constructs an identity only when its kind, schema, allocation, and hash are meaningful.
    pub fn new(
        geometry_kind: SurfaceGeometryKind,
        geometry_schema: u16,
        cell_count: u32,
        edge_count: u32,
        fingerprint: [u8; 32],
    ) -> Result<Self, SurfaceRefError> {
        let surface_ref = Self {
            geometry_kind,
            geometry_schema,
            cell_count,
            edge_count,
            fingerprint,
        };
        surface_ref.validate()?;
        Ok(surface_ref)
    }

    /// Creates the identity of a validated planar snapshot.
    ///
    /// # Panics
    ///
    /// Panics only when `snapshot` did not come from [`SpatialSnapshot::new`] and
    /// has not passed validation, or in the cryptographically negligible event
    /// that its BLAKE3 fingerprint is all zero bytes.
    pub fn for_planar(snapshot: &SpatialSnapshot) -> Self {
        Self::try_for_planar(snapshot)
            .expect("validated planar snapshots have a supported, non-empty surface identity")
    }

    /// Tries to create the identity of a planar snapshot without assuming validation.
    pub fn try_for_planar(snapshot: &SpatialSnapshot) -> Result<Self, SurfaceRefError> {
        snapshot.validate()?;
        Self::from_validated_planar(snapshot)
    }

    pub(crate) fn from_validated_planar(
        snapshot: &SpatialSnapshot,
    ) -> Result<Self, SurfaceRefError> {
        Self::new(
            SurfaceGeometryKind::PlanarV1,
            snapshot.schema_version,
            snapshot.cells.len() as u32,
            snapshot.edges.len() as u32,
            snapshot.fingerprint(),
        )
    }

    /// Creates the identity of a validated authoritative spherical snapshot.
    ///
    /// # Panics
    ///
    /// Panics only when `snapshot` has not passed its validation boundary.
    pub fn for_spherical(snapshot: &SphericalSurfaceSnapshot) -> Self {
        Self::try_for_spherical(snapshot)
            .expect("validated spherical snapshots have a supported, non-empty surface identity")
    }

    /// Tries to create the identity of a spherical snapshot without assuming validation.
    pub fn try_for_spherical(snapshot: &SphericalSurfaceSnapshot) -> Result<Self, SurfaceRefError> {
        snapshot.validate()?;
        Self::from_validated_spherical(snapshot)
    }

    pub(crate) fn from_validated_spherical(
        snapshot: &SphericalSurfaceSnapshot,
    ) -> Result<Self, SurfaceRefError> {
        let geometry_kind = match snapshot.schema_version() {
            SPHERICAL_SURFACE_SCHEMA_V1 => SurfaceGeometryKind::SphericalV1,
            SPHERICAL_SURFACE_SCHEMA_V2 => SurfaceGeometryKind::SphericalGeodesicV2,
            _ => unreachable!("validated spherical surface has a supported schema"),
        };
        Self::new(
            geometry_kind,
            snapshot.schema_version(),
            snapshot.cells().len() as u32,
            snapshot.edges().len() as u32,
            snapshot.fingerprint(),
        )
    }

    /// Rechecks this serialized identity without consulting the referenced surface.
    pub fn validate(&self) -> Result<(), SurfaceRefError> {
        let supported = self.geometry_kind.supported_schema();
        if self.geometry_schema != supported {
            return Err(SurfaceRefError::UnsupportedGeometrySchema {
                kind: self.geometry_kind,
                found: self.geometry_schema,
                supported,
            });
        }
        if self.cell_count == 0 {
            return Err(SurfaceRefError::EmptyCells);
        }
        if self.edge_count == 0 {
            return Err(SurfaceRefError::EmptyEdges);
        }
        if self.fingerprint == [0; 32] {
            return Err(SurfaceRefError::ZeroFingerprint);
        }
        Ok(())
    }

    /// Returns the geometry family and semantic version.
    pub const fn geometry_kind(self) -> SurfaceGeometryKind {
        self.geometry_kind
    }

    /// Returns the authoritative geometry schema version.
    pub const fn geometry_schema(self) -> u16 {
        self.geometry_schema
    }

    /// Returns the exact dense cell allocation.
    pub const fn cell_count(self) -> u32 {
        self.cell_count
    }

    /// Returns the exact dense edge allocation.
    pub const fn edge_count(self) -> u32 {
        self.edge_count
    }

    /// Returns the semantic content fingerprint of the authoritative surface.
    pub const fn fingerprint(self) -> [u8; 32] {
        self.fingerprint
    }
}

impl<'de> Deserialize<'de> for SurfaceRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SurfaceRefWire::deserialize(deserializer)?;
        Self::new(
            wire.geometry_kind,
            wire.geometry_schema,
            wire.cell_count,
            wire.edge_count,
            wire.fingerprint,
        )
        .map_err(D::Error::custom)
    }
}

/// Invalid or incomplete authoritative-surface identities.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SurfaceRefError {
    /// The referenced planar snapshot failed its authoritative validation.
    #[error("cannot identify an invalid planar surface: {0}")]
    InvalidPlanarSnapshot(#[from] SpatialValidationError),
    /// The referenced spherical snapshot failed its authoritative validation.
    #[error("cannot identify an invalid spherical surface: {0}")]
    InvalidSphericalSnapshot(#[from] SphericalSurfaceValidationError),
    /// The geometry kind and schema version do not form a supported contract.
    #[error("unsupported {kind:?} geometry schema {found}; supported schema is {supported}")]
    UnsupportedGeometrySchema {
        kind: SurfaceGeometryKind,
        found: u16,
        supported: u16,
    },
    /// A usable authoritative surface must contain at least one cell.
    #[error("a surface identity cannot reference an empty cell allocation")]
    EmptyCells,
    /// A usable authoritative surface must contain at least one edge.
    #[error("a surface identity cannot reference an empty edge allocation")]
    EmptyEdges,
    /// An all-zero fingerprint denotes an identity that was never populated.
    #[error("a surface identity fingerprint cannot be all zero bytes")]
    ZeroFingerprint,
}
