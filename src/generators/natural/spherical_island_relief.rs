use super::fractal::FractalProfile;
use super::morphology::noise::SphericalNoise3d;
use crate::world::natural::{
    CrustKind, CrustKindField, PlateIdField, SphericalMantleSnapshot, SphericalPlate,
    VOLCANIC_OFFSET_MAX_M,
};
use crate::world::spatial::{
    central_angle, project_tangent, SphericalSurfaceSnapshot, UnitVector3,
};
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
const HOTSPOT_TRAIL_COORDINATE_STRETCH: f64 = 5.3;
const SUPPORTED_BACKGROUND_FRACTION: f64 = 0.015;

/// Shapes authoritative mantle support into present-day spherical volcanic relief.
///
/// A source-centered tangent calculation supplies a short plate-motion cue. It
/// is disposable local geometry, not a stored reconstruction or global map axis.
pub(super) fn synthesize_spherical_hotspot_offset(
    surface: &SphericalSurfaceSnapshot,
    plates: &[SphericalPlate],
    cell_plates: &PlateIdField,
    crust_kinds: &CrustKindField,
    mantle: &SphericalMantleSnapshot,
    morphology_seed: u32,
) -> Vec<f64> {
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
            let amplitude = volcanic_amplitude(crust_kinds, cell);
            amplitude * SUPPORTED_BACKGROUND_FRACTION * f64::from(influence).powi(2)
        })
        .collect::<Vec<_>>();

    for hotspot in mantle.hotspots() {
        let source = hotspot.source_cell();
        let source_radial = surface
            .cell(source)
            .expect("validated hotspot source is surface aligned")
            .centroid;
        let source_plate = cell_plates
            .get(source.raw() as usize)
            .expect("validated tectonic field is cell aligned");
        let velocity = plates[source_plate.raw() as usize]
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
        let noise = SphericalNoise3d::new(
            morphology_seed
                .wrapping_add(HOTSPOT_SEED_STEP.wrapping_mul(hotspot.id().raw().wrapping_add(1))),
        );

        for (index, value) in result.iter_mut().enumerate() {
            let mantle_envelope = f64::from(mantle.volcanic_influence()[index]);
            if mantle_envelope <= 0.0 {
                continue;
            }
            let cell = CellId::from_raw(index as u32);
            if cell_plates.get(cell.raw() as usize) != Some(source_plate) {
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
            let fbm = ((noise.fbm_coordinate(local, surface_fbm) + 1.0) * 0.5)
                .clamp(0.0, 1.0)
                .powf(1.65);
            let ridges = noise.ridged_coordinate(local, surface_ridges).powf(2.2);
            let surface_detail = 0.72 + 0.18 * fbm + 0.10 * ridges;
            let support = mantle_envelope.powf(0.35);
            let amplitude = volcanic_amplitude(crust_kinds, cell);
            let strength_response = 0.55 + 0.45 * strength;
            let candidate =
                amplitude * strength_response * morphology.powf(1.15) * support * surface_detail;
            *value = value.max(candidate);
        }

        let source_index = source.raw() as usize;
        if mantle.volcanic_influence()[source_index] > 0.0 {
            let source_amplitude = volcanic_amplitude(crust_kinds, source);
            result[source_index] = result[source_index].max(source_amplitude * strength);
        }
    }

    for (index, value) in result.iter_mut().enumerate() {
        if mantle.volcanic_influence()[index] <= 0.0 {
            *value = 0.0;
        }
    }
    result
}

fn volcanic_amplitude(crust_kinds: &CrustKindField, cell: CellId) -> f64 {
    match crust_kinds
        .get(cell.raw() as usize)
        .expect("validated tectonic field is cell aligned")
    {
        CrustKind::Oceanic => f64::from(VOLCANIC_OFFSET_MAX_M),
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
    noise: &SphericalNoise3d,
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

    let chain_signal = ((noise.fbm_coordinate(point, profile) + 1.0) * 0.5)
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
