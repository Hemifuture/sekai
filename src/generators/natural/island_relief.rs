use super::relief_noise::{FractalProfile, ReliefNoise2d};
use crate::world::natural::{
    CrustKind, MantleSnapshot, TectonicSnapshot, VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
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
    let mut result = vec![0.0_f32; spatial.cell_count()];

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
            let supported_background = 0.08 * mantle_envelope.powf(1.4);
            let morphology = current_edifice.max(trail).max(supported_background);
            if morphology <= 0.0 {
                continue;
            }

            let fbm = ((noise.fbm(warped, HOTSPOT_FBM) + 1.0) * 0.5)
                .clamp(0.0, 1.0)
                .powf(1.65);
            let ridges = noise.ridged(warped, HOTSPOT_RIDGES).powf(2.2);
            let surface_detail = 0.72 + 0.18 * fbm + 0.10 * ridges;
            let support = mantle_envelope.powf(0.35);
            let crust = tectonic
                .crust_kind(cell)
                .expect("validated tectonic field is cell aligned");
            let amplitude = match crust {
                CrustKind::Oceanic => VOLCANIC_OFFSET_MAX_M as f64,
                CrustKind::Continental => 2_400.0,
            };
            let strength_response = 0.55 + 0.45 * strength;
            let candidate =
                amplitude * strength_response * morphology.powf(1.15) * support * surface_detail;
            *value = value.max(candidate as f32);
        }

        // Preserve the current volcanic center as the dominant edifice. Noise
        // shapes its slopes and satellites, but cannot erase the causal source.
        let source_index = source.raw() as usize;
        if mantle.volcanic_influence()[source_index] > 0.0 {
            let source_amplitude = match tectonic
                .crust_kind(source)
                .expect("validated tectonic field is cell aligned")
            {
                CrustKind::Oceanic => VOLCANIC_OFFSET_MAX_M,
                CrustKind::Continental => 2_400.0,
            };
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
