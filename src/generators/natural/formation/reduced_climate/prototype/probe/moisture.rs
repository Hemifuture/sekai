//! R7 Eulerian moisture ledger, evaluated on the actual work-cell areas.
//!
//! These are signed flux contributions, not tagged water-source fractions.
//! Fractional coastal cells share one atmosphere; weighting that atmosphere
//! by land area does not isolate evaporation from its land surface.

use super::*;
use crate::world::natural::{
    REFERENCE_SURFACE_RELATIVE_HUMIDITY, WATER_VAPORIZATION_LATENT_HEAT_J_KG,
};

pub(super) fn budget(
    grid: &ReducedClimateGeometry,
    inputs: &ReducedInputs,
    work: &ReducedWorkClimate,
) -> Value {
    let mean = |values: &[f64; MONTHS]| values.iter().sum::<f64>() / MONTHS as f64;
    let mut regions = serde_json::Map::new();
    for name in ["global", "land_fraction", "ocean_fraction", "pure_land"] {
        let mut area = 0.0;
        let mut sums = [0.0; 11];
        let mut max_local_residual = 0.0_f64;
        for c in 0..grid.cell_count() {
            let land = inputs.land_fraction[c];
            let share = match name {
                "land_fraction" => land,
                "ocean_fraction" => 1.0 - land,
                "pure_land" => f64::from(land == 1.0),
                _ => 1.0,
            };
            let weight = grid.area_m2[c] * share;
            if weight == 0.0 {
                continue;
            }
            let p = mean(&work.precipitation_mm_day[c]);
            let e = mean(&work.evaporation_mm_day[c]);
            let land_surface_e = work.land_evaporative_fraction[c] * p;
            let local_land_e = land * land_surface_e;
            let conv = work.vapour_convergence_mm_day[c];
            let rh = (0..MONTHS)
                .map(|m| {
                    work.specific_humidity[c][m]
                        / saturation_specific_humidity_kg_kg(work.air_temperature_c[c][m])
                })
                .sum::<f64>()
                / MONTHS as f64;
            // Diagnoses the humidity mismatch; this is not a latent-energy budget correction.
            let mse_excess = (0..MONTHS)
                .map(|m| {
                    WATER_VAPORIZATION_LATENT_HEAT_J_KG
                        * (REFERENCE_SURFACE_RELATIVE_HUMIDITY
                            * inputs.moisture_scale
                            * saturation_specific_humidity_kg_kg(work.theta_c[c][m])
                            - work.specific_humidity[c][m])
                })
                .sum::<f64>()
                / MONTHS as f64;
            let residual = p - e - conv.iter().sum::<f64>();
            max_local_residual = max_local_residual.max(residual.abs());
            let values = [
                p,
                e,
                local_land_e,
                e - local_land_e,
                land_surface_e,
                conv[0],
                conv[1],
                conv[2],
                rh,
                mse_excess,
                residual,
            ];
            assert!(
                values.iter().all(|v| v.is_finite()),
                "nonfinite moisture ledger"
            );
            for (sum, value) in sums.iter_mut().zip(values) {
                *sum += weight * value;
            }
            area += weight;
        }
        if area == 0.0 {
            continue;
        }
        let values = sums.map(|v| v / area);
        regions.insert(
            name.to_owned(),
            json!({
                "area_m2": area,
                "p_mm_day": values[0],
                "e_mm_day": values[1],
                "local_land_e_mm_day": values[2],
                "local_sea_e_mm_day": values[3],
            "land_surface_e_mm_day": matches!(name, "land_fraction" | "pure_land")
                .then_some(values[4]),
                "eddy_mm_day": values[5],
                "hadley_mm_day": values[6],
                "zonal_mm_day": values[7],
                "rh_reference_height": values[8],
            "mse_sea_level_fixed_rh_latent_excess_j_kg": values[9],
                "closure_mm_day": values[10],
                "max_cell_closure_mm_day": max_local_residual,
            }),
        );
    }
    json!({
        "note": "P = local_land_E + local_sea_E + eddy + Hadley + zonal; signed Eulerian terms, not moisture tagging. land_surface_E is per land area; local_land_E includes the cell land fraction.",
        "regions": regions,
    })
}
