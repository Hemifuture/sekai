use sekai::world::natural::{
    CirculationSnapshot, CirculationSolveStats, CirculationSolverId, CirculationSpec,
    PlanetForcing, CIRCULATION_SCHEMA_V1, CLIMATE_MONTH_COUNT, MAX_CUBED_SPHERE_FACE_RESOLUTION,
};

fn forcing(cell_count: usize) -> PlanetForcing {
    let equilibrium_air_temperature_c = vec![[12.0; CLIMATE_MONTH_COUNT]; cell_count];
    let equilibrium_surface_temperature_c = vec![[14.0; CLIMATE_MONTH_COUNT]; cell_count];
    let equilibrium_specific_humidity = vec![[0.01; CLIMATE_MONTH_COUNT]; cell_count];

    PlanetForcing::new(
        [7; 32],
        vec![0.0; cell_count],
        vec![0.0; cell_count],
        vec![0.3; cell_count],
        vec![1.0; cell_count],
        equilibrium_air_temperature_c,
        equilibrium_surface_temperature_c,
        equilibrium_specific_humidity,
    )
    .unwrap()
}

#[test]
fn circulation_spec_rejects_invalid_allocation_and_iteration_budgets() {
    for face_resolution in [0, MAX_CUBED_SPHERE_FACE_RESOLUTION + 1] {
        assert!(CirculationSpec {
            face_resolution,
            ..CirculationSpec::default()
        }
        .validate()
        .is_err());
    }

    assert!(CirculationSpec {
        max_steady_iterations: 0,
        ..CirculationSpec::default()
    }
    .validate()
    .is_err());
    assert!(CirculationSpec {
        max_formation_years: 0,
        ..CirculationSpec::default()
    }
    .validate()
    .is_err());
}

#[test]
fn circulation_spec_deserialization_revalidates_and_fingerprint_tracks_parameters() {
    let spec = CirculationSpec::default();
    let encoded = serde_json::to_vec(&spec).unwrap();
    let decoded: CirculationSpec = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, spec);
    assert_eq!(decoded.fingerprint().unwrap(), spec.fingerprint().unwrap());

    let mut changed = spec.clone();
    changed.gravity_m_s2 += 0.01;
    assert_ne!(changed.fingerprint().unwrap(), spec.fingerprint().unwrap());

    let mut invalid = serde_json::to_value(spec).unwrap();
    invalid["cfl_limit"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<CirculationSpec>(invalid).is_err());

    let mut unknown = serde_json::to_value(CirculationSpec::default()).unwrap();
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CirculationSpec>(unknown).is_err());
}

#[test]
fn forcing_is_dense_finite_and_content_addressed() {
    let first = forcing(24);
    let second = forcing(24);

    assert_eq!(first.cell_count(), 24);
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.grid_fingerprint(), &[7; 32]);
    assert_eq!(first.elevation_m().len(), 24);
    assert_eq!(first.land_fraction().len(), 24);
    assert_eq!(first.surface_albedo().len(), 24);
    assert_eq!(first.surface_moisture_availability().len(), 24);
    assert_eq!(first.equilibrium_air_temperature_c().len(), 24);
    assert_eq!(first.equilibrium_surface_temperature_c().len(), 24);
    assert_eq!(first.equilibrium_specific_humidity().len(), 24);
}

#[test]
fn forcing_rejects_length_nonfinite_and_fraction_violations() {
    let monthly = vec![[12.0; CLIMATE_MONTH_COUNT]; 2];
    assert!(PlanetForcing::new(
        [1; 32],
        vec![0.0; 2],
        vec![0.0; 1],
        vec![0.3; 2],
        vec![1.0; 2],
        monthly.clone(),
        monthly.clone(),
        vec![[0.01; CLIMATE_MONTH_COUNT]; 2],
    )
    .is_err());

    let mut invalid_elevation = vec![0.0; 2];
    invalid_elevation[1] = f32::NAN;
    assert!(PlanetForcing::new(
        [1; 32],
        invalid_elevation,
        vec![0.0; 2],
        vec![0.3; 2],
        vec![1.0; 2],
        monthly.clone(),
        monthly.clone(),
        vec![[0.01; CLIMATE_MONTH_COUNT]; 2],
    )
    .is_err());

    assert!(PlanetForcing::new(
        [1; 32],
        vec![0.0; 2],
        vec![0.0; 2],
        vec![1.01; 2],
        vec![1.0; 2],
        monthly.clone(),
        monthly,
        vec![[0.01; CLIMATE_MONTH_COUNT]; 2],
    )
    .is_err());
}

#[test]
fn forcing_deserialization_rejects_a_tampered_fingerprint() {
    let original = forcing(3);
    let mut wire = serde_json::to_value(&original).unwrap();
    wire["fingerprint"][0] = serde_json::json!(255);

    assert!(serde_json::from_value::<PlanetForcing>(wire).is_err());
}

#[test]
fn forcing_deserialization_rejects_unknown_fields() {
    let mut wire = serde_json::to_value(forcing(1)).unwrap();
    wire["unexpected"] = serde_json::json!(true);

    assert!(serde_json::from_value::<PlanetForcing>(wire).is_err());
}

#[test]
fn forcing_deserialization_rejects_cell_limit_plus_one_while_streaming() {
    let max_cell_count =
        6 * MAX_CUBED_SPHERE_FACE_RESOLUTION as usize * MAX_CUBED_SPHERE_FACE_RESOLUTION as usize;
    let json = json_object_with_repeated_array_element("elevation_m", "0", max_cell_count + 1);

    let error = serde_json::from_str::<PlanetForcing>(&json).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&format!("maximum item count {max_cell_count}")),
        "unexpected deserialization error: {error}"
    );
}

fn snapshot(
    cell_count: usize,
    mut wind: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
) -> Result<CirculationSnapshot, sekai::world::natural::CirculationSnapshotError> {
    if wind.is_empty() && cell_count > 0 {
        wind = vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; cell_count];
    }
    CirculationSnapshot::new(
        CIRCULATION_SCHEMA_V1,
        [1; 32],
        [2; 32],
        [3; 32],
        CirculationSolverId::BalancedSteadyV1,
        CirculationSolveStats {
            iterations_or_steps: 12,
            formation_years: 0,
            final_residual: 1.0e-5,
            relative_mass_error: 1.0e-8,
            dense_state_bytes: 4_096,
        },
        wind,
        vec![[[0.1, 0.0, 0.0]; CLIMATE_MONTH_COUNT]; cell_count],
        vec![[12.0; CLIMATE_MONTH_COUNT]; cell_count],
        vec![[14.0; CLIMATE_MONTH_COUNT]; cell_count],
        vec![[0.01; CLIMATE_MONTH_COUNT]; cell_count],
        vec![[2.0; CLIMATE_MONTH_COUNT]; cell_count],
        vec![[0.0; CLIMATE_MONTH_COUNT]; cell_count],
        vec![[0.0; CLIMATE_MONTH_COUNT]; cell_count],
    )
}

#[test]
fn shared_snapshot_preserves_identity_and_round_trips_canonically() {
    let original = snapshot(3, Vec::new()).unwrap();
    assert_eq!(original.schema_version(), CIRCULATION_SCHEMA_V1);
    assert_eq!(original.cell_count(), 3);
    assert_eq!(original.spec_fingerprint(), &[1; 32]);
    assert_eq!(original.grid_fingerprint(), &[2; 32]);
    assert_eq!(original.forcing_fingerprint(), &[3; 32]);
    assert_eq!(original.solver_id(), CirculationSolverId::BalancedSteadyV1);
    assert_eq!(original.monthly_wind_m_s().len(), 3);
    assert_eq!(original.monthly_ocean_current_m_s().len(), 3);
    assert_eq!(original.monthly_air_temperature_c().len(), 3);
    assert_eq!(original.monthly_surface_temperature_c().len(), 3);
    assert_eq!(original.monthly_specific_humidity().len(), 3);
    assert_eq!(original.monthly_precipitation_mm_day().len(), 3);
    assert_eq!(original.monthly_atmosphere_height_anomaly_m().len(), 3);
    assert_eq!(original.monthly_sea_surface_height_anomaly_m().len(), 3);
    original.validate().unwrap();

    let encoded = serde_json::to_vec(&original).unwrap();
    let decoded: CirculationSnapshot = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
}

#[test]
fn shared_snapshot_rejects_length_nonfinite_and_nonnegative_field_violations() {
    let mismatched_wind = vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; 2];
    assert!(snapshot(3, mismatched_wind).is_err());

    let mut nonfinite_wind = vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; 2];
    nonfinite_wind[1][4][2] = f32::NAN;
    assert!(snapshot(2, nonfinite_wind).is_err());

    let valid = snapshot(2, Vec::new()).unwrap();
    for field in ["monthly_specific_humidity", "monthly_precipitation_mm_day"] {
        let mut wire = serde_json::to_value(&valid).unwrap();
        wire[field][1][5] = serde_json::json!(-0.01);
        assert!(
            serde_json::from_value::<CirculationSnapshot>(wire).is_err(),
            "{field} accepted a negative value"
        );
    }
}

#[test]
fn shared_snapshot_rejects_invalid_solve_statistics_on_deserialization() {
    let valid = snapshot(1, Vec::new()).unwrap();
    let mut wire = serde_json::to_value(&valid).unwrap();
    wire["stats"]["final_residual"] = serde_json::json!(-1.0);

    assert!(serde_json::from_value::<CirculationSnapshot>(wire).is_err());
}

#[test]
fn shared_snapshot_deserialization_rejects_unknown_fields() {
    let mut wire = serde_json::to_value(snapshot(1, Vec::new()).unwrap()).unwrap();
    wire["unexpected"] = serde_json::json!(true);

    assert!(serde_json::from_value::<CirculationSnapshot>(wire).is_err());

    let mut nested = serde_json::to_value(snapshot(1, Vec::new()).unwrap()).unwrap();
    nested["stats"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CirculationSnapshot>(nested).is_err());
}

#[test]
fn shared_snapshot_deserialization_rejects_monthly_limit_plus_one_while_streaming() {
    let max_cell_count =
        6 * MAX_CUBED_SPHERE_FACE_RESOLUTION as usize * MAX_CUBED_SPHERE_FACE_RESOLUTION as usize;
    let max_monthly_value_count = max_cell_count.checked_mul(CLIMATE_MONTH_COUNT).unwrap();
    let monthly_cell = format!(
        "[{}]",
        std::iter::repeat_n("0", CLIMATE_MONTH_COUNT)
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut json = json_object_with_repeated_array_element(
        "monthly_air_temperature_c",
        &monthly_cell,
        max_cell_count,
    );
    json.truncate(json.len() - 2);
    json.push_str(",[0]]}");

    let error = serde_json::from_str::<CirculationSnapshot>(&json).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&format!("maximum item count {max_monthly_value_count}")),
        "unexpected deserialization error: {error}"
    );
}

fn json_object_with_repeated_array_element(field: &str, element: &str, len: usize) -> String {
    let mut json = String::with_capacity(field.len() + element.len().saturating_mul(len) + len + 8);
    json.push_str("{\"");
    json.push_str(field);
    json.push_str("\":[");
    for index in 0..len {
        if index > 0 {
            json.push(',');
        }
        json.push_str(element);
    }
    json.push_str("]}");
    json
}
