use sekai::generators::natural::circulation::{
    run_comparison_suite, CirculationFixture, ComparisonError,
};

#[test]
fn comparison_suite_rejects_empty_invalid_or_unbounded_work_requests() {
    assert!(matches!(
        run_comparison_suite(&[], &[CirculationFixture::AquaPlanet], 1),
        Err(ComparisonError::EmptyResolutions)
    ));
    assert!(matches!(
        run_comparison_suite(&[65], &[CirculationFixture::AquaPlanet], 1),
        Err(ComparisonError::InvalidResolution { found: 65 })
    ));
    assert!(matches!(
        run_comparison_suite(&[4], &[], 1),
        Err(ComparisonError::EmptyFixtures)
    ));
    assert!(matches!(
        run_comparison_suite(&[4], &[CirculationFixture::AquaPlanet], 0),
        Err(ComparisonError::ZeroMeasurementSamples)
    ));
}

#[test]
#[ignore = "Release-only end-to-end timing smoke test"]
fn one_case_reports_every_timing_category_and_solver_statistic() {
    let report = run_comparison_suite(&[4], &[CirculationFixture::AquaPlanet], 1).unwrap();
    assert_eq!(report.cases.len(), 1);
    let case = &report.cases[0];
    assert_eq!(case.face_resolution, 4);
    assert_eq!(case.fixture, CirculationFixture::AquaPlanet);
    for timing in [
        &case.timings.grid_build,
        &case.timings.forcing_build,
        &case.timings.steady_solve,
        &case.timings.transient_cold_solve,
        &case.timings.transient_warm_solve,
        &case.timings.validation,
        &case.timings.comparison,
    ] {
        assert_eq!(timing.samples, 1);
        assert!(timing.median_ns > 0);
        assert!(timing.maximum_ns >= timing.median_ns);
        assert!(timing.median_ns_per_cell_month.is_finite());
        assert!(timing.median_ns_per_cell_month > 0.0);
    }
    assert!(case.steady_stats.iterations_or_steps > 0);
    assert!(case.transient_cold_stats.iterations_or_steps > 0);
    assert!(case.transient_warm_stats.iterations_or_steps > 0);
    assert!(case.dense_bytes.steady_output > 0);
    assert!(case.dense_bytes.transient_cold_output > 0);
    assert!(case.dense_bytes.transient_warm_output > 0);
    assert_eq!(case.comparison.monthly.len(), 12);
}
