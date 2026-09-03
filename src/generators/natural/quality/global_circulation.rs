//! P4 morphology diagnostics and hard physical-closure quality gates.

use super::{MetricAccumulator, MetricObservation, NaturalQualityReportBuilder, QualityBuildError};
use crate::engine::BuildCancellation;
use crate::generators::natural::formation::global_circulation::GlobalClimateForcing;
use crate::world::natural::{
    FormationTerrainFields, GlobalCirculationSnapshot, LandOceanField, LandOceanKind,
    NaturalQualityReport, PrimaryReliefSnapshot, QualityMetricId, QualityMetricStatus,
    CLIMATE_MONTH_COUNT, GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2,
    GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX,
};
use crate::world::spatial::{canonical_east_north_basis, SphericalSurfaceSnapshot, SurfaceRef};

const METRIC_NAMESPACE: &str = "sekai.global-circulation-v1";
const METRIC_VERSION: u16 = 1;
const SEASONAL_FORCING_AMPLITUDE_MIN_C: f64 = 0.5;
const LOW_LATITUDE_WIND_MIN_ABS_DEGREES: f64 = 5.0;
const TROPICAL_MAX_ABS_LATITUDE_DEGREES: f64 = 30.0;
const MIDLATITUDE_WIND_MIN_ABS_DEGREES: f64 = 35.0;
const HIGH_LATITUDE_MIN_ABS_DEGREES: f64 = 60.0;
const TEMPERATURE_SEASONAL_PHASE_MIN_ABS_DEGREES: f64 = 10.0;
const NO_RESOLVED_SEASONAL_FORCING_REASON: &str =
    "January-July equilibrium-air-temperature amplitude is below 0.5 C";
const NO_HIGH_LATITUDE_PRECIPITATION_REASON: &str =
    "high-latitude annual-mean precipitation is zero";

/// The final terrain authority sampled by the shared P4 quality metrics.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ClimateQualityTerrain<'a> {
    Primary(&'a PrimaryReliefSnapshot),
    Formation(&'a FormationTerrainFields),
}

impl<'a> ClimateQualityTerrain<'a> {
    fn surface_ref(self) -> SurfaceRef {
        match self {
            Self::Primary(relief) => relief.surface_ref(),
            Self::Formation(terrain) => terrain.surface_water_geometry().surface_ref(),
        }
    }

    fn land_ocean(self) -> &'a LandOceanField {
        match self {
            Self::Primary(relief) => relief.land_ocean(),
            Self::Formation(terrain) => terrain.land_ocean(),
        }
    }

    fn elevation_m(self) -> &'a [f32] {
        match self {
            Self::Primary(relief) => relief.elevation_m(),
            Self::Formation(terrain) => terrain.current_elevation_m(),
        }
    }

    fn validate(self) -> Result<(), QualityBuildError> {
        match self {
            Self::Primary(relief) => relief
                .validate()
                .map_err(|error| invalid_input("primary_relief", error.to_string())),
            Self::Formation(terrain) => terrain
                .validate()
                .map_err(|error| invalid_input("formation_terrain", error.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExpectedMetric {
    minimum: Option<f64>,
    maximum: Option<f64>,
    hard: bool,
}

macro_rules! declare_expected_metrics {
    ($count:expr; $($name:literal => ($minimum:expr, $maximum:expr, $hard:expr)),+ $(,)?) => {
        const EXPECTED_METRIC_NAMES: [&str; $count] = [$($name),+];

        fn expected_metric(name: &str) -> Option<ExpectedMetric> {
            match name {
                $($name => Some(ExpectedMetric {
                    minimum: $minimum,
                    maximum: $maximum,
                    hard: $hard,
                }),)+
                _ => None,
            }
        }
    };
}

// This declaration is the sole P4 metric-bound registry. Its order is part of
// the canonical report identity, while every caller resolves bounds by name.
declare_expected_metrics! {
    26;
    "absorbed-shortwave-global-mean-w-m2" => (None, None, false),
    "cubed-face-seam-speed-ratio" => (None, Some(4.0), false),
    "evaporation-global-mean-mm-day" => (None, None, false),
    "evaporation-precipitation-relative-imbalance" => (
        None,
        Some(GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX),
        true
    ),
    "low-latitude-easterly-fraction" => (Some(0.35), None, false),
    "midlatitude-westerly-fraction" => (Some(0.55), None, false),
    "mixed-layer-warmer-than-thermocline-fraction" => (Some(0.70), None, false),
    "near-surface-wind-non-zonal-variance-fraction" => (None, None, false),
    "ocean-current-land-leakage-max-m-s" => (None, Some(0.0), true),
    "ocean-gyre-circulation-fraction" => (Some(0.20), None, false),
    "orographic-precipitation-response" => (Some(0.01), None, false),
    "orographic-rain-shadow-leeward-drying" => (Some(0.02), None, false),
    "orographic-uplift-enrichment-ratio" => (Some(1.20), None, false),
    "outgoing-longwave-global-mean-w-m2" => (None, None, false),
    "planetary-albedo-global-mean" => (None, None, false),
    "positive-thermocline-depth-fraction" => (Some(1.0), None, false),
    "precipitation-global-mean-mm-day" => (None, None, false),
    "precipitation-low-to-high-latitude-ratio" => (None, None, false),
    "precipitation-seasonal-hemisphere-phase-fraction" => (None, None, false),
    "sea-surface-height-max-absolute-m" => (Some(0.01), Some(6.0), false),
    "seasonal-hemisphere-phase-correlation" => (None, None, false),
    "seasonal-hemisphere-phase-fraction" => (Some(0.65), None, false),
    "toa-net-radiation-global-mean-w-m2" => (
        Some(-GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2),
        Some(GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2),
        true
    ),
    "vertical-shear-rms-m-s" => (Some(0.10), None, false),
    "warm-ocean-humidity-contrast" => (Some(0.10), None, false),
    "warm-ocean-humidity-correlation" => (None, None, false),
}

pub fn evaluate_global_circulation_quality(
    surface: &SphericalSurfaceSnapshot,
    relief: &PrimaryReliefSnapshot,
    forcing: &GlobalClimateForcing,
    snapshot: &GlobalCirculationSnapshot,
) -> Result<NaturalQualityReport, QualityBuildError> {
    evaluate_global_circulation_quality_impl(
        surface,
        ClimateQualityTerrain::Primary(relief),
        forcing,
        snapshot,
        None,
    )
}

/// Cancellation-aware production path for Standard/High stage execution.
pub fn evaluate_global_circulation_quality_cancellable(
    surface: &SphericalSurfaceSnapshot,
    relief: &PrimaryReliefSnapshot,
    forcing: &GlobalClimateForcing,
    snapshot: &GlobalCirculationSnapshot,
    cancellation: &BuildCancellation,
) -> Result<NaturalQualityReport, QualityBuildError> {
    evaluate_global_circulation_quality_impl(
        surface,
        ClimateQualityTerrain::Primary(relief),
        forcing,
        snapshot,
        Some(cancellation),
    )
}

pub(crate) fn evaluate_global_circulation_quality_for_formation_cancellable(
    surface: &SphericalSurfaceSnapshot,
    terrain: &FormationTerrainFields,
    forcing: &GlobalClimateForcing,
    snapshot: &GlobalCirculationSnapshot,
    cancellation: &BuildCancellation,
) -> Result<NaturalQualityReport, QualityBuildError> {
    evaluate_global_circulation_quality_impl(
        surface,
        ClimateQualityTerrain::Formation(terrain),
        forcing,
        snapshot,
        Some(cancellation),
    )
}

fn evaluate_global_circulation_quality_impl(
    surface: &SphericalSurfaceSnapshot,
    terrain: ClimateQualityTerrain<'_>,
    forcing: &GlobalClimateForcing,
    snapshot: &GlobalCirculationSnapshot,
    cancellation: Option<&BuildCancellation>,
) -> Result<NaturalQualityReport, QualityBuildError> {
    check_quality_cancelled(cancellation)?;
    // Typed snapshots are immutable and their constructors/deserializers have
    // already established internal invariants. The uncancellable public audit
    // retains full revalidation; the production path performs only binding
    // checks here so cancellation latency is governed by the polled scans
    // below rather than by a second topology/relief audit.
    if cancellation.is_none() {
        surface
            .validate()
            .map_err(|error| invalid_input("surface", error.to_string()))?;
        terrain.validate()?;
    }
    check_quality_cancelled(cancellation)?;
    let snapshot_validation = if let Some(cancellation) = cancellation {
        snapshot.validate_against_cancellable(surface, &|| cancellation.is_cancelled())
    } else {
        snapshot.validate_against(surface)
    };
    snapshot_validation.map_err(|error| {
        if error == crate::world::natural::GlobalCirculationValidationError::Cancelled {
            QualityBuildError::Cancelled
        } else {
            invalid_input("global_circulation", error.to_string())
        }
    })?;
    check_quality_cancelled(cancellation)?;
    let surface_ref = SurfaceRef::from_validated_spherical(surface)
        .map_err(|error| invalid_input("surface", error.to_string()))?;
    if terrain.surface_ref() != surface_ref || snapshot.surface_ref() != surface_ref {
        return Err(QualityBuildError::SurfaceMismatch {
            input: "global_circulation",
            found: snapshot.surface_ref(),
            expected: surface_ref,
        });
    }
    let terrain_binding = match (terrain, cancellation) {
        (ClimateQualityTerrain::Primary(relief), Some(cancellation)) => {
            forcing.validate_relief_identity_cancellable(relief, cancellation)
        }
        (ClimateQualityTerrain::Primary(relief), None) => forcing.validate_relief_identity(relief),
        (ClimateQualityTerrain::Formation(terrain), Some(cancellation)) => {
            forcing.validate_formation_terrain_identity_cancellable(terrain, cancellation)
        }
        (ClimateQualityTerrain::Formation(terrain), None) => {
            forcing.validate_formation_terrain_identity(terrain)
        }
    };
    terrain_binding.map_err(|error| {
        if error == crate::generators::natural::GlobalClimateForcingError::Cancelled {
            QualityBuildError::Cancelled
        } else {
            invalid_input("climate_terrain", error.to_string())
        }
    })?;
    if forcing.fingerprint() != snapshot.checkpoint().forcing_fingerprint() {
        return Err(invalid_input(
            "global_climate_forcing",
            "forcing fingerprint does not match the circulation checkpoint".to_owned(),
        ));
    }

    let fields = snapshot.fields();
    let lower = fields.near_surface_wind_m_s().values();
    // The over-flow wind: orographic lifting and its rain shadow are defined
    // relative to the air that crosses the ridge, which with a terrain-aware
    // lower layer is the upper layer (design 2026-09-02 A3 §4).
    let upper = fields
        .upper_wind_m_s()
        .ok_or_else(|| {
            invalid_input(
                "global_circulation",
                "C2 upper wind is unavailable".to_owned(),
            )
        })?
        .values();
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
    let orographic_precipitation = fields.monthly_orographic_precipitation_mm_day().values();
    let sea_surface_height = fields.monthly_sea_surface_height_anomaly_m().values();

    let mut low_total = 0_u32;
    let mut low_easterly = 0_u32;
    let mut mid_total = 0_u32;
    let mut mid_westerly = 0_u32;
    let mut shear_square = 0.0_f64;
    let mut shear_count = 0_u32;
    let mut land_leakage = 0.0_f64;
    let mut mixed_warm_total = 0_u32;
    let mut mixed_warm = 0_u32;
    let mut depth_total = 0_u32;
    let mut depth_positive = 0_u32;
    let mut sea_surface_height_max_absolute = 0.0_f64;
    let mut sea_surface_height_count = 0_u32;
    let mut warm = Vec::new();
    let mut humid = Vec::new();
    let mut seasonal_latitude = Vec::new();
    let mut seasonal_temperature = Vec::new();
    let mut seasonal_phase_total = 0_u32;
    let mut seasonal_phase_correct = 0_u32;
    let mut low_latitude_precipitation = MetricAccumulator::new();
    let mut high_latitude_precipitation = MetricAccumulator::new();
    let mut annual_wind = NonZonalWindAccumulator::new();
    let mut precipitation_seasonal_phase = MetricAccumulator::new();
    let seasonal_forcing_amplitude_c = forcing
        .planet_forcing()
        .equilibrium_air_temperature_c()
        .iter()
        .map(|months| f64::from(months[6]) - f64::from(months[0]))
        .map(f64::abs)
        .fold(0.0_f64, f64::max);
    let seasonal_forcing_is_resolved =
        seasonal_forcing_amplitude_c >= SEASONAL_FORCING_AMPLITUDE_MIN_C;

    for (cell_index, cell) in surface.cells().iter().enumerate() {
        if cell_index % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        let radial = cell.centroid.components();
        let latitude = radial[2].asin().to_degrees();
        let absolute_latitude = latitude.abs();
        let area_m2 = cell.area.get();
        let (east, _) = canonical_east_north_basis(cell.centroid);
        let land = terrain.land_ocean().raw_values()[cell_index] == LandOceanKind::Land.raw();
        {
            let (east, north) = canonical_east_north_basis(cell.centroid);
            let mut annual = [0.0_f64; 3];
            for month in &lower[cell_index] {
                for (component, value) in annual.iter_mut().zip(month) {
                    *component += f64::from(*value) / 12.0;
                }
            }
            annual_wind.push(latitude, area_m2, dot(annual, east), dot(annual, north));
        }
        for month in 0..12 {
            let zonal = dot(lower[cell_index][month].map(f64::from), east);
            if (LOW_LATITUDE_WIND_MIN_ABS_DEGREES..=TROPICAL_MAX_ABS_LATITUDE_DEGREES)
                .contains(&absolute_latitude)
            {
                low_total += 1;
                low_easterly += u32::from(zonal < 0.0);
            }
            if (MIDLATITUDE_WIND_MIN_ABS_DEGREES..=HIGH_LATITUDE_MIN_ABS_DEGREES)
                .contains(&absolute_latitude)
            {
                mid_total += 1;
                mid_westerly += u32::from(zonal > 0.0);
            }
            let precipitation_mm_day = f64::from(precipitation[cell_index][month]);
            if absolute_latitude <= TROPICAL_MAX_ABS_LATITUDE_DEGREES {
                low_latitude_precipitation.push(precipitation_mm_day, area_m2)?;
            }
            if absolute_latitude >= HIGH_LATITUDE_MIN_ABS_DEGREES {
                high_latitude_precipitation.push(precipitation_mm_day, area_m2)?;
            }
            let shear_speed = norm(shear.values()[cell_index][month].map(f64::from));
            shear_square += shear_speed * shear_speed;
            shear_count += 1;
            let current_speed = norm(current[cell_index][month].map(f64::from));
            if land {
                land_leakage = land_leakage.max(current_speed);
            } else {
                if absolute_latitude <= HIGH_LATITUDE_MIN_ABS_DEGREES {
                    mixed_warm_total += 1;
                    mixed_warm +=
                        u32::from(mixed[cell_index][month] > thermocline[cell_index][month]);
                }
                warm.push(f64::from(mixed[cell_index][month]));
                humid.push(f64::from(humidity[cell_index][month]));
                sea_surface_height_max_absolute = sea_surface_height_max_absolute
                    .max(f64::from(sea_surface_height[cell_index][month]).abs());
                sea_surface_height_count += 1;
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
        if absolute_latitude >= TEMPERATURE_SEASONAL_PHASE_MIN_ABS_DEGREES {
            seasonal_phase_total += 1;
            seasonal_phase_correct += u32::from(latitude * seasonal_difference > 0.0);
        }
        if latitude != 0.0 && absolute_latitude <= TROPICAL_MAX_ABS_LATITUDE_DEGREES {
            record_precipitation_seasonal_phase_signal(
                &mut precipitation_seasonal_phase,
                latitude,
                f64::from(precipitation[cell_index][0]),
                f64::from(precipitation[cell_index][6]),
                area_m2,
            )?;
        }
    }

    let seam_ratio = neighbor_speed_jump_ratio(surface, lower, cancellation)?;
    let (gyre_fraction, gyre_samples) =
        basin_gyre_circulation(surface, terrain, current, cancellation)?;
    let orographic =
        orographic_neighbor_response(surface, terrain, upper, precipitation, cancellation)?;
    let (orographic_fraction, orographic_samples) = orographic_precipitation_fraction(
        surface,
        precipitation,
        orographic_precipitation,
        cancellation,
    )?;
    let orographic_uplift = orographic_uplift_enrichment(
        surface,
        terrain,
        upper,
        orographic_precipitation,
        cancellation,
    )?;
    let precipitation_low_to_high = ratio_observation(
        low_latitude_precipitation.finish()?,
        high_latitude_precipitation.finish()?,
    )?;
    let precipitation_seasonal_phase = precipitation_seasonal_phase.finish()?;
    check_quality_cancelled(cancellation)?;
    let budget = snapshot.budget_report();
    let seasonal_correlation = if seasonal_forcing_is_resolved {
        MetricObservation::Available {
            value: correlation(&seasonal_latitude, &seasonal_temperature, cancellation)?.abs(),
            sample_count: u32::try_from(seasonal_latitude.len())
                .map_err(|_| QualityBuildError::SampleCountOverflow)?,
        }
    } else {
        unavailable_seasonal_observation()
    };
    let seasonal_fraction = if seasonal_forcing_is_resolved {
        MetricObservation::Available {
            value: fraction(seasonal_phase_correct, seasonal_phase_total),
            sample_count: seasonal_phase_total,
        }
    } else {
        unavailable_seasonal_observation()
    };
    let precipitation_seasonal_phase = if seasonal_forcing_is_resolved {
        precipitation_seasonal_phase
    } else {
        unavailable_seasonal_observation()
    };
    let mut builder = NaturalQualityReportBuilder::new(surface_ref);
    record_expected_metric(
        &mut builder,
        "absorbed-shortwave-global-mean-w-m2",
        available(budget.absorbed_shortwave_global_mean_w_m2(), 1),
    )?;
    record_expected_metric(
        &mut builder,
        "cubed-face-seam-speed-ratio",
        available(
            seam_ratio,
            u32::try_from(surface.edges().len())
                .map_err(|_| QualityBuildError::SampleCountOverflow)?,
        ),
    )?;
    record_expected_metric(
        &mut builder,
        "evaporation-global-mean-mm-day",
        available(budget.evaporation_global_mean_mm_day(), 1),
    )?;
    record_expected_metric(
        &mut builder,
        "evaporation-precipitation-relative-imbalance",
        available(budget.evaporation_precipitation_relative_imbalance(), 1),
    )?;
    record_expected_metric(
        &mut builder,
        "low-latitude-easterly-fraction",
        available(fraction(low_easterly, low_total), low_total),
    )?;
    record_expected_metric(
        &mut builder,
        "midlatitude-westerly-fraction",
        available(fraction(mid_westerly, mid_total), mid_total),
    )?;
    record_expected_metric(
        &mut builder,
        "mixed-layer-warmer-than-thermocline-fraction",
        available(fraction(mixed_warm, mixed_warm_total), mixed_warm_total),
    )?;
    record_expected_metric(
        &mut builder,
        "near-surface-wind-non-zonal-variance-fraction",
        available(
            annual_wind.non_zonal_variance_fraction(),
            u32::try_from(surface.cells().len())
                .map_err(|_| QualityBuildError::SampleCountOverflow)?,
        ),
    )?;
    record_expected_metric(
        &mut builder,
        "ocean-current-land-leakage-max-m-s",
        available(
            land_leakage,
            u32::try_from(surface.cells().len())
                .map_err(|_| QualityBuildError::SampleCountOverflow)?,
        ),
    )?;
    record_expected_metric(
        &mut builder,
        "ocean-gyre-circulation-fraction",
        available(gyre_fraction, gyre_samples),
    )?;
    record_expected_metric(
        &mut builder,
        "orographic-precipitation-response",
        available(orographic_fraction, orographic_samples),
    )?;
    record_expected_metric(
        &mut builder,
        "orographic-rain-shadow-leeward-drying",
        available(orographic.leeward_drying, orographic.leeward_samples),
    )?;
    record_expected_metric(
        &mut builder,
        "orographic-uplift-enrichment-ratio",
        available(
            orographic_uplift.enrichment_ratio,
            orographic_uplift.supported_samples,
        ),
    )?;
    record_expected_metric(
        &mut builder,
        "outgoing-longwave-global-mean-w-m2",
        available(budget.outgoing_longwave_global_mean_w_m2(), 1),
    )?;
    record_expected_metric(
        &mut builder,
        "planetary-albedo-global-mean",
        available(budget.planetary_albedo_global_mean(), 1),
    )?;
    record_expected_metric(
        &mut builder,
        "positive-thermocline-depth-fraction",
        available(fraction(depth_positive, depth_total), depth_total),
    )?;
    record_expected_metric(
        &mut builder,
        "precipitation-global-mean-mm-day",
        available(budget.precipitation_global_mean_mm_day(), 1),
    )?;
    record_expected_metric(
        &mut builder,
        "precipitation-low-to-high-latitude-ratio",
        precipitation_low_to_high,
    )?;
    record_expected_metric(
        &mut builder,
        "precipitation-seasonal-hemisphere-phase-fraction",
        precipitation_seasonal_phase,
    )?;
    record_expected_metric(
        &mut builder,
        "sea-surface-height-max-absolute-m",
        available(
            sea_surface_height_max_absolute,
            sea_surface_height_count.max(1),
        ),
    )?;
    record_expected_metric(
        &mut builder,
        "seasonal-hemisphere-phase-correlation",
        seasonal_correlation,
    )?;
    record_expected_metric(
        &mut builder,
        "seasonal-hemisphere-phase-fraction",
        seasonal_fraction,
    )?;
    record_expected_metric(
        &mut builder,
        "toa-net-radiation-global-mean-w-m2",
        available(budget.toa_net_radiation_global_mean_w_m2(), 1),
    )?;
    record_expected_metric(
        &mut builder,
        "vertical-shear-rms-m-s",
        available(
            (shear_square / f64::from(shear_count.max(1))).sqrt(),
            shear_count,
        ),
    )?;
    record_expected_metric(
        &mut builder,
        "warm-ocean-humidity-contrast",
        available(
            interquartile_response(&warm, &humid, cancellation)?,
            u32::try_from(warm.len()).map_err(|_| QualityBuildError::SampleCountOverflow)?,
        ),
    )?;
    record_expected_metric(
        &mut builder,
        "warm-ocean-humidity-correlation",
        available(
            correlation(&warm, &humid, cancellation)?,
            u32::try_from(warm.len()).map_err(|_| QualityBuildError::SampleCountOverflow)?,
        ),
    )?;
    Ok(builder
        .finish()?
        .bind_subject_fingerprint(*snapshot.checkpoint().fingerprint())?)
}

fn record_expected_metric(
    builder: &mut NaturalQualityReportBuilder,
    name: &'static str,
    observation: MetricObservation,
) -> Result<(), QualityBuildError> {
    let expected = expected_metric(name).expect("recorded P4 metric belongs to its registry");
    match (expected.minimum, expected.maximum) {
        (None, None) => builder.record_observation_unbounded(metric_id(name)?, observation),
        (Some(minimum), None) => {
            builder.record_observation_at_least(metric_id(name)?, observation, minimum)
        }
        (None, Some(maximum)) => {
            builder.record_observation_at_most(metric_id(name)?, observation, maximum)
        }
        (Some(minimum), Some(maximum)) => {
            builder.record_observation_between(metric_id(name)?, observation, minimum, maximum)
        }
    }
}

fn available(value: f64, sample_count: u32) -> MetricObservation {
    MetricObservation::Available {
        value,
        sample_count: sample_count.max(1),
    }
}

fn unavailable_seasonal_observation() -> MetricObservation {
    MetricObservation::Unavailable {
        reason: NO_RESOLVED_SEASONAL_FORCING_REASON.to_owned(),
    }
}

fn ratio_observation(
    numerator: MetricObservation,
    denominator: MetricObservation,
) -> Result<MetricObservation, QualityBuildError> {
    match (numerator, denominator) {
        (
            MetricObservation::Available {
                value: numerator,
                sample_count: numerator_samples,
            },
            MetricObservation::Available {
                value: denominator,
                sample_count: denominator_samples,
            },
        ) if denominator > 0.0 => Ok(MetricObservation::Available {
            value: numerator / denominator,
            sample_count: numerator_samples
                .checked_add(denominator_samples)
                .ok_or(QualityBuildError::SampleCountOverflow)?,
        }),
        (_, MetricObservation::Available { value: 0.0, .. }) => {
            Ok(MetricObservation::Unavailable {
                reason: NO_HIGH_LATITUDE_PRECIPITATION_REASON.to_owned(),
            })
        }
        (MetricObservation::Unavailable { reason }, _)
        | (_, MetricObservation::Unavailable { reason }) => {
            Ok(MetricObservation::Unavailable { reason })
        }
        (_, MetricObservation::Available { .. }) => {
            unreachable!("precipitation means are validated nonnegative")
        }
    }
}

fn record_precipitation_seasonal_phase_signal(
    accumulator: &mut MetricAccumulator,
    latitude_degrees: f64,
    january_precipitation_mm_day: f64,
    july_precipitation_mm_day: f64,
    area_m2: f64,
) -> Result<(), QualityBuildError> {
    let seasonal_difference = july_precipitation_mm_day - january_precipitation_mm_day;
    accumulator.push(
        f64::from(latitude_degrees * seasonal_difference > 0.0),
        area_m2 * seasonal_difference.abs(),
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

fn mean(
    values: &[f64],
    cancellation: Option<&BuildCancellation>,
) -> Result<f64, QualityBuildError> {
    let mut sum = 0.0_f64;
    for (index, value) in values.iter().copied().enumerate() {
        if index % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        sum += value;
    }
    Ok(sum / values.len().max(1) as f64)
}

fn correlation(
    left: &[f64],
    right: &[f64],
    cancellation: Option<&BuildCancellation>,
) -> Result<f64, QualityBuildError> {
    if left.len() != right.len() || left.is_empty() {
        return Ok(0.0);
    }
    let left_mean = mean(left, cancellation)?;
    let right_mean = mean(right, cancellation)?;
    let mut numerator = 0.0_f64;
    let mut left_square = 0.0_f64;
    let mut right_square = 0.0_f64;
    for (index, (left, right)) in left.iter().zip(right).enumerate() {
        if index % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        let left_delta = left - left_mean;
        let right_delta = right - right_mean;
        numerator += left_delta * right_delta;
        left_square += left_delta * left_delta;
        right_square += right_delta * right_delta;
    }
    let left_norm = left_square.sqrt();
    let right_norm = right_square.sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        Ok(0.0)
    } else {
        Ok((numerator / (left_norm * right_norm)).clamp(-1.0, 1.0))
    }
}

fn interquartile_response(
    driver: &[f64],
    response: &[f64],
    cancellation: Option<&BuildCancellation>,
) -> Result<f64, QualityBuildError> {
    if driver.len() != response.len() || driver.len() < 4 {
        return Ok(0.0);
    }
    let mut pairs = Vec::with_capacity(driver.len());
    for (index, pair) in driver
        .iter()
        .copied()
        .zip(response.iter().copied())
        .enumerate()
    {
        if index % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        pairs.push(pair);
    }
    sort_pairs_cancellable(&mut pairs, cancellation)?;
    let quartile = pairs.len() / 4;
    let cold_threshold = pairs[quartile - 1].0;
    let warm_threshold = pairs[pairs.len() - quartile].0;
    if cold_threshold.total_cmp(&warm_threshold).is_eq() {
        return Ok(0.0);
    }
    let mut cold = 0.0_f64;
    let mut warm = 0.0_f64;
    let mut cold_count = 0_usize;
    let mut warm_count = 0_usize;
    for (index, (driver, response)) in pairs.iter().enumerate() {
        if index % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        if *driver <= cold_threshold {
            cold += response;
            cold_count += 1;
        }
        if *driver >= warm_threshold {
            warm += response;
            warm_count += 1;
        }
    }
    cold /= cold_count.max(1) as f64;
    warm /= warm_count.max(1) as f64;
    let overall = mean(response, cancellation)?;
    Ok((warm - cold) / overall.abs().max(1.0e-12))
}

fn sort_pairs_cancellable(
    pairs: &mut [(f64, f64)],
    cancellation: Option<&BuildCancellation>,
) -> Result<(), QualityBuildError> {
    if pairs.len() < 2 {
        return Ok(());
    }
    let mut scratch = Vec::with_capacity(pairs.len());
    for (index, pair) in pairs.iter().copied().enumerate() {
        if index % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        scratch.push(pair);
    }
    let mut width = 1_usize;
    let mut data_in_pairs = true;
    while width < pairs.len() {
        check_quality_cancelled(cancellation)?;
        if data_in_pairs {
            merge_pair_runs(pairs, &mut scratch, width, cancellation)?;
        } else {
            merge_pair_runs(&scratch, pairs, width, cancellation)?;
        }
        data_in_pairs = !data_in_pairs;
        width = width.saturating_mul(2);
    }
    if !data_in_pairs {
        for (index, (target, source)) in pairs.iter_mut().zip(scratch).enumerate() {
            if index % 256 == 0 {
                check_quality_cancelled(cancellation)?;
            }
            *target = source;
        }
    }
    Ok(())
}

fn merge_pair_runs(
    source: &[(f64, f64)],
    target: &mut [(f64, f64)],
    width: usize,
    cancellation: Option<&BuildCancellation>,
) -> Result<(), QualityBuildError> {
    let mut start = 0_usize;
    while start < source.len() {
        let middle = start.saturating_add(width).min(source.len());
        let end = middle.saturating_add(width).min(source.len());
        let mut left = start;
        let mut right = middle;
        for (offset, slot) in target[start..end].iter_mut().enumerate() {
            let output = start + offset;
            if output % 256 == 0 {
                check_quality_cancelled(cancellation)?;
            }
            let take_left = right >= end
                || (left < middle && pair_order(&source[left], &source[right]).is_le());
            if take_left {
                *slot = source[left];
                left += 1;
            } else {
                *slot = source[right];
                right += 1;
            }
        }
        start = end;
    }
    Ok(())
}

fn pair_order(left: &(f64, f64), right: &(f64, f64)) -> std::cmp::Ordering {
    left.0.total_cmp(&right.0)
}

fn basin_gyre_circulation(
    surface: &SphericalSurfaceSnapshot,
    terrain: ClimateQualityTerrain<'_>,
    current: &[[[f32; 3]; 12]],
    cancellation: Option<&BuildCancellation>,
) -> Result<(f64, u32), QualityBuildError> {
    let ocean = terrain
        .land_ocean()
        .raw_values()
        .iter()
        .map(|kind| *kind == LandOceanKind::Ocean.raw())
        .collect::<Vec<_>>();
    basin_gyre_circulation_for_mask(surface, &ocean, current, cancellation)
}

fn basin_gyre_circulation_for_mask(
    surface: &SphericalSurfaceSnapshot,
    ocean: &[bool],
    current: &[[[f32; 3]; 12]],
    cancellation: Option<&BuildCancellation>,
) -> Result<(f64, u32), QualityBuildError> {
    let neighbors = surface_neighbors(surface, cancellation)?;
    let mut visited = vec![false; ocean.len()];
    let mut components = Vec::new();
    for start in 0..ocean.len() {
        if start % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        if !ocean[start] || visited[start] {
            continue;
        }
        let mut component = Vec::new();
        let mut queue = std::collections::VecDeque::from([start]);
        visited[start] = true;
        let mut visited_in_component = 0_usize;
        while let Some(cell) = queue.pop_front() {
            visited_in_component += 1;
            if visited_in_component % 256 == 0 {
                check_quality_cancelled(cancellation)?;
            }
            component.push(cell);
            for &neighbor in &neighbors[cell] {
                if ocean[neighbor] && !visited[neighbor] {
                    visited[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        if component.len() >= 4 {
            components.push(component);
        }
    }

    let mut active = 0_u32;
    let mut total = 0_u32;
    for component in components {
        for (month, _) in current[component[0]].iter().enumerate() {
            check_quality_cancelled(cancellation)?;
            let mut hemispheres = [None, None];
            for (hemisphere_index, hemisphere_sign) in [(0, -1.0_f64), (1, 1.0_f64)] {
                let eligible = component
                    .iter()
                    .copied()
                    .filter(|&cell| {
                        let z = surface.cells()[cell].centroid.components()[2];
                        hemisphere_sign * z > 0.15
                    })
                    .collect::<Vec<_>>();
                if eligible.len() < 3 {
                    continue;
                }
                let mut axis = [0.0_f64; 3];
                for (index, &cell) in eligible.iter().enumerate() {
                    if index % 256 == 0 {
                        check_quality_cancelled(cancellation)?;
                    }
                    let area = surface.cells()[cell].area.get();
                    let radial = surface.cells()[cell].centroid.components();
                    for component in 0..3 {
                        axis[component] += area * radial[component];
                    }
                }
                let axis_norm = norm(axis);
                if axis_norm <= 1.0e-12 {
                    continue;
                }
                axis = axis.map(|value| value / axis_norm);
                let mut signed_circulation = 0.0_f64;
                let mut speed_scale = 0.0_f64;
                for (index, &cell) in eligible.iter().enumerate() {
                    if index % 256 == 0 {
                        check_quality_cancelled(cancellation)?;
                    }
                    let radial = surface.cells()[cell].centroid.components();
                    let azimuth = cross(axis, radial);
                    let azimuth_norm = norm(azimuth);
                    if azimuth_norm <= 1.0e-12 {
                        continue;
                    }
                    let tangent = azimuth.map(|value| value / azimuth_norm);
                    let velocity = current[cell][month].map(f64::from);
                    let area = surface.cells()[cell].area.get();
                    signed_circulation += area * dot(velocity, tangent);
                    speed_scale += area * norm(velocity);
                }
                if speed_scale > 0.0 {
                    hemispheres[hemisphere_index] = Some((
                        (signed_circulation.abs() / speed_scale).clamp(0.0, 1.0),
                        signed_circulation.signum(),
                    ));
                }
            }
            match (hemispheres[0], hemispheres[1]) {
                (Some(south), Some(north)) => {
                    total += 2;
                    if south.1 * north.1 < 0.0 {
                        active += u32::from(south.0 >= 0.08) + u32::from(north.0 >= 0.08);
                    }
                }
                (Some(single), None) | (None, Some(single)) => {
                    total += 1;
                    active += u32::from(single.0 >= 0.08);
                }
                (None, None) => {}
            }
        }
    }
    Ok((fraction(active, total), total.max(1)))
}

#[derive(Debug, Clone, Copy)]
struct OrographicNeighborResponse {
    leeward_drying: f64,
    leeward_samples: u32,
}

fn orographic_neighbor_response(
    surface: &SphericalSurfaceSnapshot,
    terrain: ClimateQualityTerrain<'_>,
    wind: &[[[f32; 3]; 12]],
    precipitation: &[[f32; 12]],
    cancellation: Option<&BuildCancellation>,
) -> Result<OrographicNeighborResponse, QualityBuildError> {
    let land = terrain
        .land_ocean()
        .raw_values()
        .iter()
        .map(|kind| *kind == LandOceanKind::Land.raw())
        .collect::<Vec<_>>();
    orographic_neighbor_response_from_fields(
        surface,
        &land,
        terrain.elevation_m(),
        wind,
        precipitation,
        cancellation,
    )
}

fn orographic_neighbor_response_from_fields(
    surface: &SphericalSurfaceSnapshot,
    land: &[bool],
    elevation: &[f32],
    wind: &[[[f32; 3]; 12]],
    precipitation: &[[f32; 12]],
    cancellation: Option<&BuildCancellation>,
) -> Result<OrographicNeighborResponse, QualityBuildError> {
    let neighbors = surface_neighbors(surface, cancellation)?;
    let mut leeward = 0.0_f64;
    let mut leeward_samples = 0_u32;
    for cell in 0..surface.cells().len() {
        if cell % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        if !land[cell] {
            continue;
        }
        let radial = surface.cells()[cell].centroid.components();
        for month in 0..12 {
            let velocity = wind[cell][month].map(f64::from);
            let speed = norm(velocity);
            if speed < 0.5 {
                continue;
            }
            let direction = velocity.map(|value| value / speed);
            let mut upstream = None;
            let mut downstream = None;
            for &neighbor in &neighbors[cell] {
                if !land[neighbor] {
                    continue;
                }
                let neighbor_radial = surface.cells()[neighbor].centroid.components();
                let alignment = dot(tangent_direction(radial, neighbor_radial), direction);
                if upstream.is_none_or(|(_, best)| alignment < best) {
                    upstream = Some((neighbor, alignment));
                }
                if downstream.is_none_or(|(_, best)| alignment > best) {
                    downstream = Some((neighbor, alignment));
                }
            }
            let (Some((upstream, upstream_alignment)), Some((downstream, downstream_alignment))) =
                (upstream, downstream)
            else {
                continue;
            };
            if upstream_alignment > -0.15
                || downstream_alignment < 0.15
                || elevation[cell] <= elevation[upstream]
                || elevation[cell] <= elevation[downstream]
            {
                continue;
            }
            let upstream_rain = f64::from(precipitation[upstream][month]);
            let local_rain = f64::from(precipitation[cell][month]);
            let downstream_rain = f64::from(precipitation[downstream][month]);
            let scale = ((upstream_rain + local_rain + downstream_rain) / 3.0)
                .abs()
                .max(0.1);
            leeward += (upstream_rain - downstream_rain) / scale;
            leeward_samples += 1;
        }
    }
    Ok(OrographicNeighborResponse {
        leeward_drying: leeward / f64::from(leeward_samples.max(1)),
        leeward_samples: leeward_samples.max(1),
    })
}

fn orographic_precipitation_fraction(
    surface: &SphericalSurfaceSnapshot,
    total: &[[f32; 12]],
    orographic: &[[f32; 12]],
    cancellation: Option<&BuildCancellation>,
) -> Result<(f64, u32), QualityBuildError> {
    let mut total_amount = 0.0_f64;
    let mut orographic_amount = 0.0_f64;
    let mut samples = 0_u32;
    for (index, (cell, (total, orographic))) in surface
        .cells()
        .iter()
        .zip(total.iter().zip(orographic))
        .enumerate()
    {
        if index % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        for month in 0..12 {
            total_amount += cell.area.get() * f64::from(total[month]);
            orographic_amount += cell.area.get() * f64::from(orographic[month]);
            samples += u32::from(total[month] > 0.0);
        }
    }
    let fraction = if total_amount > 0.0 {
        orographic_amount / total_amount
    } else {
        0.0
    };
    Ok((fraction, samples.max(1)))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OrographicUpliftEnrichment {
    enrichment_ratio: f64,
    support_area_fraction: f64,
    supported_samples: u32,
}

fn orographic_uplift_enrichment(
    surface: &SphericalSurfaceSnapshot,
    terrain: ClimateQualityTerrain<'_>,
    wind: &[[[f32; 3]; 12]],
    orographic: &[[f32; 12]],
    cancellation: Option<&BuildCancellation>,
) -> Result<OrographicUpliftEnrichment, QualityBuildError> {
    let land = terrain
        .land_ocean()
        .raw_values()
        .iter()
        .map(|kind| *kind == LandOceanKind::Land.raw())
        .collect::<Vec<_>>();
    orographic_uplift_enrichment_from_fields(
        surface,
        &land,
        terrain.elevation_m(),
        wind,
        orographic,
        cancellation,
    )
}

fn orographic_uplift_enrichment_from_fields(
    surface: &SphericalSurfaceSnapshot,
    land: &[bool],
    elevation: &[f32],
    wind: &[[[f32; 3]; 12]],
    orographic: &[[f32; 12]],
    cancellation: Option<&BuildCancellation>,
) -> Result<OrographicUpliftEnrichment, QualityBuildError> {
    let neighbors = surface_neighbors(surface, cancellation)?;
    let mut total_area = 0.0_f64;
    let mut supported_area = 0.0_f64;
    let mut total_amount = 0.0_f64;
    let mut supported_amount = 0.0_f64;
    let mut supported_samples = 0_u32;
    for cell in 0..surface.cells().len() {
        if cell % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        let radial = surface.cells()[cell].centroid.components();
        let area = surface.cells()[cell].area.get();
        for month in 0..12 {
            let amount = area * f64::from(orographic[cell][month]);
            if !land[cell] {
                continue;
            }
            total_area += area;
            total_amount += amount;
            let velocity = wind[cell][month].map(f64::from);
            let speed = norm(velocity);
            if speed < 0.5 {
                continue;
            }
            let direction = velocity.map(|value| value / speed);
            let mut upstream = None;
            let mut downstream = None;
            for &neighbor in &neighbors[cell] {
                if !land[neighbor] {
                    continue;
                }
                let alignment = dot(
                    tangent_direction(radial, surface.cells()[neighbor].centroid.components()),
                    direction,
                );
                if upstream.is_none_or(|(_, best)| alignment < best) {
                    upstream = Some((neighbor, alignment));
                }
                if downstream.is_none_or(|(_, best)| alignment > best) {
                    downstream = Some((neighbor, alignment));
                }
            }
            let (Some((upstream, upstream_alignment)), Some((downstream, downstream_alignment))) =
                (upstream, downstream)
            else {
                continue;
            };
            if upstream_alignment <= -0.15
                && downstream_alignment >= 0.15
                && elevation[downstream] - elevation[upstream] >= 50.0
            {
                supported_area += area;
                supported_amount += amount;
                supported_samples += 1;
            }
        }
    }
    let support_area_fraction = if total_area > 0.0 {
        supported_area / total_area
    } else {
        0.0
    };
    let supported_amount_fraction = if total_amount > 0.0 {
        supported_amount / total_amount
    } else {
        0.0
    };
    let enrichment_ratio = if support_area_fraction > 0.0 {
        supported_amount_fraction / support_area_fraction
    } else {
        0.0
    };
    Ok(OrographicUpliftEnrichment {
        enrichment_ratio,
        support_area_fraction,
        supported_samples: supported_samples.max(1),
    })
}

fn surface_neighbors(
    surface: &SphericalSurfaceSnapshot,
    cancellation: Option<&BuildCancellation>,
) -> Result<Vec<Vec<usize>>, QualityBuildError> {
    let mut neighbors = vec![Vec::new(); surface.cells().len()];
    for (index, edge) in surface.edges().iter().enumerate() {
        if index % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        let first = edge.cells[0].raw() as usize;
        let second = edge.cells[1].raw() as usize;
        neighbors[first].push(second);
        neighbors[second].push(first);
    }
    Ok(neighbors)
}

fn tangent_direction(origin: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    let radial_projection = dot(origin, target);
    let tangent =
        std::array::from_fn(|component| target[component] - radial_projection * origin[component]);
    let length = norm(tangent).max(1.0e-12);
    tangent.map(|component| component / length)
}

fn neighbor_speed_jump_ratio(
    surface: &SphericalSurfaceSnapshot,
    wind: &[[[f32; 3]; 12]],
    cancellation: Option<&BuildCancellation>,
) -> Result<f64, QualityBuildError> {
    let mut square_sum = 0.0_f64;
    for (cell, months) in wind.iter().enumerate() {
        if cell % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        square_sum += months
            .iter()
            .map(|vector| norm(vector.map(f64::from)).powi(2))
            .sum::<f64>();
    }
    let rms = (square_sum / (wind.len() * 12).max(1) as f64)
        .sqrt()
        .max(1.0e-9);
    let mut maximum = 0.0_f64;
    for (edge_index, edge) in surface.edges().iter().enumerate() {
        if edge_index % 256 == 0 {
            check_quality_cancelled(cancellation)?;
        }
        let first = edge.cells[0].raw() as usize;
        let second = edge.cells[1].raw() as usize;
        for (first_month, second_month) in wind[first].iter().zip(&wind[second]) {
            let difference = std::array::from_fn(|component| {
                f64::from(first_month[component] - second_month[component])
            });
            maximum = maximum.max(norm(difference) / rms);
        }
    }
    Ok(maximum)
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn invalid_input(input: &'static str, reason: String) -> QualityBuildError {
    QualityBuildError::InvalidInput { input, reason }
}

fn check_quality_cancelled(
    cancellation: Option<&BuildCancellation>,
) -> Result<(), QualityBuildError> {
    if cancellation.is_some_and(BuildCancellation::is_cancelled) {
        Err(QualityBuildError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::{Meters, SphericalSpaceSpec};

    fn surface() -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 96,
        })
        .unwrap()
    }

    #[test]
    fn metric_inventory_is_complete_and_alphabetical() {
        assert_eq!(EXPECTED_METRIC_NAMES.len(), 26);
        assert!(
            EXPECTED_METRIC_NAMES
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "P4 metric identity is canonical only in alphabetical order"
        );
        for required in [
            "absorbed-shortwave-global-mean-w-m2",
            "evaporation-global-mean-mm-day",
            "evaporation-precipitation-relative-imbalance",
            "outgoing-longwave-global-mean-w-m2",
            "planetary-albedo-global-mean",
            "precipitation-global-mean-mm-day",
            "precipitation-low-to-high-latitude-ratio",
            "precipitation-seasonal-hemisphere-phase-fraction",
            "toa-net-radiation-global-mean-w-m2",
        ] {
            assert!(
                EXPECTED_METRIC_NAMES.contains(&required),
                "missing P4 water/energy metric {required}"
            );
        }
    }

    #[test]
    fn precipitation_phase_fraction_weights_resolved_signal_not_dry_cell_count() {
        let mut phase = MetricAccumulator::new();
        record_precipitation_seasonal_phase_signal(&mut phase, 20.0, 2.0, 6.0, 1.0).unwrap();
        record_precipitation_seasonal_phase_signal(&mut phase, 20.0, 1.0e-6, 0.0, 1_000.0).unwrap();
        record_precipitation_seasonal_phase_signal(&mut phase, -20.0, 5.0, 1.0, 1.0).unwrap();
        let MetricObservation::Available { value, .. } = phase.finish().unwrap() else {
            panic!("resolved precipitation phase must be measurable");
        };
        assert!(value > 0.999, "dry-grid noise dominated phase: {value}");
    }

    fn synthetic_report(
        failing_name: Option<&str>,
    ) -> (NaturalQualityReport, SurfaceRef, [u8; 32]) {
        let surface = surface();
        let surface_ref = SurfaceRef::for_spherical(&surface);
        let state_fingerprint = [7_u8; 32];
        let maximum_monthly_samples = surface_ref.cell_count() * CLIMATE_MONTH_COUNT as u32;
        let mut builder = NaturalQualityReportBuilder::new(surface_ref);
        for name in EXPECTED_METRIC_NAMES.into_iter().rev() {
            let expected = expected_metric(name).unwrap();
            let passing_value = match (expected.minimum, expected.maximum) {
                (Some(minimum), Some(maximum)) => (minimum + maximum) * 0.5,
                (Some(minimum), None) => minimum,
                (None, Some(maximum)) => maximum,
                (None, None) => 0.0,
            };
            let value = if failing_name == Some(name) {
                expected.maximum.map_or_else(
                    || expected.minimum.expect("hard metric has one bound") - 1.0,
                    |maximum| maximum + 1.0,
                )
            } else {
                passing_value
            };
            let sample_count = match name {
                "cubed-face-seam-speed-ratio" => surface_ref.edge_count(),
                "near-surface-wind-non-zonal-variance-fraction"
                | "ocean-current-land-leakage-max-m-s" => surface_ref.cell_count(),
                "positive-thermocline-depth-fraction" | "vertical-shear-rms-m-s" => {
                    maximum_monthly_samples
                }
                "seasonal-hemisphere-phase-correlation" => surface_ref.cell_count(),
                _ => 1,
            };
            record_expected_metric(&mut builder, name, available(value, sample_count)).unwrap();
        }
        let report = builder
            .finish()
            .unwrap()
            .bind_subject_fingerprint(state_fingerprint)
            .unwrap();
        (report, surface_ref, state_fingerprint)
    }

    #[test]
    fn canonical_report_resolves_every_bound_by_metric_name() {
        let (report, surface_ref, state_fingerprint) = synthetic_report(None);
        validate_global_circulation_quality_report(&report, surface_ref, &state_fingerprint)
            .unwrap();
        for metric in report.metrics() {
            let expected = expected_metric(metric.id().name()).unwrap();
            assert_eq!(metric.bounds().min(), expected.minimum);
            assert_eq!(metric.bounds().max(), expected.maximum);
        }
    }

    #[test]
    fn synthetic_water_and_energy_closure_failures_are_rejected() {
        for failing_name in [
            "evaporation-precipitation-relative-imbalance",
            "toa-net-radiation-global-mean-w-m2",
        ] {
            let (report, surface_ref, state_fingerprint) = synthetic_report(Some(failing_name));
            assert_eq!(
                report
                    .metrics()
                    .iter()
                    .find(|metric| metric.id().name() == failing_name)
                    .unwrap()
                    .status(),
                QualityMetricStatus::Fail,
            );
            let error = validate_global_circulation_quality_report(
                &report,
                surface_ref,
                &state_fingerprint,
            )
            .unwrap_err();
            assert!(
                error.contains(failing_name),
                "hard closure rejection did not identify {failing_name}: {error}"
            );
        }
    }

    #[test]
    fn synthetic_morphology_failure_remains_diagnostic() {
        let diagnostic_name = "low-latitude-easterly-fraction";
        let (report, surface_ref, state_fingerprint) = synthetic_report(Some(diagnostic_name));
        assert_eq!(
            report
                .metrics()
                .iter()
                .find(|metric| metric.id().name() == diagnostic_name)
                .unwrap()
                .status(),
            QualityMetricStatus::Fail,
        );
        validate_global_circulation_quality_report(&report, surface_ref, &state_fingerprint)
            .unwrap();
    }

    #[test]
    fn uniform_ambient_flow_does_not_count_as_a_basin_gyre() {
        let surface = surface();
        let current = surface
            .cells()
            .iter()
            .map(|cell| {
                let radial = cell.centroid.components();
                let ambient = [1.0_f64, 0.0, 0.0];
                let radial_component = dot(ambient, radial);
                let tangent = std::array::from_fn(|component| {
                    (ambient[component] - radial_component * radial[component]) as f32
                });
                [tangent; 12]
            })
            .collect::<Vec<_>>();
        let (fraction, samples) = basin_gyre_circulation_for_mask(
            &surface,
            &vec![true; surface.cells().len()],
            &current,
            None,
        )
        .unwrap();
        assert!(samples > 0);
        assert!(
            fraction < 0.20,
            "uniform flow produced gyre fraction {fraction}"
        );
    }

    #[test]
    fn tied_driver_values_cannot_manufacture_an_interquartile_response() {
        let driver = vec![15.0; 8];
        let response = (1..=8).map(f64::from).collect::<Vec<_>>();
        assert_eq!(
            interquartile_response(&driver, &response, None).unwrap(),
            0.0
        );
    }

    #[test]
    fn interquartile_sort_observes_cancellation_after_entering_merge_work() {
        let sample_count = 131_072_usize;
        let driver = (0..sample_count)
            .rev()
            .map(|value| value as f64)
            .collect::<Vec<_>>();
        let response = (0..sample_count)
            .map(|value| (value % 997) as f64 + 1.0)
            .collect::<Vec<_>>();
        let cancellation = BuildCancellation::new();
        let latency = std::thread::scope(|scope| {
            let worker =
                scope.spawn(|| interquartile_response(&driver, &response, Some(&cancellation)));
            // Pair collection and scratch initialization each poll once per
            // 256 samples. Crossing both counts puts cancellation inside a
            // merge pass rather than at the helper boundary.
            let merge_threshold = 2 * (sample_count as u64 / 256) + 8;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while cancellation.observation_count() < merge_threshold {
                assert!(
                    std::time::Instant::now() < deadline,
                    "interquartile statistic never entered merge work"
                );
                std::thread::yield_now();
            }
            let started = std::time::Instant::now();
            cancellation.cancel();
            assert_eq!(worker.join().unwrap(), Err(QualityBuildError::Cancelled));
            started.elapsed()
        });
        assert!(
            latency <= std::time::Duration::from_millis(250),
            "interquartile merge cancellation took {latency:?}"
        );
    }

    #[test]
    fn spatially_uniform_precipitation_cannot_pass_orographic_gates() {
        let surface = surface();
        let neighbors = surface_neighbors(&surface, None).unwrap();
        let crest = neighbors
            .iter()
            .position(|neighbors| neighbors.len() >= 3)
            .unwrap();
        let radial = surface.cells()[crest].centroid.components();
        let mut best_pair = (neighbors[crest][0], neighbors[crest][1], 1.0_f64);
        for &first in &neighbors[crest] {
            for &second in &neighbors[crest] {
                let alignment = dot(
                    tangent_direction(radial, surface.cells()[first].centroid.components()),
                    tangent_direction(radial, surface.cells()[second].centroid.components()),
                );
                if alignment < best_pair.2 {
                    best_pair = (first, second, alignment);
                }
            }
        }
        assert!(best_pair.2 < -0.3);
        let downstream =
            tangent_direction(radial, surface.cells()[best_pair.1].centroid.components());
        let mut wind = vec![[[0.0_f32; 3]; 12]; surface.cells().len()];
        wind[crest] = [downstream.map(|value| (5.0 * value) as f32); 12];
        let mut elevation = vec![0.0_f32; surface.cells().len()];
        elevation[crest] = 1_000.0;
        let response = orographic_neighbor_response_from_fields(
            &surface,
            &vec![true; surface.cells().len()],
            &elevation,
            &wind,
            &vec![[1.0_f32; 12]; surface.cells().len()],
            None,
        )
        .unwrap();
        assert!(response.leeward_samples >= 12);
        assert_eq!(response.leeward_drying, 0.0);
        let uniform = vec![[1.0_f32; 12]; surface.cells().len()];
        let absent = vec![[0.0_f32; 12]; surface.cells().len()];
        assert_eq!(
            orographic_precipitation_fraction(&surface, &uniform, &absent, None)
                .unwrap()
                .0,
            0.0
        );
        let coherent_wind = surface
            .cells()
            .iter()
            .map(|cell| {
                let radial = cell.centroid.components();
                let ambient = [1.0_f64, 0.0, 0.0];
                let radial_component = dot(ambient, radial);
                let tangent = std::array::from_fn(|component| {
                    ambient[component] - radial_component * radial[component]
                });
                let speed = norm(tangent);
                let velocity = if speed > 1.0e-9 {
                    tangent.map(|component| (5.0 * component / speed) as f32)
                } else {
                    [0.0; 3]
                };
                [velocity; 12]
            })
            .collect::<Vec<_>>();
        let broad_relief = surface
            .cells()
            .iter()
            .map(|cell| (10_000.0 * cell.centroid.components()[0]) as f32)
            .collect::<Vec<_>>();
        let broad_land = surface
            .cells()
            .iter()
            .map(|cell| cell.centroid.components()[2] >= -0.30)
            .collect::<Vec<_>>();
        assert!(broad_land.iter().any(|land| *land));
        assert!(broad_land.iter().any(|land| !*land));
        let bogus_uniform_orographic = broad_land
            .iter()
            .map(|land| if *land { [0.5_f32; 12] } else { [0.0; 12] })
            .collect::<Vec<_>>();
        let uplift = orographic_uplift_enrichment_from_fields(
            &surface,
            &broad_land,
            &broad_relief,
            &coherent_wind,
            &bogus_uniform_orographic,
            None,
        )
        .unwrap();
        assert!(
            uplift.support_area_fraction > 0.15,
            "fixture support area was only {}",
            uplift.support_area_fraction
        );
        assert!(
            uplift.enrichment_ratio < 1.20,
            "a globally uniform orographic component produced enrichment {} over support area {}",
            uplift.enrichment_ratio,
            uplift.support_area_fraction
        );
    }
}

/// Enforces the exact P4 per-world metric inventory and every hard gate.
pub(crate) fn validate_global_circulation_quality_report(
    report: &NaturalQualityReport,
    expected_surface: SurfaceRef,
    expected_state_fingerprint: &[u8; 32],
) -> Result<(), String> {
    report.validate().map_err(|error| error.to_string())?;
    if report.surface_ref() != expected_surface {
        return Err("P4 quality report is not bound to global circulation authority".to_owned());
    }
    if report.subject_fingerprint() != Some(expected_state_fingerprint) {
        return Err("P4 quality report is not bound to the exact circulation state".to_owned());
    }
    if report.metrics().len() != EXPECTED_METRIC_NAMES.len() {
        return Err(format!(
            "P4 quality report contains {} metrics; expected {}",
            report.metrics().len(),
            EXPECTED_METRIC_NAMES.len()
        ));
    }
    let maximum_monthly_samples = expected_surface
        .cell_count()
        .checked_mul(CLIMATE_MONTH_COUNT as u32)
        .ok_or_else(|| "P4 quality sample limit overflowed".to_owned())?;
    for (metric, expected_name) in report.metrics().iter().zip(EXPECTED_METRIC_NAMES) {
        if metric.id().namespace() != METRIC_NAMESPACE
            || metric.id().version() != METRIC_VERSION
            || metric.id().name() != expected_name
        {
            return Err(format!("unexpected P4 metric {}", metric.id().name()));
        }
        let expected = expected_metric(expected_name)
            .expect("validated P4 metric name belongs to the locked registry");
        let bounds = metric.bounds();
        if bounds.min() != expected.minimum || bounds.max() != expected.maximum {
            return Err(format!(
                "per-world P4 metric {expected_name} changed locked bounds from {:?}..={:?} to {:?}..={:?}",
                expected.minimum,
                expected.maximum,
                bounds.min(),
                bounds.max()
            ));
        }
        if expected.hard && metric.status() != QualityMetricStatus::Pass {
            return Err(format!(
                "hard P4 closure metric {expected_name} has status {:?}",
                metric.status()
            ));
        }
        // Morphology and Earth-likeness statuses describe this authored world;
        // only the two structural water/energy closures are hard. Binding,
        // metric inventory, named bounds, and sample counts remain structural
        // because violating them corrupts the evidence itself.
        if metric.status() == QualityMetricStatus::Unavailable {
            continue;
        }
        if metric.sample_count() > maximum_monthly_samples {
            return Err(format!(
                "per-world P4 metric {expected_name} reports {} samples; maximum is {maximum_monthly_samples}",
                metric.sample_count()
            ));
        }
        let exact_samples = match expected_name {
            "absorbed-shortwave-global-mean-w-m2"
            | "evaporation-global-mean-mm-day"
            | "evaporation-precipitation-relative-imbalance"
            | "outgoing-longwave-global-mean-w-m2"
            | "planetary-albedo-global-mean"
            | "precipitation-global-mean-mm-day"
            | "toa-net-radiation-global-mean-w-m2" => Some(1),
            "cubed-face-seam-speed-ratio" => Some(expected_surface.edge_count()),
            "near-surface-wind-non-zonal-variance-fraction"
            | "ocean-current-land-leakage-max-m-s" => Some(expected_surface.cell_count()),
            "positive-thermocline-depth-fraction" | "vertical-shear-rms-m-s" => {
                Some(maximum_monthly_samples)
            }
            "seasonal-hemisphere-phase-correlation" => Some(expected_surface.cell_count()),
            _ => None,
        };
        if exact_samples.is_some_and(|expected| metric.sample_count() != expected) {
            return Err(format!(
                "per-world P4 metric {expected_name} reports {} samples; expected {exact_samples:?}",
                metric.sample_count()
            ));
        }
    }
    Ok(())
}
/// Area-weighted decomposition of the annual-mean near-surface wind into
/// 5° zonal-band means and deviations: the share of wind variance that is
/// not axisymmetric (design 2026-09-02 §3).
struct NonZonalWindAccumulator {
    samples: Vec<(usize, f64, f64, f64)>,
}

impl NonZonalWindAccumulator {
    const BAND_DEGREES: f64 = 5.0;
    const BAND_COUNT: usize = 36;

    fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    fn push(&mut self, latitude_degrees: f64, area_m2: f64, zonal: f64, meridional: f64) {
        let band = (((latitude_degrees + 90.0) / Self::BAND_DEGREES).floor() as usize)
            .min(Self::BAND_COUNT - 1);
        self.samples.push((band, area_m2, zonal, meridional));
    }

    fn non_zonal_variance_fraction(&self) -> f64 {
        let mut band_area = [0.0_f64; Self::BAND_COUNT];
        let mut band_zonal = [0.0_f64; Self::BAND_COUNT];
        let mut band_meridional = [0.0_f64; Self::BAND_COUNT];
        for &(band, area, zonal, meridional) in &self.samples {
            band_area[band] += area;
            band_zonal[band] += area * zonal;
            band_meridional[band] += area * meridional;
        }
        let mut total = 0.0_f64;
        let mut deviation = 0.0_f64;
        for &(band, area, zonal, meridional) in &self.samples {
            if band_area[band] <= 0.0 {
                continue;
            }
            let mean_zonal = band_zonal[band] / band_area[band];
            let mean_meridional = band_meridional[band] / band_area[band];
            total += area * (zonal * zonal + meridional * meridional);
            deviation +=
                area * ((zonal - mean_zonal).powi(2) + (meridional - mean_meridional).powi(2));
        }
        if total > 0.0 {
            deviation / total
        } else {
            0.0
        }
    }
}
