use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    ClimateBudgetReport, ClimateCapabilityAvailability, ClimateCapabilityId, ClimateCapabilitySet,
    ClimateCheckpoint, ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile,
    ClimateQuantizationId, ClimateRemapReport, ClimateSolveReport, GlobalCirculationFields,
    GlobalCirculationSnapshot, MonthlyScalarField, MonthlyVector3Field, ProductionIntegratorId,
    GLOBAL_CIRCULATION_SCHEMA_V1,
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
    let layout = ClimateLayerLayout::for_profile(profile);
    ClimateCheckpoint::new(
        profile,
        ProductionIntegratorId::SplitExplicitRk3V1,
        [1; 32],
        [2; 32],
        layout.fingerprint(),
        [3; 32],
        ClimateQuantizationId::DeterministicF64V1,
        24,
        [4; 32],
    )
    .unwrap()
}

fn c2_snapshot(surface: &SphericalSurfaceSnapshot) -> GlobalCirculationSnapshot {
    let count = surface.cells().len();
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let fields = GlobalCirculationFields::new_c2(
        vectors(count, [0.0; 3]),
        vectors(count, [0.0; 3]),
        vectors(count, [0.0; 3]),
        vectors(count, [0.0; 3]),
        scalar(count, 12.0),
        scalar(count, 15.0),
        scalar(count, 8.0),
        scalar(count, 600.0),
        scalar(count, 0.008),
        scalar(count, 2.0),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 4.0),
    )
    .unwrap();
    GlobalCirculationSnapshot::new(
        GLOBAL_CIRCULATION_SCHEMA_V1,
        SurfaceRef::for_spherical(surface),
        layout,
        ProductionIntegratorId::SplitExplicitRk3V1,
        ClimateCapabilitySet::for_profile(ClimateModelProfile::C2LayeredV1),
        checkpoint(ClimateModelProfile::C2LayeredV1),
        ClimateSolveReport::new(2, 288, 2_304, 0, 1.0, 1.0e-5, 0.41, 1_000_000).unwrap(),
        ClimateBudgetReport::new(1.0e-10, 2.0e-10, 3.0e-10, 4.0e-9, 5.0e-10).unwrap(),
        ClimateRemapReport::new(1.0e-13, 2.0e-13, 3.0e-13, 4.0e-13, 100, 100).unwrap(),
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
            && layer.exchange_time_s() > 0.0
    }));

    let bytes = serde_json::to_vec(&c2).unwrap();
    let decoded: ClimateLayerLayout = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, c2);
    let mut tampered = serde_json::to_value(c2).unwrap();
    tampered["layers"][0]["reference_thickness_m"] = serde_json::json!(9_999.0);
    assert!(serde_json::from_value::<ClimateLayerLayout>(tampered).is_err());
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
    tampered["completed_months"] = serde_json::json!(36);
    assert!(serde_json::from_value::<ClimateCheckpoint>(tampered).is_err());
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
    let radial: GlobalCirculationSnapshot = serde_json::from_value(radial).unwrap();
    assert!(radial.validate_against(&source).is_err());
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
