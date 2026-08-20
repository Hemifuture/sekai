//! In-cell geodesic subdivision of the T1 amplified surface (plan Task 4R).
//!
//! Each cell fan triangle (centroid plus one adjacent boundary vertex pair)
//! is recursively four-way subdivided in the direction domain. Every
//! sub-triangle renders as one solid patch — the same visual language as
//! the cell view, only with smaller units: its color is the cell view's
//! hypsometric palette sampled once at the sub-triangle's spherical
//! centroid through the T1 sampler, carried by a dedicated provoking
//! vertex and flat-interpolated on the GPU. No lighting, no gradients.
//! Edge midpoints depend symmetrically on their two endpoint directions
//! only, so shared cell borders produce bit-identical vertices on both
//! sides and the mesh is crack-free without global vertex sharing.

use std::collections::HashMap;

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

use crate::generators::natural::{AmplificationLod, TerrainAmplifier};
use crate::view::{
    built_in_palette, sample_palette, AmplifiedSurfaceMesh, PaletteId, RiverPolylineSegment,
};
use crate::world::natural::SphericalHydrologySnapshot;
use crate::world::spatial::{SphericalSurfaceCell, SphericalSurfaceSnapshot, UnitVector3};

/// Global triangle budget for the uniform first LOD step (plan Task 4R).
///
/// Each triangle also carries one dedicated provoking vertex (1.5 vertices
/// per triangle overall) and one centroid sample, so the budget bounds
/// bake time and GPU memory together.
const AMPLIFIED_TRIANGLE_BUDGET: usize = 5_000_000;
/// Deepest uniform subdivision; distance-adaptive depth is milestone M2.
const MAX_SUBDIVISION_LEVELS: u32 = 3;
/// Placeholder for shared corner vertices; flat interpolation only ever
/// reads the provoking vertex, and validation wants full opacity.
const UNREAD_CORNER_COLOR: [u8; 4] = [0, 0, 0, 255];

/// Builds the direction-domain subdivision mesh for one published world.
///
/// Returns `None` when any cell fails to subdivide or evaluate; the display
/// then simply keeps the cell view available.
pub(super) fn build_amplified_surface_mesh(
    amplifier: &TerrainAmplifier,
    surface: &SphericalSurfaceSnapshot,
    sea_level_m: f32,
    display_radius_m: f32,
) -> Option<AmplifiedSurfaceMesh> {
    let cells = surface.cells();
    let base_triangles: usize = cells.iter().map(|cell| cell.boundary_vertices.len()).sum();
    if base_triangles == 0 {
        return None;
    }
    let levels = (0..=MAX_SUBDIVISION_LEVELS)
        .rev()
        .find(|&level| {
            base_triangles.saturating_mul(4usize.pow(level)) <= AMPLIFIED_TRIANGLE_BUDGET
        })
        .unwrap_or(0);
    let cell_spacing_m = amplifier.base_wavelength_m() * 0.5;
    let vertex_spacing_m = cell_spacing_m / f64::from(2u32.pow(levels));
    let lod =
        AmplificationLod::for_sampling_footprint(amplifier.base_wavelength_m(), vertex_spacing_m);
    let shading = FlatShading {
        lod,
        sea_m: f64::from(sea_level_m),
        radius_m: f64::from(display_radius_m.max(1.0)),
    };

    #[cfg(not(target_arch = "wasm32"))]
    let cell_iter = cells.par_iter();
    #[cfg(target_arch = "wasm32")]
    let cell_iter = cells.iter();
    let cell_meshes = cell_iter
        .map(|cell| {
            let (mut directions, mut triangles) = subdivide_cell(surface, cell, levels)?;
            // One solid color per sub-triangle: a dedicated provoking vertex
            // carries the centroid sample and flat interpolation paints the
            // whole patch with it, exactly like the cell view's units.
            let mut colors = vec![UNREAD_CORNER_COLOR; directions.len()];
            for triangle in triangles.chunks_exact_mut(3) {
                let corner = directions[triangle[0] as usize];
                let centroid = spherical_centroid(
                    corner,
                    directions[triangle[1] as usize],
                    directions[triangle[2] as usize],
                )?;
                let provoking = u32::try_from(directions.len()).ok()?;
                directions.push(corner);
                colors.push(flat_color(amplifier, &shading, centroid));
                triangle[0] = provoking;
            }
            Some((directions, colors, triangles))
        })
        .collect::<Vec<_>>();

    let mut directions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for cell_mesh in cell_meshes {
        let (cell_directions, cell_colors, cell_triangles) = cell_mesh?;
        let base = u32::try_from(directions.len()).ok()?;
        directions.extend(cell_directions.iter().map(|direction| {
            let [x, y, z] = direction.components();
            [x as f32, y as f32, z as f32]
        }));
        colors.extend(cell_colors);
        indices.extend(cell_triangles.iter().map(|&index| base + index));
    }
    AmplifiedSurfaceMesh::new(directions, colors, indices).ok()
}

/// Converts the published river network into display polylines (Task 5).
pub(super) fn river_display_polylines(
    surface: &SphericalSurfaceSnapshot,
    hydrology: &SphericalHydrologySnapshot,
) -> Vec<RiverPolylineSegment> {
    let cells = surface.cells();
    hydrology
        .river_segments()
        .iter()
        .filter_map(|segment| {
            let from = cells
                .get(segment.from().raw() as usize)?
                .centroid
                .components();
            let to = cells
                .get(segment.to().raw() as usize)?
                .centroid
                .components();
            Some(RiverPolylineSegment {
                start: [from[0] as f32, from[1] as f32, from[2] as f32],
                end: [to[0] as f32, to[1] as f32, to[2] as f32],
                strahler_order: segment.strahler_order(),
            })
        })
        .collect()
}

/// Per-patch evaluation parameters shared by the whole bake.
struct FlatShading {
    lod: AmplificationLod,
    sea_m: f64,
    radius_m: f64,
}

/// Returns the renormalized spherical centroid of one sub-triangle.
fn spherical_centroid(a: UnitVector3, b: UnitVector3, c: UnitVector3) -> Option<UnitVector3> {
    let [ax, ay, az] = a.components();
    let [bx, by, bz] = b.components();
    let [cx, cy, cz] = c.components();
    UnitVector3::new(ax + bx + cx, ay + by + cy, az + bz + cz).ok()
}

/// Subdivides one cell fan into local directions and triangle indices.
fn subdivide_cell(
    surface: &SphericalSurfaceSnapshot,
    cell: &SphericalSurfaceCell,
    levels: u32,
) -> Option<(Vec<UnitVector3>, Vec<u32>)> {
    let ring = cell
        .boundary_vertices
        .iter()
        .map(|&vertex| surface.vertex(vertex).map(|record| record.position))
        .collect::<Option<Vec<_>>>()?;
    if ring.len() < 3 {
        return None;
    }
    let mut vertices = Vec::with_capacity(1 + ring.len() * (1 << levels));
    vertices.push(cell.centroid);
    vertices.extend(ring.iter().copied());
    let mut midpoints = HashMap::new();
    let side_count = u32::try_from(ring.len()).ok()?;
    let mut triangles = Vec::with_capacity(ring.len() * 4usize.pow(levels) * TRIANGLE_CORNERS);
    for side in 0..side_count {
        let near = 1 + side;
        let far = 1 + (side + 1) % side_count;
        subdivide_triangle(
            0,
            near,
            far,
            levels,
            &mut vertices,
            &mut midpoints,
            &mut triangles,
        )?;
    }
    Some((vertices, triangles))
}

const TRIANGLE_CORNERS: usize = 3;

/// Recursive four-way subdivision preserving winding order.
fn subdivide_triangle(
    a: u32,
    b: u32,
    c: u32,
    levels: u32,
    vertices: &mut Vec<UnitVector3>,
    midpoints: &mut HashMap<(u32, u32), u32>,
    out: &mut Vec<u32>,
) -> Option<()> {
    if levels == 0 {
        out.extend_from_slice(&[a, b, c]);
        return Some(());
    }
    let ab = midpoint_index(a, b, vertices, midpoints)?;
    let bc = midpoint_index(b, c, vertices, midpoints)?;
    let ca = midpoint_index(c, a, vertices, midpoints)?;
    subdivide_triangle(a, ab, ca, levels - 1, vertices, midpoints, out)?;
    subdivide_triangle(ab, b, bc, levels - 1, vertices, midpoints, out)?;
    subdivide_triangle(ca, bc, c, levels - 1, vertices, midpoints, out)?;
    subdivide_triangle(ab, bc, ca, levels - 1, vertices, midpoints, out)
}

/// Returns the renormalized midpoint, deduplicated per cell.
///
/// The midpoint depends only on the two endpoint direction values through a
/// commutative sum, so both cells sharing a border edge derive bit-identical
/// midpoint chains independently.
fn midpoint_index(
    a: u32,
    b: u32,
    vertices: &mut Vec<UnitVector3>,
    midpoints: &mut HashMap<(u32, u32), u32>,
) -> Option<u32> {
    let key = (a.min(b), a.max(b));
    if let Some(&existing) = midpoints.get(&key) {
        return Some(existing);
    }
    let [ax, ay, az] = vertices[a as usize].components();
    let [bx, by, bz] = vertices[b as usize].components();
    let direction = UnitVector3::new(ax + bx, ay + by, az + bz).ok()?;
    let index = u32::try_from(vertices.len()).ok()?;
    vertices.push(direction);
    midpoints.insert(key, index);
    Some(index)
}

/// Evaluates one sub-triangle centroid into its solid hypsometric color.
fn flat_color(
    amplifier: &TerrainAmplifier,
    shading: &FlatShading,
    direction: UnitVector3,
) -> [u8; 4] {
    let elevation = f64::from(amplifier.sample(direction, shading.lod).elevation_m);
    let t = ((elevation - (shading.sea_m - shading.radius_m)) / (2.0 * shading.radius_m))
        .clamp(0.0, 1.0);
    let base = sample_palette(built_in_palette(PaletteId::Hypsometric), t as f32);
    let components = base.components();
    [
        encode_srgb(f64::from(components[0])),
        encode_srgb(f64::from(components[1])),
        encode_srgb(f64::from(components[2])),
        255,
    ]
}

fn encode_srgb(linear: f64) -> u8 {
    let clamped = linear.clamp(0.0, 1.0);
    let encoded = if clamped <= 0.003_130_8 {
        12.92 * clamped
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::natural::AmplificationFieldsView;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::SphericalOrogenyKind;
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    fn test_amplifier_and_surface() -> (TerrainAmplifier, SphericalSurfaceSnapshot) {
        let surface = GeodesicVoronoiBuilder::build_cancellable(
            &SphericalSpaceSpec {
                radius: Meters::new(6_371_000.0).unwrap(),
                target_cell_count: 162,
            },
            || false,
        )
        .unwrap();
        let count = surface.cells().len();
        let elevation: Vec<f32> = surface
            .cells()
            .iter()
            .map(|cell| (2_000.0 * cell.centroid.components()[2]) as f32)
            .collect();
        let zeros = vec![0.0_f32; count];
        let ones = vec![1.0_f32; count];
        let kinds = vec![SphericalOrogenyKind::None; count];
        let fields = AmplificationFieldsView {
            final_elevation_m: &elevation,
            sea_level_m: 0.0,
            sediment_thickness_m: &zeros,
            erodibility: &zeros,
            annual_precipitation_mm: &ones,
            crust_age_myr: &zeros,
            lineation_east: &ones,
            lineation_north: &zeros,
            orogeny_kind: &kinds,
            orogeny_age_myr: &zeros,
        };
        let amplifier = TerrainAmplifier::new(&surface, fields, RootSeed::new(7)).unwrap();
        (amplifier, surface)
    }

    #[test]
    fn subdivision_mesh_is_deterministic_within_budget_and_opaque() {
        let (amplifier, surface) = test_amplifier_and_surface();
        let first = build_amplified_surface_mesh(&amplifier, &surface, 0.0, 2_000.0).unwrap();
        let second = build_amplified_surface_mesh(&amplifier, &surface, 0.0, 2_000.0).unwrap();
        assert_eq!(first.directions(), second.directions());
        assert_eq!(first.colors(), second.colors());
        assert_eq!(first.indices(), second.indices());
        assert!(first.triangle_count() <= AMPLIFIED_TRIANGLE_BUDGET);
        let base_triangles: usize = surface
            .cells()
            .iter()
            .map(|cell| cell.boundary_vertices.len())
            .sum();
        assert_eq!(
            first.triangle_count(),
            base_triangles * 4usize.pow(MAX_SUBDIVISION_LEVELS)
        );
        assert!(first.colors().iter().all(|color| color[3] == 255));
        // Every sub-triangle owns its provoking vertex, so flat
        // interpolation paints each patch with exactly its centroid sample.
        let mut provoking = std::collections::HashSet::new();
        assert!(first
            .indices()
            .chunks_exact(3)
            .all(|triangle| provoking.insert(triangle[0])));
    }

    #[test]
    fn shared_border_midpoints_are_bit_identical_across_cells() {
        let (_, surface) = test_amplifier_and_surface();
        let edge = &surface.edges()[0];
        let a = surface.vertex(edge.vertices[0]).unwrap().position;
        let b = surface.vertex(edge.vertices[1]).unwrap().position;
        let [ax, ay, az] = a.components();
        let [bx, by, bz] = b.components();
        let forward = UnitVector3::new(ax + bx, ay + by, az + bz).unwrap();
        let backward = UnitVector3::new(bx + ax, by + ay, bz + az).unwrap();
        assert_eq!(forward.components(), backward.components());
    }
}
