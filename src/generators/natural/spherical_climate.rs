use thiserror::Error;

use super::climate::{
    annual_sea_level_temperature, circulation_wind, daily_mean_insolation,
    monthly_declination_degrees, ClimateGenerator, ENVIRONMENTAL_LAPSE_RATE_C_PER_M,
};
use super::spherical_moisture::solve_monthly_precipitation;
use super::topology::{multi_source_distance, NaturalTopologyIndex};
use crate::world::natural::{
    ClimateSpec, ClimateSpecError, LandOceanKind, MonthlyScalarField, MonthlyVector3Field,
    SphericalClimateValidationError, SphericalPreliminaryClimateSnapshot, SphericalReliefSnapshot,
    SphericalReliefValidationError, AIR_TEMPERATURE_MAX_C, AIR_TEMPERATURE_MIN_C,
    ANNUAL_PRECIPITATION_MAX_MM, CLIMATE_MONTH_COUNT, PRELIMINARY_CLIMATE_SCHEMA_V2,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRefError,
};
use crate::world::CellId;

const MARITIME_DECAY_FRACTION_OF_MAXIMUM_DISTANCE: f64 = 0.06;
const POLAR_TAPER_START_DEGREES_FROM_POLE: f64 = 5.0;

#[derive(Debug)]
struct SphericalThermalWindFields {
    latitude_degrees: Vec<f32>,
    maritime_influence: Vec<f32>,
    monthly_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_wind_m_s: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
}

impl ClimateGenerator {
    /// Generates current-slice monthly forcing directly on an authoritative closed sphere.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
        spec: &ClimateSpec,
    ) -> Result<SphericalPreliminaryClimateSnapshot, SphericalClimateGenerationError> {
        spec.validate()?;
        surface.validate()?;
        relief.validate_against_validated_surface(surface)?;

        let view = SphericalNaturalSurface::from_validated(surface)?;
        let topology = NaturalTopologyIndex::from_surface(&view);
        let fields = generate_thermal_wind_fields(&view, &topology, relief, spec);
        let mut precipitation = solve_monthly_precipitation(
            &view,
            relief,
            spec,
            &fields.maritime_influence,
            &fields.monthly_air_temperature_c,
        );
        limit_annual_precipitation(&mut precipitation);

        let mean_temperature = fields
            .monthly_air_temperature_c
            .iter()
            .map(|months| months.iter().sum::<f32>() / CLIMATE_MONTH_COUNT as f32)
            .collect();
        let temperature_seasonality = fields
            .monthly_air_temperature_c
            .iter()
            .map(|months| {
                months.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                    - months.iter().copied().fold(f32::INFINITY, f32::min)
            })
            .collect();
        let annual_precipitation = precipitation
            .iter()
            .map(|months| months.iter().sum::<f32>())
            .collect();
        let prevailing_wind = fields
            .monthly_wind_m_s
            .iter()
            .map(|months| {
                let sum = months.iter().fold([0.0_f32; 3], |sum, value| {
                    [sum[0] + value[0], sum[1] + value[1], sum[2] + value[2]]
                });
                sum.map(|component| component / CLIMATE_MONTH_COUNT as f32)
            })
            .collect();

        let temperature = MonthlyScalarField::from_values(fields.monthly_air_temperature_c)
            .map_err(SphericalClimateValidationError::from)?;
        let precipitation = MonthlyScalarField::from_values(precipitation)
            .map_err(SphericalClimateValidationError::from)?;
        let wind = MonthlyVector3Field::from_values(fields.monthly_wind_m_s)
            .map_err(SphericalClimateValidationError::from)?;
        let snapshot = SphericalPreliminaryClimateSnapshot::new(
            PRELIMINARY_CLIMATE_SCHEMA_V2,
            view.surface_ref(),
            fields.latitude_degrees,
            fields.maritime_influence,
            temperature,
            precipitation,
            wind,
            mean_temperature,
            temperature_seasonality,
            annual_precipitation,
            prevailing_wind,
        )?;
        snapshot.validate_against_validated_surface(surface, relief)?;
        Ok(snapshot)
    }
}

fn limit_annual_precipitation(precipitation: &mut [[f32; CLIMATE_MONTH_COUNT]]) {
    for months in precipitation {
        let annual = months.iter().sum::<f32>();
        if annual > ANNUAL_PRECIPITATION_MAX_MM {
            let scale = ANNUAL_PRECIPITATION_MAX_MM / annual;
            for value in months {
                *value *= scale;
            }
        }
    }
}

fn generate_thermal_wind_fields(
    surface: &SphericalNaturalSurface<'_>,
    topology: &NaturalTopologyIndex,
    relief: &SphericalReliefSnapshot,
    spec: &ClimateSpec,
) -> SphericalThermalWindFields {
    let count = surface.cell_count();
    let maritime_influence = maritime_influence(surface, topology, relief);
    let declinations = std::array::from_fn::<_, CLIMATE_MONTH_COUNT, _>(|month| {
        monthly_declination_degrees(month, spec.axial_tilt_degrees())
    });
    let mut latitude_degrees = Vec::with_capacity(count);
    let mut monthly_air_temperature_c = Vec::with_capacity(count);
    let mut monthly_wind_m_s = Vec::with_capacity(count);

    for (index, &maritime) in maritime_influence.iter().enumerate() {
        let cell = CellId::from_raw(index as u32);
        let radial = surface
            .cell_frame(cell)
            .expect("validated spherical cell IDs are dense")
            .radial()
            .components();
        let latitude = radial[2].asin().to_degrees() as f32;
        let monthly_insolation =
            declinations.map(|declination| daily_mean_insolation(latitude, declination));
        let annual_insolation = monthly_insolation.iter().sum::<f32>() / CLIMATE_MONTH_COUNT as f32;
        let elevation_m = relief.elevation_m().values()[index].max(0.0);
        let sea_level_annual = annual_sea_level_temperature(latitude) + spec.temperature_offset_c();
        let seasonal_response = 18.0 * (0.30 + 0.70 * (1.0 - maritime));
        let lapse_c = elevation_m * ENVIRONMENTAL_LAPSE_RATE_C_PER_M;
        let temperature = std::array::from_fn(|month| {
            let anomaly = if annual_insolation > 1.0e-6 {
                (monthly_insolation[month] / annual_insolation - 1.0).clamp(-1.4, 1.4)
            } else {
                0.0
            };
            (sea_level_annual + anomaly * seasonal_response - lapse_c)
                .clamp(AIR_TEMPERATURE_MIN_C, AIR_TEMPERATURE_MAX_C)
        });
        let wind = std::array::from_fn(|month| {
            tangent_wind(radial, latitude, declinations[month], maritime)
                .map(|component| component as f32)
        });

        latitude_degrees.push(latitude);
        monthly_air_temperature_c.push(temperature);
        monthly_wind_m_s.push(wind);
    }

    SphericalThermalWindFields {
        latitude_degrees,
        maritime_influence,
        monthly_air_temperature_c,
        monthly_wind_m_s,
    }
}

fn maritime_influence(
    surface: &SphericalNaturalSurface<'_>,
    topology: &NaturalTopologyIndex,
    relief: &SphericalReliefSnapshot,
) -> Vec<f32> {
    let ocean_sources = relief
        .land_ocean()
        .raw_values()
        .iter()
        .enumerate()
        .filter_map(|(index, &kind)| {
            (kind == LandOceanKind::Ocean.raw()).then_some(CellId::from_raw(index as u32))
        })
        .collect::<Vec<_>>();
    if ocean_sources.is_empty() {
        return vec![0.0; surface.cell_count()];
    }
    if ocean_sources.len() == surface.cell_count() {
        return vec![1.0; surface.cell_count()];
    }

    let distances = multi_source_distance(topology, &ocean_sources, None);
    let decay_distance = topology.quantized_distance_for_meters(
        surface.long_length_scale().get() * MARITIME_DECAY_FRACTION_OF_MAXIMUM_DISTANCE,
    );
    debug_assert!(decay_distance > 0);
    distances
        .into_iter()
        .enumerate()
        .map(|(index, distance)| {
            if relief.land_ocean().raw_values()[index] == LandOceanKind::Ocean.raw() {
                1.0
            } else {
                (-(distance as f64) / decay_distance as f64)
                    .exp()
                    .clamp(0.0, 1.0) as f32
            }
        })
        .collect()
}

pub(super) fn tangent_wind(
    radial: [f64; 3],
    latitude_degrees: f32,
    declination_degrees: f32,
    maritime: f32,
) -> [f64; 3] {
    let horizontal = radial[0].hypot(radial[1]);
    if horizontal <= f64::EPSILON {
        return [0.0; 3];
    }
    let east = [-radial[1] / horizontal, radial[0] / horizontal, 0.0];
    let north = [-radial[2] * east[1], radial[2] * east[0], horizontal];
    let [zonal, meridional] = circulation_wind(latitude_degrees, declination_degrees, maritime);
    let taper_start = POLAR_TAPER_START_DEGREES_FROM_POLE.to_radians().sin();
    let amount = (horizontal / taper_start).clamp(0.0, 1.0);
    let polar_taper = amount * amount * (3.0 - 2.0 * amount);
    std::array::from_fn(|component| {
        (east[component] * f64::from(zonal) + north[component] * f64::from(meridional))
            * polar_taper
    })
}

/// Failures while deriving preliminary climate directly on a closed sphere.
#[derive(Debug, Error)]
pub enum SphericalClimateGenerationError {
    /// The resolved shared climate forcing is invalid.
    #[error("invalid spherical preliminary-climate specification: {0}")]
    InvalidSpec(#[from] ClimateSpecError),
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical climate surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The relief input is invalid or belongs to a different surface.
    #[error("invalid spherical climate relief: {0}")]
    InvalidRelief(#[from] SphericalReliefValidationError),
    /// A validated surface could not produce its exact identity.
    #[error("invalid spherical climate surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    /// Generated fields violated the strict surface-bound V2 contract.
    #[error("generated spherical preliminary climate is invalid: {0}")]
    InvalidSnapshot(#[from] SphericalClimateValidationError),
}
