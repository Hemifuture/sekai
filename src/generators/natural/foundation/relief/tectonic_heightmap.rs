use thiserror::Error;

use super::directed_noise::DirectedDetailNoise;
use crate::generators::natural::foundation::crust_physics::{
    continental_isostatic_elevation_m, oceanic_plate_cooling_elevation_m,
};
use crate::generators::natural::random::LabeledSubstreams;
use crate::world::natural::{
    CrustKind, SphericalTectonicSnapshot, CRUST_BASE_ELEVATION_MAX_M, CRUST_BASE_ELEVATION_MIN_M,
    REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M, TECTONIC_OFFSET_MAX_M, TECTONIC_OFFSET_MIN_M,
};
use crate::world::spatial::SphericalSurfaceSnapshot;
use crate::world::CellId;

const HEIGHT_QUANTUM_M: f32 = 0.25;

pub(super) struct TectonicHeightComponents {
    pub(super) crust_base_m: Vec<f32>,
    pub(super) tectonic_offset_m: Vec<f32>,
    pub(super) directed_detail_m: Vec<f32>,
}

pub(super) fn build_tectonic_heightmap(
    surface: &SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
    streams: &LabeledSubstreams,
) -> Result<TectonicHeightComponents, TectonicHeightmapError> {
    let cell_count = surface.cells().len();
    if tectonic.crust_state().len() != cell_count {
        return Err(TectonicHeightmapError::CardinalityMismatch {
            surface_cells: cell_count,
            crust_cells: tectonic.crust_state().len(),
        });
    }

    let sample_spacing_m = (surface.total_cell_area().get() / cell_count as f64).sqrt();
    let detail_noise =
        DirectedDetailNoise::from_streams(streams, surface.radius().get(), sample_spacing_m);
    let mut crust_base_m = Vec::with_capacity(cell_count);
    let mut tectonic_offset_m = Vec::with_capacity(cell_count);
    let mut directed_detail_m = Vec::with_capacity(cell_count);

    for (index, surface_cell) in surface.cells().iter().enumerate() {
        let cell = CellId::from_raw(index as u32);
        let kind = tectonic
            .crust_kind(cell)
            .expect("validated spherical crust is cell aligned");
        let thickness_km = tectonic.crust_thickness_km()[index];
        let age_myr = tectonic.crust_age_myr()[index];
        let coarse_height_m = tectonic.tectonic_elevation_m()[index];
        let physical_base_m = match kind {
            CrustKind::Continental => continental_isostatic_elevation_m(thickness_km),
            CrustKind::Oceanic => oceanic_plate_cooling_elevation_m(age_myr, thickness_km),
        };
        let feasible_min = CRUST_BASE_ELEVATION_MIN_M.max(coarse_height_m - TECTONIC_OFFSET_MAX_M);
        let feasible_max = CRUST_BASE_ELEVATION_MAX_M.min(coarse_height_m - TECTONIC_OFFSET_MIN_M);
        let base_m = quantize(physical_base_m.clamp(feasible_min, feasible_max));
        let tectonic_m =
            quantize(coarse_height_m - base_m).clamp(TECTONIC_OFFSET_MIN_M, TECTONIC_OFFSET_MAX_M);
        let detail_m = quantize(detail_noise.sample_m(
            surface_cell.centroid,
            kind,
            age_myr,
            tectonic.lineation_east()[index],
            tectonic.lineation_north()[index],
            tectonic.orogeny_kind()[index],
            tectonic.orogeny_age_myr()[index],
        ) as f32)
        .clamp(REGIONAL_OFFSET_MIN_M, REGIONAL_OFFSET_MAX_M);

        for (component, value) in [
            ("crust_base_m", base_m),
            ("tectonic_offset_m", tectonic_m),
            ("directed_detail_m", detail_m),
        ] {
            if !value.is_finite() {
                return Err(TectonicHeightmapError::NonFiniteComponent { cell, component });
            }
        }
        crust_base_m.push(base_m);
        tectonic_offset_m.push(tectonic_m);
        directed_detail_m.push(detail_m);
    }

    Ok(TectonicHeightComponents {
        crust_base_m,
        tectonic_offset_m,
        directed_detail_m,
    })
}

fn quantize(value: f32) -> f32 {
    (value / HEIGHT_QUANTUM_M).round() * HEIGHT_QUANTUM_M
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(super) enum TectonicHeightmapError {
    #[error("spherical heightmap cardinality mismatch: surface has {surface_cells} cells, crust has {crust_cells}")]
    CardinalityMismatch {
        surface_cells: usize,
        crust_cells: usize,
    },
    #[error("cell {cell:?} produced a non-finite {component} heightmap component")]
    NonFiniteComponent {
        cell: CellId,
        component: &'static str,
    },
}
