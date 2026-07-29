use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, GeologicSpecArtifact, ReliefArtifact,
    RulePackSetArtifact, TectonicSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    ClimateValidationError, GeologicSpec, MonthlyScalarField, MonthlyVectorField,
    PreliminaryClimateSnapshot, TectonicSpec, CLIMATE_MONTH_COUNT, PRELIMINARY_CLIMATE_SCHEMA_V1,
};
use sekai::world::spatial::Topology;
use sekai::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec, RootSeed};

fn monthly_scalar(cell_count: usize, value: impl Fn(usize, usize) -> f32) -> MonthlyScalarField {
    MonthlyScalarField::from_values(
        (0..cell_count)
            .map(|cell| std::array::from_fn(|month| value(cell, month)))
            .collect(),
    )
    .unwrap()
}

fn monthly_vector(
    cell_count: usize,
    value: impl Fn(usize, usize) -> [f32; 2],
) -> MonthlyVectorField {
    MonthlyVectorField::from_values(
        (0..cell_count)
            .map(|cell| std::array::from_fn(|month| value(cell, month)))
            .collect(),
    )
    .unwrap()
}

fn valid_snapshot(cell_count: u32) -> PreliminaryClimateSnapshot {
    let count = cell_count as usize;
    let temperature = monthly_scalar(count, |cell, month| 4.0 + cell as f32 + month as f32);
    let precipitation =
        monthly_scalar(count, |cell, month| 30.0 + cell as f32 + month as f32 * 2.0);
    let wind = monthly_vector(count, |cell, month| {
        [-5.0 + cell as f32 * 0.1, month as f32 * 0.2 - 1.0]
    });
    let mean_temperature = temperature
        .values()
        .iter()
        .map(|months| months.iter().sum::<f32>() / CLIMATE_MONTH_COUNT as f32)
        .collect();
    let seasonality = temperature
        .values()
        .iter()
        .map(|months| {
            months.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - months.iter().copied().fold(f32::INFINITY, f32::min)
        })
        .collect();
    let annual_precipitation = precipitation
        .values()
        .iter()
        .map(|months| months.iter().sum())
        .collect();
    let prevailing_wind = wind
        .values()
        .iter()
        .map(|months| {
            let sum = months.iter().fold([0.0_f32; 2], |sum, value| {
                [sum[0] + value[0], sum[1] + value[1]]
            });
            [
                sum[0] / CLIMATE_MONTH_COUNT as f32,
                sum[1] / CLIMATE_MONTH_COUNT as f32,
            ]
        })
        .collect();

    PreliminaryClimateSnapshot::new(
        PRELIMINARY_CLIMATE_SCHEMA_V1,
        cell_count,
        (0..count)
            .map(|cell| -60.0 + 120.0 * cell as f32 / count.max(2) as f32)
            .collect(),
        vec![0.5; count],
        temperature,
        precipitation,
        wind,
        mean_temperature,
        seasonality,
        annual_precipitation,
        prevailing_wind,
    )
    .unwrap()
}

fn natural_artifacts(
    cell_count: u32,
) -> (
    std::sync::Arc<SpatialArtifact>,
    std::sync::Arc<ReliefArtifact>,
) {
    let mut external = ExternalArtifacts::new();
    external
        .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
            width: Meters::new(1_000_000.0).unwrap(),
            height: Meters::new(600_000.0).unwrap(),
            target_cell_count: cell_count,
            boundary: BoundaryCondition::Closed,
        }))
        .unwrap();
    external
        .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
        .unwrap();
    external
        .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
        .unwrap();
    external
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    external
        .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
        .unwrap();
    let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(u64::from(cell_count)),
            external,
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    (
        outcome.artifacts.get::<SpatialArtifact>().unwrap(),
        outcome.artifacts.get::<ReliefArtifact>().unwrap(),
    )
}

#[test]
fn monthly_fields_are_dense_finite_and_month_bounded() {
    let scalars = monthly_scalar(2, |cell, month| (cell * CLIMATE_MONTH_COUNT + month) as f32);
    let vectors = monthly_vector(2, |cell, month| [cell as f32, month as f32]);

    assert_eq!(scalars.len(), 2);
    assert!(!scalars.is_empty());
    assert_eq!(scalars.value(1, 11), Some(23.0));
    assert_eq!(scalars.value(1, CLIMATE_MONTH_COUNT), None);
    assert_eq!(vectors.value(1, 11), Some([1.0, 11.0]));
    assert_eq!(vectors.value(1, CLIMATE_MONTH_COUNT), None);

    let mut invalid_scalars = vec![[0.0; CLIMATE_MONTH_COUNT]];
    invalid_scalars[0][4] = f32::NAN;
    assert!(matches!(
        MonthlyScalarField::from_values(invalid_scalars),
        Err(ClimateValidationError::NonFiniteScalarValue { .. })
    ));

    let mut invalid_vectors = vec![[[0.0; 2]; CLIMATE_MONTH_COUNT]];
    invalid_vectors[0][7][1] = f32::INFINITY;
    assert!(matches!(
        MonthlyVectorField::from_values(invalid_vectors),
        Err(ClimateValidationError::NonFiniteVectorValue { .. })
    ));
}

#[test]
fn valid_snapshot_exposes_zero_copy_monthly_and_annual_fields() {
    let snapshot = valid_snapshot(3);

    assert_eq!(snapshot.schema_version(), PRELIMINARY_CLIMATE_SCHEMA_V1);
    assert_eq!(snapshot.cell_count(), 3);
    assert_eq!(
        snapshot.air_temperature_c(CellId::from_raw(1), 2),
        Some(7.0)
    );
    assert_eq!(
        snapshot.precipitation_mm(CellId::from_raw(1), 2),
        Some(35.0)
    );
    assert_eq!(
        snapshot.wind_m_s(CellId::from_raw(1), 2),
        Some([-4.9, -0.6])
    );
    assert_eq!(snapshot.air_temperature_c(CellId::from_raw(1), 12), None);
    assert_eq!(snapshot.latitude_degrees().len(), 3);
    assert_eq!(snapshot.maritime_influence().len(), 3);
    assert_eq!(snapshot.mean_annual_air_temperature_c().len(), 3);
    assert_eq!(snapshot.temperature_seasonality_c(), &[11.0; 3]);
    snapshot.validate().unwrap();
}

#[test]
fn snapshot_rejects_schema_length_range_and_summary_violations() {
    let valid = valid_snapshot(2);
    let mut wire = serde_json::to_value(&valid).unwrap();
    wire["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<PreliminaryClimateSnapshot>(wire).is_err());

    let mut wire = serde_json::to_value(&valid).unwrap();
    wire["latitude_degrees"].as_array_mut().unwrap().pop();
    assert!(serde_json::from_value::<PreliminaryClimateSnapshot>(wire).is_err());

    for (field, value) in [
        ("latitude_degrees", 100.0),
        ("maritime_influence", 1.1),
        ("mean_annual_air_temperature_c", 100.0),
        ("temperature_seasonality_c", 130.0),
        ("annual_precipitation_mm", 21_000.0),
    ] {
        let mut wire = serde_json::to_value(&valid).unwrap();
        wire[field][0] = serde_json::json!(value);
        assert!(
            serde_json::from_value::<PreliminaryClimateSnapshot>(wire).is_err(),
            "{field} accepted {value}"
        );
    }

    let mut wire = serde_json::to_value(&valid).unwrap();
    wire["monthly_air_temperature_c"][0][0] = serde_json::json!(80.0);
    assert!(serde_json::from_value::<PreliminaryClimateSnapshot>(wire).is_err());

    let mut wire = serde_json::to_value(&valid).unwrap();
    wire["monthly_precipitation_mm"][0][0] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<PreliminaryClimateSnapshot>(wire).is_err());

    let mut wire = serde_json::to_value(&valid).unwrap();
    wire["monthly_wind_m_s"][0][0][0] = serde_json::json!(90.0);
    assert!(serde_json::from_value::<PreliminaryClimateSnapshot>(wire).is_err());

    let mut wire = serde_json::to_value(&valid).unwrap();
    wire["annual_precipitation_mm"][0] = serde_json::json!(999.0);
    assert!(serde_json::from_value::<PreliminaryClimateSnapshot>(wire).is_err());
}

#[test]
fn snapshot_round_trip_revalidates_and_preserves_exact_bytes() {
    let snapshot = valid_snapshot(4);
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: PreliminaryClimateSnapshot = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(decoded, snapshot);
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
}

#[test]
fn snapshot_alignment_checks_spatial_and_relief_counts_independently() {
    let (spatial_16, relief_16) = natural_artifacts(16);
    let (_, relief_32) = natural_artifacts(32);
    let climate_count = spatial_16.snapshot().cell_count() as u32;

    valid_snapshot(climate_count)
        .validate_against(spatial_16.snapshot(), relief_16.snapshot())
        .unwrap();
    assert!(matches!(
        valid_snapshot(climate_count - 1)
            .validate_against(spatial_16.snapshot(), relief_16.snapshot()),
        Err(ClimateValidationError::SpatialCellCountMismatch { .. })
    ));
    assert!(matches!(
        valid_snapshot(climate_count).validate_against(spatial_16.snapshot(), relief_32.snapshot()),
        Err(ClimateValidationError::ReliefCellCountMismatch { .. })
    ));
}
