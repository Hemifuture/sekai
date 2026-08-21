use sekai::world::natural::{
    NaturalQualityProfile, NaturalResolutionPlan, NATURAL_RESOLUTION_PLAN_SCHEMA_V1,
};
use sekai::world::{
    Meters, SphericalSpaceSpec, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_TARGET_CELL_COUNT,
};

const EARTH_RADIUS_M: f64 = 6_371_000.0;

fn space(target_cell_count: u32) -> SphericalSpaceSpec {
    SphericalSpaceSpec {
        radius: Meters::new(EARTH_RADIUS_M).unwrap(),
        target_cell_count,
    }
}

#[test]
fn profiles_resolve_the_exact_locked_product_counts() {
    for (
        profile,
        authoritative_target,
        authoritative_resolved,
        control_target,
        control_resolved,
        climate_face_resolution,
    ) in [
        (
            NaturalQualityProfile::Draft,
            20_000,
            20_252,
            4_842,
            4_842,
            24,
        ),
        (
            NaturalQualityProfile::Standard,
            80_000,
            79_212,
            20_000,
            20_252,
            32,
        ),
        (
            NaturalQualityProfile::High,
            200_000,
            198_812,
            20_000,
            20_252,
            48,
        ),
    ] {
        assert_eq!(
            profile.authoritative_target_cell_count(),
            authoritative_target
        );
        assert_eq!(profile.tectonic_control_target_cell_count(), control_target);
        assert_eq!(profile.climate_face_resolution(), climate_face_resolution);

        let plan = profile.resolve(&space(authoritative_target)).unwrap();
        plan.validate().unwrap();
        assert_eq!(plan.schema_version(), NATURAL_RESOLUTION_PLAN_SCHEMA_V1);
        assert_eq!(plan.profile(), profile);
        assert_eq!(plan.radius().get(), EARTH_RADIUS_M);
        assert_eq!(plan.authoritative_target_cell_count(), authoritative_target);
        assert_eq!(
            plan.authoritative_resolved_cell_count(),
            authoritative_resolved
        );
        assert_eq!(plan.tectonic_control_target_cell_count(), control_target);
        assert_eq!(
            plan.tectonic_control_resolved_cell_count(),
            control_resolved
        );
        assert_eq!(plan.climate_face_resolution(), climate_face_resolution);

        let authoritative = plan.authoritative_space_spec();
        assert_eq!(authoritative.radius.get(), EARTH_RADIUS_M);
        assert_eq!(authoritative.target_cell_count, authoritative_target);
        assert_eq!(authoritative.resolved_cell_count(), authoritative_resolved);
        let control = plan.tectonic_control_space_spec();
        assert_eq!(control.radius.get(), EARTH_RADIUS_M);
        assert_eq!(control.target_cell_count, control_target);
        assert_eq!(control.resolved_cell_count(), control_resolved);
    }
}

#[test]
fn profile_resolution_rejects_a_mismatched_authoritative_target() {
    for (profile, wrong_target) in [
        (NaturalQualityProfile::Draft, 80_000),
        (NaturalQualityProfile::Standard, 20_000),
        (NaturalQualityProfile::High, 198_812),
    ] {
        let error = profile.resolve(&space(wrong_target)).unwrap_err();
        assert!(error.to_string().contains("target"));
        assert!(error.to_string().contains(&wrong_target.to_string()));
    }
}

#[test]
fn high_request_limit_is_distinct_from_the_maximum_resolved_allocation() {
    assert_eq!(MAX_SPHERICAL_TARGET_CELL_COUNT, 200_000);
    assert_eq!(MAX_SPHERICAL_CELL_COUNT, 198_812);

    let high = space(MAX_SPHERICAL_TARGET_CELL_COUNT);
    high.validate().unwrap();
    assert_eq!(high.resolved_cell_count(), MAX_SPHERICAL_CELL_COUNT);

    let mut excessive = high;
    excessive.target_cell_count = MAX_SPHERICAL_TARGET_CELL_COUNT + 1;
    assert!(excessive.validate().is_err());
}

#[test]
fn resolution_plan_round_trip_is_exact_and_strict() {
    let plan = NaturalQualityProfile::Standard
        .resolve(&space(80_000))
        .unwrap();
    let json = serde_json::to_string(&plan).unwrap();
    assert!(json.contains("\"profile\":\"standard\""));
    let decoded: NaturalResolutionPlan = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, plan);
    assert_eq!(serde_json::to_string(&decoded).unwrap(), json);

    let value = serde_json::to_value(&plan).unwrap();

    let mut wrong_schema = value.clone();
    wrong_schema["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<NaturalResolutionPlan>(wrong_schema).is_err());

    let mut wrong_resolved = value.clone();
    wrong_resolved["authoritative_resolved_cell_count"] = serde_json::json!(20_252);
    assert!(serde_json::from_value::<NaturalResolutionPlan>(wrong_resolved).is_err());

    let mut wrong_control = value.clone();
    wrong_control["tectonic_control_target_cell_count"] = serde_json::json!(4_842);
    assert!(serde_json::from_value::<NaturalResolutionPlan>(wrong_control).is_err());

    let mut unknown = value;
    unknown["custom_resolution"] = serde_json::json!(true);
    assert!(serde_json::from_value::<NaturalResolutionPlan>(unknown).is_err());
}
