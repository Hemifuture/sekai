use thiserror::Error;

use super::super::land_fraction::{select_area_weighted_sea_level, LandFractionSelectionError};
use super::super::random::{LabeledSubstreams, RELIEF_HOTSPOT_MORPHOLOGY_LABEL};
use super::super::relief::{reconcile_final_safety, ReliefGenerator};
use super::island_relief::synthesize_spherical_hotspot_offset;
use crate::engine::{Diagnostic, StageRng};
use crate::world::natural::{
    CrustKindField, ElevationField, LandOceanField, LandOceanKind, ReliefSpec, ReliefSpecError,
    ReliefValidationError, SphericalMantleSnapshot, SphericalMantleValidationError,
    SphericalOrogenyKind, SphericalReliefSnapshot, SphericalReliefValidationError,
    SphericalTectonicSnapshot, SphericalTectonicValidationError, RELIEF_SCHEMA_V4,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRefError,
};

mod directed_noise;
mod tectonic_heightmap;

use directed_noise::DirectedDetailNoise;
use tectonic_heightmap::{build_tectonic_heightmap, TectonicHeightmapError};

pub(in crate::generators::natural) fn synthesize_conditioned_regional_detail(
    surface: &SphericalSurfaceSnapshot,
    crust_kinds: &CrustKindField,
    crust_age_myr: &[f32],
    lineation_east: &[f32],
    lineation_north: &[f32],
    orogeny_kind: &[SphericalOrogenyKind],
    orogeny_age_myr: &[f32],
    streams: &LabeledSubstreams,
) -> Result<Vec<f64>, crate::engine::BuildCancellationError> {
    let sample_spacing_m = (surface.total_cell_area().get() / surface.cells().len() as f64).sqrt();
    let detail_noise =
        DirectedDetailNoise::from_streams(streams, surface.radius().get(), sample_spacing_m);
    let mut detail = Vec::with_capacity(surface.cells().len());
    for (index, surface_cell) in surface.cells().iter().enumerate() {
        if index % 256 == 0 {
            streams.check_cancelled()?;
        }
        let cell = crate::world::CellId::from_raw(index as u32);
        detail.push(
            detail_noise.sample_m(
                surface_cell.centroid,
                crust_kinds
                    .get(cell.raw() as usize)
                    .expect("validated spherical crust is cell aligned"),
                crust_age_myr[index],
                lineation_east[index],
                lineation_north[index],
                orogeny_kind[index],
                orogeny_age_myr[index],
            ),
        );
    }
    streams.check_cancelled()?;
    Ok(detail)
}

impl ReliefGenerator {
    /// Generates explainable V4 relief directly on a closed spherical surface.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        tectonic: &SphericalTectonicSnapshot,
        mantle: &SphericalMantleSnapshot,
        relief_spec: &ReliefSpec,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<SphericalReliefSnapshot, SphericalReliefGenerationError> {
        surface.validate()?;
        tectonic.validate_against_validated_surface(surface)?;
        mantle.validate_against_validated_surface(surface)?;
        relief_spec.validate()?;

        let view = SphericalNaturalSurface::from_validated(surface)?;
        let streams = LabeledSubstreams::capture(rng);
        let components = build_tectonic_heightmap(surface, tectonic, &streams)?;
        let mut crust_base = components.crust_base_m;
        let mut tectonic_offset = components.tectonic_offset_m;

        use rand::RngCore as _;
        let mut hotspot_rng = streams.stream(RELIEF_HOTSPOT_MORPHOLOGY_LABEL);
        let mut volcanic_offset = synthesize_spherical_hotspot_offset(
            surface,
            tectonic.plates(),
            tectonic.cell_plates(),
            tectonic.crust_kinds(),
            mantle,
            hotspot_rng.next_u32(),
        )
        .into_iter()
        .map(|value| value as f32)
        .collect::<Vec<_>>();
        let mut regional_offset = components.directed_detail_m;
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
        let cell_areas = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .collect::<Vec<_>>();
        let exact_elevation = elevation
            .values()
            .iter()
            .copied()
            .map(f64::from)
            .collect::<Vec<_>>();
        let selection = select_area_weighted_sea_level(
            &cell_areas,
            &exact_elevation,
            f64::from(relief_spec.target_land_fraction),
        )?;
        let sea_level_m = selection.sea_level_m as f32;
        if !sea_level_m.is_finite()
            || elevation.values().iter().zip(&exact_elevation).any(
                |(&projected_elevation, &exact)| {
                    LandOceanKind::classify(projected_elevation, sea_level_m)
                        != LandOceanKind::classify_exact(exact, selection.sea_level_m)
                },
            )
        {
            return Err(
                SphericalReliefGenerationError::InvalidLandFractionProjection {
                    exact_sea_level_m: selection.sea_level_m,
                    projected_sea_level_m: sea_level_m,
                },
            );
        }
        let land_ocean = LandOceanField::classify(&elevation, sea_level_m);
        debug_assert_eq!(
            selection.target_land_fraction,
            f64::from(relief_spec.target_land_fraction)
        );
        let classified_fraction = cell_areas
            .iter()
            .zip(land_ocean.raw_values())
            .filter_map(|(&area, &kind)| (kind == 1).then_some(area))
            .sum::<f64>()
            / surface.total_cell_area().get();
        debug_assert!((classified_fraction - selection.actual_land_fraction).abs() <= 1.0e-12);
        let snapshot = SphericalReliefSnapshot::new(
            RELIEF_SCHEMA_V4,
            view.surface_ref(),
            sea_level_m,
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

/// Errors returned while generating surface-bound spherical relief.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalReliefGenerationError {
    /// The authored target-land request is invalid.
    #[error("invalid relief spec: {0}")]
    InvalidSpec(#[from] ReliefSpecError),
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
    /// Current crust attributes could not be converted into bounded height components.
    #[error("invalid tectonic heightmap input: {message}")]
    InvalidHeightmap { message: String },
    /// The generated height field could not be classified by authoritative area.
    #[error("invalid land-area selection: {message}")]
    InvalidLandFraction { message: String },
    /// The legacy f32 wire cannot preserve the exact centimeter classification.
    #[error(
        "exact sea level {exact_sea_level_m} cannot project to legacy level {projected_sea_level_m} without changing classification"
    )]
    InvalidLandFractionProjection {
        exact_sea_level_m: f64,
        projected_sea_level_m: f32,
    },
    /// A generated dense field violated the shared relief semantics.
    #[error("invalid generated relief field: {0}")]
    InvalidReliefField(#[from] ReliefValidationError),
    /// The completed surface-bound snapshot violated its V4 contract.
    #[error("invalid generated spherical relief snapshot: {0}")]
    InvalidSnapshot(#[from] SphericalReliefValidationError),
}

impl From<LandFractionSelectionError> for SphericalReliefGenerationError {
    fn from(error: LandFractionSelectionError) -> Self {
        Self::InvalidLandFraction {
            message: error.to_string(),
        }
    }
}

impl From<TectonicHeightmapError> for SphericalReliefGenerationError {
    fn from(error: TectonicHeightmapError) -> Self {
        Self::InvalidHeightmap {
            message: error.to_string(),
        }
    }
}
