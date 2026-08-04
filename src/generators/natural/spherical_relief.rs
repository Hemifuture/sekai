use rand::RngCore;
use thiserror::Error;

use super::random::{
    LabeledSubstreams, RELIEF_HOTSPOT_MORPHOLOGY_LABEL, RELIEF_ISLAND_ARC_LABEL,
    RELIEF_REGIONAL_LABEL,
};
use super::relief::{
    reconcile_final_safety, synthesize_crust_base, synthesize_tectonic_offset_core,
    ReliefGenerator, SEA_LEVEL_M,
};
use super::relief_noise::{FractalProfile, ReliefNoise3d};
use super::spherical_island_relief::{
    synthesize_spherical_hotspot_offset, synthesize_spherical_oceanic_arc_peaks,
};
use super::topology::NaturalTopologyIndex;
use crate::engine::{Diagnostic, StageRng};
use crate::world::natural::{
    CrustKind, ElevationField, LandOceanField, ReliefValidationError, SphericalMantleSnapshot,
    SphericalMantleValidationError, SphericalReliefSnapshot, SphericalReliefValidationError,
    SphericalTectonicSnapshot, SphericalTectonicValidationError, REGIONAL_OFFSET_MAX_M,
    REGIONAL_OFFSET_MIN_M, RELIEF_SCHEMA_V4, TECTONIC_OFFSET_MAX_M, TECTONIC_OFFSET_MIN_M,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRefError,
};

const SPHERICAL_REGIONAL_PROFILE: FractalProfile = FractalProfile {
    octaves: 6,
    frequency: 1.1,
    lacunarity: 2.03,
    persistence: 0.52,
};
const SPHERICAL_REGIONAL_RIDGES: FractalProfile = FractalProfile {
    octaves: 5,
    frequency: 1.7,
    lacunarity: 2.07,
    persistence: 0.47,
};

impl ReliefGenerator {
    /// Generates explainable V4 relief directly on a closed spherical surface.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        tectonic: &SphericalTectonicSnapshot,
        mantle: &SphericalMantleSnapshot,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<SphericalReliefSnapshot, SphericalReliefGenerationError> {
        surface.validate()?;
        tectonic.validate_against_validated_surface(surface)?;
        mantle.validate_against_validated_surface(surface)?;

        let view = SphericalNaturalSurface::from_validated(surface)?;
        let topology = NaturalTopologyIndex::from_surface(&view);
        let streams = LabeledSubstreams::capture(rng);
        let mut crust_base = synthesize_crust_base(
            &topology,
            tectonic.crust_kinds(),
            tectonic.crust_thickness_km(),
        );
        let mut tectonic_offset = synthesize_tectonic_offset_core(
            &topology,
            tectonic.cell_plates(),
            tectonic.boundaries(),
            &streams,
        );
        let mut island_arc_rng = streams.stream(RELIEF_ISLAND_ARC_LABEL);
        let island_arc = synthesize_spherical_oceanic_arc_peaks(
            surface,
            &topology,
            tectonic,
            island_arc_rng.next_u32(),
        );
        for (value, peak) in tectonic_offset.iter_mut().zip(island_arc) {
            *value = (*value + peak).clamp(TECTONIC_OFFSET_MIN_M, TECTONIC_OFFSET_MAX_M);
        }

        let mut hotspot_rng = streams.stream(RELIEF_HOTSPOT_MORPHOLOGY_LABEL);
        let mut volcanic_offset =
            synthesize_spherical_hotspot_offset(surface, tectonic, mantle, hotspot_rng.next_u32());
        let mut regional_offset = synthesize_spherical_regional_offset(surface, tectonic, &streams);
        let elevation = reconcile_final_safety(
            &mut crust_base,
            &mut tectonic_offset,
            &mut volcanic_offset,
            &mut regional_offset,
            diagnostics,
        );

        let crust_base = ElevationField::from_values(crust_base)?;
        let tectonic_offset = ElevationField::from_values(tectonic_offset)?;
        let volcanic_offset = ElevationField::from_values(volcanic_offset)?;
        let regional_offset = ElevationField::from_values(regional_offset)?;
        let elevation = ElevationField::from_values(elevation)?;
        let land_ocean = LandOceanField::classify(&elevation, SEA_LEVEL_M);
        let snapshot = SphericalReliefSnapshot::new(
            RELIEF_SCHEMA_V4,
            view.surface_ref(),
            SEA_LEVEL_M,
            crust_base,
            tectonic_offset,
            volcanic_offset,
            regional_offset,
            elevation,
            land_ocean,
        )?;
        snapshot.validate_against_validated_surface(surface)?;
        Ok(snapshot)
    }
}

fn synthesize_spherical_regional_offset(
    surface: &SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
    streams: &LabeledSubstreams,
) -> Vec<f32> {
    let mut rng = streams.stream(RELIEF_REGIONAL_LABEL);
    let noise = ReliefNoise3d::new(rng.next_u32());
    let sample_spacing_m = (surface.total_cell_area().get() / surface.cells().len() as f64).sqrt();
    let profile =
        SPHERICAL_REGIONAL_PROFILE.limited_to_resolution(surface.radius().get(), sample_spacing_m);
    let ridges =
        SPHERICAL_REGIONAL_RIDGES.limited_to_resolution(surface.radius().get(), sample_spacing_m);
    let mut result = surface
        .cells()
        .iter()
        .map(|cell| {
            let point = cell.centroid.components();
            let broad = noise.fbm(point, profile);
            let ridge = noise.ridged(point, ridges) - 0.5;
            let signal = (0.82 * broad + 0.18 * ridge).clamp(-1.0, 1.0);
            let amplitude = match tectonic
                .crust_kind(cell.id)
                .expect("validated tectonic field is cell aligned")
            {
                CrustKind::Oceanic => 300.0,
                CrustKind::Continental => 450.0,
            };
            (signal * amplitude) as f32
        })
        .collect::<Vec<_>>();
    area_weighted_center_and_bound(surface, &mut result);
    result
}

fn area_weighted_center_and_bound(surface: &SphericalSurfaceSnapshot, values: &mut [f32]) {
    let total_area = surface.total_cell_area().get();
    for _ in 0..2 {
        let mean = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| f64::from(values[index]) * cell.area.get())
            .sum::<f64>()
            / total_area;
        for value in values.iter_mut() {
            *value = (*value - mean as f32).clamp(REGIONAL_OFFSET_MIN_M, REGIONAL_OFFSET_MAX_M);
        }
    }
}

/// Errors returned while generating surface-bound spherical relief.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalReliefGenerationError {
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The surface identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The supplied tectonic snapshot is incompatible with the surface.
    #[error("invalid spherical tectonic input: {0}")]
    InvalidTectonics(#[from] SphericalTectonicValidationError),
    /// The supplied mantle snapshot is incompatible with the surface.
    #[error("invalid spherical mantle input: {0}")]
    InvalidMantle(#[from] SphericalMantleValidationError),
    /// A generated dense field violated the shared relief semantics.
    #[error("invalid generated relief field: {0}")]
    InvalidReliefField(#[from] ReliefValidationError),
    /// The completed surface-bound snapshot violated its V4 contract.
    #[error("invalid generated spherical relief snapshot: {0}")]
    InvalidSnapshot(#[from] SphericalReliefValidationError),
}
