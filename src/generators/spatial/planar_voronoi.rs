use std::collections::{BTreeMap, BTreeSet};

use rand::Rng;
use thiserror::Error;

use super::JitteredGridSites;
use crate::world::spatial::{
    SpatialCell, SpatialEdge, SpatialSnapshot, SpatialValidationError, SPATIAL_SCHEMA_V1,
};
use crate::world::{
    CellId, EdgeId, Meters, PlanarSpaceSpec, SpecError, SquareMeters, UnitError, WorldPoint,
    WorldRect,
};

const CLEANUP_DISTANCE: f64 = 1.0e-12;
const EDGE_QUANTIZATION: f64 = 1.0e-9;
const MIN_NORMALIZED_AREA: f64 = 1.0e-15;

/// Errors returned when deterministic planar topology cannot be constructed.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SpatialBuildError {
    /// The planar space violates its numerical or allocation safety budget.
    #[error("invalid planar space: {0}")]
    InvalidSpec(#[from] SpecError),
    /// Site triangulation did not produce any candidate-neighbor triangles.
    #[error("site triangulation produced no triangles")]
    EmptyTriangulation,
    /// More than one site occupies the same world-space point.
    #[error("sites {first:?} and {second:?} occupy the same point")]
    DuplicateSite {
        /// The first duplicate site.
        first: CellId,
        /// The second duplicate site.
        second: CellId,
    },
    /// Clipping removed every vertex of a cell polygon.
    #[error("clipping produced an empty polygon for cell {cell:?}")]
    EmptyPolygon {
        /// The cell whose polygon became empty.
        cell: CellId,
    },
    /// A polygon calculation produced a non-finite value.
    #[error("polygon calculation is non-finite for cell {cell:?}")]
    NonFinitePolygon {
        /// The cell whose polygon calculation failed.
        cell: CellId,
    },
    /// A polygon has too few vertices or negligible normalized area.
    #[error("polygon is degenerate for cell {cell:?}")]
    DegeneratePolygon {
        /// The cell whose polygon is degenerate.
        cell: CellId,
    },
    /// A polygon segment matched more than one existing canonical segment.
    #[error("cell {cell:?} segment has ambiguous canonical matches")]
    AmbiguousSegment {
        /// The cell containing the ambiguous segment.
        cell: CellId,
    },
    /// A canonical segment has more than two owning cells.
    #[error("canonical segment has {owner_count} owners")]
    NonManifoldSegment {
        /// The number of owners found for the segment.
        owner_count: usize,
    },
    /// Constructed world units or bounds were not finite and valid.
    #[error("constructed invalid world geometry: {0}")]
    InvalidGeometry(#[from] UnitError),
    /// The constructed records did not satisfy the spatial snapshot contract.
    #[error("constructed topology failed validation: {0}")]
    InvalidSnapshot(#[from] SpatialValidationError),
}

/// Builds deterministic rectangle-clipped Voronoi topology.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanarVoronoiBuilder;

impl PlanarVoronoiBuilder {
    /// Validates the space, consumes randomness only from `rng`, and builds a spatial snapshot.
    pub fn build<R>(
        space: &PlanarSpaceSpec,
        rng: &mut R,
    ) -> Result<SpatialSnapshot, SpatialBuildError>
    where
        R: Rng + ?Sized,
    {
        space.validate()?;
        let sites = JitteredGridSites::generate_validated(space, rng);
        build_from_sites(space, sites.sites())
    }
}

#[derive(Debug, Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn from_world(point: WorldPoint, scale: f64) -> Self {
        Self {
            x: point.x().get() / scale,
            y: point.y().get() / scale,
        }
    }

    fn to_world(self, scale: f64) -> Result<WorldPoint, UnitError> {
        Ok(WorldPoint::new(
            Meters::new(self.x * scale)?,
            Meters::new(self.y * scale)?,
        ))
    }

    fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PointKey {
    x: i64,
    y: i64,
}

impl PointKey {
    fn new(point: Point) -> Self {
        Self {
            x: (point.x / EDGE_QUANTIZATION).round() as i64,
            y: (point.y / EDGE_QUANTIZATION).round() as i64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SegmentKey {
    first: PointKey,
    second: PointKey,
}

impl SegmentKey {
    fn new(first: PointKey, second: PointKey) -> Self {
        if first <= second {
            Self { first, second }
        } else {
            Self {
                first: second,
                second: first,
            }
        }
    }
}

#[derive(Debug)]
struct SegmentRecord {
    first: Point,
    second: Point,
    owners: Vec<CellId>,
}

fn build_from_sites(
    space: &PlanarSpaceSpec,
    world_sites: &[WorldPoint],
) -> Result<SpatialSnapshot, SpatialBuildError> {
    let scale = space.width.get().max(space.height.get());
    let width = space.width.get() / scale;
    let height = space.height.get() / scale;
    let sites: Vec<Point> = world_sites
        .iter()
        .copied()
        .map(|point| Point::from_world(point, scale))
        .collect();
    reject_duplicate_sites(&sites)?;

    let delaunay_points: Vec<delaunator::Point> = sites
        .iter()
        .map(|point| delaunator::Point {
            x: point.x,
            y: point.y,
        })
        .collect();
    let triangulation = delaunator::triangulate(&delaunay_points);
    if triangulation.triangles.is_empty() {
        return Err(SpatialBuildError::EmptyTriangulation);
    }
    let neighbors = candidate_neighbors(sites.len(), &triangulation.triangles);

    let mut cells = Vec::with_capacity(sites.len());
    for (index, &site) in sites.iter().enumerate() {
        let cell_id = CellId::from_raw(index as u32);
        let polygon = clipped_polygon(cell_id, site, &sites, &neighbors[index], width, height)?;
        let (area, centroid) = polygon_area_and_centroid(cell_id, &polygon)?;
        let world_polygon = polygon
            .iter()
            .copied()
            .map(|point| point.to_world(scale))
            .collect::<Result<Vec<_>, _>>()?;
        cells.push(SpatialCell {
            id: cell_id,
            site: world_sites[index],
            centroid: centroid.to_world(scale)?,
            area: SquareMeters::new(area * scale * scale)?,
            polygon: world_polygon,
            neighbors: Vec::new(),
        });
    }

    let edges = reconstruct_edges(&mut cells, scale)?;
    let zero = Meters::new(0.0)?;
    let bounds = WorldRect::new(
        WorldPoint::new(zero, zero),
        WorldPoint::new(space.width, space.height),
    )?;
    SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, space.boundary, cells, edges)
        .map_err(SpatialBuildError::from)
}

fn reject_duplicate_sites(sites: &[Point]) -> Result<(), SpatialBuildError> {
    let mut seen = BTreeMap::<(u64, u64), usize>::new();
    for (index, point) in sites.iter().enumerate() {
        let key = (point.x.to_bits(), point.y.to_bits());
        if let Some(&first) = seen.get(&key) {
            return Err(SpatialBuildError::DuplicateSite {
                first: CellId::from_raw(first as u32),
                second: CellId::from_raw(index as u32),
            });
        }
        seen.insert(key, index);
    }
    Ok(())
}

fn candidate_neighbors(site_count: usize, triangles: &[usize]) -> Vec<Vec<usize>> {
    let mut neighbors = vec![Vec::new(); site_count];
    for triangle in triangles.chunks_exact(3) {
        for (first, second) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            neighbors[first].push(second);
            neighbors[second].push(first);
        }
    }
    for site_neighbors in &mut neighbors {
        site_neighbors.sort_unstable();
        site_neighbors.dedup();
    }
    neighbors
}

fn clipped_polygon(
    cell: CellId,
    site: Point,
    sites: &[Point],
    neighbors: &[usize],
    width: f64,
    height: f64,
) -> Result<Vec<Point>, SpatialBuildError> {
    let mut polygon = vec![
        Point { x: 0.0, y: 0.0 },
        Point { x: width, y: 0.0 },
        Point {
            x: width,
            y: height,
        },
        Point { x: 0.0, y: height },
    ];
    for &neighbor in neighbors {
        polygon = clip_to_bisector(&polygon, site, sites[neighbor]);
        if polygon.is_empty() {
            return Err(SpatialBuildError::EmptyPolygon { cell });
        }
    }

    cleanup_polygon(&mut polygon);
    if polygon.len() < 3 {
        return Err(SpatialBuildError::DegeneratePolygon { cell });
    }
    let signed_area = signed_area(&polygon);
    if !signed_area.is_finite() {
        return Err(SpatialBuildError::NonFinitePolygon { cell });
    }
    if signed_area.abs() <= MIN_NORMALIZED_AREA {
        return Err(SpatialBuildError::DegeneratePolygon { cell });
    }
    if signed_area < 0.0 {
        polygon.reverse();
    }
    rotate_canonically(&mut polygon);
    Ok(polygon)
}

fn clip_to_bisector(polygon: &[Point], site: Point, neighbor: Point) -> Vec<Point> {
    let mut output = Vec::with_capacity(polygon.len() + 1);
    let mut previous = polygon[polygon.len() - 1];
    let mut previous_value = bisector_value(previous, site, neighbor);
    let tolerance = f64::EPSILON * 64.0 * site.distance(neighbor);
    let mut previous_inside = previous_value <= tolerance;

    for &current in polygon {
        let current_value = bisector_value(current, site, neighbor);
        let current_inside = current_value <= tolerance;
        if current_inside {
            if !previous_inside {
                output.push(bisector_intersection(
                    previous,
                    current,
                    previous_value,
                    current_value,
                ));
            }
            output.push(current);
        } else if previous_inside {
            output.push(bisector_intersection(
                previous,
                current,
                previous_value,
                current_value,
            ));
        }
        previous = current;
        previous_value = current_value;
        previous_inside = current_inside;
    }
    output
}

fn bisector_value(point: Point, site: Point, neighbor: Point) -> f64 {
    let delta_x = neighbor.x - site.x;
    let delta_y = neighbor.y - site.y;
    let midpoint_x = (neighbor.x + site.x) * 0.5;
    let midpoint_y = (neighbor.y + site.y) * 0.5;
    (point.x - midpoint_x) * delta_x + (point.y - midpoint_y) * delta_y
}

fn bisector_intersection(
    first: Point,
    second: Point,
    first_value: f64,
    second_value: f64,
) -> Point {
    let parameter = (first_value / (first_value - second_value)).clamp(0.0, 1.0);
    Point {
        x: first.x + (second.x - first.x) * parameter,
        y: first.y + (second.y - first.y) * parameter,
    }
}

fn cleanup_polygon(polygon: &mut Vec<Point>) {
    let mut cleaned = Vec::with_capacity(polygon.len());
    for point in polygon.iter().copied() {
        if cleaned
            .last()
            .is_none_or(|previous: &Point| previous.distance(point) >= CLEANUP_DISTANCE)
        {
            cleaned.push(point);
        }
    }
    if cleaned.len() > 1
        && cleaned[0].distance(*cleaned.last().expect("polygon is non-empty")) < CLEANUP_DISTANCE
    {
        cleaned.pop();
    }
    *polygon = cleaned;
}

fn signed_area(polygon: &[Point]) -> f64 {
    let origin = polygon[0];
    let terms = (0..polygon.len()).map(|index| {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        (first.x - origin.x) * (second.y - origin.y) - (second.x - origin.x) * (first.y - origin.y)
    });
    0.5 * compensated_sum(terms)
}

fn rotate_canonically(polygon: &mut [Point]) {
    let first = polygon
        .iter()
        .enumerate()
        .min_by(|(first_index, first), (second_index, second)| {
            PointKey::new(**first)
                .cmp(&PointKey::new(**second))
                .then_with(|| first.x.total_cmp(&second.x))
                .then_with(|| first.y.total_cmp(&second.y))
                .then_with(|| first_index.cmp(second_index))
        })
        .map(|(index, _)| index)
        .expect("validated polygons are non-empty");
    polygon.rotate_left(first);
}

fn polygon_area_and_centroid(
    cell: CellId,
    polygon: &[Point],
) -> Result<(f64, Point), SpatialBuildError> {
    let origin = polygon[0];
    let mut cross_terms = Vec::with_capacity(polygon.len());
    let mut centroid_x_terms = Vec::with_capacity(polygon.len());
    let mut centroid_y_terms = Vec::with_capacity(polygon.len());
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let first_x = first.x - origin.x;
        let first_y = first.y - origin.y;
        let second_x = second.x - origin.x;
        let second_y = second.y - origin.y;
        let cross = first_x * second_y - second_x * first_y;
        cross_terms.push(cross);
        centroid_x_terms.push((first_x + second_x) * cross);
        centroid_y_terms.push((first_y + second_y) * cross);
    }

    let cross_sum = compensated_sum(cross_terms);
    if !cross_sum.is_finite() {
        return Err(SpatialBuildError::NonFinitePolygon { cell });
    }
    let area = 0.5 * cross_sum;
    if area <= MIN_NORMALIZED_AREA {
        return Err(SpatialBuildError::DegeneratePolygon { cell });
    }
    let denominator = 3.0 * cross_sum;
    let centroid = Point {
        x: origin.x + compensated_sum(centroid_x_terms) / denominator,
        y: origin.y + compensated_sum(centroid_y_terms) / denominator,
    };
    if !centroid.x.is_finite() || !centroid.y.is_finite() || !area.is_finite() {
        return Err(SpatialBuildError::NonFinitePolygon { cell });
    }
    Ok((area, centroid))
}

fn reconstruct_edges(
    cells: &mut [SpatialCell],
    scale: f64,
) -> Result<Vec<SpatialEdge>, SpatialBuildError> {
    let mut segments = BTreeMap::<SegmentKey, SegmentRecord>::new();
    for cell in cells.iter() {
        for side in 0..cell.polygon.len() {
            let first = Point::from_world(cell.polygon[side], scale);
            let second = Point::from_world(cell.polygon[(side + 1) % cell.polygon.len()], scale);
            insert_segment(&mut segments, cell.id, first, second)?;
        }
    }

    let mut neighbors = vec![Vec::<CellId>::new(); cells.len()];
    let mut edges = Vec::with_capacity(segments.len());
    for (index, (_, record)) in segments.into_iter().enumerate() {
        let mut owners = record.owners;
        owners.sort_unstable();
        owners.dedup();
        if owners.len() > 2 {
            return Err(SpatialBuildError::NonManifoldSegment {
                owner_count: owners.len(),
            });
        }
        if owners.len() == 2 {
            neighbors[owners[0].raw() as usize].push(owners[1]);
            neighbors[owners[1].raw() as usize].push(owners[0]);
        }
        let start = record.first.to_world(scale)?;
        let end = record.second.to_world(scale)?;
        edges.push(SpatialEdge {
            id: EdgeId::from_raw(index as u32),
            start,
            end,
            length: Meters::new(
                (end.x().get() - start.x().get()).hypot(end.y().get() - start.y().get()),
            )?,
            cells: [owners.first().copied(), owners.get(1).copied()],
        });
    }

    for (cell, mut cell_neighbors) in cells.iter_mut().zip(neighbors) {
        cell_neighbors.sort_unstable();
        cell_neighbors.dedup();
        cell.neighbors = cell_neighbors;
    }
    Ok(edges)
}

fn insert_segment(
    segments: &mut BTreeMap<SegmentKey, SegmentRecord>,
    owner: CellId,
    first: Point,
    second: Point,
) -> Result<(), SpatialBuildError> {
    let (key, canonical_first, canonical_second) = canonical_segment(first, second);
    let matching_keys = neighboring_segment_keys(key)
        .into_iter()
        .filter(|candidate| {
            segments.get(candidate).is_some_and(|record| {
                endpoints_match(
                    canonical_first,
                    canonical_second,
                    record.first,
                    record.second,
                )
            })
        })
        .collect::<Vec<_>>();
    if matching_keys.len() > 1 {
        return Err(SpatialBuildError::AmbiguousSegment { cell: owner });
    }

    if let Some(existing_key) = matching_keys.first() {
        let record = segments
            .get_mut(existing_key)
            .expect("matching canonical segment exists");
        if !record.owners.contains(&owner) {
            record.owners.push(owner);
        }
        if record.owners.len() > 2 {
            return Err(SpatialBuildError::NonManifoldSegment {
                owner_count: record.owners.len(),
            });
        }
    } else {
        let std::collections::btree_map::Entry::Vacant(entry) = segments.entry(key) else {
            return Err(SpatialBuildError::AmbiguousSegment { cell: owner });
        };
        entry.insert(SegmentRecord {
            first: canonical_first,
            second: canonical_second,
            owners: vec![owner],
        });
    }
    Ok(())
}

fn canonical_segment(first: Point, second: Point) -> (SegmentKey, Point, Point) {
    let first_key = PointKey::new(first);
    let second_key = PointKey::new(second);
    let point_order = first
        .x
        .total_cmp(&second.x)
        .then_with(|| first.y.total_cmp(&second.y));
    if first_key < second_key
        || (first_key == second_key && point_order != std::cmp::Ordering::Greater)
    {
        (SegmentKey::new(first_key, second_key), first, second)
    } else {
        (SegmentKey::new(first_key, second_key), second, first)
    }
}

fn neighboring_segment_keys(key: SegmentKey) -> BTreeSet<SegmentKey> {
    let mut keys = BTreeSet::new();
    for first_x in (key.first.x - 1)..=(key.first.x + 1) {
        for first_y in (key.first.y - 1)..=(key.first.y + 1) {
            for second_x in (key.second.x - 1)..=(key.second.x + 1) {
                for second_y in (key.second.y - 1)..=(key.second.y + 1) {
                    keys.insert(SegmentKey::new(
                        PointKey {
                            x: first_x,
                            y: first_y,
                        },
                        PointKey {
                            x: second_x,
                            y: second_y,
                        },
                    ));
                }
            }
        }
    }
    keys
}

fn endpoints_match(first: Point, second: Point, other_first: Point, other_second: Point) -> bool {
    (first.distance(other_first) <= EDGE_QUANTIZATION
        && second.distance(other_second) <= EDGE_QUANTIZATION)
        || (first.distance(other_second) <= EDGE_QUANTIZATION
            && second.distance(other_first) <= EDGE_QUANTIZATION)
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
    use super::*;
    use crate::world::spatial::Topology;
    use crate::world::BoundaryCondition;

    fn point(x: f64, y: f64) -> WorldPoint {
        WorldPoint::new(Meters::new(x).unwrap(), Meters::new(y).unwrap())
    }

    #[test]
    fn four_regular_sites_form_four_equal_cells() {
        let space = PlanarSpaceSpec {
            width: Meters::new(4.0).unwrap(),
            height: Meters::new(4.0).unwrap(),
            target_cell_count: 16,
            boundary: BoundaryCondition::Closed,
        };
        let sites = [
            point(1.0, 1.0),
            point(3.0, 1.0),
            point(1.0, 3.0),
            point(3.0, 3.0),
        ];

        let snapshot = build_from_sites(&space, &sites).unwrap();

        snapshot.validate().unwrap();
        assert_eq!(snapshot.cell_count(), 4);
        assert_eq!(
            snapshot.neighbors(CellId::from_raw(0)).unwrap(),
            &[CellId::from_raw(1), CellId::from_raw(2)]
        );
        for index in 0..4 {
            let cell = snapshot.cell(CellId::from_raw(index)).unwrap();
            assert_eq!(cell.area.get(), 4.0);
        }
    }

    #[test]
    fn adjacent_quantization_buckets_share_one_segment() {
        let first = Point {
            x: 0.25 + 0.49 * EDGE_QUANTIZATION,
            y: 0.25,
        };
        let second = Point {
            x: 0.75 + 0.49 * EDGE_QUANTIZATION,
            y: 0.25,
        };
        let shifted_first = Point {
            x: first.x + 0.02 * EDGE_QUANTIZATION,
            y: first.y,
        };
        let shifted_second = Point {
            x: second.x + 0.02 * EDGE_QUANTIZATION,
            y: second.y,
        };
        let mut segments = BTreeMap::new();

        insert_segment(&mut segments, CellId::from_raw(0), first, second).unwrap();
        insert_segment(
            &mut segments,
            CellId::from_raw(1),
            shifted_first,
            shifted_second,
        )
        .unwrap();

        assert_eq!(segments.len(), 1);
        assert_eq!(segments.values().next().unwrap().owners.len(), 2);
    }

    #[test]
    fn distinct_segments_with_the_same_quantized_key_are_rejected() {
        let lower_first = Point {
            x: 0.25 - 0.49 * EDGE_QUANTIZATION,
            y: 0.25 - 0.49 * EDGE_QUANTIZATION,
        };
        let lower_second = Point {
            x: 0.75 - 0.49 * EDGE_QUANTIZATION,
            y: 0.25 - 0.49 * EDGE_QUANTIZATION,
        };
        let upper_first = Point {
            x: 0.25 + 0.49 * EDGE_QUANTIZATION,
            y: 0.25 + 0.49 * EDGE_QUANTIZATION,
        };
        let upper_second = Point {
            x: 0.75 + 0.49 * EDGE_QUANTIZATION,
            y: 0.25 + 0.49 * EDGE_QUANTIZATION,
        };
        let mut segments = BTreeMap::new();
        insert_segment(
            &mut segments,
            CellId::from_raw(0),
            lower_first,
            lower_second,
        )
        .unwrap();

        assert!(matches!(
            insert_segment(
                &mut segments,
                CellId::from_raw(1),
                upper_first,
                upper_second
            ),
            Err(SpatialBuildError::AmbiguousSegment { .. })
        ));
    }
}
