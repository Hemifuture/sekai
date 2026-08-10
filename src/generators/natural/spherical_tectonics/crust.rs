#![cfg_attr(not(test), allow(dead_code))]

use rand::RngCore;
use thiserror::Error;

use super::plates::{sample_plate_fabric, PlatePartition, AREA_WEIGHT_TOTAL};
use crate::generators::natural::morphology::area::{
    build_component_budgeted_area_mask, AreaSelectionError, ProtectedRegionSeed,
};
use crate::generators::natural::morphology::field::{
    sample_spherical_field, sample_spherical_field_or_neutral, FieldBand, FieldRecipe, FieldShape,
    MorphologyFieldError, QuantizedScalarField,
};
use crate::generators::natural::random::{
    CRUST_AFFINITY_FIELD_LABEL, CRUST_ANCHOR_LAYOUT_LABEL, CRUST_THICKNESS_FIELD_LABEL,
};
use crate::generators::natural::topology::{multi_source_distance, NaturalTopologyIndex};
use crate::world::natural::{
    CrustKind, CrustKindField, NaturalSpecError, ResolvedWorldFormationPreset, TectonicSpec,
    CONTINENTAL_CRUST_MAX_THICKNESS_KM, CONTINENTAL_CRUST_MIN_THICKNESS_KM,
    OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM,
};
use crate::world::spatial::{central_angle, SphericalSurfaceSnapshot, UnitVector3};
use crate::world::CellId;

const FIELD_CLAMP_SIGMA_MILLI: u16 = 3_000;
const SCORE_SCALE: i32 = 1_000_000;
const LOBE_WEIGHT_MILLI: i64 = 350;
const PLATE_INTERIOR_WEIGHT_MILLI: i64 = 100;

const CONTINENTS_BANDS: [FieldBand; 3] = affinity_bands(500, 320, 180);
const SUPERCONTINENT_BANDS: [FieldBand; 3] = affinity_bands(650, 250, 100);
const ARCHIPELAGO_BANDS: [FieldBand; 3] = affinity_bands(250, 450, 300);
const GREAT_ISLAND_BANDS: [FieldBand; 3] = affinity_bands(550, 300, 150);
const VOLCANIC_ISLANDS_BANDS: [FieldBand; 3] = affinity_bands(100, 400, 500);
const CONTINENTAL_THICKNESS_BANDS: [FieldBand; 2] = [
    FieldBand {
        angular_scale_rad: 36.0_f64.to_radians(),
        weight_milli: 700,
        shape: FieldShape::Smooth,
    },
    FieldBand {
        angular_scale_rad: 14.0_f64.to_radians(),
        weight_milli: 300,
        shape: FieldShape::Smooth,
    },
];
const CONTINENTAL_THICKNESS_RECIPE: FieldRecipe = FieldRecipe {
    bands: &CONTINENTAL_THICKNESS_BANDS,
    clamp_sigma_milli: FIELD_CLAMP_SIGMA_MILLI,
};
const OCEANIC_THICKNESS_BANDS: [FieldBand; 1] = [FieldBand {
    angular_scale_rad: 14.0_f64.to_radians(),
    weight_milli: 1_000,
    shape: FieldShape::Smooth,
}];
const OCEANIC_THICKNESS_RECIPE: FieldRecipe = FieldRecipe {
    bands: &OCEANIC_THICKNESS_BANDS,
    clamp_sigma_milli: FIELD_CLAMP_SIGMA_MILLI,
};

const fn affinity_bands(macro_weight: i32, meso_weight: i32, detail_weight: i32) -> [FieldBand; 3] {
    [
        FieldBand {
            angular_scale_rad: 105.0_f64.to_radians(),
            weight_milli: macro_weight,
            shape: FieldShape::Smooth,
        },
        FieldBand {
            angular_scale_rad: 38.0_f64.to_radians(),
            weight_milli: meso_weight,
            shape: FieldShape::Smooth,
        },
        FieldBand {
            angular_scale_rad: 13.0_f64.to_radians(),
            weight_milli: detail_weight,
            shape: FieldShape::Ridged,
        },
    ]
}

#[derive(Debug, Clone, Copy)]
struct PresetProfile {
    primary_clusters: usize,
    lobe_min: usize,
    lobe_max: usize,
    island_components: usize,
    island_budget_milli: u16,
    minimum_component_millionths: u16,
}

impl PresetProfile {
    const fn for_preset(preset: ResolvedWorldFormationPreset) -> Self {
        match preset {
            ResolvedWorldFormationPreset::Continents => Self {
                primary_clusters: 4,
                lobe_min: 2,
                lobe_max: 4,
                island_components: 4,
                island_budget_milli: 130,
                minimum_component_millionths: 500,
            },
            ResolvedWorldFormationPreset::Supercontinent => Self {
                primary_clusters: 1,
                lobe_min: 6,
                lobe_max: 9,
                island_components: 3,
                island_budget_milli: 75,
                minimum_component_millionths: 500,
            },
            ResolvedWorldFormationPreset::Archipelago => Self {
                primary_clusters: 3,
                lobe_min: 2,
                lobe_max: 3,
                island_components: 8,
                island_budget_milli: 450,
                minimum_component_millionths: 150,
            },
            ResolvedWorldFormationPreset::GreatIsland => Self {
                primary_clusters: 1,
                lobe_min: 3,
                lobe_max: 5,
                island_components: 3,
                island_budget_milli: 200,
                minimum_component_millionths: 250,
            },
            ResolvedWorldFormationPreset::VolcanicIslands => Self {
                primary_clusters: 0,
                lobe_min: 0,
                lobe_max: 0,
                island_components: 12,
                island_budget_milli: 1_000,
                minimum_component_millionths: 0,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CrustMorphology {
    pub(super) kinds: CrustKindField,
    pub(super) thickness_km: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum CrustMorphologyError {
    #[error("invalid tectonic specification: {0}")]
    InvalidSpec(#[from] NaturalSpecError),
    #[error(
        "crust morphology surface has {surface_cells} cells but topology has {topology_cells}"
    )]
    SurfaceCardinality {
        surface_cells: usize,
        topology_cells: usize,
    },
    #[error(
        "crust morphology plate field has {plate_cells} cells but topology has {topology_cells}"
    )]
    PlateCardinality {
        plate_cells: usize,
        topology_cells: usize,
    },
    #[error("crust morphology has {plate_targets} plate targets for {plate_count} plates")]
    PlateTargetCardinality {
        plate_targets: usize,
        plate_count: usize,
    },
    #[error("crust morphology field failed: {0}")]
    Field(#[from] MorphologyFieldError),
    #[error("crust area selection failed: {0}")]
    Area(#[from] AreaSelectionError),
}

pub(super) fn generate_crust(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    plates: &PlatePartition,
    spec: &TectonicSpec,
    preset: ResolvedWorldFormationPreset,
    streams: &crate::generators::natural::random::LabeledSubstreams,
) -> Result<CrustMorphology, CrustMorphologyError> {
    generate_crust_observed(
        surface,
        topology,
        plates,
        spec,
        preset,
        streams,
        |_, _, _| {},
    )
}

fn generate_crust_observed(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    plates: &PlatePartition,
    spec: &TectonicSpec,
    preset: ResolvedWorldFormationPreset,
    streams: &crate::generators::natural::random::LabeledSubstreams,
    mut observe: impl FnMut(&QuantizedScalarField, &[CellId], &[i32]),
) -> Result<CrustMorphology, CrustMorphologyError> {
    spec.validate()?;
    validate_cardinality(surface, topology, plates, spec)?;
    let total_weight = topology
        .area_weights()
        .iter()
        .copied()
        .map(u128::from)
        .sum::<u128>();
    let target_weight =
        (total_weight as f64 * f64::from(spec.continental_crust_fraction)).round() as u128;
    let minimum_cell = topology
        .area_weights()
        .iter()
        .copied()
        .min()
        .map(u128::from)
        .unwrap_or(1);
    let mut profile = PresetProfile::for_preset(preset);
    if preset == ResolvedWorldFormationPreset::VolcanicIslands {
        let resolvable_islands = (target_weight / minimum_cell).max(1) as usize;
        profile.island_components = profile.island_components.min(resolvable_islands);
    } else {
        let island_total = target_weight * u128::from(profile.island_budget_milli) / 1_000;
        let primary_total = target_weight - island_total;
        let resolvable_primary = (primary_total / (minimum_cell * 8)).max(1) as usize;
        profile.primary_clusters = profile.primary_clusters.min(resolvable_primary);
        let resolvable_islands = (island_total / (minimum_cell * 8)).max(1) as usize;
        profile.island_components = profile.island_components.min(resolvable_islands);
    }
    let recipe = affinity_recipe(preset);
    let base_seed = streams.stream(CRUST_AFFINITY_FIELD_LABEL).next_u32();
    let base_affinity = sample_spherical_field_or_neutral(surface, recipe, base_seed)?;
    let fabric = sample_plate_fabric(surface, streams)?;
    let mut anchor_rng = streams.stream(CRUST_ANCHOR_LAYOUT_LABEL);

    let base_scores = base_affinity
        .values()
        .iter()
        .map(|&value| (i64::from(value) * i64::from(SCORE_SCALE) / i64::from(i16::MAX)) as i32)
        .collect::<Vec<_>>();
    let primary_share = if profile.primary_clusters == 0 {
        0.0
    } else {
        f64::from(spec.continental_crust_fraction)
            * (1.0 - f64::from(profile.island_budget_milli) / 1_000.0)
            / profile.primary_clusters as f64
    };
    let primary_radius = equivalent_cap_radius(primary_share);
    let primary = select_spread_anchors(
        surface,
        topology,
        &base_scores,
        profile.primary_clusters,
        &[],
        primary_radius * 2.0,
        &mut anchor_rng,
    );
    let lobe_influence = clustered_lobe_influence(
        surface,
        topology,
        &fabric,
        &primary,
        profile,
        spec.continental_crust_fraction,
        &mut anchor_rng,
    );
    let pre_plate_scores = base_scores
        .iter()
        .zip(&lobe_influence)
        .map(|(&base, &lobe)| {
            i64_to_i32(i64::from(base) + i64::from(lobe) * LOBE_WEIGHT_MILLI / 1_000)
        })
        .collect::<Vec<_>>();
    let island_share = f64::from(spec.continental_crust_fraction)
        * f64::from(profile.island_budget_milli)
        / 1_000.0
        / profile.island_components.max(1) as f64;
    let island_radius = equivalent_cap_radius(island_share);
    let island_separation = (island_radius * 2.0).max(if primary.is_empty() {
        0.0
    } else {
        primary_radius + island_radius
    });
    let islands = select_spread_anchors(
        surface,
        topology,
        &base_scores,
        profile.island_components,
        &primary,
        island_separation,
        &mut anchor_rng,
    );
    let anchors = primary.iter().chain(&islands).copied().collect::<Vec<_>>();

    let plate_interior = plate_interior_preference(surface, topology, plates);
    let final_scores = pre_plate_scores
        .iter()
        .zip(plate_interior)
        .map(|(&base, interior)| {
            i64_to_i32(i64::from(base) + i64::from(interior) * PLATE_INTERIOR_WEIGHT_MILLI / 1_000)
        })
        .collect::<Vec<_>>();
    observe(&base_affinity, &anchors, &final_scores);

    let protected = component_budgets(&primary, &islands, profile, target_weight, minimum_cell);
    let minimum_component_weight = if profile.minimum_component_millionths == 0 {
        minimum_cell
    } else {
        (total_weight * u128::from(profile.minimum_component_millionths) / 1_000_000)
            .max(minimum_cell)
    };
    let mask = build_component_budgeted_area_mask(
        topology,
        &final_scores,
        &protected,
        target_weight,
        minimum_component_weight,
        minimum_component_weight * 2,
    )?;
    let kinds = mask
        .selected()
        .iter()
        .map(|&selected| {
            if selected {
                CrustKind::Continental
            } else {
                CrustKind::Oceanic
            }
        })
        .collect::<Vec<_>>();
    let thickness_km = generate_thickness(surface, topology, &kinds, streams)?;
    Ok(CrustMorphology {
        kinds: CrustKindField::from_kinds(kinds),
        thickness_km,
    })
}

fn validate_cardinality(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    plates: &PlatePartition,
    spec: &TectonicSpec,
) -> Result<(), CrustMorphologyError> {
    if surface.cells().len() != topology.cell_count() {
        return Err(CrustMorphologyError::SurfaceCardinality {
            surface_cells: surface.cells().len(),
            topology_cells: topology.cell_count(),
        });
    }
    if plates.owners.len() != topology.cell_count() {
        return Err(CrustMorphologyError::PlateCardinality {
            plate_cells: plates.owners.len(),
            topology_cells: topology.cell_count(),
        });
    }
    if plates.target_area_weights.len() != usize::from(spec.plate_count) {
        return Err(CrustMorphologyError::PlateTargetCardinality {
            plate_targets: plates.target_area_weights.len(),
            plate_count: usize::from(spec.plate_count),
        });
    }
    Ok(())
}

const fn affinity_recipe(preset: ResolvedWorldFormationPreset) -> FieldRecipe {
    let bands = match preset {
        ResolvedWorldFormationPreset::Continents => &CONTINENTS_BANDS,
        ResolvedWorldFormationPreset::Supercontinent => &SUPERCONTINENT_BANDS,
        ResolvedWorldFormationPreset::Archipelago => &ARCHIPELAGO_BANDS,
        ResolvedWorldFormationPreset::GreatIsland => &GREAT_ISLAND_BANDS,
        ResolvedWorldFormationPreset::VolcanicIslands => &VOLCANIC_ISLANDS_BANDS,
    };
    FieldRecipe {
        bands,
        clamp_sigma_milli: FIELD_CLAMP_SIGMA_MILLI,
    }
}

fn select_spread_anchors(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    count: usize,
    fixed: &[CellId],
    minimum_separation_rad: f64,
    rng: &mut impl RngCore,
) -> Vec<CellId> {
    const CANDIDATE_DIRECTION_COUNT: usize = 256;

    if count == 0 {
        return Vec::new();
    }
    let sampled = sample_anchor_candidates(surface, CANDIDATE_DIRECTION_COUNT, rng);
    let mut peaks = sampled
        .iter()
        .copied()
        .map(|cell| climb_to_local_maximum(topology, scores, cell))
        .collect::<Vec<_>>();
    peaks.sort_unstable();
    peaks.dedup();
    let mut selected = fixed.to_vec();
    let mut added = Vec::with_capacity(count);
    while added.len() < count {
        let mut candidates = anchor_candidates(
            surface,
            scores,
            &peaks,
            &selected,
            Some(minimum_separation_rad),
        );
        if candidates.is_empty() {
            candidates = anchor_candidates(
                surface,
                scores,
                &sampled,
                &selected,
                Some(minimum_separation_rad),
            );
        }
        if candidates.is_empty() {
            candidates = anchor_candidates(surface, scores, &sampled, &selected, None);
        }
        if candidates.is_empty() {
            let all_cells = (0..topology.cell_count())
                .map(|index| CellId::from_raw(index as u32))
                .collect::<Vec<_>>();
            candidates = anchor_candidates(surface, scores, &all_cells, &selected, None);
        }
        candidates.sort_by(|first, second| {
            second
                .0
                .cmp(&first.0)
                .then_with(|| second.1.cmp(&first.1))
                .then_with(|| first.2.cmp(&second.2))
        });
        // The affinity field already carries the labeled random variation. Picking an index from
        // a resolution-dependent shortlist makes the same continuous maximum jump to an unrelated
        // part of the sphere when the tessellation changes. Consume the layout draw so later lobe
        // parameters keep their independent stream position, but anchor the component at the best
        // geometrically separated maximum.
        let _layout_draw = rng.next_u64();
        let cell = candidates[0].2;
        selected.push(cell);
        added.push(cell);
    }
    added
}

fn sample_anchor_candidates(
    surface: &SphericalSurfaceSnapshot,
    count: usize,
    rng: &mut impl RngCore,
) -> Vec<CellId> {
    let mut cells = Vec::with_capacity(count);
    for _ in 0..count {
        let z = unit_f64(rng) * 2.0 - 1.0;
        let azimuth = unit_f64(rng) * std::f64::consts::TAU;
        let radial = (1.0 - z * z).max(0.0).sqrt();
        let direction = UnitVector3::new(radial * azimuth.cos(), radial * azimuth.sin(), z)
            .expect("finite spherical anchor direction");
        let nearest = surface
            .cells()
            .iter()
            .max_by(|first, second| {
                first
                    .centroid
                    .dot(direction)
                    .total_cmp(&second.centroid.dot(direction))
                    .then_with(|| second.id.cmp(&first.id))
            })
            .expect("validated spherical surfaces contain cells")
            .id;
        cells.push(nearest);
    }
    cells.sort_unstable();
    cells.dedup();
    cells
}

fn climb_to_local_maximum(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    start: CellId,
) -> CellId {
    let mut current = start;
    loop {
        let current_score = scores[current.raw() as usize];
        let next = topology.arcs()[current.raw() as usize]
            .iter()
            .map(|arc| arc.neighbor)
            .filter(|&neighbor| scores[neighbor.raw() as usize] > current_score)
            .max_by_key(|&neighbor| (scores[neighbor.raw() as usize], std::cmp::Reverse(neighbor)));
        let Some(next) = next else {
            return current;
        };
        current = next;
    }
}

fn anchor_candidates(
    surface: &SphericalSurfaceSnapshot,
    scores: &[i32],
    pool: &[CellId],
    selected: &[CellId],
    minimum_separation_rad: Option<f64>,
) -> Vec<(i64, i32, CellId)> {
    pool.iter()
        .copied()
        .filter(|cell| !selected.contains(cell))
        .filter_map(|cell| {
            let separation = selected
                .iter()
                .map(|&other| {
                    central_angle(
                        surface.cell(cell).unwrap().centroid,
                        surface.cell(other).unwrap().centroid,
                    )
                })
                .fold(std::f64::consts::PI, f64::min);
            minimum_separation_rad
                .is_none_or(|minimum| separation >= minimum)
                .then(|| {
                    let score = scores[cell.raw() as usize];
                    let merit = i64::from(score)
                        + (separation / std::f64::consts::PI * 1_200_000.0).round() as i64;
                    (merit, score, cell)
                })
        })
        .collect()
}

fn median_equivalent_cell_diameter(surface: &SphericalSurfaceSnapshot) -> f64 {
    let sphere_area = 4.0 * std::f64::consts::PI * surface.radius().get().powi(2);
    let mut diameters = surface
        .cells()
        .iter()
        .map(|cell| {
            let area_fraction = (cell.area.get() / sphere_area).clamp(0.0, 1.0);
            2.0 * (1.0 - 2.0 * area_fraction).clamp(-1.0, 1.0).acos()
        })
        .collect::<Vec<_>>();
    diameters.sort_by(f64::total_cmp);
    diameters[diameters.len() / 2]
}

fn equivalent_cap_radius(component_fraction: f64) -> f64 {
    (1.0 - 2.0 * component_fraction).clamp(-1.0, 1.0).acos()
}

fn clustered_lobe_influence(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    fabric: &QuantizedScalarField,
    primary: &[CellId],
    profile: PresetProfile,
    continental_fraction: f32,
    rng: &mut impl RngCore,
) -> Vec<i32> {
    if primary.is_empty() {
        return vec![0; topology.cell_count()];
    }
    let primary_fraction = f64::from(continental_fraction)
        * (1.0 - f64::from(profile.island_budget_milli) / 1_000.0)
        / primary.len() as f64;
    let cap_radius = (1.0 - 2.0 * primary_fraction).clamp(-1.0, 1.0).acos();
    let mut kernels = Vec::new();
    for &anchor in primary {
        let lobe_span = profile.lobe_max - profile.lobe_min + 1;
        let lobe_count = profile.lobe_min + (rng.next_u64() as usize % lobe_span);
        kernels.push((anchor, cap_radius * 0.90));
        for _ in 0..lobe_count {
            let path_selector = rng.next_u64();
            let offset = cap_radius * (0.25 + 0.65 * unit_f64(rng));
            let support = cap_radius * (0.55 + 0.35 * unit_f64(rng));
            let center =
                walk_along_fabric(surface, topology, fabric, anchor, offset, path_selector);
            kernels.push((center, support));
        }
    }

    let mut raw = vec![0.0_f64; topology.cell_count()];
    for (index, value) in raw.iter_mut().enumerate() {
        for &(center, support) in &kernels {
            let distance = central_angle(
                surface.cells()[index].centroid,
                surface.cells()[center.raw() as usize].centroid,
            );
            let q = distance / support.max(f64::EPSILON);
            if q < 1.0 {
                *value += (1.0 - q).powi(4) * (4.0 * q + 1.0);
            }
        }
    }
    let maximum = raw.iter().copied().fold(0.0_f64, f64::max);
    raw.into_iter()
        .map(|value| {
            if maximum <= f64::EPSILON {
                0
            } else {
                (value / maximum * f64::from(SCORE_SCALE)).round() as i32
            }
        })
        .collect()
}

fn walk_along_fabric(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    fabric: &QuantizedScalarField,
    start: CellId,
    target_angle: f64,
    path_selector: u64,
) -> CellId {
    let start_vector = surface.cell(start).unwrap().centroid;
    let mut initial = topology.arcs()[start.raw() as usize]
        .iter()
        .map(|arc| {
            (
                i32::from(fabric.get(start).unwrap())
                    .abs_diff(i32::from(fabric.get(arc.neighbor).unwrap())),
                arc.neighbor,
            )
        })
        .collect::<Vec<_>>();
    initial.sort_unstable();
    let initial_count = initial.len().min(3);
    if initial_count == 0 {
        return start;
    }
    let heading_cell = initial[path_selector as usize % initial_count].1;
    let destination = great_circle_destination(
        start_vector,
        surface.cell(heading_cell).unwrap().centroid,
        target_angle,
    );

    let mut path = vec![start];
    let mut current = start;
    let mut best = start;
    let mut best_error = target_angle;
    for _ in 0..topology.cell_count() {
        let current_value = i32::from(fabric.get(current).unwrap());
        let mut candidates = topology.arcs()[current.raw() as usize]
            .iter()
            .filter(|arc| !path.contains(&arc.neighbor))
            .map(|arc| {
                (
                    current_value.abs_diff(i32::from(fabric.get(arc.neighbor).unwrap())),
                    central_angle(surface.cell(arc.neighbor).unwrap().centroid, destination),
                    arc.neighbor,
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|first, second| {
            first
                .0
                .cmp(&second.0)
                .then_with(|| first.1.total_cmp(&second.1))
                .then_with(|| first.2.cmp(&second.2))
        });
        let top = candidates.len().min(3);
        if top == 0 {
            break;
        }
        let next = candidates[..top]
            .iter()
            .min_by(|first, second| {
                first
                    .1
                    .total_cmp(&second.1)
                    .then_with(|| first.0.cmp(&second.0))
                    .then_with(|| first.2.cmp(&second.2))
            })
            .unwrap()
            .2;
        path.push(next);
        current = next;
        let angle = central_angle(start_vector, surface.cell(current).unwrap().centroid);
        let error = (angle - target_angle).abs();
        if error < best_error {
            best = current;
            best_error = error;
        }
        if angle >= target_angle {
            break;
        }
    }
    best
}

fn great_circle_destination(
    start: crate::world::spatial::UnitVector3,
    heading: crate::world::spatial::UnitVector3,
    angle: f64,
) -> crate::world::spatial::UnitVector3 {
    let start_components = start.components();
    let heading_components = heading.components();
    let cosine = start.dot(heading).clamp(-1.0, 1.0);
    let tangent = std::array::from_fn::<_, 3, _>(|axis| {
        heading_components[axis] - start_components[axis] * cosine
    });
    let tangent_length = tangent
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    let cosine_angle = angle.cos();
    let sine_angle = angle.sin();
    crate::world::spatial::UnitVector3::new(
        start_components[0] * cosine_angle + tangent[0] / tangent_length * sine_angle,
        start_components[1] * cosine_angle + tangent[1] / tangent_length * sine_angle,
        start_components[2] * cosine_angle + tangent[2] / tangent_length * sine_angle,
    )
    .expect("a distinct neighboring cell defines a finite great-circle heading")
}

fn plate_interior_preference(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    plates: &PlatePartition,
) -> Vec<i32> {
    let boundary_cells = topology
        .arcs()
        .iter()
        .enumerate()
        .filter_map(|(index, arcs)| {
            let owner = plates.owners.get(index).unwrap();
            arcs.iter()
                .any(|arc| plates.owners.get(arc.neighbor.raw() as usize).unwrap() != owner)
                .then_some(CellId::from_raw(index as u32))
        })
        .collect::<Vec<_>>();
    let distances = multi_source_distance(topology, &boundary_cells, None);
    distances
        .into_iter()
        .enumerate()
        .map(|(index, distance)| {
            let owner = plates.owners.get(index).unwrap().raw() as usize;
            let fraction = plates.target_area_weights[owner] as f64 / AREA_WEIGHT_TOTAL as f64;
            let radius_angle = (1.0 - 2.0 * fraction).clamp(-1.0, 1.0).acos();
            let radius_cost = topology
                .quantized_distance_for_meters(surface.radius().get() * radius_angle)
                .max(1);
            ((distance as f64 / radius_cost as f64).clamp(0.0, 1.0) * f64::from(SCORE_SCALE))
                .round() as i32
        })
        .collect()
}

fn component_budgets(
    primary: &[CellId],
    islands: &[CellId],
    profile: PresetProfile,
    target_weight: u128,
    minimum_cell_weight: u128,
) -> Vec<ProtectedRegionSeed> {
    const PROTECTED_CORE_BUDGET_MILLI: u128 = 1_000;

    let protected_total = target_weight * PROTECTED_CORE_BUDGET_MILLI / 1_000;
    let island_total = protected_total * u128::from(profile.island_budget_milli) / 1_000;
    let primary_total = protected_total - island_total;
    let mut seeds = Vec::with_capacity(primary.len() + islands.len());
    append_equal_budgets(&mut seeds, primary, primary_total);
    let coarse_volcanic = primary.is_empty()
        && !islands.is_empty()
        && island_total / (islands.len() as u128) < minimum_cell_weight * 8;
    if coarse_volcanic {
        for &cell in islands {
            seeds.push(ProtectedRegionSeed {
                cell,
                budget_weight: minimum_cell_weight,
                component: seeds.len() as u16,
            });
        }
    } else {
        append_equal_budgets(&mut seeds, islands, island_total);
    }
    seeds
}

fn append_equal_budgets(
    seeds: &mut Vec<ProtectedRegionSeed>,
    cells: &[CellId],
    total_weight: u128,
) {
    if cells.is_empty() {
        return;
    }
    let base = total_weight / cells.len() as u128;
    let remainder = total_weight % cells.len() as u128;
    for (offset, &cell) in cells.iter().enumerate() {
        seeds.push(ProtectedRegionSeed {
            cell,
            budget_weight: base + u128::from(offset < remainder as usize),
            component: seeds.len() as u16,
        });
    }
}

fn generate_thickness(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    kinds: &[CrustKind],
    streams: &crate::generators::natural::random::LabeledSubstreams,
) -> Result<Vec<f32>, MorphologyFieldError> {
    generate_thickness_with_recipes(
        surface,
        topology,
        kinds,
        streams,
        CONTINENTAL_THICKNESS_RECIPE,
        OCEANIC_THICKNESS_RECIPE,
    )
}

fn optional_thickness_field(
    surface: &SphericalSurfaceSnapshot,
    recipe: FieldRecipe,
    seed: u32,
) -> Result<Option<QuantizedScalarField>, MorphologyFieldError> {
    match sample_spherical_field(surface, recipe, seed) {
        Ok(field) => Ok(Some(field)),
        Err(MorphologyFieldError::NoResolvableBand { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

fn generate_thickness_with_recipes(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    kinds: &[CrustKind],
    streams: &crate::generators::natural::random::LabeledSubstreams,
    continental_recipe: FieldRecipe,
    oceanic_recipe: FieldRecipe,
) -> Result<Vec<f32>, MorphologyFieldError> {
    let mut rng = streams.stream(CRUST_THICKNESS_FIELD_LABEL);
    let continental_field = optional_thickness_field(surface, continental_recipe, rng.next_u32())?;
    let oceanic_field = optional_thickness_field(surface, oceanic_recipe, rng.next_u32())?;
    let coast_cells = topology
        .arcs()
        .iter()
        .enumerate()
        .filter_map(|(index, arcs)| {
            arcs.iter()
                .any(|arc| kinds[arc.neighbor.raw() as usize] != kinds[index])
                .then_some(CellId::from_raw(index as u32))
        })
        .collect::<Vec<_>>();
    let coast_distances = multi_source_distance(topology, &coast_cells, None);
    let maximum_continental_distance = coast_distances
        .iter()
        .enumerate()
        .filter_map(|(index, &distance)| {
            (kinds[index] == CrustKind::Continental).then_some(distance)
        })
        .max()
        .unwrap_or(1)
        .max(1);

    Ok(kinds
        .iter()
        .enumerate()
        .map(|(index, &kind)| {
            let field = match kind {
                CrustKind::Oceanic => oceanic_field.as_ref(),
                CrustKind::Continental => continental_field.as_ref(),
            };
            let signal = field.map_or(0.0, |field| {
                f64::from(field.values()[index]) / f64::from(i16::MAX)
            });
            let unit_signal = (signal * 0.5 + 0.5).clamp(0.0, 1.0) as f32;
            match kind {
                CrustKind::Oceanic => {
                    let span = OCEANIC_CRUST_MAX_THICKNESS_KM - OCEANIC_CRUST_MIN_THICKNESS_KM;
                    (OCEANIC_CRUST_MIN_THICKNESS_KM + span * (0.25 + unit_signal * 0.35)).clamp(
                        OCEANIC_CRUST_MIN_THICKNESS_KM,
                        OCEANIC_CRUST_MAX_THICKNESS_KM,
                    )
                }
                CrustKind::Continental => {
                    let span =
                        CONTINENTAL_CRUST_MAX_THICKNESS_KM - CONTINENTAL_CRUST_MIN_THICKNESS_KM;
                    let coast = (coast_distances[index] as f64
                        / maximum_continental_distance as f64)
                        .clamp(0.0, 1.0) as f32;
                    (CONTINENTAL_CRUST_MIN_THICKNESS_KM
                        + span * (0.18 + unit_signal * 0.42 + coast * 0.30))
                        .clamp(
                            CONTINENTAL_CRUST_MIN_THICKNESS_KM,
                            CONTINENTAL_CRUST_MAX_THICKNESS_KM,
                        )
                }
            }
        })
        .collect())
}

fn unit_f64(rng: &mut impl RngCore) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1_u64 << 53) as f64
}

fn i64_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::ops::RangeInclusive;

    use super::{
        generate_crust, generate_crust_observed, median_equivalent_cell_diameter, walk_along_fabric,
    };
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::spherical_tectonics::plates::{
        generate_plate_partition, PlatePartition,
    };
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, ResolvedWorldFormationPreset, TectonicSpec, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
        CONTINENTAL_CRUST_MIN_THICKNESS_KM, OCEANIC_CRUST_MAX_THICKNESS_KM,
        OCEANIC_CRUST_MIN_THICKNESS_KM,
    };
    use crate::world::spatial::{
        central_angle, SphericalNaturalSurface, SphericalSurfaceSnapshot, UnitVector3,
    };
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    #[derive(Debug)]
    struct ObservedCrust {
        surface: SphericalSurfaceSnapshot,
        morphology: super::CrustMorphology,
        base_affinity: Vec<i16>,
        anchor_layout: Vec<CellId>,
        final_affinity: Vec<i32>,
        plates: PlatePartition,
        topology: NaturalTopologyIndex,
    }

    fn stage_rng(seed: u64) -> StageRng {
        StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("natural.spherical-tectonics", 2, "sekai.core"),
        ))
    }

    fn fixture_surface(target_cell_count: u32) -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count,
        })
        .unwrap()
    }

    #[test]
    fn fabric_walk_uses_angular_distance_instead_of_cell_steps() {
        const TARGET_ANGLE: f64 = 0.45;
        let reference = UnitVector3::new(1.0, 0.0, 0.0).unwrap();
        for target_cell_count in [642, 2_562] {
            let surface = fixture_surface(target_cell_count);
            let view = SphericalNaturalSurface::new(&surface).unwrap();
            let topology = NaturalTopologyIndex::from_surface(&view);
            let streams = LabeledSubstreams::capture(&mut stage_rng(42));
            let fabric = super::sample_plate_fabric(&surface, &streams).unwrap();
            let start = surface
                .cells()
                .iter()
                .max_by(|first, second| {
                    first
                        .centroid
                        .dot(reference)
                        .total_cmp(&second.centroid.dot(reference))
                })
                .unwrap()
                .id;
            let end = walk_along_fabric(
                &surface,
                &topology,
                &fabric,
                start,
                TARGET_ANGLE,
                0xD1CE_FAB1_CAFE_BEEFu64,
            );
            let actual = central_angle(
                surface.cell(start).unwrap().centroid,
                surface.cell(end).unwrap().centroid,
            );
            let tolerance = median_equivalent_cell_diameter(&surface) * 1.5;
            assert!(
                (actual - TARGET_ANGLE).abs() <= tolerance,
                "target={target_cell_count} cells={} actual={actual} tolerance={tolerance}",
                surface.cells().len()
            );
        }
    }

    fn fixture_crust_components(
        plate_count: u16,
        preset: ResolvedWorldFormationPreset,
        fraction: f32,
        seed: u64,
    ) -> ObservedCrust {
        fixture_crust_components_at(642, plate_count, preset, fraction, seed)
    }

    fn fixture_crust_components_at(
        target_cell_count: u32,
        plate_count: u16,
        preset: ResolvedWorldFormationPreset,
        fraction: f32,
        seed: u64,
    ) -> ObservedCrust {
        let surface = fixture_surface(target_cell_count);
        let view = SphericalNaturalSurface::new(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let spec = TectonicSpec {
            plate_count,
            continental_crust_fraction: fraction,
            ..TectonicSpec::default()
        };
        let streams = LabeledSubstreams::capture(&mut stage_rng(seed));
        let plates = generate_plate_partition(&surface, &topology, &spec, &streams).unwrap();
        let mut base_affinity = Vec::new();
        let mut anchor_layout = Vec::new();
        let mut final_affinity = Vec::new();
        let morphology = generate_crust_observed(
            &surface,
            &topology,
            &plates,
            &spec,
            preset,
            &streams,
            |base, anchors, final_scores| {
                base_affinity = base.values().to_vec();
                anchor_layout = anchors.to_vec();
                final_affinity = final_scores.to_vec();
            },
        )
        .unwrap_or_else(|error| panic!("{preset:?} crust fixture failed: {error:?}"));
        assert_eq!(
            morphology,
            generate_crust(&surface, &topology, &plates, &spec, preset, &streams,).unwrap()
        );
        ObservedCrust {
            surface,
            morphology,
            base_affinity,
            anchor_layout,
            final_affinity,
            plates,
            topology,
        }
    }

    #[test]
    fn continental_anchor_directions_are_stable_across_resolution() {
        let coarse = fixture_crust_components_at(
            642,
            12,
            ResolvedWorldFormationPreset::Continents,
            0.38,
            42,
        );
        let fine = fixture_crust_components_at(
            2_562,
            12,
            ResolvedWorldFormationPreset::Continents,
            0.38,
            42,
        );
        assert_eq!(coarse.anchor_layout.len(), fine.anchor_layout.len());
        let tolerance = median_equivalent_cell_diameter(&coarse.surface) * 1.5;
        for (name, range) in [("primary", 0..4), ("island", 4..8)] {
            let mut unmatched = fine.anchor_layout[range.clone()].to_vec();
            for &coarse_anchor in &coarse.anchor_layout[range] {
                let (nearest_index, angle) = unmatched
                    .iter()
                    .enumerate()
                    .map(|(index, &fine_anchor)| {
                        (
                            index,
                            central_angle(
                                coarse.surface.cell(coarse_anchor).unwrap().centroid,
                                fine.surface.cell(fine_anchor).unwrap().centroid,
                            ),
                        )
                    })
                    .min_by(|first, second| first.1.total_cmp(&second.1))
                    .unwrap();
                let fine_anchor = unmatched.remove(nearest_index);
                assert!(
                    angle <= tolerance,
                    "{name} anchor: angle={angle} tolerance={tolerance} coarse={coarse_anchor:?} fine={fine_anchor:?}"
                );
            }
        }
    }

    #[test]
    #[ignore = "release-only continuous-field resolution diagnostic"]
    fn continental_affinity_quantile_is_stable_across_resolution() {
        fn quantile_mask(case: &ObservedCrust, fraction: f64) -> Vec<bool> {
            let target = (case
                .topology
                .area_weights()
                .iter()
                .copied()
                .map(u128::from)
                .sum::<u128>() as f64
                * fraction)
                .round() as u128;
            let mut order = (0..case.surface.cells().len()).collect::<Vec<_>>();
            order.sort_by_key(|&index| (std::cmp::Reverse(case.final_affinity[index]), index));
            let mut mask = vec![false; order.len()];
            let mut area = 0_u128;
            for index in order {
                let next = area + u128::from(case.topology.area_weights()[index]);
                if next.abs_diff(target) <= area.abs_diff(target) {
                    mask[index] = true;
                    area = next;
                }
            }
            mask
        }

        let coarse = fixture_crust_components_at(
            5_000,
            12,
            ResolvedWorldFormationPreset::Continents,
            0.38,
            42,
        );
        let fine = fixture_crust_components_at(
            20_000,
            12,
            ResolvedWorldFormationPreset::Continents,
            0.38,
            42,
        );
        let coarse_mask = quantile_mask(&coarse, 0.38);
        let fine_mask = quantile_mask(&fine, 0.38);
        let mut intersection = 0_u128;
        let mut union = 0_u128;
        for (index, cell) in coarse.surface.cells().iter().enumerate() {
            let nearest = fine
                .surface
                .cells()
                .iter()
                .max_by(|first, second| {
                    first
                        .centroid
                        .dot(cell.centroid)
                        .total_cmp(&second.centroid.dot(cell.centroid))
                })
                .unwrap()
                .id
                .raw() as usize;
            if coarse_mask[index] || fine_mask[nearest] {
                let area = u128::from(coarse.topology.area_weights()[index]);
                union += area;
                if coarse_mask[index] && fine_mask[nearest] {
                    intersection += area;
                }
            }
        }
        let jaccard = intersection as f64 / union as f64;
        eprintln!("continental affinity quantile jaccard={jaccard:.4}");
        assert!(jaccard >= 0.75);
    }

    #[test]
    fn minimum_and_coarse_spheres_fulfill_every_protected_continental_budget() {
        for target_cell_count in [42, 162] {
            for preset in [
                ResolvedWorldFormationPreset::Continents,
                ResolvedWorldFormationPreset::Archipelago,
                ResolvedWorldFormationPreset::Supercontinent,
                ResolvedWorldFormationPreset::GreatIsland,
                ResolvedWorldFormationPreset::VolcanicIslands,
            ] {
                let case =
                    fixture_crust_components_at(target_cell_count, 12, preset, 0.38, 0xC0_FFEE);
                assert_eq!(
                    case.morphology.kinds.len(),
                    target_cell_count as usize,
                    "{preset:?}"
                );
            }
        }
    }

    #[test]
    fn minimum_sphere_supports_recommended_and_minimum_volcanic_land_fraction() {
        for fraction in [0.16, 0.10] {
            let case = fixture_crust_components_at(
                42,
                12,
                ResolvedWorldFormationPreset::VolcanicIslands,
                fraction,
                0xC0_FFEE,
            );
            assert_eq!(case.morphology.kinds.len(), 42);
            assert!(case
                .morphology
                .kinds
                .raw_values()
                .iter()
                .any(|&kind| kind == CrustKind::Continental.raw()));
            let total_weight = case
                .topology
                .area_weights()
                .iter()
                .copied()
                .map(u128::from)
                .sum::<u128>();
            let continental_weight = case
                .morphology
                .kinds
                .raw_values()
                .iter()
                .zip(case.topology.area_weights())
                .filter_map(|(&kind, &weight)| {
                    (kind == CrustKind::Continental.raw()).then_some(u128::from(weight))
                })
                .sum::<u128>();
            let actual_fraction = continental_weight as f64 / total_weight as f64;
            let maximum_cell_fraction = case
                .topology
                .area_weights()
                .iter()
                .copied()
                .max()
                .map(|weight| weight as f64)
                .unwrap()
                / total_weight as f64;
            assert!(
                (actual_fraction - f64::from(fraction)).abs() <= maximum_cell_fraction,
                "fraction={fraction} actual={actual_fraction} tolerance={maximum_cell_fraction}"
            );
        }
    }

    fn continental_components(case: &ObservedCrust) -> Vec<u128> {
        let selected = case
            .morphology
            .kinds
            .raw_values()
            .iter()
            .map(|&raw| raw == CrustKind::Continental.raw())
            .collect::<Vec<_>>();
        let mut visited = vec![false; selected.len()];
        let mut components = Vec::new();
        for start in 0..selected.len() {
            if !selected[start] || visited[start] {
                continue;
            }
            visited[start] = true;
            let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
            let mut area = 0_u128;
            while let Some(cell) = queue.pop_front() {
                let index = cell.raw() as usize;
                area += u128::from(case.topology.area_weights()[index]);
                for arc in &case.topology.arcs()[index] {
                    let neighbor = arc.neighbor.raw() as usize;
                    if selected[neighbor] && !visited[neighbor] {
                        visited[neighbor] = true;
                        queue.push_back(arc.neighbor);
                    }
                }
            }
            components.push(area);
        }
        components
    }

    fn assert_preset_contract(
        preset: ResolvedWorldFormationPreset,
        fraction: f32,
        major_component_range: RangeInclusive<usize>,
    ) {
        let case = fixture_crust_components(12, preset, fraction, 42);
        let total = case
            .topology
            .area_weights()
            .iter()
            .copied()
            .map(u128::from)
            .sum::<u128>();
        let target = (total as f64 * f64::from(fraction)).round() as u128;
        let maximum_cell = u128::from(*case.topology.area_weights().iter().max().unwrap());
        let selected_area = case
            .morphology
            .kinds
            .raw_values()
            .iter()
            .zip(case.topology.area_weights())
            .filter_map(|(&kind, &area)| {
                (kind == CrustKind::Continental.raw()).then_some(u128::from(area))
            })
            .sum::<u128>();
        assert!(selected_area.abs_diff(target) <= maximum_cell);
        let components = continental_components(&case);
        let continental_total = components.iter().sum::<u128>();
        let major = components
            .iter()
            .filter(|&&area| area * 10 >= continental_total)
            .count();
        assert!(
            major_component_range.contains(&major),
            "{preset:?} produced {major} major components from {components:?}"
        );
    }

    #[test]
    fn preset_recipes_hit_area_and_major_component_contracts() {
        assert_preset_contract(ResolvedWorldFormationPreset::Continents, 0.38, 3..=5);
        assert_preset_contract(ResolvedWorldFormationPreset::Supercontinent, 0.42, 1..=1);
        assert_preset_contract(ResolvedWorldFormationPreset::Archipelago, 0.26, 2..=6);
        assert_preset_contract(ResolvedWorldFormationPreset::GreatIsland, 0.28, 1..=1);
        assert_preset_contract(ResolvedWorldFormationPreset::VolcanicIslands, 0.16, 0..=2);
    }

    #[test]
    fn continent_field_is_related_to_but_not_equal_to_plate_ownership() {
        let case = fixture_crust_components(12, ResolvedWorldFormationPreset::Continents, 0.38, 71);
        let mut coast_edges = 0_usize;
        let mut shared_edges = 0_usize;
        for owners in case.topology.edge_owners() {
            let [Some(first), Some(second)] = *owners else {
                continue;
            };
            let first_index = first.raw() as usize;
            let second_index = second.raw() as usize;
            let coast =
                case.morphology.kinds.get(first_index) != case.morphology.kinds.get(second_index);
            if coast {
                coast_edges += 1;
                shared_edges += usize::from(
                    case.plates.owners.get(first_index) != case.plates.owners.get(second_index),
                );
            }
        }
        let overlap = shared_edges as f64 / coast_edges as f64;
        assert!(
            (0.10..=0.55).contains(&overlap),
            "coast/plate overlap {overlap}"
        );
        assert_ne!(
            case.morphology.kinds.raw_values(),
            case.plates.owners.raw_values()
        );
    }

    #[test]
    fn crust_random_base_is_orthogonal_to_plate_count_while_soft_coupling_may_change_mask() {
        let twelve =
            fixture_crust_components(12, ResolvedWorldFormationPreset::Continents, 0.38, 91);
        let seventeen =
            fixture_crust_components(17, ResolvedWorldFormationPreset::Continents, 0.38, 91);
        assert_eq!(twelve.base_affinity, seventeen.base_affinity);
        assert_eq!(twelve.anchor_layout, seventeen.anchor_layout);
        assert_ne!(twelve.final_affinity, seventeen.final_affinity);
        assert_ne!(
            twelve.morphology.kinds.raw_values(),
            seventeen.morphology.kinds.raw_values()
        );
    }

    #[test]
    fn thickness_uses_an_independent_field_and_stays_in_physical_ranges() {
        let case =
            fixture_crust_components(12, ResolvedWorldFormationPreset::Continents, 0.38, 113);
        for (index, &thickness) in case.morphology.thickness_km.iter().enumerate() {
            let range = match case.morphology.kinds.get(index).unwrap() {
                CrustKind::Oceanic => {
                    OCEANIC_CRUST_MIN_THICKNESS_KM..=OCEANIC_CRUST_MAX_THICKNESS_KM
                }
                CrustKind::Continental => {
                    CONTINENTAL_CRUST_MIN_THICKNESS_KM..=CONTINENTAL_CRUST_MAX_THICKNESS_KM
                }
            };
            assert!(
                range.contains(&thickness),
                "cell {index} thickness {thickness}"
            );
        }
        let mut affinity_order = (0..case.final_affinity.len()).collect::<Vec<_>>();
        affinity_order.sort_by_key(|&index| (case.final_affinity[index], index));
        let mut thickness_order = (0..case.morphology.thickness_km.len()).collect::<Vec<_>>();
        thickness_order.sort_by(|&first, &second| {
            case.morphology.thickness_km[first]
                .total_cmp(&case.morphology.thickness_km[second])
                .then_with(|| first.cmp(&second))
        });
        assert_ne!(affinity_order, thickness_order);

        let oceanic = (0..case.final_affinity.len())
            .filter(|&index| case.morphology.kinds.get(index) == Some(CrustKind::Oceanic))
            .collect::<Vec<_>>();
        let mut ocean_affinity_order = oceanic.clone();
        ocean_affinity_order.sort_by_key(|&index| (case.base_affinity[index], index));
        let mut ocean_thickness_order = oceanic;
        ocean_thickness_order.sort_by(|&first, &second| {
            case.morphology.thickness_km[first]
                .total_cmp(&case.morphology.thickness_km[second])
                .then_with(|| first.cmp(&second))
        });
        assert_ne!(ocean_affinity_order, ocean_thickness_order);
    }

    #[test]
    fn oceanic_thickness_is_orthogonal_to_continental_low_frequency_recipe() {
        use crate::generators::natural::morphology::field::{FieldBand, FieldRecipe, FieldShape};

        const ALTERED_CONTINENTAL_BANDS: [FieldBand; 2] = [
            FieldBand {
                angular_scale_rad: 72.0_f64.to_radians(),
                weight_milli: 850,
                shape: FieldShape::Ridged,
            },
            FieldBand {
                angular_scale_rad: 14.0_f64.to_radians(),
                weight_milli: 150,
                shape: FieldShape::Smooth,
            },
        ];
        const ALTERED_CONTINENTAL_RECIPE: FieldRecipe = FieldRecipe {
            bands: &ALTERED_CONTINENTAL_BANDS,
            clamp_sigma_milli: super::FIELD_CLAMP_SIGMA_MILLI,
        };

        let case =
            fixture_crust_components(12, ResolvedWorldFormationPreset::Continents, 0.38, 113);
        let streams = LabeledSubstreams::capture(&mut stage_rng(113));
        let baseline = super::generate_thickness_with_recipes(
            &case.surface,
            &case.topology,
            &case
                .morphology
                .kinds
                .raw_values()
                .iter()
                .map(|&raw| CrustKind::try_from_raw(raw).unwrap())
                .collect::<Vec<_>>(),
            &streams,
            super::CONTINENTAL_THICKNESS_RECIPE,
            super::OCEANIC_THICKNESS_RECIPE,
        )
        .unwrap();
        let altered = super::generate_thickness_with_recipes(
            &case.surface,
            &case.topology,
            &case
                .morphology
                .kinds
                .raw_values()
                .iter()
                .map(|&raw| CrustKind::try_from_raw(raw).unwrap())
                .collect::<Vec<_>>(),
            &streams,
            ALTERED_CONTINENTAL_RECIPE,
            super::OCEANIC_THICKNESS_RECIPE,
        )
        .unwrap();

        assert!(case
            .morphology
            .kinds
            .raw_values()
            .iter()
            .enumerate()
            .filter(|&(_, &kind)| kind == CrustKind::Oceanic.raw())
            .all(|(index, _)| baseline[index].to_bits() == altered[index].to_bits()));
        assert!(case
            .morphology
            .kinds
            .raw_values()
            .iter()
            .enumerate()
            .filter(|&(_, &kind)| kind == CrustKind::Continental.raw())
            .any(|(index, _)| baseline[index].to_bits() != altered[index].to_bits()));
    }
}
