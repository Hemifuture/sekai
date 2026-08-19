//! In-cell geodesic subdivision of the T1 amplified surface (plan Task 4R).
//!
//! Each cell fan triangle (centroid plus one adjacent boundary vertex pair)
//! is recursively four-way subdivided in the direction domain; sub-vertices
//! are renormalized onto the unit sphere and evaluated through the T1
//! sampler with the cell view's hypsometric palette and sea-anchored range.
//! Edge midpoints depend symmetrically on their two endpoint directions
//! only, so shared cell borders produce bit-identical vertices on both
//! sides and the mesh is crack-free without global vertex sharing.

use std::collections::HashMap;

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

use crate::generators::natural::{AmplificationLod, TerrainAmplifier};
use crate::view::{built_in_palette, sample_palette, AmplifiedSurfaceMesh, PaletteId};
use crate::world::spatial::{
    canonical_east_north_basis, SphericalSurfaceCell, SphericalSurfaceSnapshot, UnitVector3,
};

/// Global triangle budget for the uniform first LOD step (plan Task 4R).
const AMPLIFIED_TRIANGLE_BUDGET: usize = 8_000_000;
/// Deepest uniform subdivision; distance-adaptive depth is milestone M2.
const MAX_SUBDIVISION_LEVELS: u32 = 3;
/// Sun direction for the vertex hillshade in tangent (east, north, up)
/// components; roughly north-west, matching cartographic convention.
const HILLSHADE_LIGHT_TANGENT: [f64; 3] = [-0.55, 0.65, 0.75];
/// Metres of probe-distance elevation drop treated as unit slope.
const HILLSHADE_SLOPE_GAIN_M: f64 = 350.0;
/// Shade range so ridges brighten and valleys dim without crushing blacks.
const HILLSHADE_FLOOR: f64 = 0.45;
const HILLSHADE_SPAN: f64 = 0.75;

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
    let shading = VertexShading {
        lod,
        probe_step_rad: (vertex_spacing_m * 0.5 / amplifier.radius_m()).max(f64::EPSILON),
        sea_m: f64::from(sea_level_m),
        radius_m: f64::from(display_radius_m.max(1.0)),
        light: normalized(HILLSHADE_LIGHT_TANGENT),
    };

    #[cfg(not(target_arch = "wasm32"))]
    let cell_iter = cells.par_iter();
    #[cfg(target_arch = "wasm32")]
    let cell_iter = cells.iter();
    let cell_meshes = cell_iter
        .map(|cell| {
            let (directions, triangles) = subdivide_cell(surface, cell, levels)?;
            let colors = directions
                .iter()
                .map(|&direction| shaded_color(amplifier, &shading, direction))
                .collect::<Vec<_>>();
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

/// Per-vertex evaluation parameters shared by the whole bake.
struct VertexShading {
    lod: AmplificationLod,
    probe_step_rad: f64,
    sea_m: f64,
    radius_m: f64,
    light: [f64; 3],
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

/// Evaluates one subdivision vertex into a pre-lit sRGB color.
fn shaded_color(
    amplifier: &TerrainAmplifier,
    shading: &VertexShading,
    direction: UnitVector3,
) -> [u8; 4] {
    let center = f64::from(amplifier.sample(direction, shading.lod).elevation_m);
    let (east, north) = canonical_east_north_basis(direction);
    let east_m = probe_elevation(amplifier, shading, direction, east).unwrap_or(center);
    let north_m = probe_elevation(amplifier, shading, direction, north).unwrap_or(center);
    let normal = normalized([
        -(east_m - center) / HILLSHADE_SLOPE_GAIN_M,
        -(north_m - center) / HILLSHADE_SLOPE_GAIN_M,
        1.0,
    ]);
    let dot = (normal[0] * shading.light[0]
        + normal[1] * shading.light[1]
        + normal[2] * shading.light[2])
        .max(0.0);
    let shade = HILLSHADE_FLOOR + HILLSHADE_SPAN * dot;
    let t =
        ((center - (shading.sea_m - shading.radius_m)) / (2.0 * shading.radius_m)).clamp(0.0, 1.0);
    let base = sample_palette(built_in_palette(PaletteId::Hypsometric), t as f32);
    let components = base.components();
    [
        encode_srgb(f64::from(components[0]) * shade),
        encode_srgb(f64::from(components[1]) * shade),
        encode_srgb(f64::from(components[2]) * shade),
        255,
    ]
}

/// Samples the elevation one probe step along a tangent direction.
fn probe_elevation(
    amplifier: &TerrainAmplifier,
    shading: &VertexShading,
    direction: UnitVector3,
    tangent: [f64; 3],
) -> Option<f64> {
    let [x, y, z] = direction.components();
    let probe = UnitVector3::new(
        x + tangent[0] * shading.probe_step_rad,
        y + tangent[1] * shading.probe_step_rad,
        z + tangent[2] * shading.probe_step_rad,
    )
    .ok()?;
    Some(f64::from(amplifier.sample(probe, shading.lod).elevation_m))
}

fn normalized(vector: [f64; 3]) -> [f64; 3] {
    let norm = (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
    [vector[0] / norm, vector[1] / norm, vector[2] / norm]
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
