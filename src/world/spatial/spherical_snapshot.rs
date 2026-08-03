use std::fmt;
use std::marker::PhantomData;

use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::sphere_geometry::norm;
use super::{SphericalSurfaceValidationError, UnitVector3};
use crate::world::{
    CellId, EdgeId, Meters, SquareMeters, SurfaceVertexId, UnitError, MAX_GEODESIC_FREQUENCY,
};

/// The supported version of the serialized spherical-surface schema.
pub const SPHERICAL_SURFACE_SCHEMA_V1: u16 = 1;

const MAX_FREQUENCY_SQUARED: usize =
    MAX_GEODESIC_FREQUENCY as usize * MAX_GEODESIC_FREQUENCY as usize;
const MAX_SPHERICAL_VERTICES: usize = 20 * MAX_FREQUENCY_SQUARED;
const MAX_SPHERICAL_EDGES: usize = 30 * MAX_FREQUENCY_SQUARED;
const MAX_SPHERICAL_CELLS: usize = 10 * MAX_FREQUENCY_SQUARED + 2;
const MAX_CELL_BOUNDARY_DEGREE: usize = 6;

/// A canonical vertex stored once by the authoritative spherical surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalSurfaceVertex {
    /// The contiguous stable identifier of this vertex.
    pub id: SurfaceVertexId,
    /// The vertex direction on the unit sphere.
    #[serde(deserialize_with = "deserialize_strict_unit_vector")]
    pub position: UnitVector3,
}

/// A validated spherical polygon whose boundary references canonical records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalSurfaceCell {
    /// The contiguous stable identifier of this cell.
    pub id: CellId,
    /// The generating site on the unit sphere.
    #[serde(deserialize_with = "deserialize_strict_unit_vector")]
    pub site: UnitVector3,
    /// The spherical polygon centroid direction.
    #[serde(deserialize_with = "deserialize_strict_unit_vector")]
    pub centroid: UnitVector3,
    /// The polygon area at the snapshot radius.
    pub area: SquareMeters,
    /// Counter-clockwise canonical vertex IDs in cyclic boundary order.
    #[serde(deserialize_with = "deserialize_cell_boundary_vertices")]
    pub boundary_vertices: Vec<SurfaceVertexId>,
    /// Canonical edge IDs corresponding to the same cyclic sides.
    #[serde(deserialize_with = "deserialize_cell_boundary_edges")]
    pub boundary_edges: Vec<EdgeId>,
}

/// A canonical geodesic edge shared by exactly two spherical cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalSurfaceEdge {
    /// The contiguous stable identifier of this edge.
    pub id: EdgeId,
    /// The two canonical endpoint IDs, in ascending order.
    pub vertices: [SurfaceVertexId; 2],
    /// The two distinct owning cell IDs, in ascending order.
    pub cells: [CellId; 2],
    /// The midpoint direction along the minor great-circle arc.
    #[serde(deserialize_with = "deserialize_strict_unit_vector")]
    pub midpoint: UnitVector3,
    /// The endpoint arc length at the snapshot radius.
    pub length: Meters,
    /// The great-circle distance between the two cell sites.
    pub center_distance: Meters,
    /// The site-to-midpoint distances in the same order as `cells`.
    pub center_distances_to_midpoint: [Meters; 2],
    /// The unit tangent at `midpoint` pointing from `cells[0]` to `cells[1]`.
    #[serde(deserialize_with = "deserialize_strict_unit_vector")]
    pub normal_from_first: UnitVector3,
}

/// The immutable, versioned, authoritative geometry of a closed spherical surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalSurfaceSnapshot {
    pub(super) schema_version: u16,
    pub(super) radius: Meters,
    #[serde(deserialize_with = "deserialize_surface_vertices")]
    pub(super) vertices: Vec<SphericalSurfaceVertex>,
    #[serde(deserialize_with = "deserialize_surface_cells")]
    pub(super) cells: Vec<SphericalSurfaceCell>,
    #[serde(deserialize_with = "deserialize_surface_edges")]
    pub(super) edges: Vec<SphericalSurfaceEdge>,
    pub(super) fingerprint: [u8; 32],
}

fn deserialize_surface_vertices<'de, D>(
    deserializer: D,
) -> Result<Vec<SphericalSurfaceVertex>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_VERTICES>(deserializer)
}

fn deserialize_surface_cells<'de, D>(deserializer: D) -> Result<Vec<SphericalSurfaceCell>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_surface_edges<'de, D>(deserializer: D) -> Result<Vec<SphericalSurfaceEdge>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_EDGES>(deserializer)
}

fn deserialize_cell_boundary_vertices<'de, D>(
    deserializer: D,
) -> Result<Vec<SurfaceVertexId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_CELL_BOUNDARY_DEGREE>(deserializer)
}

fn deserialize_cell_boundary_edges<'de, D>(deserializer: D) -> Result<Vec<EdgeId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_CELL_BOUNDARY_DEGREE>(deserializer)
}

fn deserialize_bounded_vec<'de, D, T, const MAX: usize>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct BoundedVecVisitor<T, const MAX: usize>(PhantomData<T>);

    impl<'de, T, const MAX: usize> Visitor<'de> for BoundedVecVisitor<T, MAX>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a sequence with at most {MAX} elements")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if let Some(length) = sequence.size_hint() {
                if length > MAX {
                    return Err(A::Error::invalid_length(length, &self));
                }
            }
            let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX));
            while values.len() < MAX {
                let Some(value) = sequence.next_element()? else {
                    return Ok(values);
                };
                values.push(value);
            }
            if sequence.next_element::<IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(MAX + 1, &self));
            }
            Ok(values)
        }
    }

    deserializer.deserialize_seq(BoundedVecVisitor::<T, MAX>(PhantomData))
}

fn deserialize_strict_unit_vector<'de, D>(deserializer: D) -> Result<UnitVector3, D::Error>
where
    D: Deserializer<'de>,
{
    let components = <[f64; 3]>::deserialize(deserializer)?;
    if components.iter().any(|component| !component.is_finite()) {
        return Err(D::Error::custom(
            "spherical surface unit-vector components must be finite",
        ));
    }
    let vector_norm = norm(components);
    if (vector_norm - 1.0).abs() > 16.0 * f64::EPSILON {
        return Err(D::Error::custom(format_args!(
            "spherical surface vector norm must be 1, got {vector_norm}"
        )));
    }
    Ok(UnitVector3::from_verified_unit_components(components))
}

impl SphericalSurfaceSnapshot {
    /// Sorts records by stable ID, fingerprints their semantic fields, and validates them.
    pub fn new(
        schema_version: u16,
        radius: Meters,
        mut vertices: Vec<SphericalSurfaceVertex>,
        mut cells: Vec<SphericalSurfaceCell>,
        mut edges: Vec<SphericalSurfaceEdge>,
    ) -> Result<Self, SphericalSurfaceValidationError> {
        vertices.sort_by_key(|vertex| vertex.id);
        cells.sort_by_key(|cell| cell.id);
        edges.sort_by_key(|edge| edge.id);
        for cell in &mut cells {
            canonicalize_cell_boundary(cell);
        }

        let mut snapshot = Self {
            schema_version,
            radius,
            vertices,
            cells,
            edges,
            fingerprint: [0; 32],
        };
        snapshot.fingerprint = snapshot.canonical_fingerprint();
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Returns the serialized schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the single radius used to scale all unit-sphere geometry.
    pub const fn radius(&self) -> Meters {
        self.radius
    }

    /// Returns canonical vertices in contiguous ID order.
    pub fn vertices(&self) -> &[SphericalSurfaceVertex] {
        &self.vertices
    }

    /// Returns canonical cells in contiguous ID order.
    pub fn cells(&self) -> &[SphericalSurfaceCell] {
        &self.cells
    }

    /// Returns canonical edges in contiguous ID order.
    pub fn edges(&self) -> &[SphericalSurfaceEdge] {
        &self.edges
    }

    /// Returns the canonical semantic fingerprint.
    pub const fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// Looks up a vertex by its contiguous stable ID.
    pub fn vertex(&self, id: SurfaceVertexId) -> Option<&SphericalSurfaceVertex> {
        self.vertices
            .get(id.raw() as usize)
            .filter(|vertex| vertex.id == id)
    }

    /// Looks up a cell by its contiguous stable ID.
    pub fn cell(&self, id: CellId) -> Option<&SphericalSurfaceCell> {
        self.cells
            .get(id.raw() as usize)
            .filter(|cell| cell.id == id)
    }

    /// Looks up an edge by its contiguous stable ID.
    pub fn edge(&self, id: EdgeId) -> Option<&SphericalSurfaceEdge> {
        self.edges
            .get(id.raw() as usize)
            .filter(|edge| edge.id == id)
    }

    /// Returns a cell's stored cyclic edge IDs.
    pub fn cell_edges(&self, id: CellId) -> Option<&[EdgeId]> {
        self.cell(id).map(|cell| cell.boundary_edges.as_slice())
    }

    /// Derives the cell across an owned edge without storing duplicate adjacency.
    pub fn opposite_cell(&self, cell: CellId, edge: EdgeId) -> Option<CellId> {
        let owners = self.edge(edge)?.cells;
        if owners[0] == cell {
            Some(owners[1])
        } else if owners[1] == cell {
            Some(owners[0])
        } else {
            None
        }
    }

    /// Returns the compensated sum of stored spherical cell areas.
    ///
    /// # Panics
    ///
    /// Panics if the snapshot has not passed [`Self::validate`] and its stored
    /// area sum is non-finite. Use [`Self::try_total_cell_area`] for untrusted
    /// or otherwise unvalidated snapshots.
    pub fn total_cell_area(&self) -> SquareMeters {
        self.try_total_cell_area()
            .expect("validated spherical cell areas have a finite sum")
    }

    /// Tries to return the compensated sum of stored spherical cell areas.
    pub fn try_total_cell_area(&self) -> Result<SquareMeters, UnitError> {
        let total = compensated_sum(self.cells.iter().map(|cell| cell.area.get()));
        SquareMeters::new(total)
    }

    pub(super) fn canonical_fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hash_u16(&mut hasher, self.schema_version);
        hash_f64(&mut hasher, self.radius.get());

        hash_len(&mut hasher, self.vertices.len());
        for vertex in &self.vertices {
            hash_u32(&mut hasher, vertex.id.raw());
            hash_vector(&mut hasher, vertex.position);
        }

        hash_len(&mut hasher, self.cells.len());
        for cell in &self.cells {
            hash_u32(&mut hasher, cell.id.raw());
            hash_vector(&mut hasher, cell.site);
            hash_vector(&mut hasher, cell.centroid);
            hash_f64(&mut hasher, cell.area.get());
            hash_len(&mut hasher, cell.boundary_vertices.len());
            for vertex in &cell.boundary_vertices {
                hash_u32(&mut hasher, vertex.raw());
            }
            hash_len(&mut hasher, cell.boundary_edges.len());
            for edge in &cell.boundary_edges {
                hash_u32(&mut hasher, edge.raw());
            }
        }

        hash_len(&mut hasher, self.edges.len());
        for edge in &self.edges {
            hash_u32(&mut hasher, edge.id.raw());
            for vertex in edge.vertices {
                hash_u32(&mut hasher, vertex.raw());
            }
            for cell in edge.cells {
                hash_u32(&mut hasher, cell.raw());
            }
            hash_vector(&mut hasher, edge.midpoint);
            hash_f64(&mut hasher, edge.length.get());
            hash_f64(&mut hasher, edge.center_distance.get());
            for distance in edge.center_distances_to_midpoint {
                hash_f64(&mut hasher, distance.get());
            }
            hash_vector(&mut hasher, edge.normal_from_first);
        }

        *hasher.finalize().as_bytes()
    }
}

fn canonicalize_cell_boundary(cell: &mut SphericalSurfaceCell) {
    if cell.boundary_vertices.len() != cell.boundary_edges.len() {
        return;
    }
    let Some((start, _)) = cell
        .boundary_vertices
        .iter()
        .enumerate()
        .min_by_key(|(_, vertex)| **vertex)
    else {
        return;
    };
    cell.boundary_vertices.rotate_left(start);
    cell.boundary_edges.rotate_left(start);
}

fn hash_u16(hasher: &mut blake3::Hasher, value: u16) {
    hasher.update(&value.to_le_bytes());
}

fn hash_u32(hasher: &mut blake3::Hasher, value: u32) {
    hasher.update(&value.to_le_bytes());
}

fn hash_len(hasher: &mut blake3::Hasher, value: usize) {
    hasher.update(&(value as u64).to_le_bytes());
}

fn hash_f64(hasher: &mut blake3::Hasher, value: f64) {
    hasher.update(&value.to_bits().to_le_bytes());
}

fn hash_vector(hasher: &mut blake3::Hasher, vector: UnitVector3) {
    for component in vector.components() {
        hash_f64(hasher, component);
    }
}

fn compensated_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in values {
        let adjusted = value - compensation;
        let next = sum + adjusted;
        compensation = (next - sum) - adjusted;
        sum = next;
    }
    sum
}

#[cfg(test)]
mod tests {
    use serde::de::value::SeqDeserializer;
    use serde_json::json;

    use super::*;

    #[test]
    fn outer_vertex_sequence_rejects_max_plus_one_before_visiting_elements() {
        let values = std::iter::repeat_with(|| {
            json!({
                "id": 0,
                "position": [1.0, 0.0, 0.0]
            })
        })
        .take(MAX_SPHERICAL_VERTICES + 1);
        let deserializer = SeqDeserializer::<_, serde_json::Error>::new(values);

        let error = deserialize_surface_vertices(deserializer).unwrap_err();
        assert!(error.to_string().contains("at most 397620 elements"));
    }

    #[test]
    fn outer_edge_sequence_rejects_max_plus_one_before_visiting_elements() {
        let values = std::iter::repeat_with(|| json!(null)).take(MAX_SPHERICAL_EDGES + 1);
        let deserializer = SeqDeserializer::<_, serde_json::Error>::new(values);

        let error = deserialize_surface_edges(deserializer).unwrap_err();
        assert!(error.to_string().contains("at most 596430 elements"));
    }

    #[test]
    fn outer_cell_sequence_rejects_max_plus_one_before_visiting_elements() {
        let values = std::iter::repeat_with(|| json!(null)).take(MAX_SPHERICAL_CELLS + 1);
        let deserializer = SeqDeserializer::<_, serde_json::Error>::new(values);

        let error = deserialize_surface_cells(deserializer).unwrap_err();
        assert!(error.to_string().contains("at most 198812 elements"));
    }
}
