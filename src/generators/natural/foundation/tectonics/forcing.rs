//! Present-day tectonic causes evaluated from the final contact geometry.
//!
//! This module intentionally does not mutate accumulated tectonic elevation.
//! It rebuilds a final contact set and publishes instantaneous, unit-bearing
//! cause fields for downstream physical relief construction.

use thiserror::Error;

use super::contacts::{build_contacts, ContactError, ContactEvent, ContactKind};
use super::model::{FormationTectonicRecipe, TectonicState};
use super::processes::{constants, event_distance_m};
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{
    EvolvedTectonicValidationError, SphericalTectonicForcingState,
    MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR, NO_OROGENY_AGE_SENTINEL_MYR,
};
use crate::world::spatial::{central_angle, SphericalSurfaceSnapshot, UnitVector3};
use crate::world::{CellId, EdgeId};

const DIVERGENT_FORCING_MAX_DISTANCE_M: f64 = 800_000.0;
const BASE_DIVERGENT_SUBSIDENCE_MM_PER_YEAR: f64 = 0.25;

pub(super) fn evaluate_present_day_forcing(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    state: &TectonicState,
    recipe: FormationTectonicRecipe,
    step_duration_myr: f64,
) -> Result<SphericalTectonicForcingState, ForcingError> {
    let mut coverage = super::contacts::CoverageScratch::with_cell_capacity(surface.cells().len());
    let mut events = Vec::new();
    build_contacts(surface, topology, state, &mut coverage, &mut events)?;
    evaluate_contact_forcing(surface, state, recipe, &events, step_duration_myr)
}

fn evaluate_contact_forcing(
    surface: &SphericalSurfaceSnapshot,
    state: &TectonicState,
    recipe: FormationTectonicRecipe,
    events: &[ContactEvent],
    step_duration_myr: f64,
) -> Result<SphericalTectonicForcingState, ForcingError> {
    if !step_duration_myr.is_finite() || step_duration_myr <= 0.0 {
        return Err(ForcingError::InvalidStepDuration {
            found: step_duration_myr,
        });
    }
    let metres_per_step_to_mm_per_year = 1.0 / (step_duration_myr * 1_000.0);
    let cell_count = surface.cells().len();
    let mut sample_by_cell = vec![None; cell_count];
    let mut event_age_myr = vec![NO_OROGENY_AGE_SENTINEL_MYR; cell_count];
    for (sample_index, sample) in state.samples.iter().enumerate() {
        let cell_index = sample.anchor.raw() as usize;
        if cell_index >= cell_count {
            return Err(ForcingError::InvalidAnchor {
                sample: sample_index,
                anchor: sample.anchor,
                cells: cell_count,
            });
        }
        if sample_by_cell[cell_index].replace(sample_index).is_some() {
            return Err(ForcingError::DuplicateAnchor {
                cell: sample.anchor,
            });
        }
        if sample.orogeny_age_myr >= 0.0 {
            event_age_myr[cell_index] = sample.orogeny_age_myr;
        }
    }
    if let Some((cell, _)) = sample_by_cell
        .iter()
        .enumerate()
        .find(|(_, sample)| sample.is_none())
    {
        return Err(ForcingError::MissingAnchor {
            cell: CellId::from_raw(cell as u32),
        });
    }

    let mut uplift = vec![0.0_f32; cell_count];
    let mut subsidence = vec![0.0_f32; cell_count];
    let mut shortening = vec![0.0_f32; cell_count];
    let mut direct_transform = vec![false; cell_count];
    let mut direct_convergence = vec![false; cell_count];
    let mut boundary_distance =
        vec![(std::f64::consts::PI * surface.radius().get()) as f32; cell_count];
    let gain = f64::from(recipe.subduction_gain_permille) / 1_000.0;

    for event in events {
        if event.kind == ContactKind::Gap {
            continue;
        }
        let reference = event_reference(surface, event)?;
        for cell in surface.cells() {
            let distance = central_angle(cell.centroid, reference) * surface.radius().get();
            let slot = cell.id.raw() as usize;
            boundary_distance[slot] = boundary_distance[slot].min(distance as f32);
        }

        for sample in event.sample_indices.iter().flatten() {
            let sample = state.samples.get(*sample as usize).ok_or(
                ForcingError::ContactSampleOutOfBounds {
                    sample: *sample as usize,
                    samples: state.samples.len(),
                },
            )?;
            event_age_myr[sample.anchor.raw() as usize] = 0.0;
            match event.kind {
                ContactKind::Transform => direct_transform[sample.anchor.raw() as usize] = true,
                ContactKind::OceanicSubduction { .. } | ContactKind::ContinentalCollision => {
                    direct_convergence[sample.anchor.raw() as usize] = true;
                }
                ContactKind::Gap | ContactKind::Divergence => {}
            }
        }

        match event.kind {
            ContactKind::Gap | ContactKind::Transform => {}
            ContactKind::OceanicSubduction { descending } => {
                let overriding = event
                    .lineages
                    .iter()
                    .flatten()
                    .copied()
                    .find(|lineage| *lineage != descending)
                    .ok_or(ForcingError::MissingOverridingSide)?;
                let speed = super::processes::event_speed(event);
                for (sample_index, sample) in state.samples.iter().enumerate() {
                    if sample.owner != descending && sample.owner != overriding {
                        continue;
                    }
                    let direct = event
                        .sample_indices
                        .iter()
                        .flatten()
                        .any(|index| *index as usize == sample_index);
                    let distance = forcing_profile_distance(
                        event_distance_m(surface, event, sample.position)
                            .map_err(|error| ForcingError::Geometry(error.to_string()))?,
                        direct,
                    );
                    let (trench_step_m, uplift_step_m) = super::processes::subduction_profile(
                        distance,
                        speed,
                        gain,
                        step_duration_myr,
                    );
                    let cell = sample.anchor.raw() as usize;
                    if sample.owner == descending
                        && sample.material.oceanic_reference_area_m2() > 0.0
                    {
                        let rate = checked_forcing_rate(
                            sample.anchor,
                            "subsidence_rate_mm_per_year",
                            -f64::from(trench_step_m) * metres_per_step_to_mm_per_year,
                        )?;
                        if rate > 0.0 {
                            subsidence[cell] = subsidence[cell].max(rate);
                            event_age_myr[cell] = 0.0;
                        }
                    } else if sample.owner == overriding {
                        let rate = checked_forcing_rate(
                            sample.anchor,
                            "uplift_rate_mm_per_year",
                            f64::from(uplift_step_m) * metres_per_step_to_mm_per_year,
                        )?;
                        if rate > 0.0 {
                            uplift[cell] = uplift[cell].max(rate);
                            event_age_myr[cell] = 0.0;
                        }
                    }
                }
            }
            ContactKind::ContinentalCollision => {
                let lineages = event_lineages(event)?;
                let normal_speed = f64::from(event.signed_normal_speed_mm_per_year).abs();
                let speed_weight =
                    (normal_speed / constants::REFERENCE_PLATE_SPEED_MM_PER_YEAR).clamp(0.0, 1.2);
                for sample in &state.samples {
                    if !lineages.contains(&sample.owner) {
                        continue;
                    }
                    let distance = event_distance_m(surface, event, sample.position)
                        .map_err(|error| ForcingError::Geometry(error.to_string()))?;
                    let taper = compact_taper(distance, constants::COLLISION_MAX_DISTANCE_M);
                    if taper <= 0.0 {
                        continue;
                    }
                    let cell = sample.anchor.raw() as usize;
                    let shortening_rate = checked_forcing_rate(
                        sample.anchor,
                        "shortening_rate_mm_per_year",
                        normal_speed * taper,
                    )?;
                    shortening[cell] = shortening[cell].max(shortening_rate);
                    let uplift_rate = checked_forcing_rate(
                        sample.anchor,
                        "uplift_rate_mm_per_year",
                        constants::BASE_SUBDUCTION_UPLIFT_MM_PER_YEAR * speed_weight * gain * taper,
                    )?;
                    uplift[cell] = uplift[cell].max(uplift_rate);
                    event_age_myr[cell] = 0.0;
                }
            }
            ContactKind::Divergence => {
                let lineages = event_lineages(event)?;
                let speed = f64::from(event.signed_normal_speed_mm_per_year).max(0.0);
                let speed_weight =
                    (speed / constants::REFERENCE_PLATE_SPEED_MM_PER_YEAR).clamp(0.0, 1.2);
                for (sample_index, sample) in state.samples.iter().enumerate() {
                    if !lineages.contains(&sample.owner)
                        || sample.material.continental_reference_area_m2() == 0.0
                    {
                        continue;
                    }
                    let direct = event
                        .sample_indices
                        .iter()
                        .flatten()
                        .any(|index| *index as usize == sample_index);
                    let distance = event_distance_m(surface, event, sample.position)
                        .map_err(|error| ForcingError::Geometry(error.to_string()))?;
                    let taper = if direct {
                        compact_taper(
                            distance.min(DIVERGENT_FORCING_MAX_DISTANCE_M * 0.5),
                            DIVERGENT_FORCING_MAX_DISTANCE_M,
                        )
                    } else {
                        compact_taper(distance, DIVERGENT_FORCING_MAX_DISTANCE_M)
                    };
                    if taper <= 0.0 {
                        continue;
                    }
                    let cell = sample.anchor.raw() as usize;
                    subsidence[cell] = subsidence[cell]
                        .max((BASE_DIVERGENT_SUBSIDENCE_MM_PER_YEAR * speed_weight * taper) as f32);
                    event_age_myr[cell] = 0.0;
                }
            }
        }
    }

    // A transform segment contributes no normal forcing of its own. Keep a
    // genuine junction's convergent signal, but do not let broad neighbouring
    // convergent kernels paint an otherwise pure transform boundary as an
    // active mountain belt.
    for cell in 0..cell_count {
        if direct_transform[cell] && !direct_convergence[cell] {
            uplift[cell] = 0.0;
            subsidence[cell] = 0.0;
            shortening[cell] = 0.0;
        }
    }

    SphericalTectonicForcingState::new(
        uplift,
        subsidence,
        shortening,
        boundary_distance,
        event_age_myr,
    )
    .map_err(Into::into)
}

fn event_reference(
    surface: &SphericalSurfaceSnapshot,
    event: &ContactEvent,
) -> Result<UnitVector3, ForcingError> {
    if let Some(edge) = event.edge {
        return surface
            .edge(edge)
            .map(|edge| edge.midpoint)
            .ok_or(ForcingError::UnknownEdge { edge });
    }
    surface
        .cell(event.cell)
        .map(|cell| cell.centroid)
        .ok_or(ForcingError::UnknownCell { cell: event.cell })
}

fn event_lineages(event: &ContactEvent) -> Result<[super::model::LineageId; 2], ForcingError> {
    let [Some(first), Some(second)] = event.lineages else {
        return Err(ForcingError::MissingLineages);
    };
    Ok([first, second])
}

fn checked_forcing_rate(
    cell: CellId,
    field: &'static str,
    exact: f64,
) -> Result<f32, ForcingError> {
    let maximum = f64::from(MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR);
    if !exact.is_finite() || !(0.0..=maximum).contains(&exact) {
        return Err(EvolvedTectonicValidationError::ForcingRateOutOfRange {
            field,
            cell,
            found: exact,
            max: maximum,
        }
        .into());
    }
    Ok(exact as f32)
}

fn forcing_profile_distance(distance_m: f64, direct: bool) -> f64 {
    if direct {
        distance_m.max(constants::SUBDUCTION_PEAK_DISTANCE_M * 0.08)
    } else {
        distance_m
    }
}

fn compact_taper(distance_m: f64, maximum_distance_m: f64) -> f64 {
    if !distance_m.is_finite()
        || !maximum_distance_m.is_finite()
        || distance_m < 0.0
        || maximum_distance_m <= 0.0
        || distance_m >= maximum_distance_m
    {
        return 0.0;
    }
    let value = (distance_m / maximum_distance_m).clamp(0.0, 1.0);
    1.0 - value * value * (3.0 - 2.0 * value)
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum ForcingError {
    #[error("final contact classification failed: {0}")]
    Contacts(#[from] ContactError),
    #[error("present-day forcing contract failed: {0}")]
    Invalid(#[from] EvolvedTectonicValidationError),
    #[error("formation step duration must be finite and positive, got {found} Myr")]
    InvalidStepDuration { found: f64 },
    #[error("sample {sample} anchor {anchor:?} is outside {cells} cells")]
    InvalidAnchor {
        sample: usize,
        anchor: CellId,
        cells: usize,
    },
    #[error("final forcing state has multiple samples at {cell:?}")]
    DuplicateAnchor { cell: CellId },
    #[error("final forcing state has no sample at {cell:?}")]
    MissingAnchor { cell: CellId },
    #[error("contact sample {sample} is outside {samples} samples")]
    ContactSampleOutOfBounds { sample: usize, samples: usize },
    #[error("active contact references unknown edge {edge:?}")]
    UnknownEdge { edge: EdgeId },
    #[error("active contact references unknown cell {cell:?}")]
    UnknownCell { cell: CellId },
    #[error("active contact is missing its two lineages")]
    MissingLineages,
    #[error("subduction contact is missing its overriding side")]
    MissingOverridingSide,
    #[error("forcing geometry failed: {0}")]
    Geometry(String),
}

#[cfg(test)]
mod tests {
    use super::{checked_forcing_rate, evaluate_contact_forcing, ForcingError};
    use crate::generators::natural::foundation::tectonics::contacts::{ContactEvent, ContactKind};
    use crate::generators::natural::foundation::tectonics::model::{
        ActivePlate, CrustSample, FormationTectonicRecipe, LineageId, MaterialColumn, TectonicState,
    };
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, EvolvedTectonicValidationError, ResolvedFormationTimeline,
        ResolvedWorldFormationPreset, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR,
        NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::{central_angle, SphericalSurfaceSnapshot, UnitVector3};
    use crate::world::{CellId, Meters, SphericalSpaceSpec};

    fn fixture() -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        })
        .unwrap()
    }

    fn step_duration_myr() -> f64 {
        ResolvedFormationTimeline::sekai_reference().step_duration_myr()
    }

    fn state_for_edge(
        surface: &SphericalSurfaceSnapshot,
        first_kind: CrustKind,
        first_age: f32,
        second_kind: CrustKind,
        second_age: f32,
    ) -> TectonicState {
        let edge = &surface.edges()[0];
        let first = LineageId::from_raw(0);
        let second = LineageId::from_raw(1);
        let samples = surface
            .cells()
            .iter()
            .map(|cell| {
                let (owner, kind, age) = if cell.id == edge.cells[1] {
                    (second, second_kind, second_age)
                } else {
                    (first, first_kind, first_age)
                };
                let thickness_km = match kind {
                    CrustKind::Continental => 38.0,
                    CrustKind::Oceanic => 7.0,
                };
                CrustSample {
                    position: cell.site,
                    anchor: cell.id,
                    owner,
                    kind,
                    thickness_km,
                    age_myr: match kind {
                        CrustKind::Continental => CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
                        CrustKind::Oceanic => age,
                    },
                    tectonic_elevation_m: match kind {
                        CrustKind::Continental => 600.0,
                        CrustKind::Oceanic => -4_500.0,
                    },
                    lineation: [0.0; 2],
                    orogeny: SphericalOrogenyKind::None,
                    orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
                    material: MaterialColumn::pure(kind, cell.area.get(), thickness_km).unwrap(),
                }
            })
            .collect();
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 1_000).unwrap();
        TectonicState::new(
            samples,
            vec![
                ActivePlate::new(first, edge.cells[0], rotation),
                ActivePlate::new(second, edge.cells[1], rotation),
            ],
            2,
        )
        .unwrap()
    }

    fn event(
        surface: &SphericalSurfaceSnapshot,
        kind: ContactKind,
        normal_speed: f32,
    ) -> ContactEvent {
        let edge = &surface.edges()[0];
        ContactEvent {
            cell: edge.cells[0],
            edge: Some(edge.id),
            sample_indices: [Some(edge.cells[0].raw()), Some(edge.cells[1].raw())],
            lineages: [Some(LineageId::from_raw(0)), Some(LineageId::from_raw(1))],
            kind,
            signed_normal_speed_mm_per_year: normal_speed,
            tangent_speed_mm_per_year: 0.0,
            overlap_depth: 0,
        }
    }

    fn value(field: &[f32], cell: CellId) -> f32 {
        field[cell.raw() as usize]
    }

    #[test]
    fn authoritative_forcing_is_independent_of_legacy_elevation() {
        let surface = fixture();
        let state = state_for_edge(
            &surface,
            CrustKind::Oceanic,
            90.0,
            CrustKind::Continental,
            0.0,
        );
        let mut changed = state_for_edge(
            &surface,
            CrustKind::Oceanic,
            90.0,
            CrustKind::Continental,
            0.0,
        );
        for (index, sample) in changed.samples.iter_mut().enumerate() {
            sample.tectonic_elevation_m = -9_000.0 + index as f32;
        }
        let event = event(
            &surface,
            ContactKind::OceanicSubduction {
                descending: LineageId::from_raw(0),
            },
            -80.0,
        );
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);

        assert_eq!(
            evaluate_contact_forcing(
                &surface,
                &state,
                recipe,
                std::slice::from_ref(&event),
                step_duration_myr()
            )
            .unwrap(),
            evaluate_contact_forcing(
                &surface,
                &changed,
                recipe,
                std::slice::from_ref(&event),
                step_duration_myr(),
            )
            .unwrap(),
        );
    }

    #[test]
    fn forcing_support_domain_rejects_instead_of_clamping() {
        let cell = CellId::from_raw(7);
        let error = checked_forcing_rate(cell, "uplift_rate_mm_per_year", 500.25).unwrap_err();
        assert!(matches!(
            error,
            ForcingError::Invalid(EvolvedTectonicValidationError::ForcingRateOutOfRange {
                field: "uplift_rate_mm_per_year",
                cell: found_cell,
                found,
                max,
            }) if found_cell == cell
                && found.to_bits() == 500.25_f64.to_bits()
                && max.to_bits()
                    == f64::from(MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR).to_bits()
        ));
    }

    #[test]
    fn subduction_forcing_has_the_correct_descending_and_overriding_signs() {
        let surface = fixture();
        let edge = &surface.edges()[0];
        let state = state_for_edge(
            &surface,
            CrustKind::Oceanic,
            90.0,
            CrustKind::Continental,
            0.0,
        );
        let forcing = evaluate_contact_forcing(
            &surface,
            &state,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
            &[event(
                &surface,
                ContactKind::OceanicSubduction {
                    descending: LineageId::from_raw(0),
                },
                -80.0,
            )],
            step_duration_myr(),
        )
        .unwrap();

        assert!(value(forcing.subsidence_rate_mm_per_year(), edge.cells[0]) > 0.0);
        assert_eq!(value(forcing.uplift_rate_mm_per_year(), edge.cells[0]), 0.0);
        assert!(value(forcing.uplift_rate_mm_per_year(), edge.cells[1]) > 0.0);
        assert_eq!(
            value(forcing.subsidence_rate_mm_per_year(), edge.cells[1]),
            0.0
        );
    }

    #[test]
    fn older_ocean_descends_beneath_younger_ocean() {
        let surface = fixture();
        let edge = &surface.edges()[0];
        let state = state_for_edge(
            &surface,
            CrustKind::Oceanic,
            120.0,
            CrustKind::Oceanic,
            15.0,
        );
        let forcing = evaluate_contact_forcing(
            &surface,
            &state,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
            &[event(
                &surface,
                ContactKind::OceanicSubduction {
                    descending: LineageId::from_raw(0),
                },
                -80.0,
            )],
            step_duration_myr(),
        )
        .unwrap();

        assert!(value(forcing.subsidence_rate_mm_per_year(), edge.cells[0]) > 0.0);
        assert!(value(forcing.uplift_rate_mm_per_year(), edge.cells[1]) > 0.0);
    }

    #[test]
    fn active_collision_shortens_and_uplifts_both_sides_without_transfer() {
        let surface = fixture();
        let edge = &surface.edges()[0];
        let state = state_for_edge(
            &surface,
            CrustKind::Continental,
            0.0,
            CrustKind::Continental,
            0.0,
        );
        let forcing = evaluate_contact_forcing(
            &surface,
            &state,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
            &[event(&surface, ContactKind::ContinentalCollision, -55.0)],
            step_duration_myr(),
        )
        .unwrap();

        for cell in edge.cells {
            assert!(value(forcing.shortening_rate_mm_per_year(), cell) > 0.0);
            assert!(value(forcing.uplift_rate_mm_per_year(), cell) > 0.0);
        }
    }

    #[test]
    fn divergence_subsides_continental_participants_without_convergent_uplift() {
        let surface = fixture();
        let edge = &surface.edges()[0];
        let state = state_for_edge(
            &surface,
            CrustKind::Continental,
            0.0,
            CrustKind::Continental,
            0.0,
        );
        let forcing = evaluate_contact_forcing(
            &surface,
            &state,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
            &[event(&surface, ContactKind::Divergence, 40.0)],
            step_duration_myr(),
        )
        .unwrap();

        for cell in edge.cells {
            assert!(value(forcing.subsidence_rate_mm_per_year(), cell) > 0.0);
            assert_eq!(value(forcing.uplift_rate_mm_per_year(), cell), 0.0);
            assert_eq!(value(forcing.shortening_rate_mm_per_year(), cell), 0.0);
        }
    }

    #[test]
    fn transform_is_zero_but_is_a_present_day_boundary_event() {
        let surface = fixture();
        let edge = &surface.edges()[0];
        let state = state_for_edge(
            &surface,
            CrustKind::Continental,
            0.0,
            CrustKind::Continental,
            0.0,
        );
        let forcing = evaluate_contact_forcing(
            &surface,
            &state,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
            &[event(&surface, ContactKind::Transform, 0.0)],
            step_duration_myr(),
        )
        .unwrap();

        for field in [
            forcing.uplift_rate_mm_per_year(),
            forcing.subsidence_rate_mm_per_year(),
            forcing.shortening_rate_mm_per_year(),
        ] {
            assert!(field.iter().all(|value| *value == 0.0));
        }
        for cell in edge.cells {
            assert_eq!(value(forcing.event_age_myr(), cell), 0.0);
        }
    }

    #[test]
    fn distance_is_minimum_great_circle_distance_and_age_prefers_active_over_inherited() {
        let surface = fixture();
        let edge = &surface.edges()[0];
        let mut state = state_for_edge(
            &surface,
            CrustKind::Continental,
            0.0,
            CrustKind::Continental,
            0.0,
        );
        let inherited = surface
            .cells()
            .iter()
            .find(|cell| !edge.cells.contains(&cell.id))
            .unwrap()
            .id;
        state.samples[inherited.raw() as usize].orogeny = SphericalOrogenyKind::Andean;
        state.samples[inherited.raw() as usize].orogeny_age_myr = 12.0;
        let forcing = evaluate_contact_forcing(
            &surface,
            &state,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
            &[event(&surface, ContactKind::Transform, 0.0)],
            step_duration_myr(),
        )
        .unwrap();

        let expected = central_angle(surface.cell(inherited).unwrap().centroid, edge.midpoint)
            * surface.radius().get();
        assert!(
            (f64::from(value(forcing.boundary_distance_m(), inherited)) - expected).abs() < 1.0
        );
        assert_eq!(value(forcing.event_age_myr(), inherited), 12.0);
        for cell in edge.cells {
            assert_eq!(value(forcing.event_age_myr(), cell), 0.0);
        }
    }
}
