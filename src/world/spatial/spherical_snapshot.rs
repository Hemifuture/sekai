use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use super::sphere_geometry::norm;
use super::{SphericalSurfaceValidationError, UnitVector3};
use crate::world::serde_bounded::{deserialize_bounded_vec, BoundedVecSeed};
use crate::world::{
    CellId, EdgeId, Meters, SquareMeters, SurfaceVertexId, UnitError, MAX_SPHERICAL_CELL_COUNT,
    MAX_SPHERICAL_EDGE_COUNT, MAX_SPHERICAL_VERTEX_COUNT,
};

/// The supported version of the serialized spherical-surface schema.
pub const SPHERICAL_SURFACE_SCHEMA_V1: u16 = 1;
/// A closed generic geodesic-polygon mesh whose sites need not be Voronoi generators.
pub const SPHERICAL_SURFACE_SCHEMA_V2: u16 = 2;
/// Generic polygons with arbitrary simple-graph degree, bounded by the existing
/// global cell/edge allocation budgets. V1/V2 retain their six-side contract.
pub const SPHERICAL_SURFACE_SCHEMA_V3: u16 = 3;

const MAX_SPHERICAL_VERTICES: usize = MAX_SPHERICAL_VERTEX_COUNT as usize;
const MAX_SPHERICAL_EDGES: usize = MAX_SPHERICAL_EDGE_COUNT as usize;
const MAX_SPHERICAL_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
// A simple closed cell graph has at most n-1 distinct neighbors.
const MAX_GENERIC_CELL_BOUNDARY_DEGREE: usize = MAX_SPHERICAL_CELLS - 1;

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
///
/// Every value has passed the complete topology, geometry and fingerprint
/// validation exactly once, at its trust boundary: [`Self::new`], the crate's
/// cancellable constructor and deserialization are the only ways to obtain
/// one, each validates, and the fields are never mutated afterwards. Holding a
/// `SphericalSurfaceSnapshot` is therefore the proof of validity; consumers
/// bind to it through its stored [`Self::fingerprint`] instead of re-auditing
/// the whole surface ("parse, don't validate", King 2019).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SphericalSurfaceSnapshotWire")]
pub struct SphericalSurfaceSnapshot {
    pub(super) schema_version: u16,
    pub(super) radius: Meters,
    pub(super) vertices: Vec<SphericalSurfaceVertex>,
    pub(super) cells: Vec<SphericalSurfaceCell>,
    pub(super) edges: Vec<SphericalSurfaceEdge>,
    pub(super) fingerprint: [u8; 32],
}

/// The untrusted serialized form, accepted only after full validation.
#[derive(Deserialize)]
#[serde(rename = "SphericalSurfaceSnapshot", deny_unknown_fields)]
struct SphericalSurfaceSnapshotWire {
    schema_version: u16,
    radius: Meters,
    #[serde(deserialize_with = "deserialize_surface_vertices")]
    vertices: Vec<SphericalSurfaceVertex>,
    #[serde(deserialize_with = "deserialize_surface_cells")]
    cells: Vec<SphericalSurfaceCell>,
    #[serde(deserialize_with = "deserialize_surface_edges")]
    edges: Vec<SphericalSurfaceEdge>,
    fingerprint: [u8; 32],
}

impl TryFrom<SphericalSurfaceSnapshotWire> for SphericalSurfaceSnapshot {
    type Error = SphericalSurfaceValidationError;

    /// Admits stored records verbatim (no re-sorting or re-hashing), so a
    /// tampered order, geometry or fingerprint is rejected rather than repaired.
    fn try_from(wire: SphericalSurfaceSnapshotWire) -> Result<Self, Self::Error> {
        let snapshot = Self {
            schema_version: wire.schema_version,
            radius: wire.radius,
            vertices: wire.vertices,
            cells: wire.cells,
            edges: wire.edges,
            fingerprint: wire.fingerprint,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
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
    struct CellsVisitor;
    impl<'de> serde::de::Visitor<'de> for CellsVisitor {
        type Value = Vec<SphericalSurfaceCell>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "a sequence with at most {MAX_SPHERICAL_CELLS} elements")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            if let Some(length) = seq.size_hint() {
                if length > MAX_SPHERICAL_CELLS {
                    return Err(serde::de::Error::invalid_length(length, &self));
                }
            }
            let mut cells = Vec::new();
            let mut vertices = 0;
            let mut edges = 0;
            while cells.len() < MAX_SPHERICAL_CELLS {
                let Some(cell) = seq.next_element_seed(CellBudgetSeed {
                    vertices: (2 * MAX_SPHERICAL_EDGES - vertices)
                        .min(MAX_GENERIC_CELL_BOUNDARY_DEGREE),
                    edges: (2 * MAX_SPHERICAL_EDGES - edges).min(MAX_GENERIC_CELL_BOUNDARY_DEGREE),
                })?
                else {
                    return Ok(cells);
                };
                vertices += cell.boundary_vertices.len();
                edges += cell.boundary_edges.len();
                cells.push(cell);
            }
            if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom(
                    "spherical cell allocation budget exceeded",
                ));
            }
            Ok(cells)
        }
    }
    deserializer.deserialize_seq(CellsVisitor)
}

struct CellBudgetSeed {
    vertices: usize,
    edges: usize,
}
impl<'de> serde::de::DeserializeSeed<'de> for CellBudgetSeed {
    type Value = SphericalSurfaceCell;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> serde::de::Visitor<'de> for CellBudgetSeed {
    type Value = SphericalSurfaceCell;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("one spherical cell within the remaining boundary budget")
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        #[derive(Deserialize)]
        struct StrictDirection(
            #[serde(deserialize_with = "deserialize_strict_unit_vector")] UnitVector3,
        );
        let (mut id, mut site, mut centroid, mut area, mut vertices, mut edges) =
            (None, None, None, None, None, None);
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "id" if id.is_none() => id = Some(map.next_value()?),
                "site" if site.is_none() => site = Some(map.next_value::<StrictDirection>()?.0),
                "centroid" if centroid.is_none() => {
                    centroid = Some(map.next_value::<StrictDirection>()?.0)
                }
                "area" if area.is_none() => area = Some(map.next_value()?),
                "boundary_vertices" if vertices.is_none() => {
                    vertices = Some(map.next_value_seed(BoundedVecSeed::new(self.vertices))?);
                }
                "boundary_edges" if edges.is_none() => {
                    edges = Some(map.next_value_seed(BoundedVecSeed::new(self.edges))?);
                }
                _ => {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate or unknown spherical cell field: {key}"
                    )))
                }
            }
        }
        Ok(SphericalSurfaceCell {
            id: id.ok_or_else(|| serde::de::Error::missing_field("id"))?,
            site: site.ok_or_else(|| serde::de::Error::missing_field("site"))?,
            centroid: centroid.ok_or_else(|| serde::de::Error::missing_field("centroid"))?,
            area: area.ok_or_else(|| serde::de::Error::missing_field("area"))?,
            boundary_vertices: vertices
                .ok_or_else(|| serde::de::Error::missing_field("boundary_vertices"))?,
            boundary_edges: edges
                .ok_or_else(|| serde::de::Error::missing_field("boundary_edges"))?,
        })
    }
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
    deserialize_bounded_vec::<_, _, MAX_GENERIC_CELL_BOUNDARY_DEGREE>(deserializer)
}

fn deserialize_cell_boundary_edges<'de, D>(deserializer: D) -> Result<Vec<EdgeId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_GENERIC_CELL_BOUNDARY_DEGREE>(deserializer)
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
        vertices: Vec<SphericalSurfaceVertex>,
        cells: Vec<SphericalSurfaceCell>,
        edges: Vec<SphericalSurfaceEdge>,
    ) -> Result<Self, SphericalSurfaceValidationError> {
        Self::new_impl(schema_version, radius, vertices, cells, edges, None)
    }

    pub(crate) fn new_cancellable(
        schema_version: u16,
        radius: Meters,
        vertices: Vec<SphericalSurfaceVertex>,
        cells: Vec<SphericalSurfaceCell>,
        edges: Vec<SphericalSurfaceEdge>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, SphericalSurfaceValidationError> {
        Self::new_impl(
            schema_version,
            radius,
            vertices,
            cells,
            edges,
            Some(cancelled),
        )
    }

    fn new_impl(
        schema_version: u16,
        radius: Meters,
        mut vertices: Vec<SphericalSurfaceVertex>,
        mut cells: Vec<SphericalSurfaceCell>,
        mut edges: Vec<SphericalSurfaceEdge>,
        cancellation: Option<&dyn Fn() -> bool>,
    ) -> Result<Self, SphericalSurfaceValidationError> {
        check_snapshot_cancelled(cancellation)?;
        vertices.sort_by_key(|vertex| vertex.id);
        cells.sort_by_key(|cell| cell.id);
        edges.sort_by_key(|edge| edge.id);
        check_snapshot_cancelled(cancellation)?;
        for (index, cell) in cells.iter_mut().enumerate() {
            poll_snapshot_cancelled(index, cancellation)?;
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
        snapshot.fingerprint = snapshot.canonical_fingerprint_impl(cancellation)?;
        match cancellation {
            Some(cancelled) => snapshot.validate_cancellable(cancelled)?,
            None => snapshot.validate()?,
        }
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
    /// Never panics for a constructed snapshot: validation bounds the stored
    /// areas to a finite sphere total.
    pub fn total_cell_area(&self) -> SquareMeters {
        self.try_total_cell_area()
            .expect("validated spherical cell areas have a finite sum")
    }

    /// Tries to return the compensated sum of stored spherical cell areas.
    pub(super) fn try_total_cell_area(&self) -> Result<SquareMeters, UnitError> {
        let total = compensated_sum(self.cells.iter().map(|cell| cell.area.get()));
        SquareMeters::new(total)
    }

    pub(super) fn canonical_fingerprint(&self) -> [u8; 32] {
        self.canonical_fingerprint_impl(None)
            .expect("an uncancelled spherical fingerprint cannot fail")
    }

    pub(super) fn canonical_fingerprint_cancellable(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<[u8; 32], SphericalSurfaceValidationError> {
        self.canonical_fingerprint_impl(Some(cancelled))
    }

    fn canonical_fingerprint_impl(
        &self,
        cancellation: Option<&dyn Fn() -> bool>,
    ) -> Result<[u8; 32], SphericalSurfaceValidationError> {
        check_snapshot_cancelled(cancellation)?;
        let mut hasher = blake3::Hasher::new();
        hash_u16(&mut hasher, self.schema_version);
        hash_f64(&mut hasher, self.radius.get());

        hash_len(&mut hasher, self.vertices.len());
        for (index, vertex) in self.vertices.iter().enumerate() {
            poll_snapshot_cancelled(index, cancellation)?;
            hash_u32(&mut hasher, vertex.id.raw());
            hash_vector(&mut hasher, vertex.position);
        }

        hash_len(&mut hasher, self.cells.len());
        for (index, cell) in self.cells.iter().enumerate() {
            poll_snapshot_cancelled(index, cancellation)?;
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
        for (index, edge) in self.edges.iter().enumerate() {
            poll_snapshot_cancelled(index, cancellation)?;
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

        check_snapshot_cancelled(cancellation)?;
        Ok(*hasher.finalize().as_bytes())
    }
}

fn poll_snapshot_cancelled(
    index: usize,
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), SphericalSurfaceValidationError> {
    if index % 256 == 0 {
        check_snapshot_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_snapshot_cancelled(
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), SphericalSurfaceValidationError> {
    if cancellation.is_some_and(|cancelled| cancelled()) {
        Err(SphericalSurfaceValidationError::Cancelled)
    } else {
        Ok(())
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

    #[test]
    fn cell_seed_limits_each_boundary_by_the_remaining_global_budget() {
        use serde::de::DeserializeSeed;
        let text = json!({"id":0,"site":[1.0,0.0,0.0],"centroid":[1.0,0.0,0.0],
            "area":1.0,"boundary_vertices":[0,1,2],"boundary_edges":[0,1,2]})
        .to_string();
        let error = CellBudgetSeed {
            vertices: 2,
            edges: 3,
        }
        .deserialize(&mut serde_json::Deserializer::from_str(&text))
        .unwrap_err();
        assert!(error.to_string().contains("at most 2 elements"));
    }
}
