//! Seed-independent geometry of the reduced climate on the climate work domain (design
//! section 10 T2): built once per work domain, cached with its artifact and therefore
//! prewarmed at startup together with it.

use super::vector::{cross, dot, east_unit, norm};
use crate::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
use crate::generators::natural::formation::ClimateWorkDomainBuildError;
use crate::world::natural::{
    global_circulation_axisymmetric_band_count, ClimateWorkDomainSnapshot,
    EARTH_ROTATION_RATE_RAD_S, REDUCED_CLIMATE_HADLEY_WEIGHT_WIDTH_SIN_LATITUDE,
    STOMMEL_BOUNDARY_LAYER_WIDTH_GRID_SPACINGS,
};

/// Edge metrics, latitude bands, Hadley weights, streamfunction vertex geometry and Stommel
/// coefficients of one climate work domain.
#[derive(Debug, Clone)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "read by the reduced solver from A10 T4")
)]
pub(in crate::generators::natural) struct ReducedClimateGeometry {
    pub(super) radius_m: f64,
    pub(super) area_m2: Vec<f64>,
    pub(super) center: Vec<[f64; 3]>,
    pub(super) latitude_rad: Vec<f64>,
    pub(super) east: Vec<[f64; 3]>,
    pub(super) edges: Vec<ReducedEdge>,
    /// Edge indices and owner slot (0 = edge's first cell) per cell.
    pub(super) cell_edges: Vec<[(u32, u8); 4]>,
    /// Latitude bands of the production axisymmetric band geometry.
    pub(super) band_count: usize,
    pub(super) cell_band: Vec<usize>,
    pub(super) edge_band: Vec<usize>,
    /// Meridional gradient of the Coriolis parameter, `beta = 2 Omega cos(lat) / a`, per cell.
    pub(super) coriolis_gradient_per_m_s: Vec<f64>,
    /// Stommel bottom friction `r` whose boundary layer `r / beta_0` spans
    /// [`STOMMEL_BOUNDARY_LAYER_WIDTH_GRID_SPACINGS`] mean spacings at the equatorial `beta_0`.
    pub(super) stommel_friction_per_s: f64,
    grid: CubedSphereGrid,
}

/// One work-domain edge with every seed-independent coefficient the model reads.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "read by the reduced solver from A10 T4")
)]
pub(in crate::generators::natural) struct ReducedEdge {
    pub(super) cells: [u32; 2],
    pub(super) length_m: f64,
    pub(super) center_distance_m: f64,
    /// Unit normal pointing from `cells[0]` to `cells[1]`.
    pub(super) normal: [f64; 3],
    pub(super) midpoint: [f64; 3],
    pub(super) displacement: [[f64; 3]; 2],
    /// Siler et al. (2018) Hadley weight at the edge midpoint latitude.
    pub(super) hadley_weight: f64,
    /// Latitudes (radians) of the two edge vertices, ordered so that the first lies behind
    /// and the second ahead of `midpoint x normal`; the edge flux of a zonal streamfunction is
    /// `psi(first) - psi(second)` (design section 17.3).
    pub(super) vertex_latitude_rad: [f64; 2],
}

impl ReducedClimateGeometry {
    /// Rebuilds the work domain's cubed-sphere grid and derives every coefficient.
    ///
    /// # Errors
    ///
    /// Returns [`ClimateWorkDomainBuildError::Cancelled`] when `cancelled` fires, the grid
    /// construction error, or [`ClimateWorkDomainBuildError::ReconstructionMismatch`] when the
    /// rebuilt grid is not the one the domain names.
    pub(in crate::generators::natural) fn new(
        domain: &ClimateWorkDomainSnapshot,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, ClimateWorkDomainBuildError> {
        let grid = CubedSphereGrid::new_cancellable(
            domain.face_resolution(),
            domain.climate_surface().radius().get(),
            cancelled,
        )
        .map_err(super::super::climate_work_domain::map_grid_error)?;
        if grid.fingerprint() != domain.climate_grid_fingerprint() {
            return Err(ClimateWorkDomainBuildError::ReconstructionMismatch);
        }
        Ok(Self::from_grid(grid))
    }

    fn from_grid(grid: CubedSphereGrid) -> Self {
        let cells = grid.cells();
        let mut area_m2 = Vec::with_capacity(cells.len());
        let mut center = Vec::with_capacity(cells.len());
        let mut latitude_rad = Vec::with_capacity(cells.len());
        let mut east = Vec::with_capacity(cells.len());
        for cell in cells {
            let c = cell.center_unit();
            area_m2.push(cell.area_m2());
            center.push(c);
            latitude_rad.push(c[2].clamp(-1.0, 1.0).asin());
            east.push(east_unit(c));
        }
        let vertices = grid.vertices();
        let edges: Vec<ReducedEdge> = grid
            .edges()
            .iter()
            .map(|edge| {
                let midpoint = edge.midpoint_unit();
                let x = midpoint[2];
                let along = cross(midpoint, edge.normal_from_first());
                let [first, second] = edge.vertices().map(|v| vertices[v as usize]);
                let latitude = |v: [f64; 3]| (v[2] / norm(v)).clamp(-1.0, 1.0).asin();
                let ahead = dot(
                    [
                        second[0] - first[0],
                        second[1] - first[1],
                        second[2] - first[2],
                    ],
                    along,
                ) > 0.0;
                let vertex_latitude_rad = if ahead {
                    [latitude(first), latitude(second)]
                } else {
                    [latitude(second), latitude(first)]
                };
                ReducedEdge {
                    vertex_latitude_rad,
                    cells: *edge.cells(),
                    length_m: edge.length_m(),
                    center_distance_m: edge.center_distance_m(),
                    normal: edge.normal_from_first(),
                    midpoint,
                    displacement: [
                        edge.midpoint_displacement_m(0),
                        edge.midpoint_displacement_m(1),
                    ],
                    hadley_weight: (-(x * x)
                        / (REDUCED_CLIMATE_HADLEY_WEIGHT_WIDTH_SIN_LATITUDE
                            * REDUCED_CLIMATE_HADLEY_WEIGHT_WIDTH_SIN_LATITUDE))
                        .exp(),
                }
            })
            .collect();
        let cell_edges = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                cell.edges().map(|edge_id| {
                    let edge = &edges[edge_id as usize];
                    let slot = u8::from(edge.cells[0] as usize != index);
                    (edge_id, slot)
                })
            })
            .collect();
        let band_count = global_circulation_axisymmetric_band_count(grid.face_resolution());
        let band_of = |sin_latitude: f64| {
            let latitude = sin_latitude.clamp(-1.0, 1.0).asin();
            (((latitude / std::f64::consts::PI + 0.5) * band_count as f64).floor() as usize)
                .min(band_count - 1)
        };
        let cell_band = center.iter().map(|c| band_of(c[2])).collect();
        let edge_band = edges.iter().map(|e| band_of(e.midpoint[2])).collect();
        let mean_spacing_m =
            edges.iter().map(|e| e.center_distance_m).sum::<f64>() / edges.len() as f64;
        let radius_m = grid.radius_m();
        let coriolis_gradient_per_m_s = latitude_rad
            .iter()
            .map(|latitude| 2.0 * EARTH_ROTATION_RATE_RAD_S * latitude.cos() / radius_m)
            .collect();
        let stommel_friction_per_s = 2.0 * EARTH_ROTATION_RATE_RAD_S / radius_m
            * (STOMMEL_BOUNDARY_LAYER_WIDTH_GRID_SPACINGS * mean_spacing_m);
        Self {
            radius_m,
            area_m2,
            center,
            latitude_rad,
            east,
            edges,
            cell_edges,
            band_count,
            cell_band,
            edge_band,
            coriolis_gradient_per_m_s,
            stommel_friction_per_s,
            grid,
        }
    }

    /// Number of work cells.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the reduced solver from A10 T4")
    )]
    pub(super) fn cell_count(&self) -> usize {
        self.area_m2.len()
    }

    /// Discrete gradient and divergence operators of the work grid.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the reduced solver from A10 T4")
    )]
    pub(super) fn operators(&self) -> CirculationOperators<'_> {
        CirculationOperators::new(&self.grid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::BuildCancellation;
    use crate::generators::natural::ClimateWorkDomainBuilder;
    use crate::generators::spatial::ProfileSurfaceBuilder;
    use crate::world::natural::NaturalQualityProfile;
    use crate::world::Meters;

    fn geometry() -> ReducedClimateGeometry {
        let cancellation = BuildCancellation::new();
        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &cancellation,
        )
        .unwrap();
        let domain = ClimateWorkDomainBuilder::build(
            bundle.authoritative_surface(),
            NaturalQualityProfile::Draft,
            &cancellation,
        )
        .unwrap();
        ReducedClimateGeometry::new(&domain, &|| false).unwrap()
    }

    /// Contract (design section 17.3): edge fluxes taken as vertex streamfunction differences
    /// sum to zero around every cell for any zonal streamfunction, so the band-mean zonal
    /// vapour flow has no discrete divergence. A mis-ordered vertex pair breaks this at once.
    #[test]
    fn vertex_streamfunction_fluxes_are_discretely_divergence_free() {
        let geometry = geometry();
        let psi = |latitude: f64| 3.0e7 * latitude.sin() + 1.0e7 * (3.0 * latitude).cos();
        for (cell, slots) in geometry.cell_edges.iter().enumerate() {
            let mut net = 0.0_f64;
            let mut largest = 0.0_f64;
            for &(edge_id, owner) in slots {
                let edge = &geometry.edges[edge_id as usize];
                let flux = psi(edge.vertex_latitude_rad[0]) - psi(edge.vertex_latitude_rad[1]);
                net += if owner == 0 { flux } else { -flux };
                largest = largest.max(flux.abs());
            }
            assert!(
                net.abs() <= 1.0e-9 * largest.max(1.0),
                "cell {cell}: net {net}, largest edge flux {largest}"
            );
        }
    }

    /// Contract (design section 10 T2): the prewarmed formation geometry already carries the
    /// reduced geometry of its work domain, equal to a fresh build from the same snapshot.
    #[test]
    fn prewarmed_work_domain_carries_the_reduced_geometry() {
        use crate::generators::natural::{FormationGeometry, FormationGeometryKey};
        let prepared = FormationGeometry::build(
            FormationGeometryKey::new(
                NaturalQualityProfile::Draft,
                Meters::new(6_371_000.0).unwrap(),
            ),
            &BuildCancellation::new(),
        )
        .unwrap();
        let domain = prepared
            .climate_domain_for(
                NaturalQualityProfile::Draft,
                prepared.authoritative_surface(),
            )
            .expect("geometry names its own surface")
            .get();
        let cached = domain.reduced_climate_geometry();
        let fresh = ReducedClimateGeometry::new(domain.snapshot(), &|| false).unwrap();
        assert_eq!(cached.edges.len(), fresh.edges.len());
        assert!(cached
            .edges
            .iter()
            .zip(&fresh.edges)
            .all(|(a, b)| a.vertex_latitude_rad == b.vertex_latitude_rad
                && a.hadley_weight.to_bits() == b.hadley_weight.to_bits()));
        assert_eq!(
            cached.stommel_friction_per_s.to_bits(),
            fresh.stommel_friction_per_s.to_bits()
        );
    }

    /// Contract: the cached metrics tile the sphere and every Hadley weight is a partition
    /// share.
    #[test]
    fn metrics_tile_the_sphere_and_hadley_weights_are_shares() {
        let geometry = geometry();
        let sphere = 4.0 * std::f64::consts::PI * geometry.radius_m * geometry.radius_m;
        let area: f64 = geometry.area_m2.iter().sum();
        assert!((area - sphere).abs() <= 1.0e-9 * sphere);
        assert!(geometry
            .edges
            .iter()
            .all(|edge| edge.hadley_weight > 0.0 && edge.hadley_weight <= 1.0));
        assert_eq!(geometry.cell_band.len(), geometry.cell_count());
        assert!(geometry.stommel_friction_per_s > 0.0);
    }
}
