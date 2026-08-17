//! Cortial Appendix-A coarse crust relaxation.
//!
//! One indexed pass ages oceanic crust, applies the paper's density subsidence
//! and continental linear erosion, and fills only active trenches. Transform
//! contacts are deliberately absent from the uplift mask.

use super::{bounded_elevation, constants, ProcessActions, ProcessError, ProcessStats};
use crate::generators::natural::spherical_tectonics::contacts::{ContactEvent, ContactKind};
use crate::generators::natural::spherical_tectonics::model::{
    FormationTectonicRecipe, TectonicState,
};
use crate::world::natural::{CrustKind, SphericalOrogenyKind, MAX_CRUST_AGE_MYR};
use crate::world::spatial::SphericalSurfaceSnapshot;

pub(in crate::generators::natural::spherical_tectonics) fn relax_current_crust(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
    delta_myr: f32,
) -> Result<ProcessStats, ProcessError> {
    if !delta_myr.is_finite() || delta_myr < 0.0 {
        return Err(ProcessError::InvalidDeltaMyr { found: delta_myr });
    }
    actions.validate_for(next.samples.len())?;
    let trenches = actions.trench_scratch(next.samples.len());
    for event in events {
        let ContactKind::OceanicSubduction { descending } = event.kind else {
            continue;
        };
        for &index in event.sample_indices.iter().flatten() {
            let sample_index = index as usize;
            let sample =
                next.samples
                    .get(sample_index)
                    .ok_or(ProcessError::ContactSampleOutOfBounds {
                        sample: sample_index,
                        samples: next.samples.len(),
                    })?;
            if sample.owner == descending {
                trenches[sample_index] = 1;
                break;
            }
        }
    }
    let years = f64::from(delta_myr) * 1_000_000.0;
    let ocean_damping_m = constants::OCEANIC_ELEVATION_DAMPING_MM_PER_YEAR * years / 1_000.0;
    let continental_erosion_m = constants::CONTINENTAL_EROSION_MM_PER_YEAR * years / 1_000.0;
    let sediment_gain = f64::from(recipe.subduction_gain_permille) / 1_000.0;
    let trench_sediment_m =
        constants::TRENCH_SEDIMENT_MM_PER_YEAR * years / 1_000.0 * sediment_gain;

    for (index, sample) in next.samples.iter_mut().enumerate() {
        if surface.cell(sample.anchor).is_none() {
            return Err(ProcessError::InvalidAnchor {
                sample: index,
                anchor: sample.anchor,
                cells: surface.cells().len(),
            });
        }
        match sample.kind {
            CrustKind::Oceanic => {
                sample.age_myr = (sample.age_myr + delta_myr).min(MAX_CRUST_AGE_MYR);
                let depth_factor = (1.0
                    - f64::from(sample.tectonic_elevation_m)
                        / f64::from(constants::OCEANIC_TRENCH_ELEVATION_M))
                .clamp(0.0, 2.0);
                sample.tectonic_elevation_m = bounded_elevation(
                    sample.tectonic_elevation_m - (ocean_damping_m * depth_factor) as f32,
                );
                if trenches[index] != 0 {
                    sample.tectonic_elevation_m =
                        bounded_elevation(sample.tectonic_elevation_m + trench_sediment_m as f32);
                }
            }
            CrustKind::Continental => {
                let elevation_fraction = f64::from(sample.tectonic_elevation_m)
                    / f64::from(constants::HIGHEST_CONTINENTAL_ELEVATION_M);
                sample.tectonic_elevation_m = bounded_elevation(
                    sample.tectonic_elevation_m
                        - (continental_erosion_m * elevation_fraction) as f32,
                );
            }
        }
        if sample.orogeny != SphericalOrogenyKind::None {
            sample.orogeny_age_myr = (sample.orogeny_age_myr + delta_myr).min(MAX_CRUST_AGE_MYR);
        }
    }
    Ok(ProcessStats {
        relaxed_samples: next.samples.len() as u32,
        ..ProcessStats::default()
    })
}

#[cfg(test)]
mod tests {
    use super::relax_current_crust;
    use crate::generators::natural::spherical_tectonics::contacts::{ContactEvent, ContactKind};
    use crate::generators::natural::spherical_tectonics::model::{
        ActivePlate, CrustSample, FormationTectonicRecipe, LineageId, MaterialColumn, TectonicState,
    };
    use crate::generators::natural::spherical_tectonics::processes::{constants, ProcessActions};
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, ResolvedWorldFormationPreset, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::UnitVector3;
    use crate::world::{CellId, Meters, SphericalSpaceSpec};

    #[test]
    fn one_indexed_pass_ages_ocean_damps_relief_erodes_continent_and_fills_trench() {
        assert_eq!(constants::OCEANIC_ELEVATION_DAMPING_MM_PER_YEAR, 0.04);
        assert_eq!(constants::CONTINENTAL_EROSION_MM_PER_YEAR, 0.03);
        assert_eq!(constants::TRENCH_SEDIMENT_MM_PER_YEAR, 0.3);
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap();
        let owner = LineageId::from_raw(0);
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
        let make = |index: usize, kind: CrustKind, age: f32, elevation: f32| CrustSample {
            position: surface.cells()[index].site,
            anchor: CellId::from_raw(index as u32),
            owner,
            kind,
            thickness_km: if kind == CrustKind::Oceanic {
                7.0
            } else {
                40.0
            },
            age_myr: age,
            tectonic_elevation_m: elevation,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material: MaterialColumn::pure(
                kind,
                surface.cells()[index].area.get(),
                if kind == CrustKind::Oceanic {
                    7.0
                } else {
                    40.0
                },
            )
            .unwrap(),
        };
        let samples = vec![
            make(0, CrustKind::Oceanic, 20.0, -1_000.0),
            make(1, CrustKind::Oceanic, 80.0, -9_000.0),
            make(
                2,
                CrustKind::Continental,
                CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
                5_000.0,
            ),
            make(
                3,
                CrustKind::Continental,
                CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
                2_000.0,
            ),
        ];
        let mut state = TectonicState::new(
            samples,
            vec![ActivePlate::new(owner, CellId::from_raw(0), rotation)],
            1,
        )
        .unwrap();
        let subduction = ContactEvent {
            cell: CellId::from_raw(1),
            edge: None,
            sample_indices: [Some(1), Some(2)],
            lineages: [Some(owner), Some(owner)],
            kind: ContactKind::OceanicSubduction { descending: owner },
            signed_normal_speed_mm_per_year: -50.0,
            tangent_speed_mm_per_year: 0.0,
            overlap_depth: 0,
        };
        let transform = ContactEvent {
            cell: CellId::from_raw(3),
            edge: None,
            sample_indices: [Some(2), Some(3)],
            lineages: [Some(owner), Some(owner)],
            kind: ContactKind::Transform,
            signed_normal_speed_mm_per_year: 0.0,
            tangent_speed_mm_per_year: 50.0,
            overlap_depth: 0,
        };
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let mut actions = ProcessActions::with_sample_capacity(state.samples.len());
        actions.begin_step(state.samples.len());
        let stats = relax_current_crust(
            &surface,
            &[subduction, transform],
            &mut state,
            &mut actions,
            recipe,
            2.0,
        )
        .unwrap();

        assert_eq!(state.samples[0].age_myr, 22.0);
        assert_eq!(state.samples[1].age_myr, 82.0);
        assert!(state.samples[0].tectonic_elevation_m <= -1_000.0);
        assert!(
            state.samples[1].tectonic_elevation_m > -9_000.0,
            "sediment must fill an active trench"
        );
        assert!(state.samples[2].tectonic_elevation_m < 5_000.0);
        assert!(state.samples[3].tectonic_elevation_m < 2_000.0);
        assert_eq!(state.samples[3].orogeny, SphericalOrogenyKind::None);
        assert_eq!(stats.relaxed_samples, 4);
    }
}
