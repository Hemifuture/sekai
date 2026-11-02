//! Concrete Atlas value source for the existing display subdivision geometry.

use super::atlas_display::AtlasFieldDocument;
use crate::generators::natural::{
    interior_margin, subdivision_children, HierarchicalEvaluator, HierarchicalPath,
    LocatedPrimitive, HIERARCHICAL_PATH_DEPTH_MAX,
};
use crate::view::SphericalEntityLocator;
use crate::world::{natural::SurfaceWaterKind, spatial::UnitVector3, CellId};
use std::sync::Arc;

pub(super) fn checked_sector_count(count: usize) -> Result<u8, String> {
    u8::try_from(count).map_err(|_| "Atlas display sector limit exceeded".into())
}

fn source_cell_caps(surface: &crate::world::spatial::SphericalSurfaceSnapshot) -> Vec<f64> {
    surface
        .cells()
        .iter()
        .map(|cell| {
            cell.boundary_vertices
                .iter()
                .map(|id| {
                    super::amplified_mesh::angle_between(
                        cell.centroid,
                        surface.vertices()[id.raw() as usize].position,
                    )
                })
                .fold(0.0, f64::max)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DisplayElevation {
    pub elevation_m: f32,
}

/// Terrain and water values shared by the display and its installed-leaf inspector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DetailReading {
    pub cell: CellId,
    pub value: DisplayElevation,
    pub water: SurfaceWaterKind,
    pub initialized_water_m: Option<(f64, f64)>,
}

impl From<crate::generators::natural::DerivedReading> for DetailReading {
    fn from(reading: crate::generators::natural::DerivedReading) -> Self {
        Self {
            cell: reading.cell,
            value: DisplayElevation {
                elevation_m: reading.value.elevation_m,
            },
            water: reading.water,
            initialized_water_m: None,
        }
    }
}

pub(super) enum DetailEvaluator {
    Formation(Box<HierarchicalEvaluator>),
    Atlas {
        document: Arc<AtlasFieldDocument>,
        locator: Arc<SphericalEntityLocator>,
        caps: Vec<f64>,
    },
}

impl From<HierarchicalEvaluator> for DetailEvaluator {
    fn from(value: HierarchicalEvaluator) -> Self {
        Self::Formation(Box::new(value))
    }
}

impl DetailEvaluator {
    pub(super) fn atlas(
        document: Arc<AtlasFieldDocument>,
        locator: Arc<SphericalEntityLocator>,
    ) -> Result<Self, String> {
        let surface = document.surface();
        if locator.source() != &document.presentation_source() {
            return Err("Atlas detail locator has a different publication source".into());
        }
        let caps = source_cell_caps(surface);
        Ok(Self::Atlas {
            document,
            locator,
            caps,
        })
    }
    pub(super) fn cell_count(&self) -> usize {
        match self {
            Self::Formation(e) => e.cell_count(),
            Self::Atlas { document, .. } => document.surface().cells().len(),
        }
    }
    pub(super) fn sector_count(&self, cell: CellId) -> usize {
        match self {
            Self::Formation(e) => e.sector_count(cell),
            Self::Atlas { document, .. } => document.surface().cells()[cell.raw() as usize]
                .boundary_vertices
                .len(),
        }
    }
    pub(super) fn radius_m(&self) -> f64 {
        match self {
            Self::Formation(e) => e.radius_m(),
            Self::Atlas { document, .. } => document.surface().radius().get(),
        }
    }
    pub(super) fn cell_spacing_m(&self) -> f64 {
        match self {
            Self::Formation(e) => e.cell_spacing_m(),
            Self::Atlas { document, .. } => {
                (document.surface().total_cell_area().get() / self.cell_count() as f64).sqrt()
            }
        }
    }
    pub(super) fn cell_cap(&self, cell: CellId) -> f64 {
        match self {
            Self::Atlas { caps, .. } => caps[cell.raw() as usize],
            Self::Formation(e) => {
                let c = e.sector_corners(cell, 0);
                super::amplified_mesh::angle_between(c[0], c[1])
                    .max(super::amplified_mesh::angle_between(c[0], c[2]))
                    * 2.0
            }
        }
    }
    pub(super) fn sector_corners(&self, cell: CellId, sector: u8) -> [UnitVector3; 3] {
        match self {
            Self::Formation(e) => e.sector_corners(cell, sector),
            Self::Atlas { document, .. } => {
                let surface = document.surface();
                let cell = &surface.cells()[cell.raw() as usize];
                let i = usize::from(sector);
                [
                    cell.centroid,
                    surface.vertices()[cell.boundary_vertices[i].raw() as usize].position,
                    surface.vertices()[cell.boundary_vertices
                        [(i + 1) % cell.boundary_vertices.len()]
                    .raw() as usize]
                        .position,
                ]
            }
        }
    }
    pub(super) fn locate(&self, direction: UnitVector3, level: u8) -> LocatedPrimitive {
        match self {
            Self::Formation(e) => e.locate(direction, level),
            Self::Atlas { locator, .. } => {
                let cell = locator
                    .locate_cell(direction)
                    .expect("validated source covers every direction");
                if level == 0 {
                    return LocatedPrimitive::Cell(cell);
                }
                let sector = (0..self.sector_count(cell))
                    .max_by(|a, b| {
                        interior_margin(&self.sector_corners(cell, *a as u8), direction).total_cmp(
                            &interior_margin(&self.sector_corners(cell, *b as u8), direction),
                        )
                    })
                    .expect("validated polygon has sectors");
                let sector = u8::try_from(sector).expect("document checked display sector domain");
                let mut corners = self.sector_corners(cell, sector);
                let depth = usize::from(level - 1).min(HIERARCHICAL_PATH_DEPTH_MAX);
                let mut steps = [0; HIERARCHICAL_PATH_DEPTH_MAX];
                for slot in steps.iter_mut().take(depth) {
                    let children = subdivision_children(corners);
                    let child = (0..4)
                        .max_by(|a, b| {
                            interior_margin(&children[*a], direction)
                                .total_cmp(&interior_margin(&children[*b], direction))
                        })
                        .expect("four subdivision children");
                    *slot = child as u8;
                    corners = children[child];
                }
                LocatedPrimitive::Triangle {
                    cell,
                    sector,
                    path: HierarchicalPath::new(&steps[..depth]),
                }
            }
        }
    }
    pub(super) fn read(&self, direction: UnitVector3, level: u8) -> DetailReading {
        match self {
            Self::Formation(e) => e.read(direction, level).into(),
            Self::Atlas { document, .. } => {
                let (cell, direction) = match self.locate(direction, level) {
                    LocatedPrimitive::Cell(cell) => (
                        cell,
                        document.surface().cells()[cell.raw() as usize].centroid,
                    ),
                    LocatedPrimitive::Triangle { cell, sector, path } => (
                        cell,
                        leaf_center(super::amplified_mesh::descend_prefix(
                            self.sector_corners(cell, sector),
                            path.steps(),
                        )),
                    ),
                };
                let sample = document
                    .snapshot()
                    .sample_surface(direction.components())
                    .expect("valid installed leaf queries the sealed Atlas snapshot");
                DetailReading {
                    cell,
                    value: DisplayElevation {
                        elevation_m: sample.bed_elevation_m as f32,
                    },
                    water: sample.water,
                    initialized_water_m: sample
                        .water_surface_elevation_m
                        .zip(sample.water_depth_m()),
                }
            }
        }
    }
    pub(super) fn for_each_leaf_reading(
        &self,
        cell: CellId,
        sector: u8,
        prefix: &[u8],
        extra: u8,
        sink: &mut dyn FnMut(DetailReading),
    ) {
        match self {
            Self::Formation(e) => {
                e.for_each_leaf_reading(cell, sector, prefix, extra, &mut |r| sink(r.into()))
            }
            Self::Atlas { document, .. } => {
                let mut directions = Vec::with_capacity(4_usize.pow(u32::from(extra)));
                leaf_directions(
                    super::amplified_mesh::descend_prefix(
                        self.sector_corners(cell, sector),
                        prefix,
                    ),
                    extra,
                    &mut directions,
                );
                let readings = document
                    .snapshot()
                    .sample_surface_batch(&directions, &|| false)
                    .expect("valid display leaves query sealed Atlas snapshot");
                for sample in readings {
                    sink(DetailReading {
                        cell,
                        value: DisplayElevation {
                            elevation_m: sample.bed_elevation_m as f32,
                        },
                        water: sample.water,
                        initialized_water_m: sample
                            .water_surface_elevation_m
                            .zip(sample.water_depth_m()),
                    });
                }
            }
        }
    }
    pub(super) fn river_width_m(&self, reach: u32) -> Option<f32> {
        match self {
            Self::Formation(e) => e.river_width_m(reach),
            Self::Atlas { .. } => None,
        }
    }
    pub(super) fn river_path(&self, reach: u32, depth: u8) -> Vec<Vec<UnitVector3>> {
        match self {
            Self::Formation(e) => e.river_path(reach, depth),
            Self::Atlas { .. } => Vec::new(),
        }
    }
    #[cfg(test)]
    pub(super) fn river_reach_count(&self) -> usize {
        match self {
            Self::Formation(e) => e.river_reach_count(),
            Self::Atlas { document, .. } => document.snapshot().rivers().len(),
        }
    }
    #[cfg(test)]
    pub(super) fn value(&self, cell: CellId, sector: u8, path: &[u8]) -> DisplayElevation {
        match self {
            Self::Formation(e) => DisplayElevation {
                elevation_m: e.value(cell, sector, path).elevation_m,
            },
            Self::Atlas { document, .. } => DisplayElevation {
                elevation_m: document
                    .snapshot()
                    .sample_surface(
                        leaf_center(super::amplified_mesh::descend_prefix(
                            self.sector_corners(cell, sector),
                            path,
                        ))
                        .components(),
                    )
                    .expect("valid leaf")
                    .bed_elevation_m as f32,
            },
        }
    }
}

fn leaf_directions(corners: [UnitVector3; 3], extra: u8, out: &mut Vec<[f64; 3]>) {
    if extra == 0 {
        out.push(leaf_center(corners).components());
        return;
    }
    for child in subdivision_children(corners) {
        leaf_directions(child, extra - 1, out);
    }
}
fn leaf_center(corners: [UnitVector3; 3]) -> UnitVector3 {
    let c = corners.map(UnitVector3::components);
    UnitVector3::new(
        c[0][0] + c[1][0] + c[2][0],
        c[0][1] + c[1][1] + c[2][1],
        c[0][2] + c[1][2] + c[2][2],
    )
    .expect("valid fan leaf has nonzero center")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_polygon_caps_cover_all_vertices_and_sector_conversion_is_checked() {
        assert_eq!(checked_sector_count(255).unwrap(), 255);
        assert!(checked_sector_count(256).is_err());
        // The lowest supported source size suffices for irregular 7+ edge polygons.
        // No water support, climate solve or second complete world is needed for this geometry risk.
        let terrain = civ_atlas_terrain::generate_terrain(
            &civ_atlas_terrain::TerrainParams {
                seed: 42,
                shape: civ_atlas_terrain::TerrainShape {
                    cells: 12_000,
                    ..Default::default()
                },
            },
            &|| false,
        )
        .unwrap();
        let directions = terrain
            .site_xyz()
            .chunks_exact(3)
            .map(|p| UnitVector3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])).unwrap())
            .collect::<Vec<_>>();
        let surface = crate::generators::spatial::build_source_voronoi_surface(
            crate::world::Meters::new(crate::world::natural::EARTH_WATER_REFERENCE_RADIUS_M)
                .unwrap(),
            &directions,
            terrain.site_triangles(),
            &mut || false,
        )
        .unwrap();
        let caps = source_cell_caps(&surface);
        let cell = surface
            .cells()
            .iter()
            .find(|cell| cell.boundary_vertices.len() > 6)
            .unwrap();
        assert!(checked_sector_count(cell.boundary_vertices.len()).unwrap() > 6);
        for vertex in &cell.boundary_vertices {
            assert!(
                super::super::amplified_mesh::angle_between(
                    cell.centroid,
                    surface.vertices()[vertex.raw() as usize].position
                ) <= caps[cell.id.raw() as usize]
            );
        }
        let source = crate::view::SphericalPresentationSource::new(
            crate::world::RootSeed::new(42),
            crate::world::spatial::SurfaceRef::for_spherical(&surface),
            crate::engine::BuildResultHash::new([42; 32]),
            1,
        );
        let locator = SphericalEntityLocator::new(source, &surface).unwrap();
        assert_eq!(locator.locate_cell(cell.site), Some(cell.id));
    }
}
