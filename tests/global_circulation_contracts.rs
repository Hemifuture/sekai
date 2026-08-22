use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::global_circulation_model_fingerprint;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    ClimateBudgetReport, ClimateCapabilityAvailability, ClimateCapabilityId, ClimateCapabilitySet,
    ClimateCheckpoint, ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile,
    ClimateQuantizationId, ClimateRemapReport, ClimateSolveReport, GlobalCirculationFields,
    GlobalCirculationSnapshot, GlobalCirculationValidationError, MonthlyScalarField,
    MonthlyVector3Field, NaturalQualityProfile, ProductionIntegratorId,
    GLOBAL_CIRCULATION_MACRO_STEP_SECONDS, GLOBAL_CIRCULATION_SCHEMA_V2,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{Meters, SphericalSpaceSpec};

fn surface(target_cell_count: u32) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn scalar(cell_count: usize, value: f32) -> MonthlyScalarField {
    MonthlyScalarField::from_values(vec![[value; 12]; cell_count]).unwrap()
}

fn vectors(cell_count: usize, value: [f32; 3]) -> MonthlyVector3Field {
    MonthlyVector3Field::from_values(vec![[value; 12]; cell_count]).unwrap()
}

fn checkpoint(profile: ClimateModelProfile) -> ClimateCheckpoint {
    ClimateCheckpoint::new(
        sekai::world::natural::NaturalQualityProfile::Draft,
        profile,
        ProductionIntegratorId::SplitExplicitRk3V1,
        [1; 32],
        [2; 32],
        global_circulation_model_fingerprint(profile),
        [3; 32],
        ClimateQuantizationId::DeterministicF64V1,
        24,
        [4; 32],
    )
    .unwrap()
}

fn c2_fields(
    count: usize,
    lower_height_m: f32,
    upper_height_m: f32,
    mixed_height_m: f32,
    thermocline_height_m: f32,
) -> GlobalCirculationFields {
    GlobalCirculationFields::new_c2(
        vectors(count, [0.0; 3]),
        vectors(count, [0.0; 3]),
        vectors(count, [0.0; 3]),
        vectors(count, [0.0; 3]),
        scalar(count, 12.0),
        scalar(count, 15.0),
        scalar(count, 8.0),
        scalar(count, 900.0 + thermocline_height_m),
        scalar(count, 0.008),
        scalar(count, 2.0),
        scalar(count, 0.5),
        scalar(count, lower_height_m),
        scalar(count, upper_height_m),
        scalar(count, mixed_height_m),
        scalar(count, thermocline_height_m),
        scalar(count, 4.0),
    )
    .unwrap()
}

fn c2_snapshot(surface: &SphericalSurfaceSnapshot) -> GlobalCirculationSnapshot {
    let count = surface.cells().len();
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let fields = c2_fields(count, 0.0, 0.0, 0.0, 0.0);
    let checkpoint = ClimateCheckpoint::new(
        NaturalQualityProfile::Draft,
        ClimateModelProfile::C2LayeredV1,
        ProductionIntegratorId::SplitExplicitRk3V1,
        *CubedSphereGrid::new(
            NaturalQualityProfile::Draft.climate_face_resolution(),
            surface.radius().get(),
        )
        .unwrap()
        .fingerprint(),
        [2; 32],
        global_circulation_model_fingerprint(ClimateModelProfile::C2LayeredV1),
        [3; 32],
        ClimateQuantizationId::DeterministicF64V1,
        24,
        fields.fingerprint(),
    )
    .unwrap();
    GlobalCirculationSnapshot::new(
        GLOBAL_CIRCULATION_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        layout,
        ProductionIntegratorId::SplitExplicitRk3V1,
        ClimateCapabilitySet::for_profile(ClimateModelProfile::C2LayeredV1),
        checkpoint,
        ClimateSolveReport::new(
            2,
            24,
            192,
            0,
            1.0,
            1.0e-5,
            0.41,
            sekai::world::natural::expected_global_circulation_dense_state_bytes(
                NaturalQualityProfile::Draft,
                ClimateModelProfile::C2LayeredV1,
                surface.cells().len() as u32,
            )
            .unwrap(),
        )
        .unwrap(),
        ClimateBudgetReport::new(1.0e-10, 2.0e-10, 3.0e-10, 4.0e-9, 5.0e-10).unwrap(),
        ClimateRemapReport::new(1.0e-13, 2.0e-13, 3.0e-13, 4.0e-13, 5.0e-8, 100, 100).unwrap(),
        fields,
    )
    .unwrap()
}

#[test]
fn fixed_model_profiles_publish_exact_layer_roles_and_constants() {
    let c1 = ClimateLayerLayout::for_profile(ClimateModelProfile::C1SingleLayerV1);
    let c2 = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    c1.validate().unwrap();
    c2.validate().unwrap();
    assert_ne!(
        global_circulation_model_fingerprint(ClimateModelProfile::C2LayeredV1),
        c2.fingerprint(),
        "the equation identity must cover more than the serialized layer layout"
    );
    assert_eq!(
        c1.layers()
            .iter()
            .map(|layer| layer.role())
            .collect::<Vec<_>>(),
        vec![
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::OceanMixedLayer
        ]
    );
    assert_eq!(
        c2.layers()
            .iter()
            .map(|layer| layer.role())
            .collect::<Vec<_>>(),
        vec![
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
            ClimateLayerRole::OceanMixedLayer,
            ClimateLayerRole::OceanThermocline,
            ClimateLayerRole::DeepOceanReservoir,
        ]
    );
    assert!(c2.layers()[..4]
        .iter()
        .all(|layer| layer.dynamically_active()));
    assert!(!c2.layers()[4].dynamically_active());
    assert!(c2.layers().iter().all(|layer| {
        layer.reference_thickness_m() > 0.0
            && layer.density_kg_m3() > 0.0
            && layer.heat_capacity_j_kg_k() > 0.0
    }));
    assert_eq!(c1.exchanges().len(), 1);
    assert_eq!(c2.exchanges().len(), 4);
    let lower_upper = c2
        .exchange(
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        )
        .expect("C2 lower-upper exchange");
    assert_eq!(lower_upper.heat_exchange_time_s(), Some(5.0 * 86_400.0));
    assert_eq!(lower_upper.momentum_exchange_time_s(), Some(5.0 * 86_400.0));
    assert_eq!(lower_upper.moisture_exchange_time_s(), Some(5.0 * 86_400.0));
    let deep = c2
        .exchange(
            ClimateLayerRole::OceanThermocline,
            ClimateLayerRole::DeepOceanReservoir,
        )
        .expect("C2 thermocline-deep exchange");
    assert!(deep.momentum_exchange_time_s().is_none());
    assert_eq!(deep.heat_exchange_time_s(), Some(200.0 * 365.25 * 86_400.0));

    let bytes = serde_json::to_vec(&c2).unwrap();
    let decoded: ClimateLayerLayout = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, c2);
    let mut tampered = serde_json::to_value(c2).unwrap();
    tampered["layers"][0]["reference_thickness_m"] = serde_json::json!(9_999.0);
    assert!(serde_json::from_value::<ClimateLayerLayout>(tampered).is_err());

    let mut oversized = serde_json::to_value(ClimateLayerLayout::for_profile(
        ClimateModelProfile::C2LayeredV1,
    ))
    .unwrap();
    let duplicate = oversized["layers"][0].clone();
    oversized["layers"].as_array_mut().unwrap().push(duplicate);
    let error = serde_json::from_value::<ClimateLayerLayout>(oversized).unwrap_err();
    assert!(error.to_string().contains("at most 5 elements"));

    let mut oversized = serde_json::to_value(ClimateLayerLayout::for_profile(
        ClimateModelProfile::C2LayeredV1,
    ))
    .unwrap();
    let duplicate = oversized["exchanges"][0].clone();
    oversized["exchanges"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let error = serde_json::from_value::<ClimateLayerLayout>(oversized).unwrap_err();
    assert!(error.to_string().contains("at most 4 elements"));
}

#[test]
fn capabilities_are_complete_ordered_and_preserve_all_three_states() {
    let set = ClimateCapabilitySet::new(vec![
        (
            ClimateCapabilityId::SeasonalMeanV1,
            ClimateCapabilityAvailability::Available,
        ),
        (
            ClimateCapabilityId::VerticalStructureV1,
            ClimateCapabilityAvailability::Available,
        ),
        (
            ClimateCapabilityId::SeaIceV1,
            ClimateCapabilityAvailability::EvaluatedNotApplicable,
        ),
        (
            ClimateCapabilityId::LandSurfaceFeedbackV1,
            ClimateCapabilityAvailability::Unavailable,
        ),
        (
            ClimateCapabilityId::EquatorialVariabilityV1,
            ClimateCapabilityAvailability::Unavailable,
        ),
        (
            ClimateCapabilityId::TropicalCycloneClimatologyV1,
            ClimateCapabilityAvailability::Unavailable,
        ),
    ])
    .unwrap();
    assert_eq!(
        set.availability(ClimateCapabilityId::SeaIceV1),
        ClimateCapabilityAvailability::EvaluatedNotApplicable
    );
    assert_eq!(
        serde_json::from_slice::<ClimateCapabilitySet>(&serde_json::to_vec(&set).unwrap()).unwrap(),
        set
    );

    let mut missing = serde_json::to_value(&set).unwrap();
    missing["statuses"].as_array_mut().unwrap().pop();
    assert!(serde_json::from_value::<ClimateCapabilitySet>(missing).is_err());

    let mut oversized = serde_json::to_value(&set).unwrap();
    let duplicate = oversized["statuses"][0].clone();
    oversized["statuses"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let error = serde_json::from_value::<ClimateCapabilitySet>(oversized).unwrap_err();
    assert!(error.to_string().contains("at most 6 elements"));
}

#[test]
fn checkpoint_fingerprint_covers_every_resume_identity() {
    let first = checkpoint(ClimateModelProfile::C2LayeredV1);
    let second = checkpoint(ClimateModelProfile::C2LayeredV1);
    assert_eq!(first, second);
    assert_ne!(first.fingerprint(), &[0; 32]);
    assert_eq!(
        serde_json::from_slice::<ClimateCheckpoint>(&serde_json::to_vec(&first).unwrap()).unwrap(),
        first
    );

    let mut tampered = serde_json::to_value(&first).unwrap();
    tampered["completed_phase_steps"] = serde_json::json!(36);
    assert!(serde_json::from_value::<ClimateCheckpoint>(tampered).is_err());

    for (quality, completed_phase_steps) in [
        (sekai::world::natural::NaturalQualityProfile::Draft, 9 * 12),
        (
            sekai::world::natural::NaturalQualityProfile::Standard,
            11 * 12,
        ),
    ] {
        assert!(matches!(
            ClimateCheckpoint::new(
                quality,
                ClimateModelProfile::C2LayeredV1,
                ProductionIntegratorId::SplitExplicitRk3V1,
                [1; 32],
                [2; 32],
                global_circulation_model_fingerprint(ClimateModelProfile::C2LayeredV1),
                [3; 32],
                ClimateQuantizationId::DeterministicF64V1,
                completed_phase_steps,
                [4; 32],
            ),
            Err(
                sekai::world::natural::ClimateCheckpointError::CompletedPhaseStepsExceedProfile { .. }
            )
        ));
    }
}

#[test]
fn v2_time_contract_distinguishes_forcing_phases_from_integrated_time() {
    let source = surface(42);
    let snapshot = c2_snapshot(&source);
    let checkpoint = serde_json::to_value(snapshot.checkpoint()).unwrap();
    let solve = serde_json::to_value(snapshot.solve_report()).unwrap();

    assert_eq!(checkpoint["schema_version"], serde_json::json!(2));
    assert_eq!(checkpoint["completed_phase_steps"], serde_json::json!(24));
    assert!(checkpoint.get("completed_months").is_none());
    assert_eq!(solve["formation_cycles"], serde_json::json!(2));
    assert_eq!(solve["continuation_steps"], serde_json::json!(24));
    assert_eq!(
        solve["integrated_model_seconds"],
        serde_json::json!(24 * GLOBAL_CIRCULATION_MACRO_STEP_SECONDS as u64)
    );
    assert!(solve.get("formation_years").is_none());
    assert!(solve.get("macro_steps").is_none());

    let mut old_checkpoint = checkpoint;
    old_checkpoint["schema_version"] = serde_json::json!(1);
    assert!(serde_json::from_value::<ClimateCheckpoint>(old_checkpoint).is_err());

    let mut old_snapshot = serde_json::to_value(snapshot).unwrap();
    old_snapshot["schema_version"] = serde_json::json!(1);
    assert!(serde_json::from_value::<GlobalCirculationSnapshot>(old_snapshot).is_err());
}

#[test]
fn c2_snapshot_is_strict_surface_bound_tangent_and_byte_deterministic() {
    let source = surface(42);
    let snapshot = c2_snapshot(&source);
    snapshot.validate_against(&source).unwrap();
    assert_eq!(snapshot.profile(), ClimateModelProfile::C2LayeredV1);
    assert_eq!(
        snapshot.integrator(),
        ProductionIntegratorId::SplitExplicitRk3V1
    );
    assert_eq!(
        snapshot
            .capabilities()
            .availability(ClimateCapabilityId::VerticalStructureV1),
        ClimateCapabilityAvailability::Available
    );
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    let decoded: GlobalCirculationSnapshot = serde_json::from_slice(&bytes).unwrap();
    decoded.validate_against(&source).unwrap();
    assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());

    let mut unknown = serde_json::to_value(&snapshot).unwrap();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("unknown".into(), true.into());
    assert!(serde_json::from_value::<GlobalCirculationSnapshot>(unknown).is_err());

    let mut radial = serde_json::to_value(&snapshot).unwrap();
    let direction = source.cells()[0].centroid.components();
    radial["fields"]["near_surface_wind_m_s"][0][0] = serde_json::json!(direction);
    radial["fields"]["upper_wind_m_s"][0][0] = serde_json::json!(direction);
    assert!(serde_json::from_value::<GlobalCirculationSnapshot>(radial).is_err());
}

#[test]
fn contextual_snapshot_validation_rejects_a_validly_rehashed_noncanonical_grid() {
    let source = surface(42);
    let canonical = c2_snapshot(&source);
    let checkpoint = ClimateCheckpoint::new(
        canonical.checkpoint().quality_profile(),
        canonical.profile(),
        canonical.integrator(),
        [9; 32],
        *canonical.checkpoint().forcing_fingerprint(),
        *canonical.checkpoint().model_fingerprint(),
        *canonical.checkpoint().input_fingerprint(),
        ClimateQuantizationId::DeterministicF64V1,
        canonical.checkpoint().completed_phase_steps(),
        *canonical.checkpoint().state_fingerprint(),
    )
    .unwrap();
    let forged = GlobalCirculationSnapshot::new(
        canonical.schema_version(),
        canonical.surface_ref(),
        canonical.layout().clone(),
        canonical.integrator(),
        canonical.capabilities().clone(),
        checkpoint,
        *canonical.solve_report(),
        *canonical.budget_report(),
        *canonical.remap_report(),
        canonical.fields().clone(),
    )
    .unwrap();
    assert!(matches!(
        forged.validate_against(&source),
        Err(
            GlobalCirculationValidationError::CheckpointIdentityMismatch {
                field: "grid_fingerprint"
            }
        )
    ));
}

#[test]
fn snapshot_binds_selected_solver_work_to_the_locked_procedure() {
    let source = surface(42);
    let snapshot = c2_snapshot(&source);
    for (field, value) in [
        ("linear_iterations", serde_json::json!(999)),
        ("dense_state_bytes", serde_json::json!(1)),
        ("fast_substeps", serde_json::json!(24)),
    ] {
        let mut wire = serde_json::to_value(&snapshot).unwrap();
        wire["solve_report"][field] = value;
        assert!(
            serde_json::from_value::<GlobalCirculationSnapshot>(wire).is_err(),
            "snapshot accepted forged {field}"
        );
    }
}

#[test]
fn snapshot_rejects_wrong_surface_lengths_profiles_and_budget_evidence() {
    let source = surface(42);
    let snapshot = c2_snapshot(&source);
    let other = surface(162);
    assert!(snapshot.validate_against(&other).is_err());

    let mut wrong_length = serde_json::to_value(&snapshot).unwrap();
    wrong_length["fields"]["monthly_air_temperature_c"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(serde_json::from_value::<GlobalCirculationSnapshot>(wrong_length).is_err());

    assert!(ClimateBudgetReport::new(1.0e-3, 0.0, 0.0, 0.0, 0.0).is_err());
    assert!(ClimateSolveReport::new(2, 10, 10, 0, 1.0e-6, 1.0, 0.4, 1).is_err());
}

#[test]
fn snapshot_constructor_and_serde_reject_nonpositive_actual_layer_depths() {
    let source = surface(42);
    let count = source.cells().len();
    let build = |fields: GlobalCirculationFields| {
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let checkpoint = ClimateCheckpoint::new(
            sekai::world::natural::NaturalQualityProfile::Draft,
            ClimateModelProfile::C2LayeredV1,
            ProductionIntegratorId::SplitExplicitRk3V1,
            [1; 32],
            [2; 32],
            global_circulation_model_fingerprint(ClimateModelProfile::C2LayeredV1),
            [3; 32],
            ClimateQuantizationId::DeterministicF64V1,
            24,
            fields.fingerprint(),
        )
        .unwrap();
        GlobalCirculationSnapshot::new(
            GLOBAL_CIRCULATION_SCHEMA_V2,
            SurfaceRef::for_spherical(&source),
            layout,
            ProductionIntegratorId::SplitExplicitRk3V1,
            ClimateCapabilitySet::for_profile(ClimateModelProfile::C2LayeredV1),
            checkpoint,
            ClimateSolveReport::new(
                2,
                24,
                192,
                0,
                1.0,
                1.0e-5,
                0.41,
                sekai::world::natural::expected_global_circulation_dense_state_bytes(
                    NaturalQualityProfile::Draft,
                    ClimateModelProfile::C2LayeredV1,
                    source.cells().len() as u32,
                )
                .unwrap(),
            )
            .unwrap(),
            ClimateBudgetReport::new(1.0e-10, 2.0e-10, 3.0e-10, 4.0e-9, 5.0e-10).unwrap(),
            ClimateRemapReport::new(1.0e-13, 2.0e-13, 3.0e-13, 4.0e-13, 5.0e-8, 100, 100).unwrap(),
            fields,
        )
    };

    for (fields, expected_role) in [
        (
            c2_fields(count, -6_000.0, 0.0, 0.0, 0.0),
            ClimateLayerRole::LowerAtmosphere,
        ),
        (
            c2_fields(count, 0.0, -4_000.0, 0.0, 0.0),
            ClimateLayerRole::UpperAtmosphere,
        ),
        (
            c2_fields(count, 0.0, 0.0, -100.0, 0.0),
            ClimateLayerRole::OceanMixedLayer,
        ),
    ] {
        assert!(matches!(
            build(fields),
            Err(GlobalCirculationValidationError::NonPositiveLayerDepth { role, .. })
                if role == expected_role
        ));
    }

    let invalid_fields = c2_fields(count, -6_000.0, 0.0, 0.0, 0.0);
    let invalid_checkpoint = ClimateCheckpoint::new(
        sekai::world::natural::NaturalQualityProfile::Draft,
        ClimateModelProfile::C2LayeredV1,
        ProductionIntegratorId::SplitExplicitRk3V1,
        [1; 32],
        [2; 32],
        global_circulation_model_fingerprint(ClimateModelProfile::C2LayeredV1),
        [3; 32],
        ClimateQuantizationId::DeterministicF64V1,
        24,
        invalid_fields.fingerprint(),
    )
    .unwrap();
    let mut wire = serde_json::to_value(c2_snapshot(&source)).unwrap();
    wire["fields"] = serde_json::to_value(invalid_fields).unwrap();
    wire["checkpoint"] = serde_json::to_value(invalid_checkpoint).unwrap();
    let error = serde_json::from_value::<GlobalCirculationSnapshot>(wire).unwrap_err();
    assert!(error.to_string().contains("layer depth is non-positive"));
}

#[test]
fn solve_report_rejects_unconverged_unbounded_or_over_budget_product_evidence() {
    assert!(ClimateSolveReport::new(2, 24, 192, 0, 1.0, 0.5, 0.4, 1_000_000).is_err());
    assert!(ClimateSolveReport::new(13, 156, 1_248, 0, 1.0, 0.2, 0.4, 1_000_000).is_err());
    assert!(ClimateSolveReport::new(2, 24, 192, 0, 1.0, 0.2, 0.4, 512 * 1024 * 1024 + 1).is_err());

    let report = ClimateSolveReport::new(2, 24, 192, 0, 1.0, 0.2, 0.4, 1_000_000).unwrap();
    let mut tampered = serde_json::to_value(report).unwrap();
    tampered["final_residual"] = serde_json::json!(0.5);
    assert!(serde_json::from_value::<ClimateSolveReport>(tampered).is_err());
}
