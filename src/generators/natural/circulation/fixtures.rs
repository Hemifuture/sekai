use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::world::natural::{ForcingError, PlanetForcing, CLIMATE_MONTH_COUNT};

use super::{
    math::{dot, normalize},
    thermodynamics::{saturation_specific_humidity, ThermodynamicError, LAPSE_RATE_C_PER_M},
    CubedSphereGrid,
};

const AXIAL_TILT_RAD: f64 = 23.44_f64.to_radians();
const SOLAR_CONSTANT_W_M2: f64 = 1_361.0;
const STEFAN_BOLTZMANN_W_M2_K4: f64 = 5.670_374_419e-8;
const GREENHOUSE_OFFSET_K: f64 = 33.0;
const BACKGROUND_HEAT_FLUX_W_M2: f64 = 100.0;
const LOCAL_SOLAR_COUPLING: f64 = 0.58;

/// Deterministic scientific fixtures used to compare solver strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CirculationFixture {
    AquaPlanet,
    TwoBasins,
    EarthLikeHarmonics,
}

/// Builds one immutable forcing object from analytic spherical fields.
pub fn build_fixture(
    grid: &CubedSphereGrid,
    fixture: CirculationFixture,
) -> Result<PlanetForcing, FixtureBuildError> {
    let count = grid.cell_count();
    let mut elevation_m = Vec::with_capacity(count);
    let mut land_fraction = Vec::with_capacity(count);
    let mut surface_albedo = Vec::with_capacity(count);
    let mut surface_moisture_availability = Vec::with_capacity(count);
    let mut equilibrium_air_temperature_c = Vec::with_capacity(count);
    let mut equilibrium_surface_temperature_c = Vec::with_capacity(count);
    let mut equilibrium_specific_humidity = Vec::with_capacity(count);

    for cell in grid.cells() {
        let radial = cell.center_unit();
        let latitude = radial[2].asin();
        let longitude = radial[1].atan2(radial[0]);
        let (land, elevation) = surface_fields(fixture, radial, latitude, longitude)?;
        let albedo = 0.07 * (1.0 - land) + 0.27 * land;
        let moisture_availability = 1.0 - 0.8 * land;
        let mut air_months = [0.0_f32; CLIMATE_MONTH_COUNT];
        let mut surface_months = [0.0_f32; CLIMATE_MONTH_COUNT];
        let mut humidity_months = [0.0_f32; CLIMATE_MONTH_COUNT];
        for month in 0..CLIMATE_MONTH_COUNT {
            let declination = solar_declination(month);
            let insolation = daily_mean_insolation(latitude, declination);
            let surface_temperature = radiative_equilibrium_c(insolation, f64::from(albedo));
            let air_temperature = surface_temperature - 2.5;
            let lapsed_air = air_temperature - f64::from(LAPSE_RATE_C_PER_M * elevation.max(0.0));
            let saturation = saturation_specific_humidity(lapsed_air as f32)?;
            surface_months[month] = surface_temperature as f32;
            air_months[month] = air_temperature as f32;
            humidity_months[month] = 0.7 * moisture_availability * saturation;
        }
        elevation_m.push(elevation);
        land_fraction.push(land);
        surface_albedo.push(albedo);
        surface_moisture_availability.push(moisture_availability);
        equilibrium_air_temperature_c.push(air_months);
        equilibrium_surface_temperature_c.push(surface_months);
        equilibrium_specific_humidity.push(humidity_months);
    }

    Ok(PlanetForcing::new(
        *grid.fingerprint(),
        elevation_m,
        land_fraction,
        surface_albedo,
        surface_moisture_availability,
        equilibrium_air_temperature_c,
        equilibrium_surface_temperature_c,
        equilibrium_specific_humidity,
    )?)
}

fn surface_fields(
    fixture: CirculationFixture,
    radial: [f64; 3],
    latitude: f64,
    longitude: f64,
) -> Result<(f32, f32), FixtureBuildError> {
    match fixture {
        CirculationFixture::AquaPlanet => Ok((0.0, -4_000.0)),
        CirculationFixture::TwoBasins => {
            let first = in_ellipse(
                latitude,
                longitude,
                15.0_f64.to_radians(),
                (-60.0_f64).to_radians(),
                48.0_f64.to_radians(),
                58.0_f64.to_radians(),
            );
            let second = in_ellipse(
                latitude,
                longitude,
                (-10.0_f64).to_radians(),
                110.0_f64.to_radians(),
                53.0_f64.to_radians(),
                52.0_f64.to_radians(),
            );
            let land = if first || second { 1.0 } else { 0.0 };
            if land == 0.0 {
                return Ok((0.0, -4_200.0));
            }
            let belt_normal =
                normalize([0.38, 0.18, 0.907]).ok_or(FixtureBuildError::DegenerateAnalyticField)?;
            let belt_distance = dot(radial, belt_normal).abs().clamp(0.0, 1.0).asin();
            let belt_width = 7.0_f64.to_radians();
            let mountain = 3_200.0 * (-0.5 * (belt_distance / belt_width).powi(2)).exp();
            Ok((1.0, (300.0 + mountain) as f32))
        }
        CirculationFixture::EarthLikeHarmonics => {
            let signal = 0.70 * (2.0 * longitude + 0.35).sin() * latitude.cos()
                + 0.50 * (3.0 * longitude - 0.8).cos() * (2.0 * latitude).cos()
                + 0.30 * (longitude - 2.2 * latitude).sin()
                - 0.20 * (4.0 * latitude).cos();
            let land = if signal > 0.25 { 1.0 } else { 0.0 };
            if land == 0.0 {
                let bathymetry = -4_100.0 + 450.0 * (2.0 * longitude - latitude).sin();
                Ok((0.0, bathymetry as f32))
            } else {
                let ridge = (3.0 * longitude + 1.4 * latitude).sin().max(0.0);
                let relief = 250.0 + 2_600.0 * ridge.powi(2);
                Ok((1.0, relief as f32))
            }
        }
    }
}

fn in_ellipse(
    latitude: f64,
    longitude: f64,
    center_latitude: f64,
    center_longitude: f64,
    latitude_radius: f64,
    longitude_radius: f64,
) -> bool {
    let latitude_offset = (latitude - center_latitude) / latitude_radius;
    let longitude_offset =
        wrapped_longitude(longitude - center_longitude) * center_latitude.cos() / longitude_radius;
    latitude_offset * latitude_offset + longitude_offset * longitude_offset <= 1.0
}

fn wrapped_longitude(value: f64) -> f64 {
    (value + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

fn solar_declination(month: usize) -> f64 {
    let midpoint_day = (month as f64 + 0.5) * 365.0 / CLIMATE_MONTH_COUNT as f64;
    AXIAL_TILT_RAD * (std::f64::consts::TAU * (midpoint_day - 80.0) / 365.0).sin()
}

fn daily_mean_insolation(latitude: f64, declination: f64) -> f64 {
    let cosine_hour_angle = -latitude.tan() * declination.tan();
    let sunset_hour_angle = if cosine_hour_angle >= 1.0 {
        0.0
    } else if cosine_hour_angle <= -1.0 {
        std::f64::consts::PI
    } else {
        cosine_hour_angle.acos()
    };
    let insolation = SOLAR_CONSTANT_W_M2 / std::f64::consts::PI
        * (sunset_hour_angle * latitude.sin() * declination.sin()
            + latitude.cos() * declination.cos() * sunset_hour_angle.sin());
    insolation.max(0.0)
}

fn radiative_equilibrium_c(insolation_w_m2: f64, albedo: f64) -> f64 {
    let effective_flux =
        BACKGROUND_HEAT_FLUX_W_M2 + LOCAL_SOLAR_COUPLING * insolation_w_m2 * (1.0 - albedo);
    (effective_flux / STEFAN_BOLTZMANN_W_M2_K4).powf(0.25) + GREENHOUSE_OFFSET_K - 273.15
}

/// Errors returned by deterministic analytic fixture construction.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum FixtureBuildError {
    #[error(transparent)]
    Forcing(#[from] ForcingError),
    #[error(transparent)]
    Thermodynamics(#[from] ThermodynamicError),
    #[error("analytic fixture produced a degenerate spherical field")]
    DegenerateAnalyticField,
}
