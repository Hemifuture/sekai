mod support;

use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::{
    CirculationOperatorError, CirculationOperators, CubedSphereGrid, SphericalEdge,
};
use support::circulation::{area_weighted_rms, magnitude};

fn solid_rotation(grid: &CubedSphereGrid, speed_scale: f32) -> Vec<[f32; 3]> {
    grid.cells()
        .iter()
        .map(|cell| {
            let r = cell.center_unit();
            [-r[1] as f32 * speed_scale, r[0] as f32 * speed_scale, 0.0]
        })
        .collect()
}

fn divergent_flow(grid: &CubedSphereGrid, speed_scale: f32) -> Vec<[f32; 3]> {
    grid.cells()
        .iter()
        .map(|cell| {
            let radial = cell.center_unit();
            let radial_projection = speed_scale * radial[0] as f32;
            [
                speed_scale - radial_projection * radial[0] as f32,
                -radial_projection * radial[1] as f32,
                -radial_projection * radial[2] as f32,
            ]
        })
        .collect()
}

#[test]
fn cancellable_gradient_reports_the_typed_cancelled_error() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let cancellation = BuildCancellation::new();
    cancellation.cancel();

    let error = CirculationOperators::new(&grid)
        .gradient_cancellable(&vec![1.0; grid.cell_count()], &cancellation)
        .unwrap_err();

    assert_eq!(error, CirculationOperatorError::Cancelled);
}

fn total_layer_tracer(grid: &CubedSphereGrid, layer: &[f32], tracer: &[f32]) -> f64 {
    grid.cells()
        .iter()
        .zip(layer)
        .zip(tracer)
        .map(|((cell, layer), tracer)| cell.area_m2() * f64::from(*layer) * f64::from(*tracer))
        .sum()
}

fn signed_edge_flux(edge: &SphericalEdge, velocity: &[[f32; 3]]) -> f64 {
    let [first, second] = *edge.cells();
    let distances = edge.center_distances_to_midpoint_m();
    let denominator = distances[0] + distances[1];
    let first_weight = distances[1] / denominator;
    let second_weight = distances[0] / denominator;
    let interpolated: [f64; 3] = std::array::from_fn(|component| {
        first_weight * f64::from(velocity[first as usize][component])
            + second_weight * f64::from(velocity[second as usize][component])
    });
    interpolated
        .iter()
        .zip(edge.normal_from_first())
        .map(|(value, normal)| value * normal)
        .sum::<f64>()
        * edge.length_m()
}

#[test]
fn constant_scalar_has_zero_gradient_and_solid_rotation_is_nearly_divergence_free() {
    let grid = CubedSphereGrid::new(12, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let gradient = operators.gradient(&vec![3.5; grid.cell_count()]).unwrap();
    assert!(gradient.iter().all(|value| magnitude(*value) < 1.0e-10));

    let divergence = operators.divergence(&solid_rotation(&grid, 10.0)).unwrap();
    assert!(area_weighted_rms(&grid, &divergence) < 2.0e-6);
    let global_flux: f64 = grid
        .cells()
        .iter()
        .zip(&divergence)
        .map(|(cell, value)| cell.area_m2() * f64::from(*value))
        .sum();
    assert!(global_flux.abs() < 1.0);
}

#[test]
fn tangent_projection_and_coriolis_never_create_radial_velocity() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let radial = grid
        .cells()
        .iter()
        .map(|cell| {
            let r = cell.center_unit();
            [r[0] as f32 * 5.0, r[1] as f32 * 5.0, r[2] as f32 * 5.0]
        })
        .collect::<Vec<_>>();
    let tangent = operators.tangentize(&radial).unwrap();
    assert!(tangent.iter().all(|value| magnitude(*value) < 1.0e-5));

    let velocity = solid_rotation(&grid, 20.0);
    let acceleration = operators.coriolis(&velocity, 7.292_115_9e-5).unwrap();
    for (cell, value) in grid.cells().iter().zip(acceleration) {
        let r = cell.center_unit();
        let radial_component =
            r[0] * f64::from(value[0]) + r[1] * f64::from(value[1]) + r[2] * f64::from(value[2]);
        assert!(radial_component.abs() < 1.0e-9);
    }
}

#[test]
fn conservative_upwind_transport_pairs_edge_fluxes_and_respects_closed_edges() {
    let grid = CubedSphereGrid::new(12, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let velocity = solid_rotation(&grid, 10.0);
    let scalar = grid
        .cells()
        .iter()
        .map(|cell| (1.0 + 0.2 * cell.center_unit()[0]) as f32)
        .collect::<Vec<_>>();
    let open = vec![1.0; grid.edges().len()];
    let transported = operators
        .advect_scalar_conservative(&scalar, &velocity, &open, 3_600.0)
        .unwrap();
    assert!(transported.relative_mass_error() < 1.0e-6);
    assert!(transported.values().iter().all(|value| value.is_finite()));

    let closed = vec![0.0; grid.edges().len()];
    let unchanged = operators
        .advect_scalar_conservative(&scalar, &velocity, &closed, 86_400.0)
        .unwrap();
    assert_eq!(unchanged.values(), scalar.as_slice());
    assert_eq!(unchanged.relative_mass_error(), 0.0);

    let constant = vec![3.5; grid.cell_count()];
    let tracer = operators
        .advect_scalar_upwind_tracer(&constant, &velocity, &open, 3_600.0)
        .unwrap();
    assert_eq!(tracer.values(), constant.as_slice());
}

#[test]
fn divergent_transport_conserves_layer_weighted_tracer_and_preserves_a_constant() {
    let grid = CubedSphereGrid::new(12, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let velocity = divergent_flow(&grid, 20.0);
    let divergence = operators.divergence(&velocity).unwrap();
    assert!(area_weighted_rms(&grid, &divergence) > 1.0e-7);

    let layer = grid
        .cells()
        .iter()
        .map(|cell| (8_000.0 + 500.0 * cell.center_unit()[0]) as f32)
        .collect::<Vec<_>>();
    let humidity = grid
        .cells()
        .iter()
        .map(|cell| (0.01 + 0.002 * cell.center_unit()[0]) as f32)
        .collect::<Vec<_>>();
    let open = vec![1.0; grid.edges().len()];
    let dt_seconds = 3_600.0;
    let reference_depth = 8_000.0_f32;
    let transported_reference_layer = operators
        .advect_scalar_conservative(
            &vec![reference_depth; grid.cell_count()],
            &velocity,
            &open,
            dt_seconds,
        )
        .unwrap();
    let transported_humidity = operators
        .advect_linearized_layer_mixing_ratio_conservative(
            &layer,
            reference_depth,
            &humidity,
            &velocity,
            &open,
            dt_seconds,
        )
        .unwrap();
    assert!(transported_humidity.relative_mass_error() < 1.0e-6);
    assert!(transported_humidity
        .values()
        .iter()
        .all(|value| (0.008..=0.012).contains(value)));
    for ((actual, transported_reference), transported_actual) in layer
        .iter()
        .zip(transported_reference_layer.values())
        .zip(transported_humidity.layer_amounts())
    {
        let expected = *actual + (*transported_reference - reference_depth);
        assert!((transported_actual - expected).abs() < 1.0e-3);
    }

    let before = total_layer_tracer(&grid, &layer, &humidity);
    let after = total_layer_tracer(
        &grid,
        transported_humidity.layer_amounts(),
        transported_humidity.values(),
    );
    assert!(
        (after - before).abs() / before.abs() < 1.0e-7,
        "divergent transport changed global column moisture: before={before}, after={after}"
    );

    let constant = vec![0.0125; grid.cell_count()];
    let transported_constant = operators
        .advect_linearized_layer_mixing_ratio_conservative(
            &layer,
            reference_depth,
            &constant,
            &velocity,
            &open,
            dt_seconds,
        )
        .unwrap();
    assert_eq!(transported_constant.values(), constant.as_slice());
    let constant_before = total_layer_tracer(&grid, &layer, &constant);
    let constant_after = total_layer_tracer(
        &grid,
        transported_constant.layer_amounts(),
        transported_constant.values(),
    );
    assert!((constant_after - constant_before).abs() / constant_before.abs() < 1.0e-7);
}

#[test]
fn operators_reject_misaligned_nonfinite_and_invalid_permeability_input() {
    let grid = CubedSphereGrid::new(4, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    assert!(operators.gradient(&[0.0]).is_err());

    let mut velocity = vec![[0.0; 3]; grid.cell_count()];
    velocity[2][1] = f32::NAN;
    assert!(operators.divergence(&velocity).is_err());

    let scalar = vec![1.0; grid.cell_count()];
    let velocity = vec![[0.0; 3]; grid.cell_count()];
    assert!(operators
        .advect_scalar_conservative(&scalar, &velocity, &[1.0], 60.0)
        .is_err());
    let mut permeability = vec![1.0; grid.edges().len()];
    permeability[0] = 1.1;
    assert!(operators
        .advect_scalar_conservative(&scalar, &velocity, &permeability, 60.0)
        .is_err());
    assert!(operators
        .advect_scalar_conservative(&scalar, &velocity, &vec![1.0; grid.edges().len()], -1.0,)
        .is_err());

    let open = vec![1.0; grid.edges().len()];
    let divergent = divergent_flow(&grid, 20.0);
    for invalid_depth in [0.0, f32::NAN] {
        assert!(operators
            .advect_linearized_layer_mixing_ratio_conservative(
                &vec![8_000.0; grid.cell_count()],
                invalid_depth,
                &vec![0.01; grid.cell_count()],
                &divergent,
                &open,
                1.0,
            )
            .is_err());
    }
    let overflow = operators
        .advect_linearized_layer_mixing_ratio_conservative(
            &vec![f32::MAX; grid.cell_count()],
            f32::MAX,
            &vec![0.01; grid.cell_count()],
            &divergent,
            &open,
            1.0,
        )
        .unwrap_err();
    assert!(matches!(
        overflow,
        CirculationOperatorError::NumericalOverflow
    ));
}

#[test]
fn zero_permeability_removes_pressure_gradients_and_volume_fluxes() {
    let grid = CubedSphereGrid::new(6, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let scalar = grid
        .cells()
        .iter()
        .map(|cell| cell.center_unit()[0] as f32)
        .collect::<Vec<_>>();
    let velocity = solid_rotation(&grid, 10.0);
    let closed = vec![0.0; grid.edges().len()];

    let gradient = operators
        .gradient_with_permeability(&scalar, &closed)
        .unwrap();
    let divergence = operators
        .divergence_with_permeability(&velocity, &closed)
        .unwrap();
    assert!(gradient.iter().all(|value| magnitude(*value) == 0.0));
    assert!(divergence.iter().all(|value| *value == 0.0));
}

#[test]
fn steady_upwind_source_solver_reaches_the_discrete_stationary_equation() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let velocity = solid_rotation(&grid, 15.0);
    let initial = vec![0.0; grid.cell_count()];
    let target = grid
        .cells()
        .iter()
        .map(|cell| (2.0 + 0.25 * cell.center_unit()[2]) as f32)
        .collect::<Vec<_>>();
    let sink_rate = vec![2.0e-6; grid.cell_count()];
    let source = target
        .iter()
        .zip(&sink_rate)
        .map(|(target, rate)| target * rate)
        .collect::<Vec<_>>();
    let open = vec![1.0; grid.edges().len()];

    let solved = operators
        .solve_steady_upwind_tracer_source(
            &initial, &velocity, &open, &sink_rate, &source, 128, 1.0e-8,
        )
        .unwrap();

    assert!(solved.relative_residual() <= 1.0e-8);
    assert!((1..=128).contains(&solved.iterations()));
    assert!(solved.values().iter().all(|value| value.is_finite()));

    let zero_velocity = vec![[0.0; 3]; grid.cell_count()];
    let local_equilibrium = operators
        .solve_steady_upwind_tracer_source(
            &initial,
            &zero_velocity,
            &open,
            &sink_rate,
            &source,
            8,
            1.0e-10,
        )
        .unwrap();
    for (found, expected) in local_equilibrium.values().iter().zip(target) {
        assert!((found - expected).abs() < 1.0e-6);
    }
}

#[test]
fn steady_linearized_mixing_ratio_uses_reference_depth_flux() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let velocity = divergent_flow(&grid, 15.0);
    let reference_depth = 8_000.0_f32;
    let layer = grid
        .cells()
        .iter()
        .map(|cell| reference_depth + 600.0 * cell.center_unit()[0] as f32)
        .collect::<Vec<_>>();
    let target = grid
        .cells()
        .iter()
        .map(|cell| 0.01 + 0.002 * cell.center_unit()[1] as f32)
        .collect::<Vec<_>>();
    let sink_rate = vec![2.0e-5; grid.cell_count()];
    let mut source = target
        .iter()
        .zip(&sink_rate)
        .map(|(target, sink)| target * sink)
        .collect::<Vec<_>>();
    for edge in grid.edges() {
        let signed_flux = signed_edge_flux(edge, &velocity);
        if signed_flux == 0.0 {
            continue;
        }
        let [first, second] = *edge.cells();
        let (donor, receiver, magnitude) = if signed_flux > 0.0 {
            (first as usize, second as usize, signed_flux)
        } else {
            (second as usize, first as usize, -signed_flux)
        };
        let rate = magnitude * f64::from(reference_depth)
            / (grid.cells()[receiver].area_m2() * f64::from(layer[receiver]));
        source[receiver] += (rate * f64::from(target[receiver] - target[donor])) as f32;
    }

    let solved = operators
        .solve_steady_linearized_layer_mixing_ratio_source(
            &vec![0.01; grid.cell_count()],
            &layer,
            reference_depth,
            &velocity,
            &vec![1.0; grid.edges().len()],
            &sink_rate,
            &source,
            128,
            1.0e-9,
        )
        .unwrap();

    assert!(solved.relative_residual() <= 1.0e-9);
    let maximum_error = solved
        .values()
        .iter()
        .zip(target)
        .map(|(found, expected)| (found - expected).abs())
        .fold(0.0_f32, f32::max);
    assert!(maximum_error < 1.0e-6, "maximum error {maximum_error}");
}
