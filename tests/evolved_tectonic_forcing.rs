use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::TectonicGenerator;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    ResolvedWorldFormation, ResolvedWorldFormationPreset, SphericalTectonicForcingState,
    TectonicSpec, WorldFormationPreset, NO_OROGENY_AGE_SENTINEL_MYR,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

#[test]
fn forcing_contract_keeps_rates_non_negative_and_event_history_explicit() {
    let forcing = SphericalTectonicForcingState::new(
        vec![0.8, 0.0, 0.0],
        vec![0.0, 1.2, 0.0],
        vec![0.3, 0.0, 0.0],
        vec![0.0, 125_000.0, 2_000_000.0],
        vec![0.0, 18.0, NO_OROGENY_AGE_SENTINEL_MYR],
    )
    .unwrap();

    assert_eq!(forcing.len(), 3);
    assert_eq!(forcing.event_age_myr(), &[0.0, 18.0, -1.0]);
    assert!(SphericalTectonicForcingState::new(
        vec![-0.1],
        vec![0.0],
        vec![0.0],
        vec![0.0],
        vec![0.0],
    )
    .is_err());
}

#[test]
fn frozen_v4_snapshot_does_not_pretend_to_publish_present_day_forcing() {
    let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 42,
    })
    .unwrap();
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(42),
        StageIdentity::new("natural.spherical-tectonics", 5, "sekai.core"),
    ));
    let legacy = TectonicGenerator::generate_spherical(
        &surface,
        &TectonicSpec::default(),
        &formation,
        &mut rng,
    )
    .unwrap();
    let json = serde_json::to_value(legacy).unwrap();

    assert!(json.get("forcing").is_none());
    assert!(!json.to_string().contains("uplift_rate_mm_per_year"));
}
