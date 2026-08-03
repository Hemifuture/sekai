mod support;

use sekai::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
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
