use super::super::climate::{evaporation_warmth, monthly_declination_degrees};
use super::climate::tangent_wind;
use crate::world::natural::{
    ClimateSpec, LandOceanKind, SphericalReliefSnapshot, CLIMATE_MONTH_COUNT,
    MONTHLY_PRECIPITATION_MAX_MM,
};
use crate::world::spatial::{NaturalSurface, SphericalNaturalSurface};
use crate::world::{CellId, EdgeId};

const WATER_VAPOR_TRANSPORT_STEPS: usize = 48;
const OUTGOING_CFL_FRACTION: f64 = 0.45;
// Keeps ordinary circulation speeds proportional while the CFL branch remains a hard safety cap.
const REFERENCE_TRANSPORT_SPEED_M_S: f64 = 10.0;
const MIN_EDGE_NORMAL_SPEED_M_S: f64 = 1.0e-9;
const PRECIPITATION_PROXY_MM_PER_MOISTURE_UNIT: f64 = 520.0;

#[derive(Debug, Clone, Copy)]
struct EdgeMoistureFlow {
    donor: usize,
    receiver: usize,
    conductance_m2_s: f64,
    donor_mass_fraction: f64,
    condensation_fraction: f64,
}

pub(super) fn solve_monthly_precipitation(
    surface: &SphericalNaturalSurface<'_>,
    relief: &SphericalReliefSnapshot,
    spec: &ClimateSpec,
    latitude_degrees: &[f32],
    maritime: &[f32],
    temperature: &[[f32; CLIMATE_MONTH_COUNT]],
) -> Vec<[f32; CLIMATE_MONTH_COUNT]> {
    let cell_count = surface.cell_count();
    debug_assert_eq!(relief.elevation_m().values().len(), cell_count);
    debug_assert_eq!(latitude_degrees.len(), cell_count);
    debug_assert_eq!(maritime.len(), cell_count);
    debug_assert_eq!(temperature.len(), cell_count);

    let cell_area_m2 = (0..cell_count)
        .map(|index| {
            surface
                .cell(CellId::from_raw(index as u32))
                .expect("validated spherical cell IDs are dense")
                .area()
                .get()
        })
        .collect::<Vec<_>>();
    let mut precipitation = vec![[0.0; CLIMATE_MONTH_COUNT]; cell_count];
    let mut flows = Vec::with_capacity(surface.edge_count());
    let mut outgoing_conductance = vec![0.0_f64; cell_count];
    let mut outgoing_boundary_length = vec![0.0_f64; cell_count];
    let mut vapor_mass = vec![0.0_f64; cell_count];
    let mut mass_delta = vec![0.0_f64; cell_count];
    let mut condensed_mass = vec![0.0_f64; cell_count];
    let declinations = std::array::from_fn::<_, CLIMATE_MONTH_COUNT, _>(|month| {
        monthly_declination_degrees(month, spec.axial_tilt_degrees())
    });

    for (month, &declination) in declinations.iter().enumerate() {
        build_monthly_flows(
            surface,
            relief,
            latitude_degrees,
            maritime,
            temperature,
            month,
            declination,
            &cell_area_m2,
            &mut outgoing_conductance,
            &mut outgoing_boundary_length,
            &mut flows,
        );
        initialize_vapor_mass(
            relief,
            spec.moisture_scale(),
            temperature,
            month,
            &cell_area_m2,
            &mut vapor_mass,
        );

        for _ in 0..WATER_VAPOR_TRANSPORT_STEPS {
            apply_explicit_sources(
                relief,
                spec.moisture_scale(),
                temperature,
                month,
                &cell_area_m2,
                &mut vapor_mass,
            );
            apply_transport_step(
                &flows,
                &mut vapor_mass,
                &mut mass_delta,
                &mut condensed_mass,
            );
        }

        for index in 0..cell_count {
            precipitation[index][month] = (condensed_mass[index] / cell_area_m2[index]
                * PRECIPITATION_PROXY_MM_PER_MOISTURE_UNIT)
                .clamp(0.0, f64::from(MONTHLY_PRECIPITATION_MAX_MM))
                as f32;
        }
    }

    precipitation
}

#[allow(clippy::too_many_arguments)]
fn build_monthly_flows(
    surface: &SphericalNaturalSurface<'_>,
    relief: &SphericalReliefSnapshot,
    latitude_degrees: &[f32],
    maritime: &[f32],
    temperature: &[[f32; CLIMATE_MONTH_COUNT]],
    month: usize,
    declination_degrees: f32,
    cell_area_m2: &[f64],
    outgoing_conductance: &mut [f64],
    outgoing_boundary_length: &mut [f64],
    flows: &mut Vec<EdgeMoistureFlow>,
) {
    flows.clear();
    outgoing_conductance.fill(0.0);
    outgoing_boundary_length.fill(0.0);
    for edge_index in 0..surface.edge_count() {
        let edge_id = EdgeId::from_raw(edge_index as u32);
        let frame = surface
            .edge_frame(edge_id)
            .expect("validated spherical edge IDs are dense");
        let metrics = surface
            .edge(edge_id)
            .expect("validated spherical edge metrics are dense");
        let [first, second] = frame.owners();
        let first_index = first.raw() as usize;
        let second_index = second.raw() as usize;
        let midpoint = frame.midpoint().components();
        let latitude = midpoint[2].asin().to_degrees() as f32;
        let edge_maritime = (maritime[first_index] + maritime[second_index]) * 0.5;
        let wind = tangent_wind(midpoint, latitude, declination_degrees, edge_maritime);
        let normal = frame.normal_from_first().components();
        let signed_normal_speed = dot(wind, normal);
        if signed_normal_speed.abs() <= MIN_EDGE_NORMAL_SPEED_M_S {
            continue;
        }
        let (donor, receiver) = directed_owners([first_index, second_index], signed_normal_speed);
        let boundary_length = metrics.boundary_length().get();
        let conductance = signed_normal_speed.abs() * boundary_length;
        let condensation = condensation_fraction(
            relief.elevation_m().values()[receiver],
            relief.elevation_m().values()[donor],
            temperature[receiver][month],
            latitude_degrees[receiver],
            declination_degrees,
        );
        outgoing_conductance[donor] += conductance;
        outgoing_boundary_length[donor] += boundary_length;
        flows.push(EdgeMoistureFlow {
            donor,
            receiver,
            conductance_m2_s: conductance,
            donor_mass_fraction: 0.0,
            condensation_fraction: condensation,
        });
    }
    prepare_donor_mass_fractions(
        flows,
        outgoing_conductance,
        outgoing_boundary_length,
        cell_area_m2,
    );
    debug_assert!(flows.len() <= surface.edge_count());
}

fn prepare_donor_mass_fractions(
    flows: &mut [EdgeMoistureFlow],
    outgoing_conductance: &[f64],
    outgoing_boundary_length: &[f64],
    cell_area_m2: &[f64],
) {
    for flow in flows {
        let outgoing = outgoing_conductance[flow.donor];
        let boundary_length = outgoing_boundary_length[flow.donor];
        if outgoing <= 0.0 || boundary_length <= 0.0 {
            flow.donor_mass_fraction = 0.0;
            continue;
        }
        // A cell-local pseudo-time keeps relaxation resolution-independent. The
        // reference branch preserves speed response; the stability branch alone
        // limits the sum of all simultaneous donor losses.
        let stability_step_seconds = OUTGOING_CFL_FRACTION * cell_area_m2[flow.donor] / outgoing;
        let reference_step_seconds = OUTGOING_CFL_FRACTION * cell_area_m2[flow.donor]
            / (REFERENCE_TRANSPORT_SPEED_M_S * boundary_length);
        let local_step_seconds = stability_step_seconds.min(reference_step_seconds);
        flow.donor_mass_fraction =
            flow.conductance_m2_s * local_step_seconds / cell_area_m2[flow.donor];
    }
}

fn initialize_vapor_mass(
    relief: &SphericalReliefSnapshot,
    moisture_scale: f32,
    temperature: &[[f32; CLIMATE_MONTH_COUNT]],
    month: usize,
    cell_area_m2: &[f64],
    vapor_mass: &mut [f64],
) {
    for index in 0..vapor_mass.len() {
        let warmth = f64::from(evaporation_warmth(temperature[index][month]));
        let concentration = if relief.land_ocean().raw_values()[index] == LandOceanKind::Ocean.raw()
        {
            f64::from(moisture_scale) * (0.72 + 0.48 * warmth)
        } else {
            f64::from(moisture_scale) * 0.035
        };
        vapor_mass[index] = concentration * cell_area_m2[index];
    }
}

fn apply_explicit_sources(
    relief: &SphericalReliefSnapshot,
    moisture_scale: f32,
    temperature: &[[f32; CLIMATE_MONTH_COUNT]],
    month: usize,
    cell_area_m2: &[f64],
    vapor_mass: &mut [f64],
) {
    for index in 0..vapor_mass.len() {
        let warmth = f64::from(evaporation_warmth(temperature[index][month]));
        if relief.land_ocean().raw_values()[index] == LandOceanKind::Ocean.raw() {
            let equilibrium =
                f64::from(moisture_scale) * (0.72 + 0.48 * warmth) * cell_area_m2[index];
            if vapor_mass[index] < equilibrium {
                vapor_mass[index] += equilibrium - vapor_mass[index];
            }
        } else {
            let recycled_concentration = f64::from(moisture_scale) * (0.004 + 0.008 * warmth);
            vapor_mass[index] += recycled_concentration * cell_area_m2[index];
        }
    }
}

fn apply_transport_step(
    flows: &[EdgeMoistureFlow],
    vapor_mass: &mut [f64],
    mass_delta: &mut [f64],
    condensed_mass: &mut [f64],
) {
    mass_delta.fill(0.0);
    condensed_mass.fill(0.0);
    for flow in flows {
        let transported = vapor_mass[flow.donor] * flow.donor_mass_fraction;
        let condensed = transported * flow.condensation_fraction;
        mass_delta[flow.donor] -= transported;
        mass_delta[flow.receiver] += transported - condensed;
        condensed_mass[flow.receiver] += condensed;
    }
    for (vapor, delta) in vapor_mass.iter_mut().zip(mass_delta.iter()) {
        *vapor += *delta;
        debug_assert!(*vapor >= -f64::EPSILON * 64.0);
    }
}

fn directed_owners(owners: [usize; 2], signed_normal_speed: f64) -> (usize, usize) {
    if signed_normal_speed.is_sign_positive() {
        (owners[0], owners[1])
    } else {
        (owners[1], owners[0])
    }
}

fn condensation_fraction(
    receiver_elevation_m: f32,
    donor_elevation_m: f32,
    receiver_temperature_c: f32,
    receiver_latitude_degrees: f32,
    declination_degrees: f32,
) -> f64 {
    let uplift =
        f64::from(((receiver_elevation_m - donor_elevation_m).max(0.0) / 1_200.0).min(1.5));
    let warmth = f64::from(evaporation_warmth(receiver_temperature_c));
    let convergence_latitude = declination_degrees * 0.6;
    let tropical_convergence =
        f64::from((-(receiver_latitude_degrees - convergence_latitude).abs() / 15.0).exp());
    (0.018 + 0.055 * warmth + 0.075 * tropical_convergence + 0.42 * uplift).clamp(0.012, 0.78)
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(
        donor: usize,
        receiver: usize,
        conductance_m2_s: f64,
        condensation_fraction: f64,
    ) -> EdgeMoistureFlow {
        EdgeMoistureFlow {
            donor,
            receiver,
            conductance_m2_s,
            donor_mass_fraction: 0.0,
            condensation_fraction,
        }
    }

    #[test]
    fn one_edge_transfer_is_paired_and_condensation_closes_the_budget() {
        let mut flows = vec![flow(0, 1, 2.0, 0.25)];
        prepare_donor_mass_fractions(&mut flows, &[2.0, 0.0], &[1.0, 0.0], &[8.0, 8.0]);
        let mut vapor = vec![10.0, 1.0];
        let before = vapor.iter().sum::<f64>();
        let mut delta = vec![0.0; 2];
        let mut condensed = vec![0.0; 2];

        apply_transport_step(&flows, &mut vapor, &mut delta, &mut condensed);

        assert!((vapor[0] - 9.1).abs() <= 1.0e-12);
        assert!((vapor[1] - 1.675).abs() <= 1.0e-12);
        assert!((condensed[1] - 0.225).abs() <= 1.0e-12);
        assert!(
            (before - vapor.iter().sum::<f64>() - condensed.iter().sum::<f64>()).abs() <= 1.0e-12
        );
    }

    #[test]
    fn arbitrary_outgoing_fan_stays_nonnegative_under_the_local_limiter() {
        let mut flows = (1..=1_000)
            .map(|receiver| flow(0, receiver, receiver as f64, 0.0))
            .collect::<Vec<_>>();
        let outgoing = flows.iter().map(|flow| flow.conductance_m2_s).sum::<f64>();
        let mut outgoing_by_cell = vec![0.0; 1_001];
        outgoing_by_cell[0] = outgoing;
        let mut boundary_length_by_cell = vec![0.0; 1_001];
        boundary_length_by_cell[0] = flows.len() as f64;
        let areas = vec![1.0; 1_001];
        prepare_donor_mass_fractions(
            &mut flows,
            &outgoing_by_cell,
            &boundary_length_by_cell,
            &areas,
        );
        let fraction_sum = flows
            .iter()
            .map(|flow| flow.donor_mass_fraction)
            .sum::<f64>();
        assert!((fraction_sum - OUTGOING_CFL_FRACTION).abs() <= 1.0e-12);

        let mut vapor = vec![0.0; 1_001];
        vapor[0] = 1.0;
        let mut delta = vec![0.0; 1_001];
        let mut condensed = vec![0.0; 1_001];
        apply_transport_step(&flows, &mut vapor, &mut delta, &mut condensed);
        assert!(vapor.iter().all(|&value| value >= 0.0));
        assert!((vapor[0] - (1.0 - OUTGOING_CFL_FRACTION)).abs() <= 1.0e-12);
        assert!((vapor.iter().sum::<f64>() - 1.0).abs() <= 1.0e-12);
    }

    #[test]
    fn reversing_owner_labels_and_edge_normal_preserves_flow_direction() {
        assert_eq!(directed_owners([4, 9], 3.0), (4, 9));
        assert_eq!(directed_owners([9, 4], -3.0), (4, 9));
    }

    #[test]
    fn limiter_preserves_wind_speed_response_below_its_stability_ceiling() {
        let mut flows = vec![flow(0, 2, 1.0, 0.0), flow(1, 3, 5.0, 0.0)];
        prepare_donor_mass_fractions(
            &mut flows,
            &[1.0, 5.0, 0.0, 0.0],
            &[1.0, 1.0, 0.0, 0.0],
            &[8.0; 4],
        );

        assert!(
            flows[1].donor_mass_fraction > flows[0].donor_mass_fraction * 4.9,
            "slow={}, fast={}",
            flows[0].donor_mass_fraction,
            flows[1].donor_mass_fraction
        );
    }

    #[test]
    fn condensation_responds_to_receiver_climate_and_positive_upwind_relief() {
        let warm_tropical = condensation_fraction(0.0, 0.0, 30.0, 5.0, 10.0);
        let cold_polar = condensation_fraction(0.0, 0.0, -25.0, 75.0, 10.0);
        let windward_uplift = condensation_fraction(2_000.0, 0.0, 10.0, 45.0, 10.0);
        let leeward_descent = condensation_fraction(0.0, 2_000.0, 10.0, 45.0, 10.0);

        assert!(warm_tropical > cold_polar);
        assert!(windward_uplift > leeward_descent);
    }
}
