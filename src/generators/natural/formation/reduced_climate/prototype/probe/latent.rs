//! Fixed-flow counterfactuals, separate from the actual frozen E1 operator.
//! No temperature, water, circulation, or evaporation is resolved here.

use super::super::{
    eddy_driven_zonal_wind, hadley_lower_branch, moist_static_energy_j_kg, mse_edge_transport_kg_s,
    vapour_column_mass_kg_m2, water_edge_transport, WaterMonth,
};
use super::{json, ReducedClimateGeometry, ReducedInputs, ReducedWorkClimate, Value, MONTHS};
use crate::world::natural::{
    saturation_specific_humidity_kg_kg, P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K,
    P4_LOWER_LAYER_REFERENCE_PRESSURE_PA, REFERENCE_SURFACE_RELATIVE_HUMIDITY,
    STANDARD_GRAVITY_M_S2, WATER_VAPORIZATION_LATENT_HEAT_J_KG,
};

/// Evaluates every monthly edge with the exact Q1 coefficients and common donor.
/// Values are signed convergences in W/m2; equivalent mm/day is only an energy unit.
pub(super) fn audit(
    grid: &ReducedClimateGeometry,
    inputs: &ReducedInputs,
    work: &ReducedWorkClimate,
) -> Value {
    let names = [
        "qw_eddy",
        "qw_hadley",
        "qw_zonal",
        "qm_theta_eddy",
        "qm_theta_hadley",
        "qm_theta_zonal",
        "qm_air_eddy",
        "qm_air_hadley",
        "qm_air_zonal",
        "e1_frozen_eddy_latent",
        "e1_frozen_eddy_dry",
        "e1_frozen_total_mse",
        "monthly_nonlinear_total_mse",
        "e1_frozen_hadley_total_mse",
        "w2_monthly_hadley_total_mse",
    ];
    let comparisons = [
        ("qm_theta_minus_qw_eddy", 3, 0),
        ("qm_theta_minus_qw_hadley", 4, 1),
        ("qm_theta_minus_qw_zonal", 5, 2),
        ("qm_air_minus_qw_eddy", 6, 0),
        ("qm_theta_minus_qm_air_eddy", 3, 6),
        ("e1_frozen_minus_qw_eddy_latent", 9, 0),
        ("e1_frozen_minus_monthly_qm_eddy_latent", 9, 3),
        ("e1_frozen_minus_monthly_total_mse", 11, 12),
        ("e1_frozen_minus_w2_hadley_total_mse", 13, 14),
    ];
    let n = grid.cell_count();
    let mq = vapour_column_mass_kg_m2();
    let column_mass = P4_LOWER_LAYER_REFERENCE_PRESSURE_PA / STANDARD_GRAVITY_M_S2;
    let rh = (REFERENCE_SURFACE_RELATIVE_HUMIDITY * inputs.moisture_scale).clamp(0.0, 1.0);
    let latent = WATER_VAPORIZATION_LATENT_HEAT_J_KG;
    let mut convergence = vec![vec![[0.0; MONTHS]; n]; names.len()];
    let mut edge_abs = vec![[0.0; MONTHS]; names.len()];
    let mut edge_delta_abs = vec![[0.0; MONTHS]; comparisons.len()];
    let mut edge_delta_max = vec![[0.0_f64; MONTHS]; comparisons.len()];
    let mut opposite_sign_edges = vec![[0_usize; MONTHS]; comparisons.len()];
    let zonal = eddy_driven_zonal_wind(grid, inputs, column_mass);
    for month in 0..MONTHS {
        let theta: Vec<f64> = work.theta_c.iter().map(|m| m[month]).collect();
        let air: Vec<f64> = work.air_temperature_c.iter().map(|m| m[month]).collect();
        let wind: Vec<[f64; 3]> = work.wind_m_s.iter().map(|m| m[month]).collect();
        let water: Vec<f64> = work
            .specific_humidity
            .iter()
            .map(|m| m[month] * mq)
            .collect();
        let qm_theta: Vec<f64> = theta
            .iter()
            .map(|&t| rh * saturation_specific_humidity_kg_kg(t) * mq)
            .collect();
        let qm_air: Vec<f64> = air
            .iter()
            .map(|&t| rh * saturation_specific_humidity_kg_kg(t) * mq)
            .collect();
        let mse: Vec<f64> = theta
            .iter()
            .map(|&t| moist_static_energy_j_kg(t, rh))
            .collect();
        let hadley = hadley_lower_branch(
            grid,
            &mse,
            column_mass,
            inputs.physics.hadley_meridional_only,
            inputs.physics.zonal_mean_hadley,
        );
        let coefficients = water_edge_transport(
            grid,
            inputs,
            &WaterMonth {
                theta: &theta,
                air: &air,
                wind: &wind,
                zonal: &zonal,
                hadley_mass_flux: &hadley.edge_mass_flux_kg_s,
                vapour_column_mass: mq,
                land_share: &[],
                warm: None,
            },
        );
        for (edge_id, (edge, &(diffusion, hadley, advective))) in
            grid.edges.iter().zip(&coefficients).enumerate()
        {
            let [i, j] = edge.cells.map(|c| c as usize);
            // The total advective flow chooses one donor for both components, as in Q1.
            let donor = if advective > 0.0 { i } else { j };
            let transport = |q: &[f64]| {
                [
                    latent * diffusion * (q[i] - q[j]),
                    latent * hadley * q[donor],
                    latent * (advective - hadley) * q[donor],
                ]
            };
            let mut flux = [0.0; 15];
            flux[..3].copy_from_slice(&transport(&water));
            flux[3..6].copy_from_slice(&transport(&qm_theta));
            flux[6..9].copy_from_slice(&transport(&qm_air));
            let mse_transport = mse_edge_transport_kg_s(edge, column_mass);
            let dry =
                mse_transport * P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K * (theta[i] - theta[j]);
            let frozen =
                work.diagnostics.mebm_edge_conductance_w_k[edge_id] * (theta[i] - theta[j]);
            let nonlinear = mse_transport * (mse[i] - mse[j]);
            flux[9] = (1.0 - edge.hadley_weight) * (frozen - dry);
            flux[10] = (1.0 - edge.hadley_weight) * dry;
            flux[11] = frozen;
            flux[12] = nonlinear;
            flux[13] = edge.hadley_weight * frozen;
            flux[14] = edge.hadley_weight * nonlinear;
            for (term, value) in flux.into_iter().enumerate() {
                assert!(value.is_finite(), "nonfinite latent edge diagnostic");
                convergence[term][i][month] -= value / grid.area_m2[i];
                convergence[term][j][month] += value / grid.area_m2[j];
                edge_abs[term][month] += value.abs();
            }
            for (comparison, &(_, a, b)) in comparisons.iter().enumerate() {
                let delta = flux[a] - flux[b];
                edge_delta_abs[comparison][month] += delta.abs();
                edge_delta_max[comparison][month] =
                    edge_delta_max[comparison][month].max(delta.abs());
                opposite_sign_edges[comparison][month] += usize::from(flux[a] * flux[b] < 0.0);
            }
        }
    }
    let mut terms = serde_json::Map::new();
    for (term, name) in names.iter().enumerate() {
        terms.insert(
            (*name).to_owned(),
            json!({
                "convergence": regions(grid, inputs, &convergence[term]),
                "monthly_sum_absolute_edge_flux_w": edge_abs[term],
            }),
        );
    }
    let mut differences = serde_json::Map::new();
    for (comparison, &(name, a, b)) in comparisons.iter().enumerate() {
        let delta: Vec<[f64; MONTHS]> = convergence[a]
            .iter()
            .zip(&convergence[b])
            .map(|(x, y)| std::array::from_fn(|m| x[m] - y[m]))
            .collect();
        differences.insert(
            name.to_owned(),
            json!({
                "convergence": regions(grid, inputs, &delta),
                "monthly_sum_absolute_edge_flux_w": edge_delta_abs[comparison],
                "monthly_max_absolute_edge_flux_w": edge_delta_max[comparison],
                "monthly_opposite_sign_edge_count": opposite_sign_edges[comparison],
            }),
        );
    }
    let mut reconstruction_error = [0.0_f64; 3];
    for (component, (cells, error)) in convergence
        .iter()
        .zip(&mut reconstruction_error)
        .enumerate()
    {
        for (months, original) in cells.iter().zip(&work.vapour_convergence_mm_day) {
            let reconstructed = months.iter().sum::<f64>() / MONTHS as f64 / latent * 86_400.0;
            *error = error.max((reconstructed - original[component]).abs());
        }
    }
    json!({
        "note": "Fixed Q1 flows and shared total-advection donor. qm_theta uses E1 sea-level theta; qm_air isolates reference-height fixed RH. Neither counterfactual resolves Q1 or E1. E1 frozen uses the actual final Picard operator; zonal Q1 has no matching explicit E1 advection. Do not add L(P-E) to total-MSE E1.",
        "work_cells": n, "edges": grid.edges.len(),
        "column_mass_kg_m2": column_mass, "vapour_column_mass_kg_m2": mq,
        "retained_e1_operator_bytes": work.diagnostics.mebm_edge_conductance_w_k.len()
            * std::mem::size_of::<f64>(),
        "max_qw_reconstruction_error_mm_day": reconstruction_error,
        "terms": terms, "differences": differences,
    })
}

fn regions(
    grid: &ReducedClimateGeometry,
    inputs: &ReducedInputs,
    convergence: &[[f64; MONTHS]],
) -> Value {
    let mut regions = serde_json::Map::new();
    for name in ["global", "land_fraction", "ocean_fraction", "pure_land"] {
        let mut area = 0.0;
        let mut sums = [0.0; MONTHS];
        let mut square = 0.0;
        for (cell, values) in convergence.iter().enumerate() {
            let land = inputs.land_fraction[cell];
            let share = match name {
                "land_fraction" => land,
                "ocean_fraction" => 1.0 - land,
                "pure_land" => f64::from(land == 1.0),
                _ => 1.0,
            };
            let weight = grid.area_m2[cell] * share;
            area += weight;
            for (sum, value) in sums.iter_mut().zip(values) {
                *sum += weight * value;
                square += weight * value * value / MONTHS as f64;
            }
        }
        if area == 0.0 {
            continue;
        }
        let monthly = sums.map(|value| value / area);
        let annual = monthly.iter().sum::<f64>() / MONTHS as f64;
        regions.insert(
            name.to_owned(),
            json!({
                "area_m2": area,
                "monthly_mean_w_m2": monthly,
                "annual_mean_w_m2": annual,
                "cell_month_rms_w_m2": (square / area).sqrt(),
                "annual_latent_equivalent_mm_day": annual * 86_400.0
                    / WATER_VAPORIZATION_LATENT_HEAT_J_KG,
            }),
        );
    }
    Value::Object(regions)
}
