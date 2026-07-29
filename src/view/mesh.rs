use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::{CellId, WorldPoint, WorldRect};

use super::DisplayPrepareError;

/// Maximum number of cells accepted by one prepared display mesh.
pub const MAX_DISPLAY_CELLS: usize = 200_000;
/// Maximum number of vertices accepted by one prepared display mesh.
pub const MAX_DISPLAY_VERTICES: usize = 6_000_000;
/// Maximum number of indices accepted by one prepared display mesh.
pub const MAX_DISPLAY_INDICES: usize = 12_000_000;

const MAX_PICKER_REFERENCES: usize = MAX_DISPLAY_INDICES;
const NORMALIZED_TOLERANCE: f64 = 1.0e-6;
const POLYGON_EPSILON: f32 = 1.0e-7;

/// Read-only geometry needed to prepare one indexed cell display.
pub trait CellGeometrySource {
    /// Returns the world-space extent covering every polygon.
    fn bounds(&self) -> WorldRect;
    /// Returns the stable cell cardinality.
    fn cell_count(&self) -> usize;
    /// Borrows one cell polygon by stable identifier.
    fn polygon(&self, cell: CellId) -> Option<&[WorldPoint]>;
}

impl CellGeometrySource for SpatialSnapshot {
    fn bounds(&self) -> WorldRect {
        Topology::bounds(self)
    }

    fn cell_count(&self) -> usize {
        Topology::cell_count(self)
    }

    fn polygon(&self, cell: CellId) -> Option<&[WorldPoint]> {
        Topology::cell(self, cell).map(|cell| cell.polygon.as_slice())
    }
}

/// Whether absent polygons fail preparation or remain non-drawable cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshCompleteness {
    /// Every stable cell must have a polygon.
    RequireAll,
    /// Absent polygons remain valid empty slots.
    AllowMissing,
}

/// One normalized cell vertex used by renderer-neutral prepared meshes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayVertex {
    /// Position normalized against the complete world rectangle.
    pub position: [f32; 2],
    /// Raw stable cell identifier used for field lookup.
    pub cell: u32,
}

/// A deterministic, bounded cell mesh and its CPU picking index.
#[derive(Debug, Clone)]
pub struct PreparedCellMesh {
    bounds: WorldRect,
    local_extent: [f32; 2],
    cell_count: usize,
    vertices: Vec<DisplayVertex>,
    indices: Vec<u32>,
    picker: CellPicker,
}

impl PreparedCellMesh {
    /// Builds a complete prepared mesh or returns one structured validation error.
    pub fn build(
        source: &impl CellGeometrySource,
        completeness: MeshCompleteness,
    ) -> Result<Self, DisplayPrepareError> {
        Self::build_with_budgets(source, completeness, &MeshBudgets::DEFAULT)
    }

    fn build_with_budgets(
        source: &impl CellGeometrySource,
        completeness: MeshCompleteness,
        budgets: &MeshBudgets,
    ) -> Result<Self, DisplayPrepareError> {
        let cell_count = source.cell_count();
        if cell_count > MAX_DISPLAY_CELLS {
            return Err(DisplayPrepareError::CellBudgetExceeded {
                actual: cell_count,
                max: MAX_DISPLAY_CELLS,
            });
        }

        let bounds = source.bounds();
        let width = bounds.width().get();
        let height = bounds.height().get();
        let width_f32 = width as f32;
        let height_f32 = height as f32;
        if !width_f32.is_finite()
            || !height_f32.is_finite()
            || width_f32 <= 0.0
            || height_f32 <= 0.0
        {
            return Err(DisplayPrepareError::LocalExtentOutOfRange { width, height });
        }
        let local_extent = [width_f32, height_f32];

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut polygons = Vec::with_capacity(cell_count);
        for index in 0..cell_count {
            let raw_cell =
                u32::try_from(index).map_err(|_| DisplayPrepareError::IntegerOverflow {
                    context: "cell identifier",
                })?;
            let cell = CellId::from_raw(raw_cell);
            let Some(polygon) = source.polygon(cell) else {
                if completeness == MeshCompleteness::RequireAll {
                    return Err(DisplayPrepareError::MissingCellGeometry { cell });
                }
                polygons.push(None);
                continue;
            };
            if polygon.len() < 3 {
                return Err(DisplayPrepareError::MalformedCellGeometry { cell });
            }

            let (next_vertex_count, _) =
                check_mesh_growth(vertices.len(), indices.len(), polygon.len(), budgets)?;
            let mut normalized = Vec::with_capacity(polygon.len());
            for point in polygon {
                let x = (point.x().get() - bounds.min().x().get()) / width;
                let y = (point.y().get() - bounds.min().y().get()) / height;
                if !x.is_finite()
                    || !y.is_finite()
                    || !(-NORMALIZED_TOLERANCE..=1.0 + NORMALIZED_TOLERANCE).contains(&x)
                    || !(-NORMALIZED_TOLERANCE..=1.0 + NORMALIZED_TOLERANCE).contains(&y)
                {
                    return Err(DisplayPrepareError::CoordinateOutOfBounds { cell });
                }
                let position = [x.clamp(0.0, 1.0) as f32, y.clamp(0.0, 1.0) as f32];
                if !position[0].is_finite() || !position[1].is_finite() {
                    return Err(DisplayPrepareError::CoordinateConversionFailed { cell });
                }
                normalized.push(position);
            }
            if !is_counter_clockwise_convex(&normalized) {
                return Err(DisplayPrepareError::MalformedCellGeometry { cell });
            }

            vertices.reserve(next_vertex_count - vertices.len());
            let base = u32::try_from(vertices.len()).map_err(|_| {
                DisplayPrepareError::IntegerOverflow {
                    context: "vertex index",
                }
            })?;
            vertices.extend(normalized.iter().copied().map(|position| DisplayVertex {
                position,
                cell: raw_cell,
            }));
            for offset in 1..polygon.len() - 1 {
                let offset =
                    u32::try_from(offset).map_err(|_| DisplayPrepareError::IntegerOverflow {
                        context: "triangle offset",
                    })?;
                let second =
                    base.checked_add(offset)
                        .ok_or(DisplayPrepareError::IntegerOverflow {
                            context: "triangle index",
                        })?;
                let third = second
                    .checked_add(1)
                    .ok_or(DisplayPrepareError::IntegerOverflow {
                        context: "triangle index",
                    })?;
                indices.extend_from_slice(&[base, second, third]);
            }
            polygons.push(Some(normalized));
        }

        let picker = CellPicker::build(polygons, cell_count, budgets)?;
        Ok(Self {
            bounds,
            local_extent,
            cell_count,
            vertices,
            indices,
            picker,
        })
    }

    /// Returns the original world-space extent.
    pub const fn bounds(&self) -> WorldRect {
        self.bounds
    }

    /// Returns origin-shifted local width and height used by the canvas.
    pub const fn local_extent(&self) -> [f32; 2] {
        self.local_extent
    }

    /// Returns the stable number of cells, including allowed missing polygons.
    pub const fn cell_count(&self) -> usize {
        self.cell_count
    }

    /// Returns vertices in stable cell and polygon order.
    pub fn vertices(&self) -> &[DisplayVertex] {
        &self.vertices
    }

    /// Returns fan-triangulated indices in stable cell order.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Selects the first stable cell containing a normalized point.
    pub fn pick_normalized(&self, normalized: [f32; 2]) -> Option<CellId> {
        self.picker.pick(normalized)
    }

    /// Selects a cell from origin-shifted local canvas coordinates.
    pub fn pick_local(&self, local: [f32; 2]) -> Option<CellId> {
        if local.iter().any(|value| !value.is_finite())
            || local[0] < 0.0
            || local[1] < 0.0
            || local[0] > self.local_extent[0]
            || local[1] > self.local_extent[1]
        {
            return None;
        }
        self.pick_normalized([
            local[0] / self.local_extent[0],
            local[1] / self.local_extent[1],
        ])
    }
}

#[derive(Debug, Clone, Copy)]
struct MeshBudgets {
    vertices: usize,
    indices: usize,
    picker_references: usize,
}

impl MeshBudgets {
    const DEFAULT: Self = Self {
        vertices: MAX_DISPLAY_VERTICES,
        indices: MAX_DISPLAY_INDICES,
        picker_references: MAX_PICKER_REFERENCES,
    };
}

fn check_mesh_growth(
    current_vertices: usize,
    current_indices: usize,
    polygon_vertices: usize,
    budgets: &MeshBudgets,
) -> Result<(usize, usize), DisplayPrepareError> {
    let next_vertices = current_vertices.checked_add(polygon_vertices).ok_or(
        DisplayPrepareError::IntegerOverflow {
            context: "display vertex count",
        },
    )?;
    if next_vertices > budgets.vertices {
        return Err(DisplayPrepareError::VertexBudgetExceeded {
            actual: next_vertices,
            max: budgets.vertices,
        });
    }

    let triangle_count =
        polygon_vertices
            .checked_sub(2)
            .ok_or(DisplayPrepareError::IntegerOverflow {
                context: "polygon triangle count",
            })?;
    let additional_indices =
        triangle_count
            .checked_mul(3)
            .ok_or(DisplayPrepareError::IntegerOverflow {
                context: "display index count",
            })?;
    let next_indices = current_indices.checked_add(additional_indices).ok_or(
        DisplayPrepareError::IntegerOverflow {
            context: "display index count",
        },
    )?;
    if next_indices > budgets.indices {
        return Err(DisplayPrepareError::IndexBudgetExceeded {
            actual: next_indices,
            max: budgets.indices,
        });
    }
    Ok((next_vertices, next_indices))
}

#[derive(Debug, Clone)]
struct CellPicker {
    side: usize,
    bins: Vec<Vec<CellId>>,
    polygons: Vec<Option<Vec<[f32; 2]>>>,
}

impl CellPicker {
    fn build(
        polygons: Vec<Option<Vec<[f32; 2]>>>,
        cell_count: usize,
        budgets: &MeshBudgets,
    ) -> Result<Self, DisplayPrepareError> {
        let side = (cell_count as f64).sqrt().ceil().clamp(1.0, 512.0) as usize;
        let bin_count = side
            .checked_mul(side)
            .ok_or(DisplayPrepareError::IntegerOverflow {
                context: "picker bin count",
            })?;
        let mut bins = vec![Vec::new(); bin_count];
        let mut reference_count = 0_usize;
        for (index, polygon) in polygons.iter().enumerate() {
            let Some(polygon) = polygon else {
                continue;
            };
            let cell = CellId::from_raw(u32::try_from(index).map_err(|_| {
                DisplayPrepareError::IntegerOverflow {
                    context: "picker cell identifier",
                }
            })?);
            let (min, max) = polygon_bounds(polygon);
            let min_x = bin_coordinate(min[0], side);
            let max_x = bin_coordinate(max[0], side);
            let min_y = bin_coordinate(min[1], side);
            let max_y = bin_coordinate(max[1], side);
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    reference_count = reference_count.checked_add(1).ok_or(
                        DisplayPrepareError::IntegerOverflow {
                            context: "picker reference count",
                        },
                    )?;
                    if reference_count > budgets.picker_references {
                        return Err(DisplayPrepareError::PickerBudgetExceeded {
                            actual: reference_count,
                            max: budgets.picker_references,
                        });
                    }
                    bins[y * side + x].push(cell);
                }
            }
        }
        for bin in &mut bins {
            bin.sort_unstable();
            bin.dedup();
        }
        Ok(Self {
            side,
            bins,
            polygons,
        })
    }

    fn pick(&self, point: [f32; 2]) -> Option<CellId> {
        if point.iter().any(|value| !value.is_finite())
            || !(0.0..=1.0).contains(&point[0])
            || !(0.0..=1.0).contains(&point[1])
        {
            return None;
        }
        let x = bin_coordinate(point[0], self.side);
        let y = bin_coordinate(point[1], self.side);
        self.bins[y * self.side + x].iter().copied().find(|cell| {
            self.polygons
                .get(cell.raw() as usize)
                .and_then(Option::as_deref)
                .is_some_and(|polygon| point_in_polygon(point, polygon))
        })
    }
}

fn bin_coordinate(value: f32, side: usize) -> usize {
    ((value * side as f32).floor() as usize).min(side - 1)
}

fn polygon_bounds(polygon: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    polygon.iter().copied().fold(
        (
            [f32::INFINITY, f32::INFINITY],
            [f32::NEG_INFINITY, f32::NEG_INFINITY],
        ),
        |(mut min, mut max), point| {
            min[0] = min[0].min(point[0]);
            min[1] = min[1].min(point[1]);
            max[0] = max[0].max(point[0]);
            max[1] = max[1].max(point[1]);
            (min, max)
        },
    )
}

fn is_counter_clockwise_convex(polygon: &[[f32; 2]]) -> bool {
    let mut twice_area = 0.0_f32;
    for index in 0..polygon.len() {
        let a = polygon[index];
        let b = polygon[(index + 1) % polygon.len()];
        let c = polygon[(index + 2) % polygon.len()];
        twice_area += a[0] * b[1] - b[0] * a[1];
        if cross(a, b, c) < -POLYGON_EPSILON {
            return false;
        }
    }
    if twice_area <= POLYGON_EPSILON {
        return false;
    }
    let fan_origin = polygon[0];
    // The emitted f32 triangle is usable whenever its exact normalized area stays positive.
    polygon[1..]
        .windows(2)
        .all(|triangle| cross(fan_origin, triangle[0], triangle[1]) > 0.0)
}

fn point_in_polygon(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    let mut inside = false;
    for index in 0..polygon.len() {
        let a = polygon[index];
        let b = polygon[(index + 1) % polygon.len()];
        if point_on_segment(point, a, b) {
            return true;
        }
        if (a[1] > point[1]) != (b[1] > point[1]) {
            let intersection = (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0];
            if point[0] < intersection {
                inside = !inside;
            }
        }
    }
    inside
}

fn point_on_segment(point: [f32; 2], a: [f32; 2], b: [f32; 2]) -> bool {
    cross(a, b, point).abs() <= POLYGON_EPSILON
        && point[0] >= a[0].min(b[0]) - POLYGON_EPSILON
        && point[0] <= a[0].max(b[0]) + POLYGON_EPSILON
        && point[1] >= a[1].min(b[1]) - POLYGON_EPSILON
        && point[1] <= a[1].max(b[1]) + POLYGON_EPSILON
}

fn cross(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

#[cfg(test)]
mod tests {
    use super::{check_mesh_growth, MeshBudgets};
    use crate::view::DisplayPrepareError;

    #[test]
    fn growth_checks_distinguish_vertex_and_index_budgets() {
        let budgets = MeshBudgets {
            vertices: 4,
            indices: 6,
            picker_references: 8,
        };

        assert!(matches!(
            check_mesh_growth(4, 0, 1, &budgets),
            Err(DisplayPrepareError::VertexBudgetExceeded { .. })
        ));
        assert!(matches!(
            check_mesh_growth(0, 6, 3, &budgets),
            Err(DisplayPrepareError::IndexBudgetExceeded { .. })
        ));
    }
}
