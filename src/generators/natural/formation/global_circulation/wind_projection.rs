//! Area means of the reconstructed atmospheric wind over refined consumer cells.
//! The dual of each cubed-sphere vertex is a triangle or a convex quadrilateral.
//! Split the latter consistently and intersect the query ray with its planar triangle.
use super::ClimateProjectionError;
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::world::natural::{ClimateWorkDomainSnapshot, CLIMATE_MONTH_COUNT};
use crate::world::spatial::{
    cross, dot, normalize_legacy_compatible, project_tangent, spherical_triangle_area_unit_raw,
    SphericalSurfaceSnapshot,
};

type MonthlyWind = [[f32; 3]; CLIMATE_MONTH_COUNT];

struct WindStencil {
    donors: [usize; 3],
    weights: [f64; 3],
}

impl WindStencil {
    fn at(grid: &CubedSphereGrid, donors: [u32; 3], direction: [f64; 3]) -> Option<Self> {
        let donors = donors.map(|id| id as usize);
        let [a, b, c] = donors.map(|i| grid.cells()[i].center_unit());
        let raw = [
            dot(direction, cross(b, c)),
            dot(direction, cross(c, a)),
            dot(direction, cross(a, b)),
        ];
        let sum = raw.iter().sum::<f64>();
        if sum <= 0.0 || raw.iter().any(|&v| v < 0.0) {
            return None;
        }
        Some(WindStencil {
            donors,
            weights: raw.map(|v| v / sum),
        })
    }
}

pub(super) fn refine(
    grid: &CubedSphereGrid,
    domain: &ClimateWorkDomainSnapshot,
    target: &SphericalSurfaceSnapshot,
    winds: [&[MonthlyWind]; 2],
    cancellation: &BuildCancellation,
) -> Result<[Vec<MonthlyWind>; 2], ClimateProjectionError> {
    // Each of the four primal corners contributes at most two dual triangles.
    let mut triangles: Vec<_> = (0..grid.cell_count())
        .map(|_| Vec::with_capacity(8))
        .collect();
    super::dual_mesh::for_each_triangle(grid, cancellation, |owners, ids| {
        for &owner in owners {
            triangles[owner as usize].push(ids);
        }
    })
    .map_err(|_| ClimateProjectionError::Cancelled)?;
    let mut outputs = std::array::from_fn(|_| Vec::with_capacity(target.cells().len()));
    let mut weights: Vec<(usize, f64)> = Vec::new();
    for cell in target.cells() {
        if cell.id.raw() % 256 == 0 && cancellation.is_cancelled() {
            return Err(ClimateProjectionError::Cancelled);
        }
        let candidates = domain
            .climate_to_source()
            .target_row(cell.id)
            .into_iter()
            .flatten()
            .flat_map(|overlap| &triangles[overlap.source_cell().raw() as usize]);
        weights.clear();
        let mut area_sum = 0.0;
        let centre = cell.centroid.components();
        // A centroid sample per fan triangle integrates affine planar fields.
        // Combine donor weights once for both atmospheric layers and all months.
        for (i, id) in cell.boundary_vertices.iter().enumerate() {
            let previous = target.vertices()[id.raw() as usize].position.components();
            let next_id = cell.boundary_vertices[(i + 1) % cell.boundary_vertices.len()];
            let next = target.vertices()[next_id.raw() as usize]
                .position
                .components();
            let area = spherical_triangle_area_unit_raw(centre, previous, next);
            let direction = normalize_legacy_compatible(std::array::from_fn(|k| {
                centre[k] + previous[k] + next[k]
            }))
            .expect("minor spherical triangle has a centre");
            let row = candidates
                .clone()
                .find_map(|&donors| WindStencil::at(grid, donors, direction))
                .ok_or_else(|| ClimateProjectionError::InvalidDomain {
                    reason: format!("wind dual grid does not cover cell {}", cell.id.raw()),
                })?;
            for (donor, weight) in row.donors.into_iter().zip(row.weights) {
                if let Some((_, sum)) = weights.iter_mut().find(|(id, _)| *id == donor) {
                    *sum += area * weight;
                } else {
                    weights.push((donor, area * weight));
                }
            }
            area_sum += area;
        }
        for (_, w) in &mut weights {
            *w /= area_sum;
        }
        for (output, wind) in outputs.iter_mut().zip(winds) {
            output.push(std::array::from_fn(|m| {
                let vector = std::array::from_fn(|k| {
                    weights
                        .iter()
                        .map(|&(id, w)| w * f64::from(wind[id][m][k]))
                        .sum()
                });
                project_tangent(vector, cell.centroid).map(|v| v as f32)
            }));
        }
    }
    Ok(outputs)
}
