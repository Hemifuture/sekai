//! Upstream Civ Atlas site classes as the water truth, and fixed fan quadrature
//! of the canonical bed under the shared marine-connectivity and lake rules
//! (`2026-11-03-world-truth-scope-design.md` R3).

use super::AtlasGenerationError;
use crate::engine::BuildCancellation;
use crate::generators::natural::subdivision_children;
use crate::world::natural::atlas_terrain::{source_corners, AtlasSiteBody, AtlasSiteWater};
use crate::world::natural::{
    AtlasCellFields, AtlasLake, BedrockKind, LandOceanField, LandOceanKind, SurfaceWaterKind,
    CLIMATE_MONTH_COUNT,
};
use crate::world::spatial::{spherical_triangle_area_unit, SphericalSurfaceSnapshot};
use civ_atlas_terrain::{FineTerrain, Terrain};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use std::collections::VecDeque;

/// Upstream coarse water class codes (`Terrain::water_classification`).
const UPSTREAM_OCEAN: u8 = 1;
const UPSTREAM_LAKE: u8 = 2;

pub(super) struct BuiltGeometry {
    pub water: AtlasSiteWater,
    pub fields: AtlasCellFields,
    pub lakes: Vec<AtlasLake>,
    /// Lake area inside each source cell: (cell, lake, square metres).
    pub lake_cells: Vec<(usize, usize, f64)>,
}

/// Neighbours of source site `i` in the upstream adjacency (CSR).
pub(super) fn site_neighbors(terrain: &Terrain, i: usize) -> &[usize] {
    let offsets = terrain.site_neighbor_offsets();
    &terrain.site_neighbors()[offsets[i]..offsets[i + 1]]
}

/// Per-site bodies from the upstream classes. Each connected group of lake sites
/// is one lake whose constant surface is the highest upstream surface in it.
pub(super) fn site_water(terrain: &Terrain) -> (Vec<Option<AtlasSiteBody>>, Vec<f64>) {
    let class = terrain.water_classification();
    let surface = terrain.water_surface_elevation_m();
    let n = class.len();
    let mut lake_of = vec![None; n];
    let mut lake_surface = Vec::new();
    for start in 0..n {
        if class[start] != UPSTREAM_LAKE || lake_of[start].is_some() {
            continue;
        }
        let lake = lake_surface.len();
        let mut level = f64::from(surface[start]);
        lake_of[start] = Some(lake);
        let mut queue = VecDeque::from([start]);
        while let Some(i) = queue.pop_front() {
            level = level.max(f64::from(surface[i]));
            for &j in site_neighbors(terrain, i) {
                if class[j] == UPSTREAM_LAKE && lake_of[j].is_none() {
                    lake_of[j] = Some(lake);
                    queue.push_back(j);
                }
            }
        }
        lake_surface.push(level);
    }
    let sites = (0..n)
        .map(|i| match (class[i], lake_of[i]) {
            (UPSTREAM_OCEAN, _) => Some(AtlasSiteBody::Ocean),
            (_, Some(lake)) => Some(AtlasSiteBody::Lake {
                lake,
                surface_m: lake_surface[lake],
            }),
            _ => None,
        })
        .collect();
    (sites, lake_surface)
}

#[derive(Default)]
struct CellQuadrature {
    area: f64,
    bed: f64,
    exposed: f64,
    dry_area: f64,
    ocean_area: f64,
    lake_area: f64,
    ocean_volume: f64,
    lake_volume: f64,
    /// (lake, area, volume) for each lake wetting this cell.
    lakes: Vec<(usize, f64, f64)>,
}

/// Integrate one source cell: every fan triangle (site, two adjacent Voronoi
/// vertices) is split `depth - 1` times and sampled at leaf centres.
fn integrate_cell(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    water: &AtlasSiteWater,
    cell: usize,
    depth: u8,
    cancellation: &BuildCancellation,
) -> Result<CellQuadrature, AtlasGenerationError> {
    super::check_cancelled(cancellation)?;
    let source = &surface.cells()[cell];
    let vertex = |k: usize| {
        let ring = &source.boundary_vertices;
        surface.vertices()[ring[k % ring.len()].raw() as usize].position
    };
    let mut leaves = (0..source.boundary_vertices.len())
        .map(|k| [source.site, vertex(k), vertex(k + 1)])
        .collect::<Vec<_>>();
    for _ in 1..depth {
        leaves = leaves
            .iter()
            .flat_map(|t| subdivision_children(*t))
            .collect();
    }
    let radius2 = surface.radius().get().powi(2);
    let centres = leaves
        .iter()
        .map(|[a, b, c]| {
            let sum = std::array::from_fn::<f64, 3, _>(|i| {
                a.components()[i] + b.components()[i] + c.components()[i]
            });
            let norm = sum.iter().map(|x| x * x).sum::<f64>().sqrt();
            sum.map(|x| x / norm)
        })
        .collect::<Vec<_>>();
    let beds = fine.sample_located_surface_batch(&centres, &|| cancellation.is_cancelled())?;
    let mut out = CellQuadrature::default();
    for (leaf, sample) in leaves.iter().zip(beds) {
        let area = spherical_triangle_area_unit(leaf[0], leaf[1], leaf[2]) * radius2;
        let bed = sample.bed_elevation_m;
        let (kind, level, lake) = water.classify(source_corners(fine, sample.source_face), &sample);
        out.area += area;
        out.bed += area * bed;
        out.exposed += area * level.map_or(bed, |level| level.max(bed));
        let volume = level.map_or(0.0, |level| area * (level - bed));
        match kind {
            SurfaceWaterKind::Ocean => {
                out.ocean_area += area;
                out.ocean_volume += volume;
            }
            SurfaceWaterKind::Lake => {
                let lake = lake.expect("lake samples name their lake");
                out.lake_area += area;
                out.lake_volume += volume;
                match out.lakes.iter_mut().find(|entry| entry.0 == lake) {
                    Some(entry) => {
                        entry.1 += area;
                        entry.2 += volume;
                    }
                    None => out.lakes.push((lake, area, volume)),
                }
            }
            SurfaceWaterKind::DryLand => out.dry_area += area,
        }
    }
    Ok(out)
}

/// Build source classifications, marine connectivity and cell fields. Cells are integrated in
/// parallel; each cell's sum is sequential, so results do not depend on threads.
pub(super) fn build_geometry(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    depth: u8,
    cancellation: &BuildCancellation,
) -> Result<BuiltGeometry, AtlasGenerationError> {
    let terrain = fine.coarse_terrain();
    let n = surface.cells().len();
    if terrain.water_classification().len() != n {
        return Err(AtlasGenerationError::Geometry(
            "upstream classes do not match the source cells".into(),
        ));
    }
    let (sites, lake_surface) = site_water(terrain);
    let coast = super::coast::build(fine, &sites, cancellation)?;
    let water = AtlasSiteWater::new(sites, coast);
    // Rayon is native-only; the browser build integrates cells sequentially.
    #[cfg(not(target_arch = "wasm32"))]
    let cell_indices = (0..n).into_par_iter();
    #[cfg(target_arch = "wasm32")]
    let cell_indices = 0..n;
    let cells = cell_indices
        .map(|cell| integrate_cell(fine, surface, &water, cell, depth, cancellation))
        .collect::<Result<Vec<_>, _>>()?;
    let mut lakes = lake_surface
        .iter()
        .map(|&surface_m| AtlasLake {
            surface_elevation_m: surface_m,
            area_m2: 0.0,
            volume_m3: 0.0,
            resolved_monthly_inflow_m3_s: [0.0; CLIMATE_MONTH_COUNT],
            resolved_monthly_outflow_m3_s: [0.0; CLIMATE_MONTH_COUNT],
            direct_monthly_precipitation_m3_s: [0.0; CLIMATE_MONTH_COUNT],
        })
        .collect::<Vec<_>>();
    let mut lake_cells = Vec::new();
    for (cell, q) in cells.iter().enumerate() {
        for &(lake, area, volume) in &q.lakes {
            lakes[lake].area_m2 += area;
            lakes[lake].volume_m3 += volume;
            lake_cells.push((cell, lake, area));
        }
    }
    let mean = |f: fn(&CellQuadrature) -> f64| {
        cells
            .iter()
            .map(|q| (f(q) / q.area) as f32)
            .collect::<Vec<_>>()
    };
    let relative_permeability = terrain
        .continental_origin()
        .iter()
        .map(|&c| {
            (if c == 0 {
                BedrockKind::OceanicMafic
            } else {
                BedrockKind::ContinentalCrystalline
            })
            .base_transport_properties()
            .1
        })
        .collect();
    let fields = AtlasCellFields {
        land_ocean: LandOceanField::from_kinds(
            water
                .sites
                .iter()
                .map(|body| {
                    if *body == Some(AtlasSiteBody::Ocean) {
                        LandOceanKind::Ocean
                    } else {
                        LandOceanKind::Land
                    }
                })
                .collect(),
        ),
        bed_elevation_m: mean(|q| q.bed),
        exposed_elevation_m: mean(|q| q.exposed),
        ocean_fraction: mean(|q| q.ocean_area),
        lake_fraction: mean(|q| q.lake_area),
        dry_fraction: mean(|q| q.dry_area),
        ocean_volume_m3: cells.iter().map(|q| q.ocean_volume).collect(),
        lake_volume_m3: cells.iter().map(|q| q.lake_volume).collect(),
        relative_permeability,
    };
    log::debug!(
        "Atlas water sites={n} lakes={} quadrature_depth={depth}",
        lakes.len()
    );
    Ok(BuiltGeometry {
        water,
        fields,
        lakes,
        lake_cells,
    })
}
