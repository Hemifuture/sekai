//! PlaTec-style coherent initial lithosphere on the authoritative sphere.
//!
//! This is only the start state for the bounded Cortial evolution. Stable
//! farthest-point seeds define a perturbed spherical Voronoi partition, while
//! independent coherent fields initialize material, thickness and ocean age.
//! No connectivity trimming, region growth or final-land shaping occurs here.

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::VecDeque;

use rand::RngCore;
use thiserror::Error;

use super::model::{
    ActivePlate, CrustSample, FormationTectonicRecipe, LineageId, TectonicModelError, TectonicState,
};
use crate::generators::natural::fractal::FractalProfile;
use crate::generators::natural::morphology::noise::SphericalNoise3d;
use crate::generators::natural::random::{
    LabeledSubstreams, INITIAL_CRUST_V3_LABEL, INITIAL_PLATES_V3_LABEL, PLATE_MOTION_V3_LABEL,
};
use crate::generators::natural::topology::{farthest_point_seeds, NaturalTopologyIndex};
use crate::world::natural::{
    CrustKind, NaturalSpecError, SphericalOrogenyKind, SphericalPlateRotation,
    SphericalTectonicValidationError, TectonicActivity, TectonicSpec,
    CONTINENTAL_CRUST_AGE_SENTINEL_MYR, MAX_CRUST_AGE_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
};
use crate::world::spatial::{project_tangent, SphericalSurfaceSnapshot, UnitVector3};
use crate::world::CellId;

const MAXIMUM_SEED_WARP_RAD: f64 = 0.07;
const CONTINENTAL_THICKNESS_BASE_KM: f64 = 32.0;
const CONTINENTAL_THICKNESS_SPAN_KM: f64 = 24.0;
const OCEANIC_THICKNESS_BASE_KM: f64 = 5.0;
const OCEANIC_THICKNESS_SPAN_KM: f64 = 5.0;
const INITIAL_OCEANIC_AGE_MIN_MYR: f64 = 8.0;
const INITIAL_OCEANIC_AGE_SPAN_MYR: f64 = 172.0;

pub(super) fn build_initial_state(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
) -> Result<TectonicState, InitialStateError> {
    spec.validate()?;
    let cell_count = surface.cells().len();
    if cell_count != topology.cell_count() {
        return Err(InitialStateError::CardinalityMismatch {
            surface_cells: cell_count,
            topology_cells: topology.cell_count(),
        });
    }
    let plate_count = usize::from(spec.plate_count);
    if plate_count > cell_count {
        return Err(InitialStateError::PlateCountExceedsCells {
            plates: plate_count,
            cells: cell_count,
        });
    }

    let (seeds, centers) = initial_plate_centers(surface, topology, plate_count, recipe, streams)?;
    let owners = assign_nearest_centers(surface, &centers);
    validate_initial_partition(topology, &seeds, &owners)?;
    let plates = initial_plates(surface, spec.activity, &seeds, streams)?;
    let samples = initial_crust_samples(surface, spec, recipe, streams, &owners);
    TectonicState::new(samples, plates, spec.plate_count.into()).map_err(Into::into)
}

fn initial_plate_centers(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    plate_count: usize,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
) -> Result<(Vec<CellId>, Vec<UnitVector3>), InitialStateError> {
    let mut rng = streams.stream(INITIAL_PLATES_V3_LABEL);
    let seeds = farthest_point_seeds(topology, plate_count, rng.next_u64());
    let warp_noise = [
        SphericalNoise3d::new(rng.next_u32()),
        SphericalNoise3d::new(rng.next_u32()),
        SphericalNoise3d::new(rng.next_u32()),
    ];
    let warp_profile = FractalProfile {
        octaves: 2,
        frequency: recipe.base_scale_rad.recip(),
        lacunarity: 2.03,
        persistence: 0.5,
    };
    let maximum_warp = (recipe.base_scale_rad * 0.06).min(MAXIMUM_SEED_WARP_RAD);
    let centers = seeds
        .iter()
        .map(|&seed| {
            let radial = surface
                .cell(seed)
                .expect("farthest-point seeds are authoritative cells")
                .centroid;
            let variation = std::array::from_fn(|axis| warp_noise[axis].fbm(radial, warp_profile));
            perturb_direction(radial, variation, maximum_warp)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((seeds, centers))
}

fn perturb_direction(
    radial: UnitVector3,
    variation: [f64; 3],
    maximum_angle: f64,
) -> Result<UnitVector3, InitialStateError> {
    let tangent = project_tangent(variation, radial);
    let tangent_norm = norm(tangent);
    if tangent_norm <= f64::EPSILON {
        return Ok(radial);
    }
    let tangent_unit = tangent.map(|component| component / tangent_norm);
    let angle = maximum_angle * (tangent_norm / 3.0_f64.sqrt()).clamp(0.0, 1.0);
    let [x, y, z] = radial.components();
    UnitVector3::new(
        angle.cos() * x + angle.sin() * tangent_unit[0],
        angle.cos() * y + angle.sin() * tangent_unit[1],
        angle.cos() * z + angle.sin() * tangent_unit[2],
    )
    .map_err(|_| InitialStateError::InvalidWarpedDirection)
}

fn assign_nearest_centers(
    surface: &SphericalSurfaceSnapshot,
    centers: &[UnitVector3],
) -> Vec<LineageId> {
    surface
        .cells()
        .iter()
        .map(|cell| {
            let mut best_index = 0_usize;
            let mut best_dot = cell.centroid.dot(centers[0]);
            for (index, &center) in centers.iter().enumerate().skip(1) {
                let candidate = cell.centroid.dot(center);
                if candidate > best_dot {
                    best_dot = candidate;
                    best_index = index;
                }
            }
            LineageId::from_raw(best_index as u32)
        })
        .collect()
}

fn validate_initial_partition(
    topology: &NaturalTopologyIndex,
    seeds: &[CellId],
    owners: &[LineageId],
) -> Result<(), InitialStateError> {
    for (index, &seed) in seeds.iter().enumerate() {
        let lineage = LineageId::from_raw(index as u32);
        if owners[seed.raw() as usize] != lineage {
            return Err(InitialStateError::SeedOwnershipLost { lineage, seed });
        }
        let expected = owners.iter().filter(|&&owner| owner == lineage).count();
        let reached = connected_owner_count(topology, owners, lineage);
        if reached != expected {
            return Err(InitialStateError::DisconnectedInitialPlate {
                lineage,
                reached,
                expected,
            });
        }
    }
    Ok(())
}

fn connected_owner_count(
    topology: &NaturalTopologyIndex,
    owners: &[LineageId],
    owner: LineageId,
) -> usize {
    let Some(start) = owners.iter().position(|&candidate| candidate == owner) else {
        return 0;
    };
    let mut reached = vec![false; owners.len()];
    let mut queue = VecDeque::from([start]);
    reached[start] = true;
    let mut count = 0;
    while let Some(index) = queue.pop_front() {
        count += 1;
        for arc in &topology.arcs()[index] {
            let neighbor = arc.neighbor.raw() as usize;
            if !reached[neighbor] && owners[neighbor] == owner {
                reached[neighbor] = true;
                queue.push_back(neighbor);
            }
        }
    }
    count
}

fn initial_plates(
    surface: &SphericalSurfaceSnapshot,
    activity: TectonicActivity,
    seeds: &[CellId],
    streams: &LabeledSubstreams,
) -> Result<Vec<ActivePlate>, InitialStateError> {
    let mut rng = streams.stream(PLATE_MOTION_V3_LABEL);
    seeds
        .iter()
        .copied()
        .enumerate()
        .map(|(index, representative)| {
            let pole = random_unit_direction(&mut rng)?;
            let unit = unit_interval(rng.next_u64());
            let (minimum_speed, maximum_speed) = match activity {
                TectonicActivity::Quiet => (20.0, 50.0),
                TectonicActivity::Moderate => (40.0, 90.0),
                TectonicActivity::Active => (60.0, 120.0),
            };
            let speed_mm_per_year = minimum_speed + (maximum_speed - minimum_speed) * unit;
            let angular_rate =
                (speed_mm_per_year * 1.0e9_f64 / surface.radius().get()).round() as u64;
            let rotation = SphericalPlateRotation::new(pole, angular_rate.max(1))?;
            rotation.validate_for_radius(surface.radius())?;
            Ok(ActivePlate::new(
                LineageId::from_raw(index as u32),
                representative,
                rotation,
            ))
        })
        .collect()
}

fn initial_crust_samples(
    surface: &SphericalSurfaceSnapshot,
    spec: &TectonicSpec,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
    owners: &[LineageId],
) -> Vec<CrustSample> {
    let mut rng = streams.stream(INITIAL_CRUST_V3_LABEL);
    let crust_noise = SphericalNoise3d::new(rng.next_u32());
    let thickness_noise = SphericalNoise3d::new(rng.next_u32());
    let age_noise = SphericalNoise3d::new(rng.next_u32());
    let scores = surface
        .cells()
        .iter()
        .map(|cell| crust_noise.fbm(cell.centroid, recipe.initial_crust_profile))
        .collect::<Vec<_>>();
    let continental = continental_quantile(surface, &scores, spec.continental_crust_fraction);

    surface
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let thickness_signal =
                normalized_signal(thickness_noise.fbm(cell.centroid, recipe.initial_crust_profile));
            let age_signal =
                normalized_signal(age_noise.fbm(cell.centroid, recipe.initial_crust_profile));
            let (kind, thickness_km, age_myr, tectonic_elevation_m) = if continental[index] {
                (
                    CrustKind::Continental,
                    (CONTINENTAL_THICKNESS_BASE_KM
                        + CONTINENTAL_THICKNESS_SPAN_KM * thickness_signal)
                        as f32,
                    CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
                    (250.0 + 750.0 * normalized_signal(scores[index])) as f32,
                )
            } else {
                let age = INITIAL_OCEANIC_AGE_MIN_MYR + INITIAL_OCEANIC_AGE_SPAN_MYR * age_signal;
                (
                    CrustKind::Oceanic,
                    (OCEANIC_THICKNESS_BASE_KM + OCEANIC_THICKNESS_SPAN_KM * thickness_signal)
                        as f32,
                    age.min(f64::from(MAX_CRUST_AGE_MYR)) as f32,
                    (-2_800.0 - 2_200.0 * (age / 180.0) + 160.0 * (thickness_signal - 0.5)) as f32,
                )
            };
            CrustSample {
                position: cell.centroid,
                anchor: cell.id,
                owner: owners[index],
                kind,
                thickness_km,
                age_myr,
                tectonic_elevation_m,
                lineation: [0.0; 2],
                orogeny: SphericalOrogenyKind::None,
                orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            }
        })
        .collect()
}

fn continental_quantile(
    surface: &SphericalSurfaceSnapshot,
    scores: &[f64],
    target_fraction: f32,
) -> Vec<bool> {
    let mut order = (0..scores.len()).collect::<Vec<_>>();
    order.sort_by(|&first, &second| {
        scores[second]
            .total_cmp(&scores[first])
            .then_with(|| first.cmp(&second))
    });
    let target = surface.total_cell_area().get() * f64::from(target_fraction);
    let mut selected_count = 0;
    let mut selected_area = 0.0;
    for (rank, &index) in order.iter().enumerate() {
        let next_area = selected_area + surface.cells()[index].area.get();
        if (next_area - target).abs() <= (selected_area - target).abs() {
            selected_area = next_area;
            selected_count = rank + 1;
        } else {
            break;
        }
    }
    let mut selected = vec![false; scores.len()];
    for &index in order.iter().take(selected_count) {
        selected[index] = true;
    }
    selected
}

fn random_unit_direction(rng: &mut impl RngCore) -> Result<UnitVector3, InitialStateError> {
    let vertical = unit_interval(rng.next_u64()).mul_add(2.0, -1.0);
    let azimuth = std::f64::consts::TAU * unit_interval(rng.next_u64());
    let horizontal = (1.0 - vertical * vertical).max(0.0).sqrt();
    UnitVector3::new(
        horizontal * azimuth.cos(),
        horizontal * azimuth.sin(),
        vertical,
    )
    .map_err(|_| InitialStateError::InvalidRotationAxis)
}

fn unit_interval(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1_u64 << 53) as f64
}

fn normalized_signal(signal: f64) -> f64 {
    signal.mul_add(0.5, 0.5).clamp(0.0, 1.0)
}

fn norm(vector: [f64; 3]) -> f64 {
    vector
        .iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt()
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum InitialStateError {
    #[error("invalid tectonic specification: {0}")]
    InvalidSpec(#[from] NaturalSpecError),
    #[error(
        "initial tectonics surface has {surface_cells} cells but topology has {topology_cells}"
    )]
    CardinalityMismatch {
        surface_cells: usize,
        topology_cells: usize,
    },
    #[error("requested {plates} initial plates for only {cells} surface cells")]
    PlateCountExceedsCells { plates: usize, cells: usize },
    #[error("a coherent seed warp did not produce a finite spherical direction")]
    InvalidWarpedDirection,
    #[error("the plate-motion stream did not produce a finite rotation axis")]
    InvalidRotationAxis,
    #[error("invalid initial plate rotation: {0}")]
    InvalidRotation(#[from] SphericalTectonicValidationError),
    #[error("initial lineage {lineage:?} lost representative seed {seed:?}")]
    SeedOwnershipLost { lineage: LineageId, seed: CellId },
    #[error("initial lineage {lineage:?} reaches {reached} of {expected} owned cells")]
    DisconnectedInitialPlate {
        lineage: LineageId,
        reached: usize,
        expected: usize,
    },
    #[error("invalid transient tectonic state: {0}")]
    InvalidState(#[from] TectonicModelError),
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::build_initial_state;
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::spherical_tectonics::model::FormationTectonicRecipe;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, ResolvedWorldFormationPreset, SphericalOrogenyKind, TectonicSpec,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
        CONTINENTAL_CRUST_MIN_THICKNESS_KM, MAX_CRUST_AGE_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
        OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM,
    };
    use crate::world::spatial::SphericalNaturalSurface;
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    fn fixture(
        cells: u32,
    ) -> (
        crate::world::spatial::SphericalSurfaceSnapshot,
        NaturalTopologyIndex,
    ) {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: cells,
        })
        .unwrap();
        let view = SphericalNaturalSurface::from_validated(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        (surface, topology)
    }

    fn streams(seed: u64) -> LabeledSubstreams {
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("initial-tectonics-test", 1, "sekai.test"),
        ));
        LabeledSubstreams::capture(&mut rng)
    }

    fn connected_owner_count(
        topology: &NaturalTopologyIndex,
        owners: &[crate::generators::natural::spherical_tectonics::model::LineageId],
        owner: crate::generators::natural::spherical_tectonics::model::LineageId,
    ) -> usize {
        let Some(start) = owners.iter().position(|&candidate| candidate == owner) else {
            return 0;
        };
        let mut reached = vec![false; owners.len()];
        let mut queue = VecDeque::from([start]);
        reached[start] = true;
        let mut count = 0;
        while let Some(index) = queue.pop_front() {
            count += 1;
            for arc in &topology.arcs()[index] {
                let neighbor = arc.neighbor.raw() as usize;
                if !reached[neighbor] && owners[neighbor] == owner {
                    reached[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        count
    }

    #[test]
    fn initial_state_is_dense_connected_area_bounded_and_materially_valid() {
        for cells in [42, 162, 642] {
            let (surface, topology) = fixture(cells);
            let spec = TectonicSpec::default();
            let recipe =
                FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
            let state =
                build_initial_state(&surface, &topology, &spec, recipe, &streams(71)).unwrap();
            let owners = state.initial_owners();

            assert_eq!(state.samples.len(), surface.cells().len());
            assert_eq!(state.plates.len(), usize::from(spec.plate_count));
            assert_eq!(owners.len(), surface.cells().len());
            for (index, sample) in state.samples.iter().enumerate() {
                assert_eq!(sample.anchor, CellId::from_raw(index as u32));
                let norm = sample
                    .position
                    .components()
                    .iter()
                    .map(|component| component * component)
                    .sum::<f64>()
                    .sqrt();
                assert!((norm - 1.0).abs() <= 16.0 * f64::EPSILON);
                assert_eq!(sample.lineation, [0.0; 2]);
                assert_eq!(sample.orogeny, SphericalOrogenyKind::None);
                assert_eq!(sample.orogeny_age_myr, NO_OROGENY_AGE_SENTINEL_MYR);
                match sample.kind {
                    CrustKind::Continental => {
                        assert!((CONTINENTAL_CRUST_MIN_THICKNESS_KM
                            ..=CONTINENTAL_CRUST_MAX_THICKNESS_KM)
                            .contains(&sample.thickness_km));
                        assert_eq!(sample.age_myr, CONTINENTAL_CRUST_AGE_SENTINEL_MYR);
                    }
                    CrustKind::Oceanic => {
                        assert!(
                            (OCEANIC_CRUST_MIN_THICKNESS_KM..=OCEANIC_CRUST_MAX_THICKNESS_KM)
                                .contains(&sample.thickness_km)
                        );
                        assert!((0.0..=MAX_CRUST_AGE_MYR).contains(&sample.age_myr));
                    }
                }
            }

            for plate in &state.plates {
                let owned = owners
                    .iter()
                    .filter(|&&owner| owner == plate.lineage)
                    .count();
                assert!(owned > 0);
                assert_eq!(
                    connected_owner_count(&topology, &owners, plate.lineage),
                    owned
                );
                assert_eq!(owners[plate.representative.raw() as usize], plate.lineage);
            }

            let continental_area = surface
                .cells()
                .iter()
                .zip(&state.samples)
                .filter(|(_, sample)| sample.kind == CrustKind::Continental)
                .map(|(cell, _)| cell.area.get())
                .sum::<f64>();
            let target =
                surface.total_cell_area().get() * f64::from(spec.continental_crust_fraction);
            let max_cell_area = surface
                .cells()
                .iter()
                .map(|cell| cell.area.get())
                .max_by(f64::total_cmp)
                .unwrap();
            assert!((continental_area - target).abs() <= max_cell_area);
        }
    }

    #[test]
    fn initial_state_is_seeded_and_formation_spectrum_changes_coherence() {
        let (surface, topology) = fixture(642);
        let spec = TectonicSpec::default();
        let build = |seed, preset| {
            build_initial_state(
                &surface,
                &topology,
                &spec,
                FormationTectonicRecipe::for_preset(preset),
                &streams(seed),
            )
            .unwrap()
        };
        let first = build(91, ResolvedWorldFormationPreset::Continents);
        let repeated = build(91, ResolvedWorldFormationPreset::Continents);
        let changed = build(92, ResolvedWorldFormationPreset::Continents);
        assert_eq!(first.initial_owners(), repeated.initial_owners());
        assert_eq!(
            first
                .samples
                .iter()
                .map(|sample| (
                    sample.kind,
                    sample.thickness_km.to_bits(),
                    sample.age_myr.to_bits()
                ))
                .collect::<Vec<_>>(),
            repeated
                .samples
                .iter()
                .map(|sample| (
                    sample.kind,
                    sample.thickness_km.to_bits(),
                    sample.age_myr.to_bits()
                ))
                .collect::<Vec<_>>()
        );
        assert_ne!(first.initial_owners(), changed.initial_owners());

        let supercontinent = build(91, ResolvedWorldFormationPreset::Supercontinent);
        let archipelago = build(91, ResolvedWorldFormationPreset::Archipelago);
        let boundary_fraction =
            |state: &crate::generators::natural::spherical_tectonics::model::TectonicState| {
                let cross_kind = surface
                    .edges()
                    .iter()
                    .filter(|edge| {
                        let [first, second] = edge
                            .cells
                            .map(|cell| state.samples[cell.raw() as usize].kind);
                        first != second
                    })
                    .count();
                cross_kind as f64 / surface.edges().len() as f64
            };
        assert!(boundary_fraction(&supercontinent) < boundary_fraction(&archipelago));
    }
}
