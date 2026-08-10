use thiserror::Error;

use super::random::LabeledSubstreams;
use super::tectonics::TectonicGenerator;
use super::topology::NaturalTopologyIndex;
use crate::engine::StageRng;
use crate::world::natural::{
    NaturalSpecError, ResolvedWorldFormation, SphericalCrustState, SphericalTectonicSnapshot,
    SphericalTectonicValidationError, TectonicSpec, WorldFormationSpecError,
    TECTONIC_SNAPSHOT_SCHEMA_V3,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRefError,
};
use crate::world::{EdgeId, PlateId};

mod boundaries;
mod crust;
mod initial_state;
mod model;
mod motion;
mod plates;

use boundaries::classify_and_aggregate_boundaries;
use crust::{generate_crust, CrustMorphologyError};
use motion::{assign_plate_rotations, PlateMotionError};
use plates::{generate_plate_partition, PlateMorphologyError};

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
        let partition = generate_plate_partition(surface, &topology, spec, &streams)
            .map_err(map_plate_morphology_error)?;
        let plates =
            assign_plate_rotations(surface, &topology, &partition, spec.activity, &streams)
                .map_err(map_plate_motion_error)?;
        let crust = generate_crust(
            surface,
            &topology,
            &partition,
            spec,
            formation.resolved(),
            &streams,
        )
        .map_err(map_crust_morphology_error)?;
        let (boundaries, boundary_segments) = classify_and_aggregate_boundaries(
            surface,
            &topology,
            &plates,
            &partition.owners,
            &crust,
        );
        let snapshot = SphericalTectonicSnapshot::new(
            TECTONIC_SNAPSHOT_SCHEMA_V3,
            view.surface_ref(),
            plates,
            partition.owners,
            SphericalCrustState::from_pre_evolution_fields(crust.kinds, crust.thickness_km)?,
            boundaries,
            boundary_segments,
        )?;
        snapshot.validate_against_validated_surface(surface)?;
        Ok(snapshot)
    }
}

fn map_plate_morphology_error(error: PlateMorphologyError) -> SphericalTectonicGenerationError {
    match error {
        PlateMorphologyError::PlateCountExceedsCells { plates, cells } => {
            SphericalTectonicGenerationError::PlateCountExceedsCells {
                plates: plates as u16,
                cells,
            }
        }
        error => SphericalTectonicGenerationError::Morphology {
            domain: "plate partition",
            message: error.to_string(),
        },
    }
}

fn map_crust_morphology_error(error: CrustMorphologyError) -> SphericalTectonicGenerationError {
    SphericalTectonicGenerationError::Morphology {
        domain: "continental crust",
        message: error.to_string(),
    }
}

fn map_plate_motion_error(error: PlateMotionError) -> SphericalTectonicGenerationError {
    match error {
        PlateMotionError::InvalidRotation(error) => {
            SphericalTectonicGenerationError::InvalidSnapshot(error)
        }
        PlateMotionError::UnsatisfiedRelativeMotion {
            edge,
            plates,
            minimum_mm_per_year,
            found_mm_per_year,
        } => SphericalTectonicGenerationError::UnsatisfiedRelativeMotion {
            edge,
            plates,
            minimum_mm_per_year,
            found_mm_per_year,
        },
        error @ PlateMotionError::Cardinality { .. } => {
            SphericalTectonicGenerationError::Morphology {
                domain: "plate motion",
                message: error.to_string(),
            }
        }
    }
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
    /// The required continental fraction cannot fit in the eligible surface area.
    #[error(
        "continental crust needs area weight {requested_area_weight}, but only {available_area_weight} is eligible"
    )]
    InsufficientCrustFormationArea {
        /// Quantized area required by the explicit tectonic specification.
        requested_area_weight: u128,
        /// Quantized surface area available to continental crust.
        available_area_weight: u128,
    },
    /// No fixed Euler candidate kept one plate interface above the activity floor.
    #[error(
        "edge {edge:?} between plates {plates:?} reaches only {found_mm_per_year} mm/year, below the required {minimum_mm_per_year} mm/year"
    )]
    UnsatisfiedRelativeMotion {
        /// The authoritative cross-plate edge.
        edge: EdgeId,
        /// The adjacent plate pair in ascending identifier order.
        plates: [PlateId; 2],
        /// The activity-dependent minimum relative speed.
        minimum_mm_per_year: f64,
        /// The generated local relative speed.
        found_mm_per_year: f64,
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
