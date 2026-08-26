mod support;

use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{SurfaceFormationGenerationError, SurfaceFormationGenerator};
use sekai::world::natural::SURFACE_FORMATION_HORIZON_YEARS;

use support::surface_formation::surface_formation_fixture;

#[test]
fn draft_generation_consumes_the_complete_surface_horizon() {
    let inputs = surface_formation_fixture().inputs();
    let snapshot = SurfaceFormationGenerator::generate(inputs, &BuildCancellation::new())
        .expect("the finite-time Draft product should publish");

    assert_eq!(
        snapshot
            .evolution_report()
            .integrated_duration_years()
            .to_bits(),
        SURFACE_FORMATION_HORIZON_YEARS.to_bits()
    );
    assert_eq!(
        snapshot
            .checkpoint()
            .upstream()
            .formation_climate_checkpoint_fingerprint(),
        snapshot.formation_climate().checkpoint().fingerprint()
    );
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
