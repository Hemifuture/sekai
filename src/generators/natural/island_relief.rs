use std::collections::BTreeMap;

use super::relief_noise::{FractalProfile, ReliefNoise2d};
use super::topology::NaturalTopologyIndex;
use crate::world::natural::{
    BoundaryKind, CrustKind, MantleSnapshot, TectonicSnapshot, VOLCANIC_OFFSET_MAX_M,
    VOLCANIC_OFFSET_MIN_M,
};
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::CellId;

const HOTSPOT_FBM: FractalProfile = FractalProfile {
    octaves: 5,
    frequency: 1.35,
    lacunarity: 2.03,
    persistence: 0.5,
};
const HOTSPOT_RIDGES: FractalProfile = FractalProfile {
    octaves: 4,
    frequency: 2.2,
    lacunarity: 2.08,
    persistence: 0.46,
};
const HOTSPOT_SEED_STEP: u32 = 0x9E37_79B9;
const SUPPORTED_BACKGROUND_FRACTION: f64 = 0.015;
const ARC_SEED_STEP: u32 = 0x85EB_CA6B;
const ARC_FBM: FractalProfile = FractalProfile {
    octaves: 5,
    frequency: 5.0,
    lacunarity: 2.01,
    persistence: 0.48,
};
const ARC_RIDGES: FractalProfile = FractalProfile {
    octaves: 4,
    frequency: 8.0,
    lacunarity: 2.07,
    persistence: 0.44,
};

/// Interprets present-day mantle forcing as compact volcanic morphology.
///
/// The mantle field owns where volcanism may occur. This function only shapes
/// positive relief inside that support, using the current plate velocity as a
/// short directional cue rather than manufacturing a simulated history.
pub(super) fn synthesize_hotspot_offset(
    spatial: &SpatialSnapshot,
    tectonic: &TectonicSnapshot,
    mantle: &MantleSnapshot,
    morphology_seed: u32,
) -> Vec<f32> {
    let mut result = mantle
        .volcanic_influence()
        .iter()
        .enumerate()
        .map(|(index, &influence)| {
            if influence <= 0.0 {
                return 0.0;
            }
            let cell = CellId::from_raw(index as u32);
            let amplitude = volcanic_amplitude(tectonic, cell);
            (f64::from(amplitude) * SUPPORTED_BACKGROUND_FRACTION * f64::from(influence).powf(2.0))
                as f32
        })
        .collect::<Vec<_>>();

    for hotspot in mantle.hotspots() {
        let source = hotspot.source_cell();
        let source_center = spatial
            .cell(source)
            .expect("validated hotspot source is spatially aligned")
            .centroid;
        let plate_id = tectonic
            .plate_for_cell(source)
            .expect("validated tectonic field is cell aligned");
        let velocity = tectonic.plates()[plate_id.raw() as usize]
            .velocity
            .components_mm_per_year();
        let direction = normalized_direction(velocity);
        let speed = f64::from(velocity[0]).hypot(f64::from(velocity[1]));
        let speed_fraction = (speed / (120.0_f64 * 2.0_f64.sqrt())).clamp(0.0, 1.0);
        let radius_m = hotspot.support_radius_m().get();
        let strength = f64::from(hotspot.strength_permille()) / 1_000.0;
        let noise = ReliefNoise2d::new(
            morphology_seed
                .wrapping_add(HOTSPOT_SEED_STEP.wrapping_mul(hotspot.id().raw().wrapping_add(1))),
        );

        for (index, value) in result.iter_mut().enumerate() {
            let mantle_envelope = f64::from(mantle.volcanic_influence()[index]);
            if mantle_envelope <= 0.0 {
                continue;
            }
            let cell = CellId::from_raw(index as u32);
            let center = spatial
                .cell(cell)
                .expect("validated spatial IDs are contiguous")
                .centroid;
            let local = [
                (center.x().get() - source_center.x().get()) / radius_m,
                (center.y().get() - source_center.y().get()) / radius_m,
            ];
            if local[0].hypot(local[1]) > 1.15 {
                continue;
            }

            let warped = noise.warp(local, 1.15, 0.1);
            let current_edifice = compact_peak(warped[0].hypot(warped[1]) / 0.48);
            let trail = directional_trail(warped, direction, speed_fraction, &noise);
            let morphology = current_edifice.max(trail);
            if morphology <= 0.0 {
                continue;
            }

            let fbm = ((noise.fbm(warped, HOTSPOT_FBM) + 1.0) * 0.5)
                .clamp(0.0, 1.0)
                .powf(1.65);
            let ridges = noise.ridged(warped, HOTSPOT_RIDGES).powf(2.2);
            let surface_detail = 0.72 + 0.18 * fbm + 0.10 * ridges;
            let support = mantle_envelope.powf(0.35);
            let amplitude = f64::from(volcanic_amplitude(tectonic, cell));
            let strength_response = 0.55 + 0.45 * strength;
            let candidate =
                amplitude * strength_response * morphology.powf(1.15) * support * surface_detail;
            *value = value.max(candidate as f32);
        }

        // Preserve the current volcanic center as the dominant edifice. Noise
        // shapes its slopes and satellites, but cannot erase the causal source.
        let source_index = source.raw() as usize;
        if mantle.volcanic_influence()[source_index] > 0.0 {
            let source_amplitude = volcanic_amplitude(tectonic, source);
            result[source_index] = result[source_index].max(source_amplitude * strength as f32);
        }
    }

    for (index, value) in result.iter_mut().enumerate() {
        if mantle.volcanic_influence()[index] <= 0.0 {
            *value = 0.0;
        } else {
            *value = value.clamp(VOLCANIC_OFFSET_MIN_M, VOLCANIC_OFFSET_MAX_M);
        }
    }
    result
}

fn volcanic_amplitude(tectonic: &TectonicSnapshot, cell: CellId) -> f32 {
    match tectonic
        .crust_kind(cell)
        .expect("validated tectonic field is cell aligned")
    {
        CrustKind::Oceanic => VOLCANIC_OFFSET_MAX_M,
        CrustKind::Continental => 2_400.0,
    }
}

/// Adds narrow volcanic summits only where an oceanic plate overrides another
/// oceanic plate at a present-day subduction segment.
pub(super) fn synthesize_oceanic_arc_peaks(
    spatial: &SpatialSnapshot,
    topology: &NaturalTopologyIndex,
    tectonic: &TectonicSnapshot,
    morphology_seed: u32,
) -> Vec<f32> {
    let mut result = vec![0.0_f32; spatial.cell_count()];

    for segment in tectonic
        .boundary_segments()
        .iter()
        .filter(|segment| segment.kind == BoundaryKind::Subduction)
    {
        let subducting = segment
            .subducting_plate
            .expect("validated subduction segment has a descending plate");
        let overriding = if segment.plates[0] == subducting {
            segment.plates[1]
        } else {
            segment.plates[0]
        };
        let mut candidates = BTreeMap::<CellId, f32>::new();
        for &edge_id in &segment.member_edges {
            let edge = &spatial.edges()[edge_id.raw() as usize];
            let [Some(first), Some(second)] = edge.cells else {
                continue;
            };
            if tectonic.crust_kind(first) != Some(CrustKind::Oceanic)
                || tectonic.crust_kind(second) != Some(CrustKind::Oceanic)
            {
                continue;
            }
            let first_plate = tectonic
                .plate_for_cell(first)
                .expect("validated tectonic field is cell aligned");
            let (trench_cell, boundary_arc_cell) = if first_plate == subducting {
                (first, second)
            } else {
                (second, first)
            };
            debug_assert_eq!(tectonic.plate_for_cell(boundary_arc_cell), Some(overriding));
            if topology.boundary_cells()[boundary_arc_cell.raw() as usize] {
                continue;
            }
            let arc_cell = inland_arc_cell(
                spatial,
                topology,
                tectonic,
                boundary_arc_cell,
                trench_cell,
                overriding,
            );
            let strength = tectonic
                .boundary_for_edge(edge_id)
                .expect("validated tectonic boundary is edge aligned")
                .strength;
            candidates
                .entry(arc_cell)
                .and_modify(|stored| *stored = stored.max(strength))
                .or_insert(strength);
        }
        if candidates.is_empty() {
            continue;
        }

        let noise = ReliefNoise2d::new(
            morphology_seed
                .wrapping_add(ARC_SEED_STEP.wrapping_mul(segment.id.raw().wrapping_add(1))),
        );
        let mut ranked = candidates
            .into_iter()
            .map(|(cell, strength)| {
                let center = topology.quantized_centers()[cell.raw() as usize];
                let point = [
                    center[0] as f64 / 1_000_000.0,
                    center[1] as f64 / 1_000_000.0,
                ];
                let warped = noise.warp(point, 3.0, 0.035);
                let fbm = ((noise.fbm(warped, ARC_FBM) + 1.0) * 0.5)
                    .clamp(0.0, 1.0)
                    .powf(2.0);
                let ridge = noise.ridged(warped, ARC_RIDGES).powf(2.5);
                let score = (0.68 * fbm + 0.32 * ridge).clamp(0.0, 1.0);
                (cell, strength, score)
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|first, second| {
            second
                .2
                .total_cmp(&first.2)
                .then_with(|| first.0.cmp(&second.0))
        });

        let target_count = ranked.len().div_ceil(4).max(1);
        let mut selected = Vec::with_capacity(target_count);
        for &(cell, strength, score) in &ranked {
            if selected
                .iter()
                .all(|&(other, _, _)| topology.edge_between(cell, other).is_none())
            {
                selected.push((cell, strength, score));
                if selected.len() == target_count {
                    break;
                }
            }
        }

        for (cell, strength, score) in selected {
            let amplitude = ((1_900.0 + 900.0 * score) * f64::from(strength)) as f32;
            let source = &mut result[cell.raw() as usize];
            *source = source.max(amplitude);
            for arc in &topology.arcs()[cell.raw() as usize] {
                if tectonic.plate_for_cell(arc.neighbor) != Some(overriding)
                    || tectonic.crust_kind(arc.neighbor) != Some(CrustKind::Oceanic)
                {
                    continue;
                }
                let shoulder = &mut result[arc.neighbor.raw() as usize];
                *shoulder = shoulder.max(amplitude * 0.22);
            }
        }
    }

    result
}

fn inland_arc_cell(
    spatial: &SpatialSnapshot,
    topology: &NaturalTopologyIndex,
    tectonic: &TectonicSnapshot,
    boundary_arc_cell: CellId,
    trench_cell: CellId,
    overriding_plate: crate::world::PlateId,
) -> CellId {
    let arc_center = spatial
        .cell(boundary_arc_cell)
        .expect("validated spatial IDs are contiguous")
        .centroid;
    let trench_center = spatial
        .cell(trench_cell)
        .expect("validated spatial IDs are contiguous")
        .centroid;
    let inward = [
        arc_center.x().get() - trench_center.x().get(),
        arc_center.y().get() - trench_center.y().get(),
    ];
    let inward_length = inward[0].hypot(inward[1]);

    topology.arcs()[boundary_arc_cell.raw() as usize]
        .iter()
        .filter(|arc| {
            !topology.boundary_cells()[arc.neighbor.raw() as usize]
                && tectonic.plate_for_cell(arc.neighbor) == Some(overriding_plate)
                && tectonic.crust_kind(arc.neighbor) == Some(CrustKind::Oceanic)
        })
        .filter_map(|arc| {
            let center = spatial
                .cell(arc.neighbor)
                .expect("validated spatial IDs are contiguous")
                .centroid;
            let delta = [
                center.x().get() - arc_center.x().get(),
                center.y().get() - arc_center.y().get(),
            ];
            let length = delta[0].hypot(delta[1]);
            (length > f64::EPSILON && inward_length > f64::EPSILON).then(|| {
                let alignment =
                    (delta[0] * inward[0] + delta[1] * inward[1]) / (length * inward_length);
                (arc.neighbor, alignment)
            })
        })
        .filter(|(_, alignment)| *alignment > 0.0)
        .max_by(|first, second| {
            first
                .1
                .total_cmp(&second.1)
                .then_with(|| second.0.cmp(&first.0))
        })
        .map(|(cell, _)| cell)
        .unwrap_or(boundary_arc_cell)
}

fn normalized_direction(velocity: [i16; 2]) -> Option<[f64; 2]> {
    let x = f64::from(velocity[0]);
    let y = f64::from(velocity[1]);
    let length = x.hypot(y);
    (length > f64::EPSILON).then_some([x / length, y / length])
}

fn directional_trail(
    point: [f64; 2],
    direction: Option<[f64; 2]>,
    speed_fraction: f64,
    noise: &ReliefNoise2d,
) -> f64 {
    let Some(direction) = direction else {
        return 0.0;
    };
    let along = point[0] * direction[0] + point[1] * direction[1];
    if along <= 0.08 {
        return 0.0;
    }
    let across = -point[0] * direction[1] + point[1] * direction[0];
    let reach = 0.48 + 0.32 * speed_fraction;
    if along >= reach {
        return 0.0;
    }

    let chain_signal = ((noise.fbm([along * 3.1, across * 5.3], HOTSPOT_FBM) + 1.0) * 0.5)
        .clamp(0.0, 1.0)
        .powf(2.0);
    let lateral_width = 0.13 + 0.05 * chain_signal;
    let lateral = compact_peak(across.abs() / lateral_width);
    let longitudinal = (std::f64::consts::PI * (along - 0.08) / (reach - 0.08))
        .sin()
        .max(0.0)
        .powf(0.8);
    let separated_summits = (0.5 + 0.5 * (along * 10.0 * std::f64::consts::PI).cos()).powf(2.4);
    lateral * longitudinal * (0.38 + 0.62 * separated_summits) * (0.72 + 0.28 * chain_signal)
}

fn compact_peak(normalized_distance: f64) -> f64 {
    if normalized_distance >= 1.0 {
        return 0.0;
    }
    let remaining = 1.0 - normalized_distance * normalized_distance;
    remaining * remaining
}
