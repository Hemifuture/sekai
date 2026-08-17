//! Locked P4 morphology and physical-structure quality gates.

use super::{MetricObservation, NaturalQualityReportBuilder, QualityBuildError};
use crate::world::natural::{
    GlobalCirculationSnapshot, LandOceanKind, NaturalQualityReport, PrimaryReliefSnapshot,
    QualityMetricId,
};
use crate::world::spatial::{canonical_east_north_basis, SphericalSurfaceSnapshot, SurfaceRef};

const METRIC_NAMESPACE: &str = "sekai.global-circulation-v1";
const METRIC_VERSION: u16 = 1;

pub fn evaluate_global_circulation_quality(
    surface: &SphericalSurfaceSnapshot,
    relief: &PrimaryReliefSnapshot,
    snapshot: &GlobalCirculationSnapshot,
) -> Result<NaturalQualityReport, QualityBuildError> {
    surface
        .validate()
        .map_err(|error| invalid_input("surface", error.to_string()))?;
    relief
        .validate()
        .map_err(|error| invalid_input("primary_relief", error.to_string()))?;
    snapshot
        .validate_against(surface)
        .map_err(|error| invalid_input("global_circulation", error.to_string()))?;
    let surface_ref = SurfaceRef::for_spherical(surface);
    if relief.surface_ref() != surface_ref || snapshot.surface_ref() != surface_ref {
        return Err(QualityBuildError::SurfaceMismatch {
            input: "global_circulation",
            found: snapshot.surface_ref(),
            expected: surface_ref,
        });
    }

    let fields = snapshot.fields();
    let lower = fields.near_surface_wind_m_s().values();
    let _upper = fields.upper_wind_m_s().ok_or_else(|| {
        invalid_input(
            "global_circulation",
            "C2 upper wind is unavailable".to_owned(),
        )
    })?;
    let shear = fields.vertical_wind_shear_m_s().ok_or_else(|| {
        invalid_input(
            "global_circulation",
            "C2 vertical shear is unavailable".to_owned(),
        )
    })?;
    let current = fields.surface_ocean_current_m_s().values();
    let mixed = fields.monthly_sea_surface_temperature_c().values();
    let thermocline = fields
        .monthly_thermocline_temperature_c()
        .ok_or_else(|| {
            invalid_input(
                "global_circulation",
                "C2 thermocline is unavailable".to_owned(),
            )
        })?
        .values();
    let depth = fields
        .monthly_thermocline_depth_m()
        .ok_or_else(|| {
            invalid_input(
                "global_circulation",
                "C2 thermocline depth is unavailable".to_owned(),
            )
        })?
        .values();
    let humidity = fields.monthly_specific_humidity().values();
    let precipitation = fields.monthly_precipitation_mm_day().values();

    let mut low_total = 0_u32;
    let mut low_easterly = 0_u32;
    let mut mid_total = 0_u32;
    let mut mid_westerly = 0_u32;
    let mut shear_square = 0.0_f64;
    let mut shear_count = 0_u32;
    let mut land_leakage = 0.0_f64;
    let mut ocean_gyre_total = 0_u32;
    let mut ocean_gyre_active = 0_u32;
    let mut mixed_warm_total = 0_u32;
    let mut mixed_warm = 0_u32;
    let mut depth_total = 0_u32;
    let mut depth_positive = 0_u32;
    let mut warm = Vec::new();
    let mut humid = Vec::new();
    let mut seasonal_latitude = Vec::new();
    let mut seasonal_temperature = Vec::new();
    let mut seasonal_phase_total = 0_u32;
    let mut seasonal_phase_correct = 0_u32;
    let terrain_gradient = surface_scalar_gradient(surface, relief.elevation_m());
    let mut orographic_uplift = Vec::new();
    let mut orographic_flow = Vec::new();
    let mut orographic_precipitation = Vec::new();

    for (cell_index, cell) in surface.cells().iter().enumerate() {
        let radial = cell.centroid.components();
        let latitude = radial[2].asin().to_degrees();
        let (east, _) = canonical_east_north_basis(cell.centroid);
        let land = relief.land_ocean().raw_values()[cell_index] == LandOceanKind::Land.raw();
        for month in 0..12 {
            let zonal = dot(lower[cell_index][month].map(f64::from), east);
            if (5.0..=30.0).contains(&latitude.abs()) {
                low_total += 1;
                low_easterly += u32::from(zonal < 0.0);
            }
            if (35.0..=60.0).contains(&latitude.abs()) {
                mid_total += 1;
                mid_westerly += u32::from(zonal > 0.0);
            }
            let shear_speed = norm(shear.values()[cell_index][month].map(f64::from));
            shear_square += shear_speed * shear_speed;
            shear_count += 1;
            let current_speed = norm(current[cell_index][month].map(f64::from));
            if land {
                land_leakage = land_leakage.max(current_speed);
            } else {
                if (15.0..=65.0).contains(&latitude.abs()) {
                    ocean_gyre_total += 1;
                    ocean_gyre_active += u32::from(current_speed > 1.0e-3);
                }
                if latitude.abs() <= 60.0 {
                    mixed_warm_total += 1;
                    mixed_warm +=
                        u32::from(mixed[cell_index][month] > thermocline[cell_index][month]);
                }
                warm.push(f64::from(mixed[cell_index][month]));
                humid.push(f64::from(humidity[cell_index][month]));
            }
            depth_total += 1;
            depth_positive += u32::from(depth[cell_index][month] > 0.0);
        }
        let seasonal_difference = f64::from(
            fields.monthly_air_temperature_c().values()[cell_index][6]
                - fields.monthly_air_temperature_c().values()[cell_index][0],
        );
        seasonal_latitude.push(latitude);
        seasonal_temperature.push(seasonal_difference);
        if latitude.abs() >= 10.0 {
            seasonal_phase_total += 1;
            seasonal_phase_correct += u32::from(latitude * seasonal_difference > 0.0);
        }
        if land {
            for month in 0..12 {
                let terrain_flow = dot(
                    lower[cell_index][month].map(f64::from),
                    terrain_gradient[cell_index],
                );
                orographic_uplift.push(terrain_flow.max(0.0));
                orographic_flow.push(terrain_flow);
                orographic_precipitation.push(f64::from(precipitation[cell_index][month]));
            }
        }
    }

    let seam_ratio = neighbor_speed_jump_ratio(surface, lower);
    let orographic_response = correlation(&orographic_uplift, &orographic_precipitation);
    let rain_shadow_response = correlation(&orographic_flow, &orographic_precipitation);
    let mut builder = NaturalQualityReportBuilder::new(surface_ref);
    record_at_least(
        &mut builder,
        "low-latitude-easterly-fraction",
        fraction(low_easterly, low_total),
        low_total,
        0.35,
    )?;
    record_at_least(
        &mut builder,
        "midlatitude-westerly-fraction",
        fraction(mid_westerly, mid_total),
        mid_total,
        0.55,
    )?;
    record_at_least(
        &mut builder,
        "vertical-shear-rms-m-s",
        (shear_square / f64::from(shear_count.max(1))).sqrt(),
        shear_count,
        0.10,
    )?;
    builder.record_at_most(
        metric_id("ocean-current-land-leakage-max-m-s")?,
        land_leakage,
        u32::try_from(surface.cells().len()).map_err(|_| QualityBuildError::SampleCountOverflow)?,
        0.25,
    )?;
    record_at_least(
        &mut builder,
        "ocean-gyre-circulation-fraction",
        fraction(ocean_gyre_active, ocean_gyre_total),
        ocean_gyre_total,
        0.20,
    )?;
    record_at_least(
        &mut builder,
        "mixed-layer-warmer-than-thermocline-fraction",
        fraction(mixed_warm, mixed_warm_total),
        mixed_warm_total,
        0.70,
    )?;
    record_at_least(
        &mut builder,
        "positive-thermocline-depth-fraction",
        fraction(depth_positive, depth_total),
        depth_total,
        1.0,
    )?;
    record_at_least(
        &mut builder,
        "warm-ocean-humidity-correlation",
        correlation(&warm, &humid),
        u32::try_from(warm.len()).map_err(|_| QualityBuildError::SampleCountOverflow)?,
        0.50,
    )?;
    record_at_least(
        &mut builder,
        "orographic-precipitation-response",
        orographic_response,
        u32::try_from(orographic_uplift.len())
            .map_err(|_| QualityBuildError::SampleCountOverflow)?,
        0.10,
    )?;
    record_at_least(
        &mut builder,
        "orographic-rain-shadow-correlation",
        rain_shadow_response,
        u32::try_from(orographic_flow.len()).map_err(|_| QualityBuildError::SampleCountOverflow)?,
        0.10,
    )?;
    record_at_least(
        &mut builder,
        "seasonal-hemisphere-phase-correlation",
        correlation(&seasonal_latitude, &seasonal_temperature).abs(),
        u32::try_from(seasonal_latitude.len())
            .map_err(|_| QualityBuildError::SampleCountOverflow)?,
        0.30,
    )?;
    record_at_least(
        &mut builder,
        "seasonal-hemisphere-phase-fraction",
        fraction(seasonal_phase_correct, seasonal_phase_total),
        seasonal_phase_total,
        0.65,
    )?;
    builder.record_at_most(
        metric_id("cubed-face-seam-speed-ratio")?,
        seam_ratio,
        u32::try_from(surface.edges().len()).map_err(|_| QualityBuildError::SampleCountOverflow)?,
        4.0,
    )?;
    builder.finish()
}

fn record_at_least(
    builder: &mut NaturalQualityReportBuilder,
    name: &'static str,
    value: f64,
    sample_count: u32,
    minimum: f64,
) -> Result<(), QualityBuildError> {
    builder.record_observation_at_least(
        metric_id(name)?,
        MetricObservation::Available {
            value,
            sample_count: sample_count.max(1),
        },
        minimum,
    )
}

fn metric_id(name: &str) -> Result<QualityMetricId, QualityBuildError> {
    Ok(QualityMetricId::new(
        METRIC_NAMESPACE,
        name,
        METRIC_VERSION,
    )?)
}

fn fraction(numerator: u32, denominator: u32) -> f64 {
    f64::from(numerator) / f64::from(denominator.max(1))
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

fn correlation(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let left_mean = mean(left);
    let right_mean = mean(right);
    let numerator = left
        .iter()
        .zip(right)
        .map(|(left, right)| (left - left_mean) * (right - right_mean))
        .sum::<f64>();
    let left_norm = left
        .iter()
        .map(|value| (value - left_mean).powi(2))
        .sum::<f64>()
        .sqrt();
    let right_norm = right
        .iter()
        .map(|value| (value - right_mean).powi(2))
        .sum::<f64>()
        .sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        (numerator / (left_norm * right_norm)).clamp(-1.0, 1.0)
    }
}

fn surface_scalar_gradient(surface: &SphericalSurfaceSnapshot, values: &[f32]) -> Vec<[f64; 3]> {
    let mut gradients = vec![[0.0; 3]; surface.cells().len()];
    let mut degrees = vec![0_u32; surface.cells().len()];
    for edge in surface.edges() {
        let first = edge.cells[0].raw() as usize;
        let second = edge.cells[1].raw() as usize;
        let first_radial = surface.cells()[first].centroid.components();
        let second_radial = surface.cells()[second].centroid.components();
        let first_direction = tangent_direction(first_radial, second_radial);
        let second_direction = tangent_direction(second_radial, first_radial);
        let slope = f64::from(values[second] - values[first]) / edge.center_distance.get();
        for component in 0..3 {
            gradients[first][component] += slope * first_direction[component];
            gradients[second][component] -= slope * second_direction[component];
        }
        degrees[first] += 1;
        degrees[second] += 1;
    }
    for (gradient, degree) in gradients.iter_mut().zip(degrees) {
        let divisor = f64::from(degree.max(1));
        for component in gradient {
            *component /= divisor;
        }
    }
    gradients
}

fn tangent_direction(origin: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    let radial_projection = dot(origin, target);
    let tangent =
        std::array::from_fn(|component| target[component] - radial_projection * origin[component]);
    let length = norm(tangent).max(1.0e-12);
    tangent.map(|component| component / length)
}

fn neighbor_speed_jump_ratio(surface: &SphericalSurfaceSnapshot, wind: &[[[f32; 3]; 12]]) -> f64 {
    let rms = (wind
        .iter()
        .flatten()
        .map(|vector| norm(vector.map(f64::from)).powi(2))
        .sum::<f64>()
        / (wind.len() * 12).max(1) as f64)
        .sqrt()
        .max(1.0e-9);
    surface
        .edges()
        .iter()
        .flat_map(|edge| {
            let first = edge.cells[0].raw() as usize;
            let second = edge.cells[1].raw() as usize;
            (0..12).map(move |month| {
                let difference = std::array::from_fn(|component| {
                    f64::from(wind[first][month][component] - wind[second][month][component])
                });
                norm(difference) / rms
            })
        })
        .fold(0.0_f64, f64::max)
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn invalid_input(input: &'static str, reason: String) -> QualityBuildError {
    QualityBuildError::InvalidInput { input, reason }
}
