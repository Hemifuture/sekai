use thiserror::Error;

use super::random::LabeledSubstreams;
use super::tectonics::TectonicGenerator;
use super::topology::NaturalTopologyIndex;
use crate::engine::StageRng;
use crate::world::natural::{
    CrustKindField, NaturalSpecError, ResolvedWorldFormation, SphericalCrustState,
    SphericalTectonicSnapshot, SphericalTectonicValidationError, TectonicSpec,
    WorldFormationSpecError, TECTONIC_SNAPSHOT_SCHEMA_V3,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRefError,
};

mod boundaries;
mod contacts;
mod initial_state;
mod kinematics;
mod model;
mod processes;
mod resample;
mod runner;

use boundaries::classify_and_aggregate_boundaries;
use model::CrustSample;
use runner::run_tectonic_evolution;

impl TectonicGenerator {
    /// Generates a surface-bound current snapshot on a validated closed spherical world.
    ///
    /// Plate motion is stored as one rigid Euler rotation per plate. Boundary
    /// kinematics are evaluated in each authoritative edge's local tangent frame.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        spec: &TectonicSpec,
        formation: &ResolvedWorldFormation,
        rng: &mut StageRng,
    ) -> Result<SphericalTectonicSnapshot, SphericalTectonicGenerationError> {
        spec.validate()?;
        formation.validate()?;
        surface.validate()?;
        if spec.plate_count as usize > surface.cells().len() {
            return Err(SphericalTectonicGenerationError::PlateCountExceedsCells {
                plates: spec.plate_count,
                cells: surface.cells().len(),
            });
        }

        let view = SphericalNaturalSurface::from_validated(surface)?;
        let topology = NaturalTopologyIndex::from_surface(&view);
        let streams = LabeledSubstreams::capture(rng);
        let current =
            run_tectonic_evolution(surface, &topology, spec, formation.resolved(), &streams)
                .map_err(|error| SphericalTectonicGenerationError::Morphology {
                    domain: "tectonic evolution",
                    message: error.to_string(),
                })?;
        let crust = crust_state_from_samples(&current.samples)?;
        let (boundaries, boundary_segments) = classify_and_aggregate_boundaries(
            surface,
            &topology,
            &current.plates,
            &current.cell_plates,
            &crust,
        )?;
        let snapshot = SphericalTectonicSnapshot::new(
            TECTONIC_SNAPSHOT_SCHEMA_V3,
            view.surface_ref(),
            current.plates,
            current.cell_plates,
            crust,
            boundaries,
            boundary_segments,
        )?;
        snapshot.validate_against_validated_surface(surface)?;
        Ok(snapshot)
    }
}

fn crust_state_from_samples(
    samples: &[CrustSample],
) -> Result<SphericalCrustState, SphericalTectonicValidationError> {
    let mut kinds = Vec::with_capacity(samples.len());
    let mut thickness_km = Vec::with_capacity(samples.len());
    let mut age_myr = Vec::with_capacity(samples.len());
    let mut tectonic_elevation_m = Vec::with_capacity(samples.len());
    let mut lineation_east = Vec::with_capacity(samples.len());
    let mut lineation_north = Vec::with_capacity(samples.len());
    let mut orogeny_kind = Vec::with_capacity(samples.len());
    let mut orogeny_age_myr = Vec::with_capacity(samples.len());
    for sample in samples {
        kinds.push(sample.kind);
        thickness_km.push(sample.thickness_km);
        age_myr.push(sample.age_myr);
        tectonic_elevation_m.push(sample.tectonic_elevation_m);
        lineation_east.push(sample.lineation[0]);
        lineation_north.push(sample.lineation[1]);
        orogeny_kind.push(sample.orogeny);
        orogeny_age_myr.push(sample.orogeny_age_myr);
    }
    SphericalCrustState::new(
        CrustKindField::from_kinds(kinds),
        thickness_km,
        age_myr,
        tectonic_elevation_m,
        lineation_east,
        lineation_north,
        orogeny_kind,
        orogeny_age_myr,
    )
}

/// Errors returned when a closed spherical tectonic snapshot cannot be generated.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalTectonicGenerationError {
    /// The requested tectonic specification is invalid.
    #[error("invalid tectonic specification: {0}")]
    InvalidSpec(#[from] NaturalSpecError),
    /// The supplied resolved formation selection is invalid.
    #[error("invalid resolved world formation: {0}")]
    InvalidFormation(#[from] WorldFormationSpecError),
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The authoritative spherical surface identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The requested plate count exceeds the available surface cells.
    #[error("requested {plates} plates for only {cells} spherical surface cells")]
    PlateCountExceedsCells {
        /// The requested number of plates.
        plates: u16,
        /// The number of available cells.
        cells: usize,
    },
    /// The requested current-state morphology could not be constructed.
    #[error("{domain} morphology failed: {message}")]
    Morphology {
        /// The orthogonal morphology domain that rejected the candidate.
        domain: &'static str,
        /// The internal typed error rendered without exposing build intermediates.
        message: String,
    },
    /// Generated spherical tectonic data violated a snapshot invariant.
    #[error("generated spherical tectonic snapshot is invalid: {0}")]
    InvalidSnapshot(#[from] SphericalTectonicValidationError),
}

#[cfg(test)]
mod tests {
    use super::model::FormationTectonicRecipe;
    use crate::world::natural::ResolvedWorldFormationPreset;

    #[test]
    fn facade_keeps_domain_modules_orthogonal() {
        let source = include_str!("spherical_tectonics.rs");
        let facade = source
            .split("#[cfg(test)]")
            .next()
            .expect("the facade source precedes its tests");
        for forbidden in [
            "const EULER_POLES:",
            "fn assign_plate_rotations(",
            "struct BoundaryEventDraft",
            "fn classify_and_aggregate_boundaries(",
            "fn aggregate_boundary_events(",
        ] {
            assert!(
                !facade.contains(forbidden),
                "spherical tectonic facade still owns `{forbidden}`"
            );
        }
    }

    #[test]
    fn formation_recipes_select_spectra_and_bounded_process_multipliers() {
        use ResolvedWorldFormationPreset::{
            Archipelago, Continents, GreatIsland, Supercontinent, VolcanicIslands,
        };

        let supercontinent = FormationTectonicRecipe::for_preset(Supercontinent);
        let archipelago = FormationTectonicRecipe::for_preset(Archipelago);
        let continents = FormationTectonicRecipe::for_preset(Continents);
        let great_island = FormationTectonicRecipe::for_preset(GreatIsland);
        let volcanic = FormationTectonicRecipe::for_preset(VolcanicIslands);

        assert!(supercontinent.base_scale_rad > archipelago.base_scale_rad);
        assert!(great_island.rift_rate_permille < continents.rift_rate_permille);
        assert!(volcanic.island_arc_gain_permille > continents.island_arc_gain_permille);
        for recipe in [
            supercontinent,
            archipelago,
            continents,
            great_island,
            volcanic,
        ] {
            recipe.initial_crust_profile.assert_valid();
            assert!(recipe.base_scale_rad.is_finite() && recipe.base_scale_rad > 0.0);
            assert!((500..=1_500).contains(&recipe.rift_rate_permille));
            assert!((500..=1_500).contains(&recipe.subduction_gain_permille));
            assert!((500..=1_500).contains(&recipe.island_arc_gain_permille));
        }
    }
}
