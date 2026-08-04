use std::collections::BTreeMap;

use super::relief_noise::{FractalProfile, ReliefNoise3d};
use super::topology::{NaturalTopologyIndex, NeighborArc};
use crate::world::natural::{
    BoundaryKind, CrustKind, SphericalMantleSnapshot, SphericalTectonicSnapshot,
    VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
};
use crate::world::spatial::{
    central_angle, project_tangent, SphericalSurfaceSnapshot, UnitVector3,
};
use crate::world::{CellId, PlateId};

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
const HOTSPOT_TRAIL_COORDINATE_STRETCH: f64 = 5.3;
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
const ARC_PEAK_SCORE_THRESHOLD: f64 = 0.52;

/// Shapes authoritative mantle support into present-day spherical volcanic relief.
///
/// A source-centered tangent calculation supplies a short plate-motion cue. It
/// is disposable local geometry, not a stored reconstruction or global map axis.
pub(super) fn synthesize_spherical_hotspot_offset(
    surface: &SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
    mantle: &SphericalMantleSnapshot,
    morphology_seed: u32,
) -> Vec<f32> {
    let sample_spacing_m = representative_cell_spacing_m(surface);
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
            (f64::from(amplitude) * SUPPORTED_BACKGROUND_FRACTION * f64::from(influence).powi(2))
                as f32
        })
        .collect::<Vec<_>>();

    for hotspot in mantle.hotspots() {
        let source = hotspot.source_cell();
        let source_radial = surface
            .cell(source)
            .expect("validated hotspot source is surface aligned")
            .centroid;
        let source_plate = tectonic
            .plate_for_cell(source)
            .expect("validated tectonic field is cell aligned");
        let velocity = tectonic.plates()[source_plate.raw() as usize]
            .rotation()
            .velocity_mm_per_year(surface.radius(), source_radial)
            .expect("validated Euler rotation is radius compatible");
        let direction = normalize(velocity);
        let speed_fraction = (norm(velocity) / 120.0).clamp(0.0, 1.0);
        let support_radius_m = hotspot.support_radius_m().get();
        let surface_fbm = HOTSPOT_FBM.limited_to_resolution(support_radius_m, sample_spacing_m);
        let surface_ridges =
            HOTSPOT_RIDGES.limited_to_resolution(support_radius_m, sample_spacing_m);
        let trail_fbm = HOTSPOT_FBM.limited_to_resolution(
            support_radius_m / HOTSPOT_TRAIL_COORDINATE_STRETCH,
            sample_spacing_m,
        );
        let strength = f64::from(hotspot.strength_permille()) / 1_000.0;
        let noise = ReliefNoise3d::new(
            morphology_seed
                .wrapping_add(HOTSPOT_SEED_STEP.wrapping_mul(hotspot.id().raw().wrapping_add(1))),
        );

        for (index, value) in result.iter_mut().enumerate() {
            let mantle_envelope = f64::from(mantle.volcanic_influence()[index]);
            if mantle_envelope <= 0.0 {
                continue;
            }
            let cell = CellId::from_raw(index as u32);
            if tectonic.plate_for_cell(cell) != Some(source_plate) {
                continue;
            }
            let radial = surface
                .cell(cell)
                .expect("validated spherical cell IDs are contiguous")
                .centroid;
            let Some(local) = source_tangent_coordinate(
                source_radial,
                radial,
                surface.radius().get(),
                support_radius_m,
            ) else {
                continue;
            };
            let distance = norm(local);
            if distance > 1.15 {
                continue;
            }

            let current_edifice = compact_peak(distance / 0.48);
            let trail = directional_trail(local, direction, speed_fraction, &noise, trail_fbm);
            let morphology = current_edifice.max(trail);
            if morphology <= 0.0 {
                continue;
            }

            // Sample detail in the source-centered coordinate normalized by
            // hotspot support. Sampling the unit radial here would make the
            // physical wavelength depend on whole-planet radius instead.
            let fbm = ((noise.fbm(local, surface_fbm) + 1.0) * 0.5)
                .clamp(0.0, 1.0)
                .powf(1.65);
            let ridges = noise.ridged(local, surface_ridges).powf(2.2);
            let surface_detail = 0.72 + 0.18 * fbm + 0.10 * ridges;
            let support = mantle_envelope.powf(0.35);
            let amplitude = f64::from(volcanic_amplitude(tectonic, cell));
            let strength_response = 0.55 + 0.45 * strength;
            let candidate =
                amplitude * strength_response * morphology.powf(1.15) * support * surface_detail;
            *value = value.max(candidate as f32);
        }

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

/// Adds sparse oceanic island-arc summits on the overriding side of subduction.
pub(super) fn synthesize_spherical_oceanic_arc_peaks(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    tectonic: &SphericalTectonicSnapshot,
    morphology_seed: u32,
) -> Vec<f32> {
    let mut result = vec![0.0_f32; surface.cells().len()];
    let sample_spacing_m = representative_cell_spacing_m(surface);
    let arc_fbm = ARC_FBM.limited_to_resolution(surface.radius().get(), sample_spacing_m);
    let arc_ridges = ARC_RIDGES.limited_to_resolution(surface.radius().get(), sample_spacing_m);
    let mut candidate_scores = vec![None; surface.cells().len()];

    for segment in tectonic
        .boundary_segments()
        .iter()
        .filter(|segment| segment.kind() == BoundaryKind::Subduction)
    {
        let subducting = segment
            .subducting_plate()
            .expect("validated subduction segment has a descending plate");
        let plates = segment.plates();
        let overriding = if plates[0] == subducting {
            plates[1]
        } else {
            plates[0]
        };
        let mut candidates = BTreeMap::<CellId, f32>::new();
        for &edge_id in segment.member_edges() {
            let edge = surface
                .edge(edge_id)
                .expect("validated segment edge is surface aligned");
            let [first, second] = edge.cells;
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
            let arc_cell = inland_arc_cell(
                surface,
                topology,
                tectonic,
                boundary_arc_cell,
                trench_cell,
                overriding,
            );
            let strength = tectonic.boundaries()[edge_id.raw() as usize].strength;
            candidates
                .entry(arc_cell)
                .and_modify(|stored| *stored = stored.max(strength))
                .or_insert(strength);
        }
        if candidates.is_empty() {
            continue;
        }

        let noise = ReliefNoise3d::new(
            morphology_seed
                .wrapping_add(ARC_SEED_STEP.wrapping_mul(segment.id().raw().wrapping_add(1))),
        );
        let ranked = candidates
            .into_iter()
            .map(|(cell, strength)| {
                let point = surface
                    .cell(cell)
                    .expect("validated spherical cell IDs are contiguous")
                    .centroid
                    .components();
                let fbm = ((noise.fbm(point, arc_fbm) + 1.0) * 0.5)
                    .clamp(0.0, 1.0)
                    .powi(2);
                let ridge = noise.ridged(point, arc_ridges).powf(2.5);
                let score = (0.68 * fbm + 0.32 * ridge).clamp(0.0, 1.0);
                (cell, strength, score)
            })
            .collect::<Vec<_>>();
        let selected = select_sparse_arc_peaks(topology.arcs(), &ranked, &mut candidate_scores);

        for (cell, strength, score) in selected {
            let amplitude = ((1_900.0 + 900.0 * score) * f64::from(strength)) as f32;
            result[cell.raw() as usize] = result[cell.raw() as usize].max(amplitude);
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

fn volcanic_amplitude(tectonic: &SphericalTectonicSnapshot, cell: CellId) -> f32 {
    match tectonic
        .crust_kind(cell)
        .expect("validated tectonic field is cell aligned")
    {
        CrustKind::Oceanic => VOLCANIC_OFFSET_MAX_M,
        CrustKind::Continental => 2_400.0,
    }
}

fn source_tangent_coordinate(
    source: UnitVector3,
    target: UnitVector3,
    radius_m: f64,
    support_radius_m: f64,
) -> Option<[f64; 3]> {
    let angle = central_angle(source, target);
    if angle <= f64::EPSILON {
        return Some([0.0; 3]);
    }
    let direction = normalize(project_tangent(target.components(), source))?;
    let normalized_distance = angle * radius_m / support_radius_m;
    Some(direction.map(|component| component * normalized_distance))
}

fn directional_trail(
    point: [f64; 3],
    direction: Option<[f64; 3]>,
    speed_fraction: f64,
    noise: &ReliefNoise3d,
    profile: FractalProfile,
) -> f64 {
    let Some(direction) = direction else {
        return 0.0;
    };
    let along = dot(point, direction);
    if along <= 0.08 {
        return 0.0;
    }
    let distance_squared = dot(point, point);
    let across = (distance_squared - along * along).max(0.0).sqrt();
    let reach = 0.48 + 0.32 * speed_fraction;
    if along >= reach {
        return 0.0;
    }

    let chain_signal = ((noise.fbm(point, profile) + 1.0) * 0.5)
        .clamp(0.0, 1.0)
        .powi(2);
    let lateral_width = 0.13 + 0.05 * chain_signal;
    let lateral = compact_peak(across / lateral_width);
    let longitudinal = (std::f64::consts::PI * (along - 0.08) / (reach - 0.08))
        .sin()
        .max(0.0)
        .powf(0.8);
    let separated_summits = (0.5 + 0.5 * (along * 10.0 * std::f64::consts::PI).cos()).powf(2.4);
    lateral * longitudinal * (0.38 + 0.62 * separated_summits) * (0.72 + 0.28 * chain_signal)
}

type ArcPeakCandidate = (CellId, f32, f64);

fn select_sparse_arc_peaks(
    arcs: &[Vec<NeighborArc>],
    candidates: &[ArcPeakCandidate],
    candidate_scores: &mut [Option<f64>],
) -> Vec<ArcPeakCandidate> {
    let Some(stable_maximum) = candidates.iter().copied().reduce(|best, candidate| {
        if candidate.2 > best.2 || (candidate.2 == best.2 && candidate.0 < best.0) {
            candidate
        } else {
            best
        }
    }) else {
        return Vec::new();
    };
    for &(cell, _, score) in candidates {
        candidate_scores[cell.raw() as usize] = Some(score);
    }
    let mut selected = Vec::new();
    for &candidate @ (cell, _, score) in candidates {
        let is_fallback = cell == stable_maximum.0;
        if !is_fallback && score < ARC_PEAK_SCORE_THRESHOLD {
            continue;
        }
        let is_local_maximum = arcs[cell.raw() as usize].iter().all(|arc| {
            candidate_scores[arc.neighbor.raw() as usize].is_none_or(|neighbor_score| {
                score > neighbor_score || (score == neighbor_score && cell < arc.neighbor)
            })
        });
        if is_local_maximum {
            selected.push(candidate);
        }
    }
    for &(cell, _, _) in candidates {
        candidate_scores[cell.raw() as usize] = None;
    }
    selected
}

fn inland_arc_cell(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    tectonic: &SphericalTectonicSnapshot,
    boundary_arc_cell: CellId,
    trench_cell: CellId,
    overriding_plate: PlateId,
) -> CellId {
    let arc_radial = surface
        .cell(boundary_arc_cell)
        .expect("validated spherical cell IDs are contiguous")
        .centroid;
    let trench_radial = surface
        .cell(trench_cell)
        .expect("validated spherical cell IDs are contiguous")
        .centroid;
    let Some(toward_trench) = normalize(project_tangent(trench_radial.components(), arc_radial))
    else {
        return boundary_arc_cell;
    };
    let away_from_trench = toward_trench.map(|component| -component);

    topology.arcs()[boundary_arc_cell.raw() as usize]
        .iter()
        .filter(|arc| {
            tectonic.plate_for_cell(arc.neighbor) == Some(overriding_plate)
                && tectonic.crust_kind(arc.neighbor) == Some(CrustKind::Oceanic)
        })
        .filter_map(|arc| {
            let neighbor_radial = surface
                .cell(arc.neighbor)
                .expect("validated spherical cell IDs are contiguous")
                .centroid;
            normalize(project_tangent(neighbor_radial.components(), arc_radial))
                .map(|direction| (arc.neighbor, dot(direction, away_from_trench)))
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

fn representative_cell_spacing_m(surface: &SphericalSurfaceSnapshot) -> f64 {
    (surface.total_cell_area().get() / surface.cells().len() as f64).sqrt()
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = norm(vector);
    (length > f64::EPSILON).then(|| vector.map(|component| component / length))
}

fn compact_peak(normalized_distance: f64) -> f64 {
    if normalized_distance >= 1.0 {
        return 0.0;
    }
    let remaining = 1.0 - normalized_distance * normalized_distance;
    remaining * remaining
}
