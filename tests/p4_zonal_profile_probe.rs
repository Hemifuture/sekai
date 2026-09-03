//! Zonal-mean diagnostic of the P4 endpoint climate on Draft seed 42: per 5°
//! band, area-weighted annual means of air temperature, its distance from the
//! local radiative-equilibrium target, specific and relative humidity,
//! precipitation, evaporation, and the TOA budget, next to approximate Earth
//! zonal means (GPCP 1979–2010 precipitation; NCEP/ERA 2 m temperature).
//! It is the measurement that seeds the P4 水热校正 design.

mod support;

use sekai::world::natural::{
    gray_equilibrium_surface_temperature_c, gray_longwave_slope_w_m2_k,
    p4_seasonal_storage_heat_capacities_j_m2_k, saturation_specific_humidity_kg_kg,
    seasonal_storage_equilibrium_temperature_c, CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M,
};
use support::causal_formation::causal_formation_fixture;

const BANDS: usize = 36;

/// Approximate Earth annual zonal means by 10° band centre, southern to
/// northern pole: (2 m temperature °C, precipitation mm/day).
const EARTH_REFERENCE: [(f64, f64, f64); 18] = [
    (-85.0, -35.0, 0.3),
    (-75.0, -20.0, 0.6),
    (-65.0, -8.0, 1.6),
    (-55.0, 2.0, 2.6),
    (-45.0, 8.0, 2.8),
    (-35.0, 15.0, 2.1),
    (-25.0, 20.0, 1.6),
    (-15.0, 24.0, 2.6),
    (-5.0, 26.0, 4.6),
    (5.0, 26.0, 4.6),
    (15.0, 25.0, 2.6),
    (25.0, 21.0, 1.6),
    (35.0, 15.0, 2.0),
    (45.0, 8.0, 2.6),
    (55.0, 1.0, 2.3),
    (65.0, -8.0, 1.5),
    (75.0, -15.0, 0.8),
    (85.0, -20.0, 0.4),
];

#[test]
#[ignore = "release-only Draft seed 42 zonal-mean climate diagnostic"]
fn p4_zonal_profile() {
    let fixture = causal_formation_fixture();
    let bundle = fixture.artifact.bundle();
    let climate = bundle.climate();
    let fields = climate.fields();
    let solve = climate.solve_report();
    eprintln!(
        "[zonal] cycles={} final_residual={:.4}",
        solve.formation_cycles(),
        solve.final_residual()
    );
    let elevation = bundle
        .surface_formation()
        .terrain_fields()
        .current_elevation_m();
    let land = bundle
        .surface_formation()
        .terrain_fields()
        .land_ocean()
        .raw_values();
    let sea_level = bundle
        .surface_formation()
        .terrain_fields()
        .surface_water_geometry()
        .sea_level_m();

    // columns: area, T, T_eq, q, qsat, P, E, orographic P, ASR, OLR, Tjul-Tjan, land area,
    // SST, SST jul-jan, air-target Jul-Jan (storage-consistent seasonal target, A4 §3.1),
    // SST-target Jul-Jan
    let mut sums = vec![[0.0_f64; 16]; BANDS];
    let (air_storage, mixed_storage) = p4_seasonal_storage_heat_capacities_j_m2_k();
    let annual = |months: &[f32; 12]| months.iter().map(|v| f64::from(*v)).sum::<f64>() / 12.0;
    for (index, cell) in fixture.surface.cells().iter().enumerate() {
        let latitude = cell.centroid.components()[2].asin().to_degrees();
        let band = (((latitude + 90.0) / 5.0).floor() as usize).min(BANDS - 1);
        let area = cell.area.get();
        let temperature = annual(&fields.monthly_air_temperature_c().values()[index]);
        let asr = annual(&fields.monthly_absorbed_shortwave_w_m2().values()[index]);
        let olr = annual(&fields.monthly_outgoing_longwave_w_m2().values()[index]);
        let humidity = annual(&fields.monthly_specific_humidity().values()[index]);
        let precipitation = annual(&fields.monthly_precipitation_mm_day().values()[index]);
        let evaporation = annual(&fields.monthly_evaporation_mm_day().values()[index]);
        let orographic = annual(&fields.monthly_orographic_precipitation_mm_day().values()[index]);
        let orography = if land[index] == 1 {
            f64::from(elevation[index] - sea_level).max(0.0)
        } else {
            0.0
        };
        let equilibrium = gray_equilibrium_surface_temperature_c(asr)
            - CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M * orography;
        let saturation = saturation_specific_humidity_kg_kg(temperature);
        let months = &fields.monthly_air_temperature_c().values()[index];
        let seasonal = f64::from(months[6] - months[0]);
        let row = &mut sums[band];
        row[0] += area;
        row[1] += area * temperature;
        row[2] += area * equilibrium;
        row[3] += area * humidity;
        row[4] += area * saturation;
        row[5] += area * precipitation;
        row[6] += area * evaporation;
        row[7] += area * orographic;
        row[8] += area * asr;
        row[9] += area * olr;
        row[10] += area * seasonal;
        row[11] += if land[index] == 1 { area } else { 0.0 };
        let sst = &fields.monthly_sea_surface_temperature_c().values()[index];
        row[12] += area * annual(sst);
        row[13] += area * f64::from(sst[6] - sst[0]);
        // Reconstruct the forcing's storage-consistent seasonal targets (A4 §3.1)
        // from the published monthly absorbed shortwave.
        let monthly_asr = &fields.monthly_absorbed_shortwave_w_m2().values()[index];
        let mut asr_months = [0.0_f64; 12];
        for (target, value) in asr_months.iter_mut().zip(monthly_asr) {
            *target = f64::from(*value);
        }
        let land_fraction = if land[index] == 1 { 1.0 } else { 0.0 };
        let slope = gray_longwave_slope_w_m2_k(equilibrium);
        let air_target = seasonal_storage_equilibrium_temperature_c(
            &asr_months,
            equilibrium,
            slope,
            air_storage + (1.0 - land_fraction) * mixed_storage,
        );
        let sea_target = seasonal_storage_equilibrium_temperature_c(
            &asr_months,
            equilibrium,
            slope,
            mixed_storage,
        );
        row[14] += area * (air_target[6] - air_target[0]);
        row[15] += area * (sea_target[6] - sea_target[0]);
    }
    eprintln!(
        "[zonal]   lat  land%   T_air   T_eq  dT     q g/kg  RH%    P     E    P-E  oroP   ASR    OLR   TOA  Tjul-Tjan   SST  SSTjul-jan Tt7-1 St7-1 | Earth T   Earth P"
    );
    let mut global = [0.0_f64; 16];
    for (band, row) in sums.iter().enumerate() {
        if row[0] <= 0.0 {
            continue;
        }
        for (g, r) in global.iter_mut().zip(row) {
            *g += r;
        }
        let a = row[0];
        let latitude = -90.0 + 5.0 * band as f64 + 2.5;
        let reference = EARTH_REFERENCE
            .iter()
            .min_by(|l, r| (l.0 - latitude).abs().total_cmp(&(r.0 - latitude).abs()))
            .unwrap();
        eprintln!(
            "[zonal] {latitude:>5.1} {:>5.1} {:>7.1} {:>6.1} {:>5.1} {:>7.2} {:>5.0} {:>5.2} {:>5.2} {:>5.2} {:>5.2} {:>6.1} {:>6.1} {:>5.1} {:>8.1} {:>6.1} {:>8.1} {:>6.1} {:>6.1}   | {:>6.1} {:>8.1}",
            100.0 * row[11] / a,
            row[1] / a,
            row[2] / a,
            (row[1] - row[2]) / a,
            1000.0 * row[3] / a,
            100.0 * (row[3] / a) / (row[4] / a).max(1e-9),
            row[5] / a,
            row[6] / a,
            (row[5] - row[6]) / a,
            row[7] / a,
            row[8] / a,
            row[9] / a,
            (row[8] - row[9]) / a,
            row[10] / a,
            row[12] / a,
            row[13] / a,
            row[14] / a,
            row[15] / a,
            reference.1,
            reference.2
        );
    }
    let a = global[0];
    // Evaporation closure diagnostic (A5 Task 0): the bulk formula wants a
    // near-surface wind, the model hands it the 6 km slab mean. Compare the
    // ocean-mean slab wind against Earth's ~6.6 m/s 10 m ocean wind, and
    // reconstruct the bulk flux from its own factors.
    let mut ocean_area = 0.0_f64;
    let mut ocean_wind = 0.0_f64;
    let mut ocean_deficit = 0.0_f64;
    let mut ocean_evaporation = 0.0_f64;
    for (index, cell) in fixture.surface.cells().iter().enumerate() {
        if land[index] == 1 {
            continue;
        }
        let area = cell.area.get();
        let winds = &fields.near_surface_wind_m_s().values()[index];
        let sst = &fields.monthly_sea_surface_temperature_c().values()[index];
        let humidity = &fields.monthly_specific_humidity().values()[index];
        let evaporation = &fields.monthly_evaporation_mm_day().values()[index];
        for month in 0..12 {
            let speed = winds[month]
                .iter()
                .map(|component| f64::from(*component).powi(2))
                .sum::<f64>()
                .sqrt();
            let deficit = (saturation_specific_humidity_kg_kg(f64::from(sst[month]))
                - f64::from(humidity[month]))
            .max(0.0);
            ocean_area += area;
            ocean_wind += area * speed;
            ocean_deficit += area * deficit;
            ocean_evaporation += area * f64::from(evaporation[month]);
        }
    }
    eprintln!(
        "[evap] ocean mean |U| {:.2} m/s (Earth 10 m ocean wind 6.6), saturation deficit {:.2} g/kg, E {:.3} mm/day",
        ocean_wind / ocean_area,
        1000.0 * ocean_deficit / ocean_area,
        ocean_evaporation / ocean_area,
    );
    eprintln!(
        "[zonal] GLOBAL land {:.1}% T {:.2} T_eq {:.2} dT {:.2} q {:.2} g/kg RH {:.0}% P {:.3} E {:.3} ASR {:.1} OLR {:.1} TOA {:.2}",
        100.0 * global[11] / a,
        global[1] / a,
        global[2] / a,
        (global[1] - global[2]) / a,
        1000.0 * global[3] / a,
        100.0 * (global[3] / a) / (global[4] / a),
        global[5] / a,
        global[6] / a,
        global[8] / a,
        global[9] / a,
        (global[8] - global[9]) / a
    );
}
