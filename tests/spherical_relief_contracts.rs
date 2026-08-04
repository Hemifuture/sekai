use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::{MantleGenerator, TectonicGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    ElevationField, GeologicSpec, LandOceanField, LandOceanKind, MantleFormationBias,
    ReliefValidationError, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    SphericalReliefSnapshot, SphericalReliefValidationError, TectonicSpec, WorldFormationPreset,
    ELEVATION_MAX_M, RELIEF_SCHEMA_V4, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::{SurfaceGeometryKind, SurfaceRef, SPATIAL_SCHEMA_V1};
use sekai::world::{
    Meters, RootSeed, SphericalSpaceSpec, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT,
};

fn surface(radius_m: f64) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
        target_cell_count: 42,
    })
    .unwrap()
}

fn stage_rng(name: &'static str, seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(name, 1, "sekai.contract-tests"),
    ))
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn upstream(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
) -> (
    sekai::world::natural::SphericalTectonicSnapshot,
    sekai::world::natural::SphericalMantleSnapshot,
) {
    let tectonic = TectonicGenerator::generate_spherical(
        surface,
        &TectonicSpec::default(),
        &formation(),
        &mut stage_rng("spherical-relief-contract-tectonics", 17),
    )
    .unwrap();
    let mantle = MantleGenerator::generate_spherical(
        surface,
        &GeologicSpec::default(),
        MantleFormationBias::Neutral,
        &mut stage_rng("spherical-relief-contract-mantle", 23),
    )
    .unwrap();
    (tectonic, mantle)
}

fn valid_snapshot(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
) -> SphericalReliefSnapshot {
    let count = surface.cells().len();
    let mut base = vec![-100.0; count];
    base[0] = 100.0;
    let zero = vec![0.0; count];
    let elevation = base.clone();
    let land_ocean = LandOceanField::from_kinds(
        elevation
            .iter()
            .map(|&value| LandOceanKind::classify(value, 0.0))
            .collect(),
    );
    SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::for_spherical(surface),
        0.0,
        ElevationField::from_values(base).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero).unwrap(),
        ElevationField::from_values(elevation).unwrap(),
        land_ocean,
    )
    .unwrap()
}

#[test]
fn spherical_relief_round_trips_with_exact_surface_and_upstream_identity() {
    let surface = surface(6_371_000.0);
    let (tectonic, mantle) = upstream(&surface);
    let snapshot = valid_snapshot(&surface);

    snapshot.validate().unwrap();
    snapshot
        .validate_against(&surface, &tectonic, &mantle)
        .unwrap();
    assert_eq!(snapshot.schema_version(), RELIEF_SCHEMA_V4);
    assert_eq!(snapshot.surface_ref(), SurfaceRef::for_spherical(&surface));
    assert_eq!(snapshot.cell_count(), surface.cells().len() as u32);
    assert_eq!(snapshot.sea_level_m(), 0.0);
    assert_eq!(
        snapshot.crust_base_elevation_m().len(),
        surface.cells().len()
    );
    assert_eq!(snapshot.tectonic_offset_m().len(), surface.cells().len());
    assert_eq!(snapshot.volcanic_offset_m().len(), surface.cells().len());
    assert_eq!(snapshot.regional_offset_m().len(), surface.cells().len());
    assert_eq!(snapshot.elevation_m().len(), surface.cells().len());
    assert_eq!(
        snapshot.land_ocean_kind(sekai::world::CellId::from_raw(0)),
        Some(LandOceanKind::Land)
    );

    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded["schema_version"], RELIEF_SCHEMA_V4);
    assert_eq!(encoded["surface_ref"]["geometry_kind"], "spherical_v1");
    assert!(encoded.get("cell_count").is_none());
    let decoded: SphericalReliefSnapshot = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);

    let mut unknown = encoded;
    unknown["projection"] = serde_json::json!("equirectangular");
    assert!(serde_json::from_value::<SphericalReliefSnapshot>(unknown).is_err());
}

#[test]
fn spherical_relief_rejects_wrong_schema_kind_lengths_ranges_and_component_identity() {
    let sphere = surface(6_371_000.0);
    let valid = valid_snapshot(&sphere);
    let count = sphere.cells().len();
    let encoded = serde_json::to_value(&valid).unwrap();

    let mut wrong_schema = encoded.clone();
    wrong_schema["schema_version"] = serde_json::json!(3);
    assert!(serde_json::from_value::<SphericalReliefSnapshot>(wrong_schema).is_err());

    let planar_ref = SurfaceRef::new(
        SurfaceGeometryKind::PlanarV1,
        SPATIAL_SCHEMA_V1,
        count as u32,
        sphere.edges().len() as u32,
        [7; 32],
    )
    .unwrap();
    let mut wrong_kind = encoded.clone();
    wrong_kind["surface_ref"] = serde_json::to_value(planar_ref).unwrap();
    assert!(serde_json::from_value::<SphericalReliefSnapshot>(wrong_kind).is_err());

    let mut short = encoded.clone();
    short["tectonic_offset_m"].as_array_mut().unwrap().pop();
    assert!(serde_json::from_value::<SphericalReliefSnapshot>(short).is_err());

    let mut out_of_range = encoded.clone();
    out_of_range["elevation_m"][3] = serde_json::json!(ELEVATION_MAX_M + 1.0);
    assert!(serde_json::from_value::<SphericalReliefSnapshot>(out_of_range).is_err());

    let mut inconsistent = encoded.clone();
    inconsistent["elevation_m"][3] = serde_json::json!(-99.0);
    let error = serde_json::from_value::<SphericalReliefSnapshot>(inconsistent)
        .unwrap_err()
        .to_string();
    assert!(error.contains("component sum"), "{error}");

    let bad = SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::for_spherical(&sphere),
        0.0,
        ElevationField::from_values(vec![0.0; count - 1]).unwrap(),
        ElevationField::from_values(vec![0.0; count]).unwrap(),
        ElevationField::from_values(vec![0.0; count]).unwrap(),
        ElevationField::from_values(vec![0.0; count]).unwrap(),
        ElevationField::from_values(vec![0.0; count]).unwrap(),
        LandOceanField::from_kinds(vec![LandOceanKind::Land; count]),
    );
    assert!(matches!(
        bad,
        Err(SphericalReliefValidationError::InvalidReliefFields(
            ReliefValidationError::FieldLengthMismatch { .. }
        ))
    ));
}

#[test]
fn spherical_relief_wire_bounds_every_dense_sequence_before_validation() {
    let sphere = surface(6_371_000.0);
    let encoded = serde_json::to_value(valid_snapshot(&sphere)).unwrap();
    for field in [
        "crust_base_elevation_m",
        "tectonic_offset_m",
        "volcanic_offset_m",
        "regional_offset_m",
        "elevation_m",
        "land_ocean_kind",
    ] {
        let mut oversized = encoded.clone();
        oversized[field] = serde_json::Value::Array(vec![
            serde_json::json!(0.0);
            MAX_SPHERICAL_CELL_COUNT as usize + 1
        ]);
        let error = serde_json::from_value::<SphericalReliefSnapshot>(oversized)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("at most {MAX_SPHERICAL_CELL_COUNT} elements")),
            "{field}: {error}"
        );
    }

    for (field, found) in [
        ("cell_count", MAX_SPHERICAL_CELL_COUNT + 1),
        ("edge_count", MAX_SPHERICAL_EDGE_COUNT + 1),
    ] {
        let mut oversized = encoded.clone();
        oversized["surface_ref"][field] = serde_json::json!(found);
        let error = serde_json::from_value::<SphericalReliefSnapshot>(oversized)
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeds spherical limit"), "{error}");
    }
}

#[test]
fn equal_count_different_surface_and_mismatched_upstreams_are_rejected() {
    let first = surface(6_371_000.0);
    let second = surface(6_000_000.0);
    assert_eq!(first.cells().len(), second.cells().len());
    assert_eq!(first.edges().len(), second.edges().len());
    let first_snapshot = valid_snapshot(&first);
    let (first_tectonic, first_mantle) = upstream(&first);
    let (second_tectonic, second_mantle) = upstream(&second);

    assert!(matches!(
        first_snapshot.validate_against(&second, &second_tectonic, &second_mantle),
        Err(SphericalReliefValidationError::SurfaceMismatch { .. })
    ));
    assert!(first_snapshot
        .validate_against(&first, &second_tectonic, &first_mantle)
        .is_err());
    assert!(first_snapshot
        .validate_against(&first, &first_tectonic, &second_mantle)
        .is_err());
}
