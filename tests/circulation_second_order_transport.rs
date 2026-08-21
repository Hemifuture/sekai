use sekai::generators::natural::circulation::{
    CirculationOperators, CubedSphereGrid, SecondOrderTransportWorkspace,
};

fn solid_rotation(grid: &CubedSphereGrid, speed_m_s: f32) -> Vec<[f32; 3]> {
    grid.cells()
        .iter()
        .map(|cell| {
            let [x, y, _] = cell.center_unit();
            [-speed_m_s * y as f32, speed_m_s * x as f32, 0.0]
        })
        .collect()
}

fn extensive_total(grid: &CubedSphereGrid, values: &[f32]) -> f64 {
    grid.cells()
        .iter()
        .zip(values)
        .map(|(cell, value)| cell.area_m2() * f64::from(*value))
        .sum()
}

fn component_extensive_total(grid: &CubedSphereGrid, values: &[f32], positive_x: bool) -> f64 {
    grid.cells()
        .iter()
        .zip(values)
        .filter(|(cell, _)| (cell.center_unit()[0] >= 0.0) == positive_x)
        .map(|(cell, value)| cell.area_m2() * f64::from(*value))
        .sum()
}

#[test]
fn limited_reconstruction_improves_a_smooth_linear_solid_rotation() {
    let grid = CubedSphereGrid::new(12, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let velocity = solid_rotation(&grid, 80.0);
    let permeability = vec![1.0; grid.edges().len()];
    let scalar = grid
        .cells()
        .iter()
        .map(|cell| (1.0 + 0.2 * cell.center_unit()[0]) as f32)
        .collect::<Vec<_>>();
    let dt = 2_000.0;
    let angle = 80.0 * dt / grid.radius_m();
    let expected = grid
        .cells()
        .iter()
        .map(|cell| {
            let [x, y, _] = cell.center_unit();
            (1.0 + 0.2 * (x * angle.cos() + y * angle.sin())) as f32
        })
        .collect::<Vec<_>>();
    let first = operators
        .advect_scalar_conservative(&scalar, &velocity, &permeability, dt)
        .unwrap();
    let mut workspace = SecondOrderTransportWorkspace::for_grid(&grid);
    let second = operators
        .advect_scalar_monotone_second_order_into(
            &scalar,
            &velocity,
            &permeability,
            dt,
            true,
            &mut workspace,
        )
        .unwrap();
    let rms = |values: &[f32]| {
        (values
            .iter()
            .zip(&expected)
            .map(|(value, expected)| f64::from(value - expected).powi(2))
            .sum::<f64>()
            / values.len() as f64)
            .sqrt()
    };
    assert!(rms(second.values()) < rms(first.values()));
}

#[test]
fn transport_preserves_extrema_positivity_and_paired_global_mass() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let velocity = solid_rotation(&grid, 120.0);
    let permeability = vec![1.0; grid.edges().len()];
    let scalar = grid
        .cells()
        .iter()
        .map(|cell| f32::from(cell.center_unit()[0] > 0.0))
        .collect::<Vec<_>>();
    let before = extensive_total(&grid, &scalar);
    let first_order = operators
        .advect_scalar_conservative(&scalar, &velocity, &permeability, 1_000.0)
        .unwrap();
    let mut workspace = SecondOrderTransportWorkspace::for_grid(&grid);
    let result = operators
        .advect_scalar_monotone_second_order_into(
            &scalar,
            &velocity,
            &permeability,
            1_000.0,
            true,
            &mut workspace,
        )
        .unwrap();
    let minimum = result
        .values()
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let maximum = result
        .values()
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        minimum >= 0.0 && maximum <= 1.0,
        "transport extrema [{minimum}, {maximum}], first-order maximum {}",
        first_order
            .values()
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max)
    );
    let after = extensive_total(&grid, result.values());
    assert!((after - before).abs() / before <= 2.0e-7);
    assert!(result.relative_mass_error() <= 2.0e-7);
}

#[test]
fn large_step_positivity_scaling_prevents_negative_donors() {
    let grid = CubedSphereGrid::new(4, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let velocity = solid_rotation(&grid, 1_000.0);
    let permeability = vec![1.0; grid.edges().len()];
    let scalar = grid
        .cells()
        .iter()
        .map(|cell| if cell.id() == 0 { 1.0 } else { 0.0 })
        .collect::<Vec<_>>();
    let mut workspace = SecondOrderTransportWorkspace::for_grid(&grid);
    let result = operators
        .advect_scalar_monotone_second_order_into(
            &scalar,
            &velocity,
            &permeability,
            100_000.0,
            true,
            &mut workspace,
        )
        .unwrap();
    assert!(result.values().iter().all(|value| *value >= 0.0));
    assert!(result.positivity_scaled_cells() > 0);
}

#[test]
fn disconnected_transport_components_retain_their_own_extensive_tracer() {
    let grid = CubedSphereGrid::new(6, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let velocity = solid_rotation(&grid, 900.0);
    let permeability = grid
        .edges()
        .iter()
        .map(|edge| {
            let [first, second] = *edge.cells();
            let first_positive = grid.cells()[first as usize].center_unit()[0] >= 0.0;
            let second_positive = grid.cells()[second as usize].center_unit()[0] >= 0.0;
            f32::from(first_positive == second_positive)
        })
        .collect::<Vec<_>>();
    let scalar = grid
        .cells()
        .iter()
        .map(|cell| {
            let [x, y, z] = cell.center_unit();
            if x >= 0.0 {
                if y + z >= 0.0 {
                    1.0
                } else {
                    0.05
                }
            } else if y - z >= 0.0 {
                20.0
            } else {
                7.0
            }
        })
        .collect::<Vec<_>>();
    let before = [
        component_extensive_total(&grid, &scalar, false),
        component_extensive_total(&grid, &scalar, true),
    ];
    let mut workspace = SecondOrderTransportWorkspace::for_grid(&grid);
    let result = operators
        .advect_scalar_monotone_second_order_into(
            &scalar,
            &velocity,
            &permeability,
            80_000.0,
            true,
            &mut workspace,
        )
        .unwrap();

    for (index, positive_x) in [false, true].into_iter().enumerate() {
        let after = component_extensive_total(&grid, result.values(), positive_x);
        let relative = (after - before[index]).abs() / before[index].abs();
        assert!(
            relative <= 2.0e-7,
            "component {index} exchanged tracer across a closed barrier: {relative}"
        );
    }
}

#[test]
fn seams_and_velocity_reversal_have_no_special_case_and_workspace_reuses_storage() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let velocity = solid_rotation(&grid, 40.0);
    let reverse = velocity
        .iter()
        .map(|value| value.map(|component| -component))
        .collect::<Vec<_>>();
    let permeability = vec![1.0; grid.edges().len()];
    let scalar = grid
        .cells()
        .iter()
        .map(|cell| (1.0 + 0.1 * cell.center_unit()[1]) as f32)
        .collect::<Vec<_>>();
    let mut workspace = SecondOrderTransportWorkspace::for_grid(&grid);
    let allocation = workspace.allocation_signature();
    let forward = operators
        .advect_scalar_monotone_second_order_into(
            &scalar,
            &velocity,
            &permeability,
            500.0,
            true,
            &mut workspace,
        )
        .unwrap()
        .values()
        .to_vec();
    assert_eq!(workspace.allocation_signature(), allocation);
    let restored = operators
        .advect_scalar_monotone_second_order_into(
            &forward,
            &reverse,
            &permeability,
            500.0,
            true,
            &mut workspace,
        )
        .unwrap()
        .values()
        .to_vec();
    assert_eq!(workspace.allocation_signature(), allocation);
    let max_error = restored
        .iter()
        .zip(&scalar)
        .map(|(left, right)| (left - right).abs())
        .fold(0.0_f32, f32::max);
    assert!(max_error <= 2.0e-4, "reversal error {max_error}");

    let n = usize::from(grid.face_resolution());
    let seam_error = restored
        .iter()
        .zip(&scalar)
        .enumerate()
        .filter(|(index, _)| {
            let local = index % (n * n);
            let row = local / n;
            let column = local % n;
            row == 0 || column == 0 || row + 1 == n || column + 1 == n
        })
        .map(|(_, (left, right))| (left - right).abs())
        .fold(0.0_f32, f32::max);
    assert!(seam_error <= max_error + f32::EPSILON);
}
