mod support;

use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{SurfaceFormationGenerationError, SurfaceFormationGenerator};
use sekai::world::natural::SURFACE_FORMATION_MAX_CLIMATE_SOLVES;

use support::surface_formation::surface_formation_fixture;

// The default absolute-steady-state equation has no in-domain solution during
// Tasks 0-8, so this intermediate baseline cannot assert a successful product.
// Task 9 restores default success plus component, conservation, endpoint
// climate, dense-owner, and deterministic-product evidence after replacing
// that equation with the frozen finite-horizon advance.

#[test]
fn a_reduced_climate_solve_budget_fails_deterministically_without_publishing() {
    let inputs = surface_formation_fixture().inputs();
    let first = SurfaceFormationGenerator::generate_with_climate_solve_limit(
        inputs,
        1,
        &BuildCancellation::new(),
    );
    let second = SurfaceFormationGenerator::generate_with_climate_solve_limit(
        inputs,
        1,
        &BuildCancellation::new(),
    );
    let (first_iterations, first_residual) = match first {
        Err(SurfaceFormationGenerationError::NotConverged {
            climate_solve_count,
            terminal_residual,
        }) => (climate_solve_count, terminal_residual.normalized_max()),
        other => panic!("a single climate solve must not converge: {other:?}"),
    };
    let (second_iterations, second_residual) = match second {
        Err(SurfaceFormationGenerationError::NotConverged {
            climate_solve_count,
            terminal_residual,
        }) => (climate_solve_count, terminal_residual.normalized_max()),
        other => panic!("a single climate solve must not converge: {other:?}"),
    };
    assert_eq!(first_iterations, 1);
    assert_eq!(second_iterations, 1);
    assert!(first_residual > 1.0);
    assert_eq!(
        first_residual.to_bits(),
        second_residual.to_bits(),
        "one complete climate solve is not deterministic"
    );

    assert!(matches!(
        SurfaceFormationGenerator::generate_with_climate_solve_limit(
            inputs,
            0,
            &BuildCancellation::new()
        ),
        Err(SurfaceFormationGenerationError::InvalidIterationLimit { .. })
    ));
    assert!(matches!(
        SurfaceFormationGenerator::generate_with_climate_solve_limit(
            inputs,
            SURFACE_FORMATION_MAX_CLIMATE_SOLVES + 1,
            &BuildCancellation::new()
        ),
        Err(SurfaceFormationGenerationError::InvalidIterationLimit { .. })
    ));
}

#[test]
fn active_cancellation_stops_the_solve_and_publishes_nothing() {
    let inputs = surface_formation_fixture().inputs();
    let signal = BuildCancellation::new();
    let worker_signal = signal.clone();
    let worker =
        std::thread::spawn(move || SurfaceFormationGenerator::generate(inputs, &worker_signal));
    let deadline = Instant::now() + Duration::from_secs(30);
    while signal.observation_count() < 4_096 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(signal.observation_count() >= 4_096);
    signal.cancel();
    let result = worker.join().unwrap();
    assert!(matches!(
        result,
        Err(SurfaceFormationGenerationError::Cancelled)
    ));
}
