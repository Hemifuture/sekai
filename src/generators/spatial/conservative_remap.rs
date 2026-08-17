use thiserror::Error;

use crate::world::spatial::{
    canonical_east_north_basis, spherical_triangle_area_unit, ConservativeSurfaceMap,
    ConservativeSurfaceMapError, SphericalSurfaceSnapshot, SurfaceOverlapWeight, SurfaceRef,
    TangentTransform, UnitVector3, CONSERVATIVE_SURFACE_MAP_SCHEMA_V1,
};
use crate::world::{CellId, SurfaceVertexId};

const CLIP_HALFSPACE_EPSILON: f64 = 2.0e-13;
const DUPLICATE_VERTEX_DISTANCE_SQUARED: f64 = 1.0e-24;
const MIN_INTERSECTION_STERADIANS: f64 = 1.0e-18;
const RAW_FINE_CELL_CLOSURE_LIMIT: f64 = 1.0e-9;
const BALANCE_CLOSURE_LIMIT: f64 = 1.0e-12;
const MAX_BALANCE_ITERATIONS: u16 = 96;
const MAX_GEOMETRIC_ADJUSTMENT: f64 = 1.0e-4;
const INITIAL_ADJACENCY_RINGS: usize = 2;
const MAX_ADJACENCY_RINGS: usize = 8;
const RADIUS_RELATIVE_TOLERANCE: f64 = 128.0 * f64::EPSILON;

/// Builds deterministic conservative overlap maps between validated spherical surfaces.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConservativeSurfaceMapBuilder;

impl ConservativeSurfaceMapBuilder {
    /// Builds a non-cancellable conservative map.
    pub fn build(
        source: &SphericalSurfaceSnapshot,
        target: &SphericalSurfaceSnapshot,
    ) -> Result<ConservativeSurfaceMap, ConservativeRemapError> {
        Self::build_cancellable(source, target, || false)
    }

    /// Builds a map while periodically observing a monotonic cancellation callback.
    pub fn build_cancellable(
        source: &SphericalSurfaceSnapshot,
        target: &SphericalSurfaceSnapshot,
        mut is_cancelled: impl FnMut() -> bool,
    ) -> Result<ConservativeSurfaceMap, ConservativeRemapError> {
        check_cancelled(&mut is_cancelled)?;
        source
            .validate()
            .map_err(|error| ConservativeRemapError::InvalidSurface {
                role: "source",
                reason: error.to_string(),
            })?;
        target
            .validate()
            .map_err(|error| ConservativeRemapError::InvalidSurface {
                role: "target",
                reason: error.to_string(),
            })?;
        validate_radius(source, target)?;
        check_cancelled(&mut is_cancelled)?;

        let source_ref = SurfaceRef::for_spherical(source);
        let target_ref = SurfaceRef::for_spherical(target);
        if source_ref == target_ref {
            return identity_map(source, source_ref);
        }

        let orientation = GeometryOrientation::new(source, target, source_ref, target_ref);
        let coarse_neighbors = build_neighbors(orientation.coarse);
        let search = KdTree::new(orientation.coarse);
        let mut marks = vec![0_u32; orientation.coarse.cells().len()];
        let mut epoch = 0_u32;
        let mut overlaps = Vec::with_capacity(orientation.fine.cells().len() * 3);

        for (fine_index, fine_cell) in orientation.fine.cells().iter().enumerate() {
            if fine_index % 256 == 0 {
                check_cancelled(&mut is_cancelled)?;
            }
            let nearest = search.nearest(fine_cell.centroid).ok_or(
                ConservativeRemapError::SpatialSearchFailed {
                    fine_cell: fine_cell.id,
                },
            )?;
            let mut closed = None;
            for rings in INITIAL_ADJACENCY_RINGS..=MAX_ADJACENCY_RINGS {
                epoch = epoch.wrapping_add(1);
                if epoch == 0 {
                    marks.fill(0);
                    epoch = 1;
                }
                let candidates =
                    adjacency_candidates(nearest, rings, &coarse_neighbors, &mut marks, epoch);
                let row = intersect_fine_cell(
                    orientation.fine,
                    fine_index,
                    orientation.coarse,
                    &candidates,
                )?;
                let raw_sum = compensated_sum(row.iter().map(|overlap| overlap.area_m2))?;
                let expected = fine_cell.area.get();
                let relative_error = relative_error(raw_sum, expected);
                if relative_error <= RAW_FINE_CELL_CLOSURE_LIMIT {
                    closed = Some(row);
                    break;
                }
                if rings == MAX_ADJACENCY_RINGS {
                    return Err(ConservativeRemapError::UncoveredFineCell {
                        fine_cell: fine_cell.id,
                        covered_m2: raw_sum,
                        expected_m2: expected,
                        relative_error,
                        max: RAW_FINE_CELL_CLOSURE_LIMIT,
                    });
                }
            }
            let row = closed.expect("the bounded ring loop either closes or returns");
            if row.is_empty() {
                return Err(ConservativeRemapError::EmptyFineCellOverlap {
                    fine_cell: fine_cell.id,
                });
            }
            overlaps.extend(row);
        }

        overlaps.sort_unstable_by_key(|overlap| (overlap.fine_cell, overlap.coarse_cell));
        let balance_iterations = balance_margins(
            &mut overlaps,
            orientation.fine,
            orientation.coarse,
            &mut is_cancelled,
        )?;
        let max_adjustment = overlaps
            .iter()
            .map(|overlap| relative_error(overlap.area_m2, overlap.raw_area_m2))
            .fold(0.0_f64, f64::max);
        if max_adjustment > MAX_GEOMETRIC_ADJUSTMENT {
            return Err(ConservativeRemapError::ExcessiveGeometricAdjustment {
                found: max_adjustment,
                max: MAX_GEOMETRIC_ADJUSTMENT,
            });
        }
        check_cancelled(&mut is_cancelled)?;
        orientation.finish(overlaps, balance_iterations, max_adjustment)
    }
}

fn identity_map(
    surface: &SphericalSurfaceSnapshot,
    surface_ref: SurfaceRef,
) -> Result<ConservativeSurfaceMap, ConservativeRemapError> {
    let areas = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(areas.len() + 1);
    let mut weights = Vec::with_capacity(areas.len());
    offsets.push(0);
    for (index, &area) in areas.iter().enumerate() {
        weights.push(SurfaceOverlapWeight::new(
            CellId::from_raw(index as u32),
            area,
            TangentTransform::identity(),
        )?);
        offsets.push(weights.len() as u32);
    }
    Ok(ConservativeSurfaceMap::new(
        CONSERVATIVE_SURFACE_MAP_SCHEMA_V1,
        surface_ref,
        surface_ref,
        areas.clone(),
        areas,
        offsets,
        weights,
        0,
        0.0,
    )?)
}

struct GeometryOrientation<'a> {
    original_source: &'a SphericalSurfaceSnapshot,
    original_target: &'a SphericalSurfaceSnapshot,
    source_ref: SurfaceRef,
    target_ref: SurfaceRef,
    fine: &'a SphericalSurfaceSnapshot,
    coarse: &'a SphericalSurfaceSnapshot,
    source_is_fine: bool,
}

impl<'a> GeometryOrientation<'a> {
    fn new(
        source: &'a SphericalSurfaceSnapshot,
        target: &'a SphericalSurfaceSnapshot,
        source_ref: SurfaceRef,
        target_ref: SurfaceRef,
    ) -> Self {
        let source_is_fine = source.cells().len() > target.cells().len()
            || (source.cells().len() == target.cells().len() && source_ref > target_ref);
        let (fine, coarse) = if source_is_fine {
            (source, target)
        } else {
            (target, source)
        };
        Self {
            original_source: source,
            original_target: target,
            source_ref,
            target_ref,
            fine,
            coarse,
            source_is_fine,
        }
    }

    fn finish(
        self,
        overlaps: Vec<GeometricOverlap>,
        balance_iterations: u16,
        max_adjustment: f64,
    ) -> Result<ConservativeSurfaceMap, ConservativeRemapError> {
        let source_areas = cell_areas(self.original_source);
        let target_areas = cell_areas(self.original_target);
        let mut target_rows = vec![Vec::<(u32, f64)>::new(); target_areas.len()];
        for overlap in overlaps {
            let (source_cell, target_cell) = if self.source_is_fine {
                (overlap.fine_cell, overlap.coarse_cell)
            } else {
                (overlap.coarse_cell, overlap.fine_cell)
            };
            target_rows[target_cell as usize].push((source_cell, overlap.area_m2));
        }

        let mut offsets = Vec::with_capacity(target_rows.len() + 1);
        let mut weights = Vec::new();
        offsets.push(0);
        for (target_index, row) in target_rows.iter_mut().enumerate() {
            row.sort_unstable_by_key(|(source, _)| *source);
            for &(source_index, area_m2) in row.iter() {
                let transform = tangent_transform(
                    &self.original_source.cells()[source_index as usize].centroid,
                    &self.original_target.cells()[target_index].centroid,
                )?;
                weights.push(SurfaceOverlapWeight::new(
                    CellId::from_raw(source_index),
                    area_m2,
                    transform,
                )?);
            }
            offsets.push(u32::try_from(weights.len()).map_err(|_| {
                ConservativeRemapError::SparseAllocationOverflow {
                    overlaps: weights.len(),
                }
            })?);
        }

        Ok(ConservativeSurfaceMap::new(
            CONSERVATIVE_SURFACE_MAP_SCHEMA_V1,
            self.source_ref,
            self.target_ref,
            source_areas,
            target_areas,
            offsets,
            weights,
            balance_iterations,
            max_adjustment,
        )?)
    }
}

fn validate_radius(
    source: &SphericalSurfaceSnapshot,
    target: &SphericalSurfaceSnapshot,
) -> Result<(), ConservativeRemapError> {
    let source_m = source.radius().get();
    let target_m = target.radius().get();
    let tolerance_m = RADIUS_RELATIVE_TOLERANCE * source_m.abs().max(target_m.abs());
    if (source_m - target_m).abs() > tolerance_m {
        return Err(ConservativeRemapError::RadiusMismatch {
            source_m,
            target_m,
            tolerance_m,
        });
    }
    Ok(())
}

fn cell_areas(surface: &SphericalSurfaceSnapshot) -> Vec<f64> {
    surface.cells().iter().map(|cell| cell.area.get()).collect()
}

fn build_neighbors(surface: &SphericalSurfaceSnapshot) -> Vec<Vec<usize>> {
    surface
        .cells()
        .iter()
        .map(|cell| {
            let mut neighbors = cell
                .boundary_edges
                .iter()
                .filter_map(|&edge| surface.opposite_cell(cell.id, edge))
                .map(|cell| cell.raw() as usize)
                .collect::<Vec<_>>();
            neighbors.sort_unstable();
            neighbors.dedup();
            neighbors
        })
        .collect()
}

fn adjacency_candidates(
    start: usize,
    rings: usize,
    neighbors: &[Vec<usize>],
    marks: &mut [u32],
    epoch: u32,
) -> Vec<usize> {
    let mut all = vec![start];
    let mut frontier = vec![start];
    marks[start] = epoch;
    for _ in 0..rings {
        let mut next = Vec::new();
        for &cell in &frontier {
            for &neighbor in &neighbors[cell] {
                if marks[neighbor] != epoch {
                    marks[neighbor] = epoch;
                    next.push(neighbor);
                    all.push(neighbor);
                }
            }
        }
        next.sort_unstable();
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    all.sort_unstable();
    all
}

#[derive(Debug, Clone, Copy)]
struct GeometricOverlap {
    fine_cell: u32,
    coarse_cell: u32,
    raw_area_m2: f64,
    area_m2: f64,
}

fn intersect_fine_cell(
    fine: &SphericalSurfaceSnapshot,
    fine_index: usize,
    coarse: &SphericalSurfaceSnapshot,
    candidates: &[usize],
) -> Result<Vec<GeometricOverlap>, ConservativeRemapError> {
    let fine_cell = &fine.cells()[fine_index];
    let fine_polygon = cell_polygon(fine, &fine_cell.boundary_vertices);
    let radius_squared = fine.radius().get() * fine.radius().get();
    let mut row = Vec::new();
    for &coarse_index in candidates {
        let coarse_cell = &coarse.cells()[coarse_index];
        let clipped = clip_against_cell(&fine_polygon, coarse, coarse_cell.id)?;
        let steradians = spherical_polygon_area(&clipped)?;
        if steradians <= MIN_INTERSECTION_STERADIANS {
            continue;
        }
        let area_m2 = steradians * radius_squared;
        if !area_m2.is_finite() || area_m2 <= 0.0 {
            return Err(ConservativeRemapError::InvalidIntersectionArea {
                fine_cell: fine_cell.id,
                coarse_cell: coarse_cell.id,
                found_m2: area_m2,
            });
        }
        row.push(GeometricOverlap {
            fine_cell: fine_cell.id.raw(),
            coarse_cell: coarse_cell.id.raw(),
            raw_area_m2: area_m2,
            area_m2,
        });
    }
    row.sort_unstable_by_key(|overlap| overlap.coarse_cell);
    Ok(row)
}

fn cell_polygon(
    surface: &SphericalSurfaceSnapshot,
    vertices: &[SurfaceVertexId],
) -> Vec<UnitVector3> {
    vertices
        .iter()
        .map(|&vertex| surface.vertices()[vertex.raw() as usize].position)
        .collect()
}

fn clip_against_cell(
    polygon: &[UnitVector3],
    surface: &SphericalSurfaceSnapshot,
    cell_id: CellId,
) -> Result<Vec<UnitVector3>, ConservativeRemapError> {
    let cell = &surface.cells()[cell_id.raw() as usize];
    let mut current = polygon.to_vec();
    for side in 0..cell.boundary_vertices.len() {
        if current.len() < 3 {
            return Ok(Vec::new());
        }
        let first = surface.vertices()[cell.boundary_vertices[side].raw() as usize].position;
        let second = surface.vertices()
            [cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()].raw() as usize]
            .position;
        let normal =
            cross_unit(first, second).ok_or(ConservativeRemapError::DegenerateClipBoundary {
                coarse_cell: cell_id,
                side,
            })?;
        current = clip_halfspace(&current, normal)?;
    }
    deduplicate_polygon(&mut current);
    Ok(current)
}

fn clip_halfspace(
    polygon: &[UnitVector3],
    inward_normal: UnitVector3,
) -> Result<Vec<UnitVector3>, ConservativeRemapError> {
    if polygon.is_empty() {
        return Ok(Vec::new());
    }
    let mut output = Vec::with_capacity(polygon.len() + 1);
    let mut previous = polygon[polygon.len() - 1];
    let mut previous_distance = inward_normal.dot(previous);
    let mut previous_inside = previous_distance >= -CLIP_HALFSPACE_EPSILON;
    for &current in polygon {
        let current_distance = inward_normal.dot(current);
        let current_inside = current_distance >= -CLIP_HALFSPACE_EPSILON;
        if current_inside != previous_inside {
            output.push(arc_plane_intersection(
                previous,
                current,
                previous_distance,
                current_distance,
            )?);
        }
        if current_inside {
            output.push(current);
        }
        previous = current;
        previous_distance = current_distance;
        previous_inside = current_inside;
    }
    deduplicate_polygon(&mut output);
    Ok(output)
}

fn arc_plane_intersection(
    first: UnitVector3,
    second: UnitVector3,
    first_distance: f64,
    second_distance: f64,
) -> Result<UnitVector3, ConservativeRemapError> {
    if first_distance.abs() <= CLIP_HALFSPACE_EPSILON {
        return Ok(first);
    }
    if second_distance.abs() <= CLIP_HALFSPACE_EPSILON {
        return Ok(second);
    }
    if first_distance.is_sign_positive() == second_distance.is_sign_positive() {
        return Ok(if first_distance.abs() <= second_distance.abs() {
            first
        } else {
            second
        });
    }
    let first_weight = second_distance.abs();
    let second_weight = first_distance.abs();
    let first_components = first.components();
    let second_components = second.components();
    UnitVector3::new(
        first_components[0] * first_weight + second_components[0] * second_weight,
        first_components[1] * first_weight + second_components[1] * second_weight,
        first_components[2] * first_weight + second_components[2] * second_weight,
    )
    .map_err(|_| ConservativeRemapError::DegenerateArcIntersection)
}

fn deduplicate_polygon(polygon: &mut Vec<UnitVector3>) {
    let mut output = Vec::with_capacity(polygon.len());
    for &vertex in polygon.iter() {
        if output.last().is_none_or(|&previous| {
            squared_distance(previous, vertex) > DUPLICATE_VERTEX_DISTANCE_SQUARED
        }) {
            output.push(vertex);
        }
    }
    if output.len() > 1
        && squared_distance(output[0], output[output.len() - 1])
            <= DUPLICATE_VERTEX_DISTANCE_SQUARED
    {
        output.pop();
    }
    *polygon = output;
}

fn spherical_polygon_area(polygon: &[UnitVector3]) -> Result<f64, ConservativeRemapError> {
    if polygon.len() < 3 {
        return Ok(0.0);
    }
    let centroid_components = polygon.iter().fold([0.0; 3], |mut sum, vertex| {
        let components = vertex.components();
        for axis in 0..3 {
            sum[axis] += components[axis];
        }
        sum
    });
    let centroid = UnitVector3::new(
        centroid_components[0],
        centroid_components[1],
        centroid_components[2],
    )
    .map_err(|_| ConservativeRemapError::DegenerateIntersectionPolygon)?;
    let mut sum = CompensatedSum::default();
    for index in 0..polygon.len() {
        let area = spherical_triangle_area_unit(
            centroid,
            polygon[index],
            polygon[(index + 1) % polygon.len()],
        );
        if !area.is_finite() || area < 0.0 {
            return Err(ConservativeRemapError::DegenerateIntersectionPolygon);
        }
        sum.add(area)?;
    }
    sum.total()
}

fn balance_margins(
    overlaps: &mut [GeometricOverlap],
    fine: &SphericalSurfaceSnapshot,
    coarse: &SphericalSurfaceSnapshot,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<u16, ConservativeRemapError> {
    let fine_areas = cell_areas(fine);
    let coarse_areas = cell_areas(coarse);
    let mut row_ranges = vec![(0_usize, 0_usize); fine_areas.len()];
    let mut cursor = 0;
    for (fine_index, range) in row_ranges.iter_mut().enumerate() {
        let start = cursor;
        while cursor < overlaps.len() && overlaps[cursor].fine_cell as usize == fine_index {
            cursor += 1;
        }
        if start == cursor {
            return Err(ConservativeRemapError::EmptyFineCellOverlap {
                fine_cell: CellId::from_raw(fine_index as u32),
            });
        }
        *range = (start, cursor);
    }
    if cursor != overlaps.len() {
        return Err(ConservativeRemapError::SparseAllocationOverflow {
            overlaps: overlaps.len(),
        });
    }

    let mut coarse_sums = vec![0.0; coarse_areas.len()];
    for iteration in 1..=MAX_BALANCE_ITERATIONS {
        check_cancelled(is_cancelled)?;
        for (fine_index, &(start, end)) in row_ranges.iter().enumerate() {
            let sum = compensated_sum(overlaps[start..end].iter().map(|item| item.area_m2))?;
            if sum <= 0.0 {
                return Err(ConservativeRemapError::ZeroBalanceMargin {
                    role: "fine row",
                    cell: CellId::from_raw(fine_index as u32),
                });
            }
            let scale = fine_areas[fine_index] / sum;
            for overlap in &mut overlaps[start..end] {
                overlap.area_m2 *= scale;
            }
        }

        coarse_sums.fill(0.0);
        let mut compensated = vec![CompensatedSum::default(); coarse_areas.len()];
        for overlap in overlaps.iter() {
            compensated[overlap.coarse_cell as usize].add(overlap.area_m2)?;
        }
        for (sum, value) in compensated.into_iter().zip(&mut coarse_sums) {
            *value = sum.total()?;
        }
        for (coarse_index, &sum) in coarse_sums.iter().enumerate() {
            if sum <= 0.0 {
                return Err(ConservativeRemapError::ZeroBalanceMargin {
                    role: "coarse column",
                    cell: CellId::from_raw(coarse_index as u32),
                });
            }
        }
        for overlap in overlaps.iter_mut() {
            overlap.area_m2 *= coarse_areas[overlap.coarse_cell as usize]
                / coarse_sums[overlap.coarse_cell as usize];
            if !overlap.area_m2.is_finite() || overlap.area_m2 <= 0.0 {
                return Err(ConservativeRemapError::NonFiniteBalance);
            }
        }

        let (max_fine_error, max_coarse_error) =
            margin_errors(overlaps, &row_ranges, &fine_areas, &coarse_areas)?;
        if max_fine_error <= BALANCE_CLOSURE_LIMIT && max_coarse_error <= BALANCE_CLOSURE_LIMIT {
            return Ok(iteration);
        }
    }
    let (max_fine_error, max_coarse_error) =
        margin_errors(overlaps, &row_ranges, &fine_areas, &coarse_areas)?;
    Err(ConservativeRemapError::BalanceDidNotConverge {
        iterations: MAX_BALANCE_ITERATIONS,
        max_fine_relative_error: max_fine_error,
        max_coarse_relative_error: max_coarse_error,
        max: BALANCE_CLOSURE_LIMIT,
    })
}

fn margin_errors(
    overlaps: &[GeometricOverlap],
    row_ranges: &[(usize, usize)],
    fine_areas: &[f64],
    coarse_areas: &[f64],
) -> Result<(f64, f64), ConservativeRemapError> {
    let mut max_fine = 0.0_f64;
    for (fine_index, &(start, end)) in row_ranges.iter().enumerate() {
        let sum = compensated_sum(overlaps[start..end].iter().map(|item| item.area_m2))?;
        max_fine = max_fine.max(relative_error(sum, fine_areas[fine_index]));
    }
    let mut coarse_sums = vec![CompensatedSum::default(); coarse_areas.len()];
    for overlap in overlaps {
        coarse_sums[overlap.coarse_cell as usize].add(overlap.area_m2)?;
    }
    let mut max_coarse = 0.0_f64;
    for (sum, &expected) in coarse_sums.into_iter().zip(coarse_areas) {
        max_coarse = max_coarse.max(relative_error(sum.total()?, expected));
    }
    Ok((max_fine, max_coarse))
}

fn tangent_transform(
    source: &UnitVector3,
    target: &UnitVector3,
) -> Result<TangentTransform, ConservativeRemapError> {
    let (source_east, source_north) = canonical_east_north_basis(*source);
    let (target_east, target_north) = canonical_east_north_basis(*target);
    Ok(TangentTransform::new([
        dot3(target_east, source_east),
        dot3(target_east, source_north),
        dot3(target_north, source_east),
        dot3(target_north, source_north),
    ])?)
}

fn cross_unit(first: UnitVector3, second: UnitVector3) -> Option<UnitVector3> {
    let a = first.components();
    let b = second.components();
    UnitVector3::new(
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )
    .ok()
}

fn squared_distance(first: UnitVector3, second: UnitVector3) -> f64 {
    let first = first.components();
    let second = second.components();
    (first[0] - second[0]).powi(2) + (first[1] - second[1]).powi(2) + (first[2] - second[2]).powi(2)
}

fn dot3(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn relative_error(found: f64, expected: f64) -> f64 {
    (found - expected).abs() / expected.abs().max(f64::MIN_POSITIVE)
}

fn compensated_sum(values: impl IntoIterator<Item = f64>) -> Result<f64, ConservativeRemapError> {
    let mut sum = CompensatedSum::default();
    for value in values {
        sum.add(value)?;
    }
    sum.total()
}

#[derive(Debug, Clone, Copy, Default)]
struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) -> Result<(), ConservativeRemapError> {
        let next = self.sum + value;
        if !next.is_finite() {
            return Err(ConservativeRemapError::NonFiniteBalance);
        }
        let correction = if self.sum.abs() >= value.abs() {
            (self.sum - next) + value
        } else {
            (value - next) + self.sum
        };
        self.sum = next;
        self.correction += correction;
        if !self.correction.is_finite() {
            return Err(ConservativeRemapError::NonFiniteBalance);
        }
        Ok(())
    }

    fn total(self) -> Result<f64, ConservativeRemapError> {
        let total = self.sum + self.correction;
        total
            .is_finite()
            .then_some(total)
            .ok_or(ConservativeRemapError::NonFiniteBalance)
    }
}

struct KdTree {
    nodes: Vec<KdNode>,
    points: Vec<[f64; 3]>,
    root: Option<usize>,
}

#[derive(Debug)]
struct KdNode {
    cell: usize,
    axis: usize,
    left: Option<usize>,
    right: Option<usize>,
}

impl KdTree {
    fn new(surface: &SphericalSurfaceSnapshot) -> Self {
        let mut cells = (0..surface.cells().len()).collect::<Vec<_>>();
        let mut nodes = Vec::with_capacity(cells.len());
        let root = build_kd_node(&mut cells, 0, surface, &mut nodes);
        let points = surface
            .cells()
            .iter()
            .map(|cell| cell.site.components())
            .collect();
        Self {
            nodes,
            points,
            root,
        }
    }

    fn nearest(&self, query: UnitVector3) -> Option<usize> {
        let root = self.root?;
        let mut best = (f64::INFINITY, usize::MAX);
        nearest_kd_node(
            root,
            &self.nodes,
            &self.points,
            query.components(),
            &mut best,
        );
        (best.1 != usize::MAX).then_some(best.1)
    }
}

fn build_kd_node(
    cells: &mut [usize],
    depth: usize,
    surface: &SphericalSurfaceSnapshot,
    nodes: &mut Vec<KdNode>,
) -> Option<usize> {
    if cells.is_empty() {
        return None;
    }
    let axis = depth % 3;
    cells.sort_unstable_by(|&left, &right| {
        surface.cells()[left].site.components()[axis]
            .total_cmp(&surface.cells()[right].site.components()[axis])
            .then_with(|| left.cmp(&right))
    });
    let middle = cells.len() / 2;
    let (left_cells, rest) = cells.split_at_mut(middle);
    let (middle_cell, right_cells) = rest.split_first_mut().expect("nonempty split");
    let node_index = nodes.len();
    nodes.push(KdNode {
        cell: *middle_cell,
        axis,
        left: None,
        right: None,
    });
    let left = build_kd_node(left_cells, depth + 1, surface, nodes);
    let right = build_kd_node(right_cells, depth + 1, surface, nodes);
    nodes[node_index].left = left;
    nodes[node_index].right = right;
    Some(node_index)
}

fn nearest_kd_node(
    node_index: usize,
    nodes: &[KdNode],
    points: &[[f64; 3]],
    query: [f64; 3],
    best: &mut (f64, usize),
) {
    let node = &nodes[node_index];
    let point = points[node.cell];
    let distance = (point[0] - query[0]).powi(2)
        + (point[1] - query[1]).powi(2)
        + (point[2] - query[2]).powi(2);
    if distance.total_cmp(&best.0).is_lt()
        || (distance.to_bits() == best.0.to_bits() && node.cell < best.1)
    {
        *best = (distance, node.cell);
    }
    let delta = query[node.axis] - point[node.axis];
    let (near, far) = if delta <= 0.0 {
        (node.left, node.right)
    } else {
        (node.right, node.left)
    };
    if let Some(near) = near {
        nearest_kd_node(near, nodes, points, query, best);
    }
    if delta * delta <= best.0 {
        if let Some(far) = far {
            nearest_kd_node(far, nodes, points, query, best);
        }
    }
}

fn check_cancelled(is_cancelled: &mut impl FnMut() -> bool) -> Result<(), ConservativeRemapError> {
    if is_cancelled() {
        Err(ConservativeRemapError::Cancelled)
    } else {
        Ok(())
    }
}

/// Failures returned by spherical overlap construction and balancing.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ConservativeRemapError {
    #[error("conservative remap was cancelled")]
    Cancelled,
    #[error("invalid {role} spherical surface: {reason}")]
    InvalidSurface { role: &'static str, reason: String },
    #[error("source radius {source_m} m and target radius {target_m} m differ by more than {tolerance_m} m")]
    RadiusMismatch {
        source_m: f64,
        target_m: f64,
        tolerance_m: f64,
    },
    #[error("nearest coarse cell search failed for fine cell {fine_cell:?}")]
    SpatialSearchFailed { fine_cell: CellId },
    #[error("coarse cell {coarse_cell:?} side {side} has a degenerate clip plane")]
    DegenerateClipBoundary { coarse_cell: CellId, side: usize },
    #[error("a spherical arc did not intersect a clip plane robustly")]
    DegenerateArcIntersection,
    #[error("a spherical intersection polygon is degenerate")]
    DegenerateIntersectionPolygon,
    #[error(
        "fine cell {fine_cell:?} and coarse cell {coarse_cell:?} produced invalid area {found_m2}"
    )]
    InvalidIntersectionArea {
        fine_cell: CellId,
        coarse_cell: CellId,
        found_m2: f64,
    },
    #[error("fine cell {fine_cell:?} overlap covers {covered_m2} of {expected_m2} m2; relative error {relative_error} > {max}")]
    UncoveredFineCell {
        fine_cell: CellId,
        covered_m2: f64,
        expected_m2: f64,
        relative_error: f64,
        max: f64,
    },
    #[error("fine cell {fine_cell:?} has no positive coarse overlap")]
    EmptyFineCellOverlap { fine_cell: CellId },
    #[error("{role} {cell:?} has a zero balancing margin")]
    ZeroBalanceMargin { role: &'static str, cell: CellId },
    #[error("remap balancing produced a non-finite value")]
    NonFiniteBalance,
    #[error("remap margins did not converge in {iterations} iterations: fine {max_fine_relative_error}, coarse {max_coarse_relative_error}, maximum {max}")]
    BalanceDidNotConverge {
        iterations: u16,
        max_fine_relative_error: f64,
        max_coarse_relative_error: f64,
        max: f64,
    },
    #[error("balanced overlap changed raw geometry by {found}; maximum is {max}")]
    ExcessiveGeometricAdjustment { found: f64, max: f64 },
    #[error("sparse overlap count {overlaps} exceeds addressable storage")]
    SparseAllocationOverflow { overlaps: usize },
    #[error("constructed conservative map is invalid: {0}")]
    InvalidMap(#[from] ConservativeSurfaceMapError),
}

#[cfg(test)]
mod tests {
    use super::{cell_polygon, clip_against_cell, spherical_polygon_area, KdTree};
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::{Meters, SphericalSpaceSpec};

    fn surface() -> crate::world::spatial::SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(1.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap()
    }

    #[test]
    fn clipping_a_cell_by_itself_preserves_its_unit_area() {
        let surface = surface();
        let cell = &surface.cells()[0];
        let polygon = cell_polygon(&surface, &cell.boundary_vertices);
        let clipped = clip_against_cell(&polygon, &surface, cell.id).unwrap();
        let area = spherical_polygon_area(&clipped).unwrap();
        assert!((area - cell.area.get()).abs() <= 1.0e-12);
    }

    #[test]
    fn kd_tree_returns_each_exact_generating_site() {
        let surface = surface();
        let tree = KdTree::new(&surface);
        for cell in surface.cells() {
            assert_eq!(tree.nearest(cell.site), Some(cell.id.raw() as usize));
        }
    }
}
