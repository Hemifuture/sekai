use thiserror::Error;

use super::super::random::LabeledSubstreams;
use super::super::{MantleGenerator, SphericalMantleGenerationError};
use crate::engine::StageRng;
use crate::world::natural::{
    effective_crust_density_kg_m3, sediment_source_for_bedrock, BedrockKind, BedrockKindField,
    CrustKind, EvolvedTectonicSnapshot, EvolvedTectonicValidationError, GeologicSpec,
    GeologicSpecError, GeologicSubstrateSnapshot, GeologicSubstrateValidationError,
    ResolvedWorldFormation, SedimentSourceKindField, WorldFormationSpecError,
    GEOLOGIC_SUBSTRATE_SCHEMA_V1,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};

/// Present volcanic influence required to publish current volcanic cover.
pub const VOLCANIC_COVER_INFLUENCE_THRESHOLD: f32 = 0.55;
/// Continental shortening rate treated as strong enough for metamorphic cover.
pub const METAMORPHIC_SHORTENING_THRESHOLD_MM_PER_YEAR: f32 = 6.0;
/// Continental uplift rate treated as strong enough for metamorphic cover.
pub const METAMORPHIC_UPLIFT_THRESHOLD_MM_PER_YEAR: f32 = 0.8;
/// Minimum active continental subsidence that permits basin cover.
pub const SEDIMENTARY_SUBSIDENCE_THRESHOLD_MM_PER_YEAR: f32 = 0.08;
/// Maximum fracture intensity under which basin cover remains coherent.
pub const SEDIMENTARY_FRACTURE_MAX: f32 = 0.38;

const FRACTURE_BOUNDARY_SUPPORT_M: f32 = 750_000.0;
const FRACTURE_RATE_SCALE_MM_PER_YEAR: f32 = 5.0;
const CANCELLATION_POLL_STRIDE: usize = 256;

/// Deterministic conversion of conservative V5 causes into a P3 substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeologicSubstrateGenerator;

impl GeologicSubstrateGenerator {
    /// Generates a strict substrate without inventing a second tectonic state.
    pub fn generate(
        surface: &SphericalSurfaceSnapshot,
        evolved: &EvolvedTectonicSnapshot,
        spec: &GeologicSpec,
        formation: &ResolvedWorldFormation,
        rng: &mut StageRng,
    ) -> Result<GeologicSubstrateSnapshot, GeologicSubstrateGenerationError> {
        validate_inputs(surface, evolved, spec, formation)?;
        rng.check_cancelled()
            .map_err(|_| GeologicSubstrateGenerationError::Cancelled)?;
        let streams = LabeledSubstreams::capture(rng);
        Self::generate_from_streams(surface, evolved, spec, formation, &streams)
    }

    pub(in crate::generators::natural) fn generate_from_streams(
        surface: &SphericalSurfaceSnapshot,
        evolved: &EvolvedTectonicSnapshot,
        spec: &GeologicSpec,
        formation: &ResolvedWorldFormation,
        streams: &LabeledSubstreams,
    ) -> Result<GeologicSubstrateSnapshot, GeologicSubstrateGenerationError> {
        validate_inputs(surface, evolved, spec, formation)?;
        check_cancelled(streams)?;

        let mantle = MantleGenerator::generate_spherical_from_streams(
            surface,
            spec,
            formation.mantle_bias(),
            streams,
        )?;
        check_cancelled(streams)?;

        let count = surface.cells().len();
        let tectonic = evolved.authoritative_view();
        let material = tectonic.material();
        let forcing = tectonic.forcing();
        let mut density = Vec::with_capacity(count);
        let mut bedrock = Vec::with_capacity(count);
        let mut fracture = Vec::with_capacity(count);
        let mut erodibility = Vec::with_capacity(count);
        let mut permeability = Vec::with_capacity(count);

        for index in 0..count {
            if index % CANCELLATION_POLL_STRIDE == 0 {
                check_cancelled(streams)?;
            }
            let crust = tectonic
                .crust_kinds()
                .get(index)
                .expect("validated evolved tectonics has a dense crust field");
            let effective_density = effective_crust_density_kg_m3(
                material.continental_volume_m3()[index],
                material.oceanic_volume_m3()[index],
            )?;
            let local_fracture = substrate_fracture_intensity(
                forcing.boundary_distance_m()[index],
                forcing.shortening_rate_mm_per_year()[index],
                forcing.uplift_rate_mm_per_year()[index],
                forcing.subsidence_rate_mm_per_year()[index],
                mantle.volcanic_influence()[index],
            );
            let local_bedrock = classify_substrate_bedrock(
                crust,
                mantle.volcanic_influence()[index],
                forcing.shortening_rate_mm_per_year()[index],
                forcing.uplift_rate_mm_per_year()[index],
                forcing.subsidence_rate_mm_per_year()[index],
                local_fracture,
            );
            let (local_erodibility, local_permeability) = substrate_properties(
                local_bedrock,
                local_fracture,
                forcing.subsidence_rate_mm_per_year()[index],
            );

            density.push(effective_density);
            bedrock.push(local_bedrock);
            fracture.push(local_fracture);
            erodibility.push(local_erodibility);
            permeability.push(local_permeability);
        }

        check_cancelled(streams)?;
        let sediment_sources = SedimentSourceKindField::from_kinds(
            bedrock
                .iter()
                .copied()
                .map(sediment_source_for_bedrock)
                .collect(),
        );
        let snapshot = GeologicSubstrateSnapshot::new(
            GEOLOGIC_SUBSTRATE_SCHEMA_V1,
            SurfaceRef::for_spherical(surface),
            mantle,
            tectonic.crust_kinds().clone(),
            tectonic.crust_thickness_km().to_vec(),
            tectonic.crust_age_myr().to_vec(),
            density,
            BedrockKindField::from_kinds(bedrock),
            fracture,
            erodibility,
            permeability,
            sediment_sources,
        )?;
        snapshot.validate_against(surface, evolved)?;
        Ok(snapshot)
    }
}

fn validate_inputs(
    surface: &SphericalSurfaceSnapshot,
    evolved: &EvolvedTectonicSnapshot,
    spec: &GeologicSpec,
    formation: &ResolvedWorldFormation,
) -> Result<(), GeologicSubstrateGenerationError> {
    spec.validate()?;
    formation.validate()?;
    evolved.validate_against(surface)?;
    Ok(())
}

fn check_cancelled(streams: &LabeledSubstreams) -> Result<(), GeologicSubstrateGenerationError> {
    streams
        .check_cancelled()
        .map_err(|_| GeologicSubstrateGenerationError::Cancelled)
}

/// Applies the locked P3 causal lithology priority to one validated cell.
pub fn classify_substrate_bedrock(
    crust: CrustKind,
    volcanic_influence: f32,
    shortening_rate_mm_per_year: f32,
    uplift_rate_mm_per_year: f32,
    subsidence_rate_mm_per_year: f32,
    fracture_intensity: f32,
) -> BedrockKind {
    if volcanic_influence >= VOLCANIC_COVER_INFLUENCE_THRESHOLD {
        return BedrockKind::Volcanic;
    }
    if crust == CrustKind::Oceanic {
        return BedrockKind::OceanicMafic;
    }
    if shortening_rate_mm_per_year >= METAMORPHIC_SHORTENING_THRESHOLD_MM_PER_YEAR
        || uplift_rate_mm_per_year >= METAMORPHIC_UPLIFT_THRESHOLD_MM_PER_YEAR
    {
        return BedrockKind::Metamorphic;
    }
    if subsidence_rate_mm_per_year >= SEDIMENTARY_SUBSIDENCE_THRESHOLD_MM_PER_YEAR
        && fracture_intensity <= SEDIMENTARY_FRACTURE_MAX
    {
        return BedrockKind::Sedimentary;
    }
    BedrockKind::ContinentalCrystalline
}

fn substrate_fracture_intensity(
    boundary_distance_m: f32,
    shortening_rate_mm_per_year: f32,
    uplift_rate_mm_per_year: f32,
    subsidence_rate_mm_per_year: f32,
    volcanic_influence: f32,
) -> f32 {
    let normalized_distance = (boundary_distance_m / FRACTURE_BOUNDARY_SUPPORT_M).clamp(0.0, 1.0);
    let boundary_proximity =
        1.0 - normalized_distance * normalized_distance * (3.0 - 2.0 * normalized_distance);
    let maximum_rate = shortening_rate_mm_per_year
        .max(uplift_rate_mm_per_year)
        .max(subsidence_rate_mm_per_year);
    let rate_activity = 1.0 - (-maximum_rate / FRACTURE_RATE_SCALE_MM_PER_YEAR).exp();
    (0.08 + 0.52 * boundary_proximity + 0.25 * rate_activity + 0.15 * volcanic_influence)
        .clamp(0.0, 1.0)
}

fn substrate_properties(
    bedrock: BedrockKind,
    fracture_intensity: f32,
    subsidence_rate_mm_per_year: f32,
) -> (f32, f32) {
    let (base_erodibility, base_permeability) = match bedrock {
        BedrockKind::OceanicMafic => (0.42, 0.34),
        BedrockKind::ContinentalCrystalline => (0.20, 0.16),
        BedrockKind::Sedimentary => (0.72, 0.56),
        BedrockKind::Metamorphic => (0.14, 0.10),
        BedrockKind::Volcanic => (0.34, 0.28),
    };
    let basin_term = subsidence_rate_mm_per_year / (subsidence_rate_mm_per_year + 0.25_f32);
    (
        (base_erodibility + 0.14 * fracture_intensity + 0.06 * basin_term).clamp(0.0, 1.0),
        (base_permeability + 0.32 * fracture_intensity + 0.10 * basin_term).clamp(0.0, 1.0),
    )
}

/// Failures that prevent publication of a causal P3 substrate.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum GeologicSubstrateGenerationError {
    /// The owning build requested cooperative cancellation.
    #[error("geologic substrate generation was cancelled")]
    Cancelled,
    /// The requested geologic controls are invalid.
    #[error("invalid geologic specification: {0}")]
    InvalidSpec(#[from] GeologicSpecError),
    /// The resolved formation selection is invalid.
    #[error("invalid resolved formation: {0}")]
    InvalidFormation(#[from] WorldFormationSpecError),
    /// The authoritative V5 causes or surface binding are invalid.
    #[error("invalid evolved tectonics: {0}")]
    InvalidEvolved(#[from] EvolvedTectonicValidationError),
    /// Mantle forcing could not be generated on this sphere.
    #[error("mantle generation failed: {0}")]
    Mantle(#[from] SphericalMantleGenerationError),
    /// The generated substrate violated its immutable contract.
    #[error("generated geologic substrate is invalid: {0}")]
    InvalidSnapshot(#[from] GeologicSubstrateValidationError),
}
