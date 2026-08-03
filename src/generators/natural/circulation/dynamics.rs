use thiserror::Error;

use crate::world::natural::{CirculationSpec, PlanetForcing};

use super::{
    linear::{solve_gmres, MatrixFreeSolveError, MatrixFreeSolveFailure},
    math::{cross, dot, project_tangent, scale},
    thermodynamics::balance_thermodynamics,
    CirculationEdgePermeability, CirculationOperatorError, CirculationOperators, CubedSphereGrid,
    ThermodynamicError, ThermodynamicState,
};

pub(crate) const WIND_STRESS_RATE_S_INV: f64 = 3.0e-8;
const AIR_TO_WATER_DENSITY_RATIO: f64 = 1.2 / 1_025.0;
const LAYER_LINEAR_ITERATIONS: u16 = 4_096;
const LAYER_LINEAR_RESTART: u16 = 128;
const LAYER_LINEAR_TOLERANCE: f64 = 1.0e-6;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CirculationState {
    pub(crate) wind_m_s: Vec<[f32; 3]>,
    pub(crate) ocean_current_m_s: Vec<[f32; 3]>,
    pub(crate) atmosphere_height_anomaly_m: Vec<f32>,
    pub(crate) sea_surface_height_anomaly_m: Vec<f32>,
    pub(crate) thermodynamics: ThermodynamicState,
}

pub(crate) struct BalancedIteration {
    pub(crate) state: CirculationState,
    pub(crate) precipitation_mm_day: Vec<f32>,
    pub(crate) residual: f64,
    pub(crate) relative_mass_error: f64,
}

pub(crate) fn initial_state(
    grid: &CubedSphereGrid,
    forcing: &PlanetForcing,
    spec: &CirculationSpec,
    month: usize,
) -> Result<CirculationState, DynamicsError> {
    let thermodynamics = ThermodynamicState::from_forcing(grid, forcing, month)?;
    let atmosphere_height_anomaly_m =
        thermal_height_target(grid, thermodynamics.air_temperature_c(), spec);
    let sea_surface_height_anomaly_m =
        inverse_barometer_height(grid, forcing, &atmosphere_height_anomaly_m);
    Ok(CirculationState {
        wind_m_s: vec![[0.0; 3]; grid.cell_count()],
        ocean_current_m_s: vec![[0.0; 3]; grid.cell_count()],
        atmosphere_height_anomaly_m,
        sea_surface_height_anomaly_m,
        thermodynamics,
    })
}

pub(crate) fn balanced_iteration(
    previous: &CirculationState,
    operators: &CirculationOperators<'_>,
    forcing: &PlanetForcing,
    spec: &CirculationSpec,
    permeability: &CirculationEdgePermeability,
    month: usize,
) -> Result<BalancedIteration, DynamicsError> {
    let grid = operators.grid();
    let atmosphere_equilibrium_height_m =
        thermal_height_target(grid, previous.thermodynamics.air_temperature_c(), spec);
    let (atmosphere_height_anomaly_m, wind_m_s) = solve_balanced_layer(
        grid,
        operators,
        &previous.atmosphere_height_anomaly_m,
        &atmosphere_equilibrium_height_m,
        f64::from(spec.atmosphere_reference_depth_m),
        f64::from(spec.atmosphere_reduced_gravity_m_s2),
        f64::from(spec.atmosphere_drag_s_inv),
        f64::from(spec.layer_relaxation_s_inv),
        spec.rotation_rate_rad_s,
        permeability.atmosphere(),
        None,
        None,
        "atmosphere",
    )?;

    let sea_surface_equilibrium_height_m =
        inverse_barometer_height(grid, forcing, &atmosphere_height_anomaly_m);
    let (sea_surface_height_anomaly_m, ocean_current_m_s) = solve_balanced_layer(
        grid,
        operators,
        &previous.sea_surface_height_anomaly_m,
        &sea_surface_equilibrium_height_m,
        f64::from(spec.ocean_reference_depth_m),
        f64::from(spec.ocean_reduced_gravity_m_s2),
        f64::from(spec.ocean_drag_s_inv) + WIND_STRESS_RATE_S_INV,
        f64::from(spec.layer_relaxation_s_inv),
        spec.rotation_rate_rad_s,
        permeability.ocean(),
        Some(forcing.land_fraction()),
        Some((&wind_m_s, WIND_STRESS_RATE_S_INV)),
        "ocean",
    )?;

    let balanced_thermodynamics = balance_thermodynamics(
        operators,
        forcing,
        spec,
        &previous.thermodynamics,
        &wind_m_s,
        &ocean_current_m_s,
        permeability,
        month,
    )?;
    let thermodynamics = balanced_thermodynamics.state;
    let state = CirculationState {
        wind_m_s,
        ocean_current_m_s,
        atmosphere_height_anomaly_m,
        sea_surface_height_anomaly_m,
        thermodynamics,
    };
    let residual = state_residual(grid, previous, &state);
    if !residual.is_finite() {
        return Err(DynamicsError::NonFiniteState);
    }
    Ok(BalancedIteration {
        state,
        precipitation_mm_day: balanced_thermodynamics.precipitation_mm_day,
        residual,
        relative_mass_error: balanced_thermodynamics.relative_moisture_transport_error,
    })
}

pub(crate) fn dense_state_bytes(cell_count: usize) -> Result<u64, DynamicsError> {
    let scalar_slots = cell_count
        .checked_mul(11)
        .and_then(|value| value.checked_mul(std::mem::size_of::<f32>()))
        .ok_or(DynamicsError::AllocationOverflow)?;
    u64::try_from(scalar_slots).map_err(|_| DynamicsError::AllocationOverflow)
}

pub(crate) fn thermal_height_target(
    grid: &CubedSphereGrid,
    air_temperature_c: &[f32],
    spec: &CirculationSpec,
) -> Vec<f32> {
    let total_area: f64 = grid.cells().iter().map(|cell| cell.area_m2()).sum();
    let mean_kelvin = grid
        .cells()
        .iter()
        .zip(air_temperature_c)
        .map(|(cell, temperature)| cell.area_m2() * (f64::from(*temperature) + 273.15))
        .sum::<f64>()
        / total_area;
    air_temperature_c
        .iter()
        .map(|temperature| {
            (f64::from(spec.atmosphere_reference_depth_m)
                * (f64::from(*temperature) + 273.15 - mean_kelvin)
                / mean_kelvin) as f32
        })
        .collect()
}

pub(crate) fn inverse_barometer_height(
    grid: &CubedSphereGrid,
    forcing: &PlanetForcing,
    atmosphere_height_m: &[f32],
) -> Vec<f32> {
    let mut values = atmosphere_height_m
        .iter()
        .zip(forcing.land_fraction())
        .map(|(height, land)| {
            if *land >= 1.0 {
                0.0
            } else {
                (-AIR_TO_WATER_DENSITY_RATIO * f64::from(*height)) as f32
            }
        })
        .collect::<Vec<_>>();
    let (weighted_sum, ocean_area) = grid
        .cells()
        .iter()
        .zip(forcing.land_fraction())
        .zip(&values)
        .fold((0.0_f64, 0.0_f64), |(sum, area), ((cell, land), value)| {
            let ocean = f64::from(1.0 - *land);
            (
                sum + cell.area_m2() * ocean * f64::from(*value),
                area + cell.area_m2() * ocean,
            )
        });
    if ocean_area > 0.0 {
        let mean = weighted_sum / ocean_area;
        for (value, land) in values.iter_mut().zip(forcing.land_fraction()) {
            if *land < 1.0 {
                *value = (f64::from(*value) - mean) as f32;
            }
        }
    }
    values
}

fn scaled_vectors(values: &[[f32; 3]], scalar: f64) -> Vec<[f32; 3]> {
    values
        .iter()
        .map(|value| {
            [
                (f64::from(value[0]) * scalar) as f32,
                (f64::from(value[1]) * scalar) as f32,
                (f64::from(value[2]) * scalar) as f32,
            ]
        })
        .collect()
}

fn scaled_vectors_f64(values: &[[f64; 3]], scalar: f64) -> Vec<[f64; 3]> {
    values.iter().map(|value| scale(*value, scalar)).collect()
}

#[allow(clippy::too_many_arguments)]
fn solve_balanced_layer(
    grid: &CubedSphereGrid,
    operators: &CirculationOperators<'_>,
    initial_height_m: &[f32],
    equilibrium_m: &[f32],
    reference_depth_m: f64,
    reduced_gravity_m_s2: f64,
    drag_s_inv: f64,
    relaxation_s_inv: f64,
    rotation_rate_rad_s: f64,
    edge_permeability: &[f32],
    land_fraction: Option<&[f32]>,
    external: Option<(&[[f32; 3]], f64)>,
    layer: &'static str,
) -> Result<(Vec<f32>, Vec<[f32; 3]>), DynamicsError> {
    let inverse_diagonal = layer_inverse_diagonal(
        grid,
        reference_depth_m,
        reduced_gravity_m_s2,
        drag_s_inv,
        relaxation_s_inv,
        rotation_rate_rad_s,
        edge_permeability,
        land_fraction,
    );
    let zero_acceleration = vec![[0.0; 3]; grid.cell_count()];
    let external_velocity = balanced_velocity(
        grid,
        &zero_acceleration,
        external,
        drag_s_inv,
        rotation_rate_rad_s,
        land_fraction,
    );
    let external_velocity = operators.tangentize(&external_velocity)?;
    let external_divergence =
        operators.divergence_with_permeability(&external_velocity, edge_permeability)?;
    let right_hand_side = equilibrium_m
        .iter()
        .zip(&external_divergence)
        .zip(&inverse_diagonal)
        .map(|((equilibrium, divergence), inverse_diagonal)| {
            inverse_diagonal
                * (relaxation_s_inv * f64::from(*equilibrium)
                    - reference_depth_m * f64::from(*divergence))
        })
        .collect::<Vec<_>>();
    let initial = initial_height_m
        .iter()
        .map(|value| f64::from(*value))
        .collect::<Vec<_>>();
    let solved = solve_gmres(
        &initial,
        &right_hand_side,
        LAYER_LINEAR_ITERATIONS,
        LAYER_LINEAR_RESTART,
        LAYER_LINEAR_TOLERANCE,
        |values, output| {
            let gradient = operators.gradient_f64_with_permeability(values, edge_permeability)?;
            let acceleration = scaled_vectors_f64(&gradient, -reduced_gravity_m_s2);
            let pressure_velocity = balanced_velocity_f64(
                grid,
                &acceleration,
                None,
                drag_s_inv,
                rotation_rate_rad_s,
                land_fraction,
            );
            let divergence = operators
                .divergence_f64_with_permeability(&pressure_velocity, edge_permeability)?;
            for index in 0..grid.cell_count() {
                output[index] = inverse_diagonal[index]
                    * (relaxation_s_inv * values[index] + reference_depth_m * divergence[index]);
            }
            Ok::<(), CirculationOperatorError>(())
        },
    )
    .map_err(|failure| match failure {
        MatrixFreeSolveFailure::Application(error) => DynamicsError::Operator(error),
        MatrixFreeSolveFailure::Solve(reason) => DynamicsError::LayerLinearSolve { layer, reason },
    })?;
    let mut height = checked_f32_scalars(&solved.values)?;
    remove_layer_mean(grid, &mut height, land_fraction);
    let gradient = operators.gradient_with_permeability(&height, edge_permeability)?;
    let acceleration = scaled_vectors(&gradient, -reduced_gravity_m_s2);
    let velocity = balanced_velocity(
        grid,
        &acceleration,
        external,
        drag_s_inv,
        rotation_rate_rad_s,
        land_fraction,
    );
    let velocity = operators.tangentize(&velocity)?;
    Ok((height, velocity))
}

#[allow(clippy::too_many_arguments)]
fn layer_inverse_diagonal(
    grid: &CubedSphereGrid,
    reference_depth_m: f64,
    reduced_gravity_m_s2: f64,
    drag_s_inv: f64,
    relaxation_s_inv: f64,
    rotation_rate_rad_s: f64,
    edge_permeability: &[f32],
    land_fraction: Option<&[f32]>,
) -> Vec<f64> {
    grid.cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let laplacian_diagonal = cell
                .edges()
                .iter()
                .map(|edge_id| {
                    let edge = &grid.edges()[*edge_id as usize];
                    edge.length_m() * f64::from(edge_permeability[*edge_id as usize])
                        / (cell.area_m2() * edge.center_distance_m())
                })
                .sum::<f64>();
            let coriolis = 2.0 * rotation_rate_rad_s * cell.center_unit()[2];
            let divergent_mobility_m_s =
                reduced_gravity_m_s2 * drag_s_inv / (drag_s_inv * drag_s_inv + coriolis * coriolis);
            let active_fraction = land_fraction
                .map(|land| f64::from(1.0 - land[index]))
                .unwrap_or(1.0);
            (relaxation_s_inv
                + reference_depth_m * divergent_mobility_m_s * laplacian_diagonal * active_fraction)
                .recip()
        })
        .collect()
}

fn checked_f32_scalars(values: &[f64]) -> Result<Vec<f32>, CirculationOperatorError> {
    let mut converted = Vec::with_capacity(values.len());
    for value in values {
        if !value.is_finite() || *value < f64::from(f32::MIN) || *value > f64::from(f32::MAX) {
            return Err(CirculationOperatorError::NumericalOverflow);
        }
        converted.push(*value as f32);
    }
    Ok(converted)
}

pub(crate) fn remove_layer_mean(
    grid: &CubedSphereGrid,
    values: &mut [f32],
    land_fraction: Option<&[f32]>,
) {
    let (weighted_sum, active_area) =
        grid.cells()
            .iter()
            .enumerate()
            .fold((0.0_f64, 0.0_f64), |(sum, area), (index, cell)| {
                let active = land_fraction
                    .map(|land| f64::from(1.0 - land[index]))
                    .unwrap_or(1.0);
                (
                    sum + cell.area_m2() * active * f64::from(values[index]),
                    area + cell.area_m2() * active,
                )
            });
    if active_area <= 0.0 {
        return;
    }
    let mean = weighted_sum / active_area;
    for (index, value) in values.iter_mut().enumerate() {
        if land_fraction.is_none_or(|land| land[index] < 1.0) {
            *value = (f64::from(*value) - mean) as f32;
        }
    }
}

fn balanced_velocity(
    grid: &CubedSphereGrid,
    acceleration: &[[f32; 3]],
    external: Option<(&[[f32; 3]], f64)>,
    drag_s_inv: f64,
    rotation_rate_rad_s: f64,
    land_fraction: Option<&[f32]>,
) -> Vec<[f32; 3]> {
    let acceleration = acceleration
        .iter()
        .map(|value| {
            [
                f64::from(value[0]),
                f64::from(value[1]),
                f64::from(value[2]),
            ]
        })
        .collect::<Vec<_>>();
    let external = external.map(|(field, rate)| {
        (
            field
                .iter()
                .map(|value| {
                    [
                        f64::from(value[0]),
                        f64::from(value[1]),
                        f64::from(value[2]),
                    ]
                })
                .collect::<Vec<_>>(),
            rate,
        )
    });
    balanced_velocity_f64(
        grid,
        &acceleration,
        external
            .as_ref()
            .map(|(field, rate)| (field.as_slice(), *rate)),
        drag_s_inv,
        rotation_rate_rad_s,
        land_fraction,
    )
    .into_iter()
    .map(|value| [value[0] as f32, value[1] as f32, value[2] as f32])
    .collect()
}

fn balanced_velocity_f64(
    grid: &CubedSphereGrid,
    acceleration: &[[f64; 3]],
    external: Option<(&[[f64; 3]], f64)>,
    drag_s_inv: f64,
    rotation_rate_rad_s: f64,
    land_fraction: Option<&[f32]>,
) -> Vec<[f64; 3]> {
    grid.cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let radial = cell.center_unit();
            let mut forcing = acceleration[index];
            if let Some((field, rate)) = external {
                for component in 0..3 {
                    forcing[component] += rate * field[index][component];
                }
            }
            let forcing = project_tangent(forcing, radial);
            let coriolis = 2.0 * rotation_rate_rad_s * radial[2];
            let rotated = cross(radial, forcing);
            let denominator = drag_s_inv * drag_s_inv + coriolis * coriolis;
            let mut velocity = [0.0_f64; 3];
            for component in 0..3 {
                velocity[component] =
                    (drag_s_inv * forcing[component] - coriolis * rotated[component]) / denominator;
            }
            if let Some(land) = land_fraction {
                velocity = scale(velocity, f64::from(1.0 - land[index]));
            }
            project_tangent(velocity, radial)
        })
        .collect()
}

pub(crate) fn state_residual(
    grid: &CubedSphereGrid,
    previous: &CirculationState,
    next: &CirculationState,
) -> f64 {
    [
        vector_delta_rms(grid, &previous.wind_m_s, &next.wind_m_s) / 10.0,
        vector_delta_rms(grid, &previous.ocean_current_m_s, &next.ocean_current_m_s),
        scalar_delta_rms(
            grid,
            &previous.atmosphere_height_anomaly_m,
            &next.atmosphere_height_anomaly_m,
        ) / 100.0,
        scalar_delta_rms(
            grid,
            &previous.sea_surface_height_anomaly_m,
            &next.sea_surface_height_anomaly_m,
        ),
        scalar_delta_rms(
            grid,
            previous.thermodynamics.air_temperature_c(),
            next.thermodynamics.air_temperature_c(),
        ) / 300.0,
        scalar_delta_rms(
            grid,
            previous.thermodynamics.surface_temperature_c(),
            next.thermodynamics.surface_temperature_c(),
        ) / 300.0,
        scalar_delta_rms(
            grid,
            previous.thermodynamics.specific_humidity(),
            next.thermodynamics.specific_humidity(),
        ) / 0.01,
    ]
    .into_iter()
    .fold(0.0_f64, f64::max)
}

fn scalar_delta_rms(grid: &CubedSphereGrid, first: &[f32], second: &[f32]) -> f64 {
    let (weighted, area) = grid.cells().iter().zip(first).zip(second).fold(
        (0.0_f64, 0.0_f64),
        |(sum, area), ((cell, first), second)| {
            let delta = f64::from(*second) - f64::from(*first);
            (sum + cell.area_m2() * delta * delta, area + cell.area_m2())
        },
    );
    (weighted / area).sqrt()
}

fn vector_delta_rms(grid: &CubedSphereGrid, first: &[[f32; 3]], second: &[[f32; 3]]) -> f64 {
    let (weighted, area) = grid.cells().iter().zip(first).zip(second).fold(
        (0.0_f64, 0.0_f64),
        |(sum, area), ((cell, first), second)| {
            let delta = [
                f64::from(second[0] - first[0]),
                f64::from(second[1] - first[1]),
                f64::from(second[2] - first[2]),
            ];
            (
                sum + cell.area_m2() * dot(delta, delta),
                area + cell.area_m2(),
            )
        },
    );
    (weighted / area).sqrt()
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(crate) enum DynamicsError {
    #[error(transparent)]
    Operator(#[from] CirculationOperatorError),
    #[error(transparent)]
    Thermodynamics(#[from] ThermodynamicError),
    #[error("stationary {layer} layer solve failed: {reason:?}")]
    LayerLinearSolve {
        layer: &'static str,
        reason: MatrixFreeSolveError,
    },
    #[error("circulation state became non-finite")]
    NonFiniteState,
    #[error("circulation dense-state allocation arithmetic overflowed")]
    AllocationOverflow,
}
