//! Conservative control-to-authority publication for evolved tectonics.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use thiserror::Error;

use super::boundaries::classify_and_aggregate_boundaries;
#[cfg(test)]
use super::forcing::evaluate_present_day_forcing;
#[cfg(test)]
use super::model::FormationTectonicRecipe;
use super::model::{
    CrustSample, EvolutionLineageLedger, EvolutionMaterialLedger, LineageId, TectonicState,
};
#[cfg(test)]
use super::runner::evolve_control_state_v5_with_resample_observer;
use super::runner::{evolve_control_state_v5, RunnerError};
use crate::engine::{BuildCancellationError, StageRng};
use crate::generators::natural::random::LabeledSubstreams;
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::generators::spatial::{
    remap_categories_u16, remap_extensive_f64, remap_intensive_f32, remap_tangent_components_f64,
    ConservativeRemapError, ProfileSurfaceBundle,
};
use crate::world::natural::{
    BoundaryKind, CrustKind, CrustKindField, EvolvedTectonicSnapshot,
    EvolvedTectonicValidationError, PlateIdField, ResolvedWorldFormation,
    SphericalCrustMaterialState, SphericalCrustState, SphericalOrogenyKind, SphericalPlate,
    SphericalTectonicForcingState, SphericalTectonicMaterialBudget, SphericalTectonicSnapshot,
    SphericalTectonicValidationError, TectonicSpec, CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
    EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1, MAX_CRUST_AGE_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    TECTONIC_SNAPSHOT_SCHEMA_V3,
};
use crate::world::spatial::{
    ConservativeSurfaceMap, SphericalNaturalSurface, SphericalSurfaceSnapshot, SurfaceRef,
    SurfaceRefError,
};
use crate::world::{CellId, PlateId};

pub(in crate::generators::natural) fn generate_evolved_spherical(
    bundle: &ProfileSurfaceBundle,
    spec: &TectonicSpec,
    formation: &ResolvedWorldFormation,
    rng: &mut StageRng,
) -> Result<EvolvedTectonicSnapshot, EvolvedPublicationError> {
    rng.check_cancelled()?;
    let control = bundle.tectonic_control_surface();
    let authority = bundle.authoritative_surface();
    let control_view = SphericalNaturalSurface::from_validated(control)?;
    let authority_view = SphericalNaturalSurface::from_validated(authority)?;
    let control_topology = NaturalTopologyIndex::from_surface(&control_view);
    let authority_topology = NaturalTopologyIndex::from_surface(&authority_view);
    let streams = LabeledSubstreams::capture(rng);
    streams.check_cancelled()?;
    let evolved = evolve_control_state_v5(control, &control_topology, spec, formation, &streams)?;
    publish_evolved_control_state(
        bundle,
        &authority_topology,
        &streams,
        &evolved.current,
        &evolved.forcing,
        &evolved.material_ledger,
        &evolved.lineage_ledger,
    )
}

#[cfg(test)]
pub(in crate::generators::natural) fn generate_evolved_spherical_with_test_resample_observer(
    bundle: &ProfileSurfaceBundle,
    spec: &TectonicSpec,
    formation: &ResolvedWorldFormation,
    rng: &mut StageRng,
    mut on_resampled: impl FnMut(u16, &EvolvedTectonicSnapshot) -> Result<(), EvolvedPublicationError>,
) -> Result<EvolvedTectonicSnapshot, EvolvedPublicationError> {
    rng.check_cancelled()?;
    let control = bundle.tectonic_control_surface();
    let authority = bundle.authoritative_surface();
    let control_view = SphericalNaturalSurface::from_validated(control)?;
    let authority_view = SphericalNaturalSurface::from_validated(authority)?;
    let control_topology = NaturalTopologyIndex::from_surface(&control_view);
    let authority_topology = NaturalTopologyIndex::from_surface(&authority_view);
    let streams = LabeledSubstreams::capture(rng);
    let recipe = FormationTectonicRecipe::for_preset(formation.resolved());
    let delta_myr = formation.timeline().step_duration_myr();
    streams.check_cancelled()?;
    let evolved = evolve_control_state_v5_with_resample_observer(
        control,
        &control_topology,
        spec,
        formation,
        &streams,
        |accepted_steps, current, material_ledger, lineage_ledger| {
            let forcing = evaluate_present_day_forcing(
                control,
                &control_topology,
                current,
                recipe,
                delta_myr,
            )?;
            let snapshot = publish_evolved_control_state(
                bundle,
                &authority_topology,
                &streams,
                current,
                &forcing,
                material_ledger,
                lineage_ledger,
            )
            .map_err(|error| RunnerError::TestObserver {
                message: error.to_string(),
            })?;
            on_resampled(accepted_steps, &snapshot).map_err(|error| RunnerError::TestObserver {
                message: error.to_string(),
            })
        },
    )?;
    publish_evolved_control_state(
        bundle,
        &authority_topology,
        &streams,
        &evolved.current,
        &evolved.forcing,
        &evolved.material_ledger,
        &evolved.lineage_ledger,
    )
}

fn publish_evolved_control_state(
    bundle: &ProfileSurfaceBundle,
    authority_topology: &NaturalTopologyIndex,
    streams: &LabeledSubstreams,
    current: &TectonicState,
    forcing_state: &SphericalTectonicForcingState,
    material_ledger: &EvolutionMaterialLedger,
    lineage_ledger: &EvolutionLineageLedger,
) -> Result<EvolvedTectonicSnapshot, EvolvedPublicationError> {
    let control = bundle.tectonic_control_surface();
    let authority = bundle.authoritative_surface();
    let map = bundle.control_to_authoritative_map();
    let dense = dense_control_samples(control, &current.samples)?;

    let continental_area = remap_extensive_f64(
        map,
        &dense
            .iter()
            .map(|sample| sample.material.continental_reference_area_m2())
            .collect::<Vec<_>>(),
    )?;
    streams.check_cancelled()?;
    let continental_volume = remap_extensive_f64(
        map,
        &dense
            .iter()
            .map(|sample| sample.material.continental_volume_m3())
            .collect::<Vec<_>>(),
    )?;
    let oceanic_area = remap_extensive_f64(
        map,
        &dense
            .iter()
            .map(|sample| sample.material.oceanic_reference_area_m2())
            .collect::<Vec<_>>(),
    )?;
    let oceanic_volume = remap_extensive_f64(
        map,
        &dense
            .iter()
            .map(|sample| sample.material.oceanic_volume_m3())
            .collect::<Vec<_>>(),
    )?;
    streams.check_cancelled()?;
    let material = SphericalCrustMaterialState::new(
        continental_area.values().to_vec(),
        continental_volume.values().to_vec(),
        oceanic_area.values().to_vec(),
        oceanic_volume.values().to_vec(),
    )?;

    let owner_source = dense
        .iter()
        .map(|sample| {
            u16::try_from(sample.owner.raw()).map_err(|_| {
                EvolvedPublicationError::LineageCategoryOverflow {
                    lineage: sample.owner.raw(),
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let owner_categories = remap_categories_u16(map, &owner_source)?;
    let provisional_owners = owner_categories
        .values()
        .iter()
        .map(|owner| u32::from(*owner))
        .collect::<Vec<_>>();
    let live_lineages = current
        .plates
        .iter()
        .map(|plate| plate.lineage.raw())
        .collect::<Vec<_>>();
    let repaired = repair_connected_owners(
        map,
        authority_topology,
        &dense,
        &provisional_owners,
        &live_lineages,
    )?;
    streams.check_cancelled()?;

    let mut lineage_to_plate = BTreeMap::new();
    let mut plates = Vec::with_capacity(live_lineages.len());
    for (index, &raw_lineage) in live_lineages.iter().enumerate() {
        let lineage = LineageId::from_raw(raw_lineage);
        let source_plate =
            current
                .plate(lineage)
                .ok_or(EvolvedPublicationError::MissingLineageRotation {
                    lineage: raw_lineage,
                })?;
        let plate = PlateId::from_raw(index as u32);
        let marker = *repaired.markers.get(&raw_lineage).ok_or(
            EvolvedPublicationError::MissingLineageMarker {
                lineage: raw_lineage,
            },
        )?;
        lineage_to_plate.insert(raw_lineage, plate);
        plates.push(SphericalPlate::new(plate, marker, source_plate.rotation));
    }
    let cell_plates = PlateIdField::from_ids(
        repaired
            .owners
            .iter()
            .map(|owner| {
                lineage_to_plate
                    .get(owner)
                    .copied()
                    .ok_or(EvolvedPublicationError::UnknownAuthorityLineage { lineage: *owner })
            })
            .collect::<Result<Vec<_>, _>>()?,
    );

    let crust_categories = remap_categories_u16(
        map,
        &dense
            .iter()
            .map(|sample| sample.kind.raw() as u16)
            .collect::<Vec<_>>(),
    )?;
    let orogeny_categories = remap_categories_u16(
        map,
        &dense
            .iter()
            .map(|sample| orogeny_raw(sample.orogeny))
            .collect::<Vec<_>>(),
    )?;
    let kinds = (0..material.len())
        .map(|index| {
            material
                .compatibility_kind(index)
                .expect("validated material cells always have a dominant component")
        })
        .collect::<Vec<_>>();
    let thickness_km = (0..material.len())
        .map(|index| {
            material
                .compatibility_thickness_km(index)
                .expect("validated material cells always have a dominant thickness")
        })
        .collect::<Vec<_>>();
    let age_myr = remap_ocean_age(map, &dense, &kinds);
    let tectonic_elevation_m = remap_intensive_f32(
        map,
        &dense
            .iter()
            .map(|sample| sample.tectonic_elevation_m)
            .collect::<Vec<_>>(),
    )?;
    let lineation = remap_tangent_components_f64(
        map,
        &dense
            .iter()
            .map(|sample| {
                [
                    f64::from(sample.lineation[0]),
                    f64::from(sample.lineation[1]),
                ]
            })
            .collect::<Vec<_>>(),
    )?
    .into_iter()
    .map(normalize_lineation)
    .collect::<Vec<_>>();
    let orogeny_kind = orogeny_categories
        .values()
        .iter()
        .map(|value| orogeny_from_raw(*value))
        .collect::<Result<Vec<_>, _>>()?;
    let mut orogeny_age_myr = remap_masked_intensive(
        map,
        &dense
            .iter()
            .map(|sample| sample.orogeny_age_myr)
            .collect::<Vec<_>>(),
        &dense
            .iter()
            .map(|sample| sample.orogeny != SphericalOrogenyKind::None)
            .collect::<Vec<_>>(),
        NO_OROGENY_AGE_SENTINEL_MYR,
    );
    for (kind, age) in orogeny_kind.iter().zip(&mut orogeny_age_myr) {
        if *kind == SphericalOrogenyKind::None {
            *age = NO_OROGENY_AGE_SENTINEL_MYR;
        } else if *age < 0.0 {
            *age = 0.0;
        }
    }
    let crust = SphericalCrustState::new(
        CrustKindField::from_kinds(kinds),
        thickness_km,
        age_myr,
        tectonic_elevation_m,
        lineation.iter().map(|value| value[0]).collect(),
        lineation.iter().map(|value| value[1]).collect(),
        orogeny_kind,
        orogeny_age_myr,
    )?;

    let (boundaries, boundary_segments) = classify_and_aggregate_boundaries(
        authority,
        authority_topology,
        &plates,
        &cell_plates,
        &crust,
    )?;
    let compatibility = SphericalTectonicSnapshot::new(
        TECTONIC_SNAPSHOT_SCHEMA_V3,
        SurfaceRef::try_for_spherical(authority)?,
        plates,
        cell_plates,
        crust,
        boundaries,
        boundary_segments,
    )?;
    compatibility.validate_against(authority)?;
    streams.check_cancelled()?;

    let mut uplift = remap_intensive_f32(map, forcing_state.uplift_rate_mm_per_year())?;
    let mut subsidence = remap_intensive_f32(map, forcing_state.subsidence_rate_mm_per_year())?;
    let mut shortening = remap_intensive_f32(map, forcing_state.shortening_rate_mm_per_year())?;
    let boundary_distance = remap_intensive_f32(map, forcing_state.boundary_distance_m())?;
    let mut event_age = remap_masked_intensive(
        map,
        forcing_state.event_age_myr(),
        &forcing_state
            .event_age_myr()
            .iter()
            .map(|age| *age >= 0.0)
            .collect::<Vec<_>>(),
        NO_OROGENY_AGE_SENTINEL_MYR,
    );
    enforce_authority_boundary_forcing(
        authority,
        &compatibility,
        &mut uplift,
        &mut subsidence,
        &mut shortening,
        &mut event_age,
    );
    let forcing = SphericalTectonicForcingState::new(
        uplift,
        subsidence,
        shortening,
        boundary_distance,
        event_age,
    )?;

    let final_control = current.material_totals()?;
    let material_budget = SphericalTectonicMaterialBudget::new(
        material_ledger.initial_control(),
        material_ledger.processes()?,
        final_control,
        material.totals(),
        authority
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .fold(0.0_f64, f64::max),
        owner_categories
            .ambiguous_target_area_fraction()
            .max(crust_categories.ambiguous_target_area_fraction())
            .max(orogeny_categories.ambiguous_target_area_fraction()),
    )?;
    let lineage_budget = lineage_ledger.budget(current)?;
    let snapshot = EvolvedTectonicSnapshot::new(
        EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1,
        bundle.resolution_plan().clone(),
        compatibility,
        material,
        forcing,
        material_budget,
        lineage_budget,
    )?;
    snapshot.validate_against(authority)?;
    streams.check_cancelled()?;
    Ok(snapshot)
}

fn dense_control_samples<'a>(
    control: &SphericalSurfaceSnapshot,
    samples: &'a [CrustSample],
) -> Result<Vec<&'a CrustSample>, EvolvedPublicationError> {
    let mut dense = vec![None; control.cells().len()];
    for (sample_index, sample) in samples.iter().enumerate() {
        let index = sample.anchor.raw() as usize;
        let Some(slot) = dense.get_mut(index) else {
            return Err(EvolvedPublicationError::InvalidControlAnchor {
                sample: sample_index,
                anchor: sample.anchor,
            });
        };
        if slot.replace(sample).is_some() {
            return Err(EvolvedPublicationError::DuplicateControlAnchor {
                anchor: sample.anchor,
            });
        }
    }
    dense
        .into_iter()
        .enumerate()
        .map(|(index, sample)| {
            sample.ok_or(EvolvedPublicationError::MissingControlAnchor {
                anchor: CellId::from_raw(index as u32),
            })
        })
        .collect()
}

#[derive(Debug)]
struct RepairedOwners {
    owners: Vec<u32>,
    markers: BTreeMap<u32, CellId>,
}

fn repair_connected_owners(
    map: &ConservativeSurfaceMap,
    topology: &NaturalTopologyIndex,
    source: &[&CrustSample],
    provisional: &[u32],
    live_lineages: &[u32],
) -> Result<RepairedOwners, EvolvedPublicationError> {
    if provisional.len() != topology.cell_count() {
        return Err(EvolvedPublicationError::AuthorityCardinalityMismatch {
            owners: provisional.len(),
            cells: topology.cell_count(),
        });
    }
    let mut used = vec![false; provisional.len()];
    let mut markers = BTreeMap::new();
    for &lineage in live_lineages {
        let mut candidates = (0..provisional.len())
            .map(|target| {
                let overlap = map
                    .target_row(CellId::from_raw(target as u32))
                    .expect("validated maps contain every target row")
                    .iter()
                    .filter(|weight| {
                        source[weight.source_cell().raw() as usize].owner.raw() == lineage
                    })
                    .map(|weight| weight.area_m2())
                    .sum::<f64>();
                (target, overlap, provisional[target] == lineage)
            })
            .filter(|(_, overlap, _)| *overlap > 0.0)
            .collect::<Vec<_>>();
        candidates.sort_by(|first, second| {
            second
                .2
                .cmp(&first.2)
                .then_with(|| second.1.total_cmp(&first.1))
                .then_with(|| first.0.cmp(&second.0))
        });
        let target = candidates
            .into_iter()
            .map(|candidate| candidate.0)
            .find(|target| !used[*target])
            .ok_or(EvolvedPublicationError::NoAuthorityMarker { lineage })?;
        used[target] = true;
        markers.insert(lineage, CellId::from_raw(target as u32));
    }

    let mut costs = vec![u64::MAX; provisional.len()];
    let mut owners = vec![u32::MAX; provisional.len()];
    let mut pending = BinaryHeap::new();
    for (&lineage, &cell) in &markers {
        let index = cell.raw() as usize;
        costs[index] = 0;
        owners[index] = lineage;
        pending.push(Reverse((0_u64, lineage, cell.raw())));
    }
    let mismatch_penalty = topology.quantized_short_side_fraction(12.0).max(1);
    while let Some(Reverse((cost, lineage, raw_cell))) = pending.pop() {
        let cell = raw_cell as usize;
        if costs[cell] != cost || owners[cell] != lineage {
            continue;
        }
        for arc in &topology.arcs()[cell] {
            let neighbor = arc.neighbor.raw() as usize;
            let data_cost = if provisional[neighbor] == lineage {
                0
            } else {
                mismatch_penalty
            };
            let candidate = cost
                .saturating_add(arc.traversal_cost)
                .saturating_add(data_cost);
            if (candidate, lineage) < (costs[neighbor], owners[neighbor]) {
                costs[neighbor] = candidate;
                owners[neighbor] = lineage;
                pending.push(Reverse((candidate, lineage, arc.neighbor.raw())));
            }
        }
    }
    if let Some(cell) = owners.iter().position(|owner| *owner == u32::MAX) {
        return Err(EvolvedPublicationError::UnassignedAuthorityCell {
            cell: CellId::from_raw(cell as u32),
        });
    }
    Ok(RepairedOwners { owners, markers })
}

fn remap_ocean_age(
    map: &ConservativeSurfaceMap,
    source: &[&CrustSample],
    target_kinds: &[CrustKind],
) -> Vec<f32> {
    target_kinds
        .iter()
        .enumerate()
        .map(|(target, kind)| {
            if *kind == CrustKind::Continental {
                return CONTINENTAL_CRUST_AGE_SENTINEL_MYR;
            }
            let mut numerator = 0.0_f64;
            let mut denominator = 0.0_f64;
            for weight in map
                .target_row(CellId::from_raw(target as u32))
                .expect("validated maps contain every target row")
            {
                let index = weight.source_cell().raw() as usize;
                let sample = source[index];
                if sample.age_myr < 0.0 {
                    continue;
                }
                let ocean_fraction =
                    sample.material.oceanic_reference_area_m2() / map.source_cell_areas_m2()[index];
                let material_weight = weight.area_m2() * ocean_fraction;
                numerator += material_weight * f64::from(sample.age_myr);
                denominator += material_weight;
            }
            if denominator > 0.0 {
                (numerator / denominator).clamp(0.0, f64::from(MAX_CRUST_AGE_MYR)) as f32
            } else {
                0.0
            }
        })
        .collect()
}

fn remap_masked_intensive(
    map: &ConservativeSurfaceMap,
    source: &[f32],
    valid: &[bool],
    sentinel: f32,
) -> Vec<f32> {
    debug_assert_eq!(source.len(), valid.len());
    (0..map.target_ref().cell_count() as usize)
        .map(|target| {
            let mut numerator = 0.0_f64;
            let mut denominator = 0.0_f64;
            for weight in map
                .target_row(CellId::from_raw(target as u32))
                .expect("validated maps contain every target row")
            {
                let source_index = weight.source_cell().raw() as usize;
                if valid[source_index] {
                    numerator += weight.area_m2() * f64::from(source[source_index]);
                    denominator += weight.area_m2();
                }
            }
            if denominator > 0.0 {
                (numerator / denominator).clamp(0.0, f64::from(MAX_CRUST_AGE_MYR)) as f32
            } else {
                sentinel
            }
        })
        .collect()
}

fn normalize_lineation(value: [f64; 2]) -> [f32; 2] {
    let length = value[0].hypot(value[1]);
    if length <= 1.0e-12 {
        [0.0; 2]
    } else {
        [(value[0] / length) as f32, (value[1] / length) as f32]
    }
}

fn orogeny_raw(kind: SphericalOrogenyKind) -> u16 {
    match kind {
        SphericalOrogenyKind::None => 0,
        SphericalOrogenyKind::Andean => 1,
        SphericalOrogenyKind::Himalayan => 2,
    }
}

fn orogeny_from_raw(raw: u16) -> Result<SphericalOrogenyKind, EvolvedPublicationError> {
    match raw {
        0 => Ok(SphericalOrogenyKind::None),
        1 => Ok(SphericalOrogenyKind::Andean),
        2 => Ok(SphericalOrogenyKind::Himalayan),
        _ => Err(EvolvedPublicationError::InvalidOrogenyCategory { raw }),
    }
}

fn enforce_authority_boundary_forcing(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
    uplift: &mut [f32],
    subsidence: &mut [f32],
    shortening: &mut [f32],
    event_age: &mut [f32],
) {
    let mut transform = vec![false; surface.cells().len()];
    let mut convergence = vec![false; surface.cells().len()];
    for (edge, boundary) in surface.edges().iter().zip(snapshot.boundaries()) {
        match boundary.kind {
            BoundaryKind::Transform => {
                for cell in edge.cells {
                    transform[cell.raw() as usize] = true;
                    event_age[cell.raw() as usize] = 0.0;
                }
            }
            BoundaryKind::ContinentalCollision | BoundaryKind::Subduction => {
                for cell in edge.cells {
                    convergence[cell.raw() as usize] = true;
                    event_age[cell.raw() as usize] = 0.0;
                }
            }
            BoundaryKind::ContinentalRift | BoundaryKind::OceanicRidge => {
                for cell in edge.cells {
                    event_age[cell.raw() as usize] = 0.0;
                }
            }
            BoundaryKind::None | BoundaryKind::Weak => {}
        }
    }
    for cell in 0..transform.len() {
        if transform[cell] && !convergence[cell] {
            uplift[cell] = 0.0;
            subsidence[cell] = 0.0;
            shortening[cell] = 0.0;
        }
    }
}

#[derive(Debug, Error)]
pub(in crate::generators::natural) enum EvolvedPublicationError {
    #[error("evolved tectonic publication was cancelled")]
    Cancelled(#[from] BuildCancellationError),
    #[error("control evolution failed: {0}")]
    Runner(String),
    #[error("conservative field remapping failed: {0}")]
    Remap(#[from] ConservativeRemapError),
    #[error("surface identity failed: {0}")]
    SurfaceIdentity(#[from] SurfaceRefError),
    #[error("published compatibility snapshot failed: {0}")]
    Compatibility(#[from] SphericalTectonicValidationError),
    #[error("published evolved snapshot failed: {0}")]
    Evolved(#[from] EvolvedTectonicValidationError),
    #[error("material column failed: {0}")]
    Material(String),
    #[error("lineage ledger failed: {0}")]
    Lineage(String),
    #[error("control sample {sample} has invalid anchor {anchor:?}")]
    InvalidControlAnchor { sample: usize, anchor: CellId },
    #[error("control anchor {anchor:?} occurs more than once")]
    DuplicateControlAnchor { anchor: CellId },
    #[error("control anchor {anchor:?} has no sample")]
    MissingControlAnchor { anchor: CellId },
    #[error("lineage {lineage} does not fit the stable remap category encoding")]
    LineageCategoryOverflow { lineage: u32 },
    #[error("authority owner field has {owners} cells; topology has {cells}")]
    AuthorityCardinalityMismatch { owners: usize, cells: usize },
    #[error("lineage {lineage} has no unique authoritative marker")]
    NoAuthorityMarker { lineage: u32 },
    #[error("authority cell {cell:?} was not reached by connected owner reconstruction")]
    UnassignedAuthorityCell { cell: CellId },
    #[error("lineage {lineage} has no retained rotation")]
    MissingLineageRotation { lineage: u32 },
    #[error("lineage {lineage} has no authoritative marker")]
    MissingLineageMarker { lineage: u32 },
    #[error("authority owner references unknown lineage {lineage}")]
    UnknownAuthorityLineage { lineage: u32 },
    #[error("orogeny category {raw} is unsupported")]
    InvalidOrogenyCategory { raw: u16 },
}

impl From<RunnerError> for EvolvedPublicationError {
    fn from(error: RunnerError) -> Self {
        Self::Runner(error.to_string())
    }
}

impl From<super::model::MaterialColumnError> for EvolvedPublicationError {
    fn from(error: super::model::MaterialColumnError) -> Self {
        Self::Material(error.to_string())
    }
}

impl From<super::model::LineageLedgerError> for EvolvedPublicationError {
    fn from(error: super::model::LineageLedgerError) -> Self {
        Self::Lineage(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use crate::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
    use crate::generators::natural::spherical_tectonics::{
        generate_evolved_spherical, generate_evolved_spherical_with_test_resample_observer,
    };
    use crate::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
    use crate::world::natural::{
        NaturalQualityProfile, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
        WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
    };
    use crate::world::{Meters, RootSeed};

    #[test]
    fn test_only_resample_observer_preserves_the_monolithic_p2_product() {
        let bundle = bundle();
        let formation = formation();
        let mut monolithic_rng = rng(42);
        let monolithic = generate_evolved_spherical(
            bundle,
            &TectonicSpec::default(),
            &formation,
            &mut monolithic_rng,
        )
        .unwrap();

        let mut observed_boundaries = 0_u16;
        let mut final_accepted_step = 0_u16;
        let mut observed_rng = rng(42);
        let observed = generate_evolved_spherical_with_test_resample_observer(
            bundle,
            &TectonicSpec::default(),
            &formation,
            &mut observed_rng,
            |accepted_steps, _snapshot| {
                assert!(accepted_steps > final_accepted_step);
                observed_boundaries += 1;
                final_accepted_step = accepted_steps;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            serde_json::to_vec(&observed).unwrap(),
            serde_json::to_vec(&monolithic).unwrap(),
        );
        assert!(observed_boundaries > 0);
        assert!(observed_boundaries < formation.timeline().step_count());
        assert_eq!(final_accepted_step, formation.timeline().step_count());
    }

    fn bundle() -> &'static ProfileSurfaceBundle {
        static BUNDLE: OnceLock<ProfileSurfaceBundle> = OnceLock::new();
        BUNDLE.get_or_init(|| {
            ProfileSurfaceBuilder::build(
                NaturalQualityProfile::Draft,
                Meters::new(6_371_000.0).unwrap(),
                &BuildCancellation::new(),
            )
            .unwrap()
        })
    }

    fn formation() -> ResolvedWorldFormation {
        ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Continents,
        )
        .unwrap()
    }

    fn rng(seed: u64) -> StageRng {
        StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
        ))
    }
}
