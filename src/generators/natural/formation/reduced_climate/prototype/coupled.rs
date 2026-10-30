//! R9 controlled edge and periodic-budget kernels; no coupled climate solver.

use super::moist_static_energy_j_kg;
use crate::world::natural::{
    CLIMATE_MONTH_COUNT, SECONDS_PER_CLIMATOLOGICAL_MONTH, STANDARD_GRAVITY_M_S2,
    WATER_VAPORIZATION_LATENT_HEAT_J_KG,
};

#[derive(Debug, Clone, Copy, PartialEq)]
struct SurfacePoint {
    air_temperature_c: f64,
    specific_humidity: f64,
    reference_height_m: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PairedEdgeFlux {
    vapour_kg_s: f64,
    dry_w: f64,
    moist_w: f64,
    upper_mass_kg_s: f64,
}

/// Returns standard-approximation dry/moist static energy at the point's declared height.
fn specific_energies_j_kg(point: SurfacePoint) -> (f64, f64) {
    // Reuse the existing zero-moisture enthalpy to share its Celsius/Kelvin conversion.
    let dry = moist_static_energy_j_kg(point.air_temperature_c, 0.0)
        + STANDARD_GRAVITY_M_S2 * point.reference_height_m;
    (
        dry,
        dry + WATER_VAPORIZATION_LATENT_HEAT_J_KG * point.specific_humidity,
    )
}

/// Uses a validated eddy exchange coefficient, not a second geometry formula.
fn paired_edge_fluxes(
    points: [SurfacePoint; 2],
    eddy_exchange_kg_s: f64,
    hadley_mass_kg_s: f64,
    zonal_mass_kg_s: f64,
    upper_mse_j_kg: f64,
) -> PairedEdgeFlux {
    let dry = points.map(|point| specific_energies_j_kg(point).0);
    let lower_mass = hadley_mass_kg_s + zonal_mass_kg_s;
    let donor = usize::from(lower_mass <= 0.0);
    let upper_mass = -hadley_mass_kg_s;
    let vapour = eddy_exchange_kg_s * (points[0].specific_humidity - points[1].specific_humidity)
        + lower_mass * points[donor].specific_humidity;
    let dry_flux = eddy_exchange_kg_s * (dry[0] - dry[1])
        + lower_mass * dry[donor]
        + upper_mass * upper_mse_j_kg;
    PairedEdgeFlux {
        vapour_kg_s: vapour,
        dry_w: dry_flux,
        moist_w: dry_flux + WATER_VAPORIZATION_LATENT_HEAT_J_KG * vapour,
        upper_mass_kg_s: upper_mass,
    }
}

/// Backward difference on the model's periodic, equally spaced monthly samples.
pub(super) fn periodic_storage_rate(
    values: &[f64; CLIMATE_MONTH_COUNT],
) -> [f64; CLIMATE_MONTH_COUNT] {
    std::array::from_fn(|m| {
        (values[m] - values[(m + CLIMATE_MONTH_COUNT - 1) % CLIMATE_MONTH_COUNT])
            / SECONDS_PER_CLIMATOLOGICAL_MONTH
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BudgetTerms {
    thermal_storage_w_m2: f64,
    vapour_storage_kg_m2_s: f64,
    net_radiation_w_m2: f64,
    evaporation_kg_m2_s: f64,
    precipitation_kg_m2_s: f64,
    vapour_convergence_kg_m2_s: f64,
    dry_convergence_w_m2: f64,
    moist_convergence_w_m2: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BudgetResiduals {
    water_kg_m2_s: f64,
    total_w_m2: f64,
    dry_w_m2: f64,
}

fn budget_residuals(terms: BudgetTerms) -> BudgetResiduals {
    let latent = WATER_VAPORIZATION_LATENT_HEAT_J_KG;
    BudgetResiduals {
        water_kg_m2_s: terms.vapour_storage_kg_m2_s - terms.evaporation_kg_m2_s
            + terms.precipitation_kg_m2_s
            - terms.vapour_convergence_kg_m2_s,
        total_w_m2: terms.thermal_storage_w_m2 + latent * terms.vapour_storage_kg_m2_s
            - terms.net_radiation_w_m2
            - terms.moist_convergence_w_m2,
        dry_w_m2: terms.thermal_storage_w_m2
            - terms.net_radiation_w_m2
            - terms.dry_convergence_w_m2
            - latent * (terms.precipitation_kg_m2_s - terms.evaporation_kg_m2_s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::natural::{
        p4_seasonal_storage_heat_capacities_j_m2_k, P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K,
        REDUCED_CLIMATE_LINEAR_RELATIVE_TOLERANCE, STANDARD_ATMOSPHERE_SEA_LEVEL_TEMPERATURE_C,
        TRANSEBM_OLR_SLOPE_W_M2_K,
    };

    fn assert_scaled(error: f64, scale: f64) {
        assert!(
            error.abs() <= REDUCED_CLIMATE_LINEAR_RELATIVE_TOLERANCE * scale,
            "scaled residual {error:e} / {scale:e}"
        );
    }

    /// One edge is sufficient: geometry's coefficient contract is covered by its own suite.
    #[test]
    fn r9_edge_pair_preserves_reference_height_and_shared_donor() {
        let first = SurfacePoint {
            air_temperature_c: STANDARD_ATMOSPHERE_SEA_LEVEL_TEMPERATURE_C,
            specific_humidity: 0.01,
            reference_height_m: 0.0,
        };
        let mut second = SurfacePoint {
            specific_humidity: 0.005,
            reference_height_m: 1_000.0,
            ..first
        };
        second.air_temperature_c += (WATER_VAPORIZATION_LATENT_HEAT_J_KG
            * (first.specific_humidity - second.specific_humidity)
            - STANDARD_GRAVITY_M_S2 * second.reference_height_m)
            / P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K;
        let points = [first, second];
        let (_, h0) = specific_energies_j_kg(first);
        let (_, h1) = specific_energies_j_kg(second);
        assert_scaled(h0 - h1, h0.abs() + h1.abs());

        let eddy = paired_edge_fluxes(points, 1.0, 0.0, 0.0, 0.0);
        let expected_vapour = first.specific_humidity - second.specific_humidity;
        assert_scaled(eddy.vapour_kg_s - expected_vapour, expected_vapour.abs());
        assert!(eddy.dry_w < 0.0 && eddy.vapour_kg_s > 0.0);
        assert_scaled(
            eddy.moist_w,
            eddy.dry_w.abs() + WATER_VAPORIZATION_LATENT_HEAT_J_KG * eddy.vapour_kg_s.abs(),
        );

        let upper = h0 + P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K;
        // Total lower flow is -1 kg/s although the Hadley part is +1 kg/s.
        let reverse = paired_edge_fluxes(points, 0.0, 1.0, -2.0, upper);
        let (s1, _) = specific_energies_j_kg(second);
        assert_scaled(
            reverse.vapour_kg_s + second.specific_humidity,
            second.specific_humidity,
        );
        assert_scaled(reverse.dry_w + s1 + upper, s1.abs() + upper.abs());
        assert_eq!(reverse.upper_mass_kg_s, -1.0);
    }

    /// Arbitrary nonzero residuals must still obey the energy-minus-water identity.
    #[test]
    fn r9_periodic_water_and_energy_reduce_to_the_same_dry_budget() {
        let water = std::array::from_fn(|m| 20.0 + (m as f64).sin());
        let temperature = std::array::from_fn(|m| 15.0 + (m as f64).cos());
        let water_rate = periodic_storage_rate(&water);
        let temperature_rate = periodic_storage_rate(&temperature);
        let (capacity, _) = p4_seasonal_storage_heat_capacities_j_m2_k();
        let latent = WATER_VAPORIZATION_LATENT_HEAT_J_KG;
        for m in 0..CLIMATE_MONTH_COUNT {
            let residual = budget_residuals(BudgetTerms {
                thermal_storage_w_m2: capacity * temperature_rate[m],
                vapour_storage_kg_m2_s: water_rate[m],
                net_radiation_w_m2: 1.0,
                evaporation_kg_m2_s: 2.0e-5,
                precipitation_kg_m2_s: 1.0e-5,
                vapour_convergence_kg_m2_s: -1.0e-5,
                dry_convergence_w_m2: 2.0,
                moist_convergence_w_m2: 2.0 - latent * 1.0e-5,
            });
            let difference =
                residual.total_w_m2 - latent * residual.water_kg_m2_s - residual.dry_w_m2;
            assert_scaled(
                difference,
                residual.total_w_m2.abs()
                    + latent * residual.water_kg_m2_s.abs()
                    + residual.dry_w_m2.abs(),
            );
        }
        assert_scaled(
            water_rate.iter().sum(),
            water_rate.iter().map(|x| x.abs()).sum(),
        );
        assert_scaled(
            temperature_rate.iter().sum(),
            temperature_rate.iter().map(|x| x.abs()).sum(),
        );
    }

    /// Substitution of an independent discrete analytic response tests phase and wrap indexing.
    #[test]
    fn r9_backward_euler_single_harmonic_matches_discrete_response() {
        let (capacity, _) = p4_seasonal_storage_heat_capacities_j_m2_k();
        let damping = TRANSEBM_OLR_SLOPE_W_M2_K;
        let omega = std::f64::consts::TAU / CLIMATE_MONTH_COUNT as f64;
        let real = damping + capacity / SECONDS_PER_CLIMATOLOGICAL_MONTH * (1.0 - omega.cos());
        let imaginary = capacity / SECONDS_PER_CLIMATOLOGICAL_MONTH * omega.sin();
        let mean = STANDARD_ATMOSPHERE_SEA_LEVEL_TEMPERATURE_C;
        let response = std::array::from_fn(|m| {
            let phase = omega * m as f64;
            mean + damping * (real * phase.cos() + imaginary * phase.sin())
                / (real * real + imaginary * imaginary)
        });
        let rate = periodic_storage_rate(&response);
        for m in 0..CLIMATE_MONTH_COUNT {
            let forcing = damping * (mean + (omega * m as f64).cos());
            let residual = capacity * rate[m] + damping * response[m] - forcing;
            assert_scaled(
                residual,
                (capacity * rate[m]).abs() + (damping * response[m]).abs() + forcing.abs(),
            );
        }
        assert_eq!(
            periodic_storage_rate(&[mean; CLIMATE_MONTH_COUNT]),
            [0.0; CLIMATE_MONTH_COUNT]
        );
    }
}
