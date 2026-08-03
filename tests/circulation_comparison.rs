mod support;

use sekai::generators::natural::circulation::{
    build_fixture, compare_snapshots, BalancedSteadySolver, CirculationFixture, CirculationSolver,
    ComparisonReport, CubedSphereGrid, TransientShallowWaterSolver,
};
use sekai::world::natural::{
    CirculationSnapshot, CirculationSolverId, CirculationSpec, CIRCULATION_SCHEMA_V1,
};
use support::circulation::{artificial_snapshot, mismatched_snapshots};

#[test]
fn identical_fields_have_unit_correlation_zero_error_and_full_direction_agreement() {
    let (grid, snapshot) = artificial_snapshot();
    let report = compare_snapshots(&grid, &snapshot, &snapshot).unwrap();
    assert_report_finite(&report);
    assert_eq!(report.monthly.len(), 12);
    for month in &report.monthly {
        assert!((month.wind.vector_correlation - 1.0).abs() < 1.0e-12);
        assert_eq!(month.wind.normalized_rmse, 0.0);
        assert_eq!(month.wind.direction_agreement, 1.0);
        assert_eq!(month.air_temperature.correlation, 1.0);
        assert_eq!(month.air_temperature.rmse, 0.0);
        assert_eq!(month.air_temperature.bias, 0.0);
        assert_eq!(month.precipitation.total_relative_bias, 0.0);
    }
}

#[test]
fn comparison_rejects_different_forcing_fingerprints() {
    let (grid, first, second) = mismatched_snapshots();
    assert!(compare_snapshots(&grid, &first, &second).is_err());
}

#[test]
fn analytic_scaling_reversal_and_bias_produce_exact_metrics_and_named_failures() {
    let (grid, reference) = artificial_snapshot();
    let wind = reference
        .monthly_wind_m_s()
        .iter()
        .map(|months| months.map(|vector| vector.map(|component| 2.0 * component)))
        .collect();
    let current = reference
        .monthly_ocean_current_m_s()
        .iter()
        .map(|months| months.map(|vector| vector.map(|component| -component)))
        .collect();
    let air = reference
        .monthly_air_temperature_c()
        .iter()
        .map(|months| months.map(|value| value + 2.0))
        .collect();
    let precipitation = reference
        .monthly_precipitation_mm_day()
        .iter()
        .map(|months| months.map(|value| 1.1 * value))
        .collect();
    let candidate = CirculationSnapshot::new(
        CIRCULATION_SCHEMA_V1,
        *reference.spec_fingerprint(),
        *reference.grid_fingerprint(),
        *reference.forcing_fingerprint(),
        CirculationSolverId::TransientShallowWaterV1,
        *reference.stats(),
        wind,
        current,
        air,
        reference.monthly_surface_temperature_c().to_vec(),
        reference.monthly_specific_humidity().to_vec(),
        precipitation,
        reference.monthly_atmosphere_height_anomaly_m().to_vec(),
        reference.monthly_sea_surface_height_anomaly_m().to_vec(),
    )
    .unwrap();

    let report = compare_snapshots(&grid, &candidate, &reference).unwrap();
    for month in &report.monthly {
        assert!((month.wind.vector_correlation - 1.0).abs() < 1.0e-12);
        assert!((month.wind.normalized_rmse - 1.0).abs() < 1.0e-6);
        assert_eq!(month.wind.direction_agreement, 1.0);
        assert!((month.ocean_current.vector_correlation + 1.0).abs() < 1.0e-12);
        assert!((month.ocean_current.normalized_rmse - 2.0).abs() < 1.0e-6);
        assert_eq!(month.ocean_current.direction_agreement, 0.0);
        assert!((month.air_temperature.correlation - 1.0).abs() < 1.0e-12);
        assert!((month.air_temperature.rmse - 2.0).abs() < 1.0e-6);
        assert!((month.air_temperature.bias - 2.0).abs() < 1.0e-6);
        assert!((month.precipitation.total_relative_bias - 0.1).abs() < 1.0e-6);
    }
    assert!(!report.wysiwyg.eligible);
    assert!(report
        .wysiwyg
        .failures
        .iter()
        .any(|failure| failure.metric == "wind.normalized_rmse"));
    assert!(report
        .wysiwyg
        .failures
        .iter()
        .any(|failure| failure.metric == "ocean_current.direction_agreement"));
    assert!(report
        .wysiwyg
        .failures
        .iter()
        .any(|failure| failure.metric == "air_temperature.bias_c"));
    assert!(report
        .wysiwyg
        .failures
        .iter()
        .any(|failure| failure.metric == "annual_precipitation.total_relative_bias"));
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "full transient fixture comparison is a Release evidence gate"
)]
fn real_solver_reports_are_finite_complete_and_deterministically_serialized() {
    let spec = CirculationSpec {
        face_resolution: 8,
        ..CirculationSpec::default()
    };
    let grid = CubedSphereGrid::new(spec.face_resolution, spec.planet_radius_m).unwrap();
    for fixture in [
        CirculationFixture::AquaPlanet,
        CirculationFixture::TwoBasins,
        CirculationFixture::EarthLikeHarmonics,
    ] {
        let forcing = build_fixture(&grid, fixture).unwrap();
        let steady = BalancedSteadySolver.solve(&grid, &forcing, &spec).unwrap();
        let transient = TransientShallowWaterSolver::cold_start()
            .solve(&grid, &forcing, &spec)
            .unwrap();
        let report = compare_snapshots(&grid, &steady, &transient).unwrap();
        assert_report_finite(&report);
        assert_eq!(report.monthly.len(), 12);
        assert_eq!(report.candidate_stats, *steady.stats());
        assert_eq!(report.reference_stats, *transient.stats());
        assert!(report.candidate_stats.dense_state_bytes > 0);
        assert!(report.reference_stats.dense_state_bytes > 0);
        let first = serde_json::to_vec(&report).unwrap();
        let second = serde_json::to_vec(&report).unwrap();
        assert_eq!(first, second, "{fixture:?}");
    }
}

fn assert_report_finite(report: &ComparisonReport) {
    for month in &report.monthly {
        for vector in [&month.wind, &month.ocean_current] {
            assert!(vector.vector_correlation.is_finite());
            assert!(vector.normalized_rmse.is_finite());
            assert!(vector.direction_agreement.is_finite());
            assert!(vector.reference_rms.is_finite());
            assert!(vector.direction_sampled_area_fraction.is_finite());
        }
        for scalar in [
            &month.air_temperature,
            &month.surface_temperature,
            &month.specific_humidity,
            &month.precipitation,
            &month.atmosphere_height,
            &month.sea_surface_height,
        ] {
            assert!(scalar.correlation.is_finite());
            assert!(scalar.rmse.is_finite());
            assert!(scalar.bias.is_finite());
            assert!(scalar.total_relative_bias.is_finite());
            assert!(scalar.candidate_area_mean.is_finite());
            assert!(scalar.reference_area_mean.is_finite());
        }
    }
}
