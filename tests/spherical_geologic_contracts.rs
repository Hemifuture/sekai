use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::{MantleGenerator, TectonicGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BedrockKind, BedrockKindField, CrustKind, ElevationField, GeologicSpec,
    GeologicValidationError, LandOceanField, LandOceanKind, MantleFormationBias,
    ResolvedWorldFormation, ResolvedWorldFormationPreset, SphericalGeologicSnapshot,
    SphericalGeologicValidationError, SphericalMantleSnapshot, SphericalReliefSnapshot,
    SphericalTectonicSnapshot, TectonicSpec, WorldFormationPreset, GEOLOGIC_SNAPSHOT_SCHEMA_V2,
    RELIEF_SCHEMA_V4, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::{SurfaceGeometryKind, SurfaceRef, SPATIAL_SCHEMA_V1};
use sekai::world::{
    CellId, Meters, RootSeed, SphericalSpaceSpec, MAX_SPHERICAL_CELL_COUNT,
    MAX_SPHERICAL_EDGE_COUNT,
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
    SphericalTectonicSnapshot,
    SphericalMantleSnapshot,
    SphericalReliefSnapshot,
) {
    let tectonic = TectonicGenerator::generate_spherical(
        surface,
        &TectonicSpec::default(),
        &formation(),
        &mut stage_rng("spherical-geology-contract-tectonics", 31),
    )
    .unwrap();
    let mantle = MantleGenerator::generate_spherical(
        surface,
        &GeologicSpec::default(),
        MantleFormationBias::Neutral,
        &mut stage_rng("spherical-geology-contract-mantle", 37),
    )
    .unwrap();
    let count = surface.cells().len();
    let base = ElevationField::from_values(vec![100.0; count]).unwrap();
    let zero = ElevationField::from_values(vec![0.0; count]).unwrap();
    let relief = SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::for_spherical(surface),
        0.0,
        base.clone(),
        zero.clone(),
        zero.clone(),
        zero,
        base,
        LandOceanField::from_kinds(vec![LandOceanKind::Land; count]),
    )
    .unwrap();
    (tectonic, mantle, relief)
}

fn valid_snapshot(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
) -> SphericalGeologicSnapshot {
    let count = surface.cells().len();
    let bedrock = (0..count)
        .map(
            |index| match tectonic.crust_kind(CellId::from_raw(index as u32)).unwrap() {
                CrustKind::Oceanic => BedrockKind::OceanicMafic,
                CrustKind::Continental => BedrockKind::ContinentalCrystalline,
            },
        )
        .collect();
    SphericalGeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        BedrockKindField::from_kinds(bedrock),
        vec![0.2; count],
        vec![0.8; count],
        vec![0.3; count],
        vec![0.4; count],
        vec![0.5; count],
        vec![0.6; count],
    )
    .unwrap()
}

#[test]
fn spherical_geology_round_trips_with_exact_surface_and_upstream_identity() {
    let sphere = surface(6_371_000.0);
    let (tectonic, mantle, relief) = upstream(&sphere);
    let snapshot = valid_snapshot(&sphere, &tectonic);

    snapshot.validate().unwrap();
    snapshot
        .validate_against(&sphere, &tectonic, &mantle, &relief)
        .unwrap();
    assert_eq!(snapshot.schema_version(), GEOLOGIC_SNAPSHOT_SCHEMA_V2);
    assert_eq!(snapshot.surface_ref(), SurfaceRef::for_spherical(&sphere));
    assert_eq!(snapshot.cell_count(), sphere.cells().len() as u32);
    assert_eq!(snapshot.bedrock_kinds().len(), sphere.cells().len());
    assert_eq!(snapshot.fracture_intensity().len(), sphere.cells().len());
    assert_eq!(snapshot.erosion_resistance().len(), sphere.cells().len());
    assert_eq!(snapshot.relative_permeability().len(), sphere.cells().len());
    assert_eq!(
        snapshot.metallic_mineral_potential().len(),
        sphere.cells().len()
    );
    assert_eq!(snapshot.geothermal_potential().len(), sphere.cells().len());
    assert_eq!(
        snapshot.sedimentary_basin_potential().len(),
        sphere.cells().len()
    );

    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded["schema_version"], GEOLOGIC_SNAPSHOT_SCHEMA_V2);
    assert_eq!(encoded["surface_ref"]["geometry_kind"], "spherical_v1");
    assert!(encoded.get("cell_count").is_none());
    let decoded: SphericalGeologicSnapshot = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);

    let mut unknown = encoded;
    unknown["projection"] = serde_json::json!("equirectangular");
    assert!(serde_json::from_value::<SphericalGeologicSnapshot>(unknown).is_err());
}

#[test]
fn spherical_geology_rejects_wrong_schema_kind_fields_and_crust_incompatibility() {
    let sphere = surface(6_371_000.0);
    let (tectonic, mantle, relief) = upstream(&sphere);
    let valid = valid_snapshot(&sphere, &tectonic);
    let count = sphere.cells().len();
    let encoded = serde_json::to_value(&valid).unwrap();

    let mut wrong_schema = encoded.clone();
    wrong_schema["schema_version"] = serde_json::json!(1);
    assert!(serde_json::from_value::<SphericalGeologicSnapshot>(wrong_schema).is_err());

    let planar_ref = SurfaceRef::new(
        SurfaceGeometryKind::PlanarV1,
        SPATIAL_SCHEMA_V1,
        count as u32,
        sphere.edges().len() as u32,
        [5; 32],
    )
    .unwrap();
    let mut wrong_kind = encoded.clone();
    wrong_kind["surface_ref"] = serde_json::to_value(planar_ref).unwrap();
    assert!(serde_json::from_value::<SphericalGeologicSnapshot>(wrong_kind).is_err());

    let mut short = encoded.clone();
    short["fracture_intensity"].as_array_mut().unwrap().pop();
    assert!(serde_json::from_value::<SphericalGeologicSnapshot>(short).is_err());

    let mut bad_value = encoded.clone();
    bad_value["geothermal_potential"][2] = serde_json::json!(1.01);
    assert!(serde_json::from_value::<SphericalGeologicSnapshot>(bad_value).is_err());

    let mut bad_kind = encoded;
    bad_kind["bedrock_kinds"][2] = serde_json::json!(99);
    assert!(serde_json::from_value::<SphericalGeologicSnapshot>(bad_kind).is_err());

    let incompatible = tectonic
        .crust_kind(CellId::from_raw(0))
        .map(|crust| match crust {
            CrustKind::Oceanic => BedrockKind::ContinentalCrystalline,
            CrustKind::Continental => BedrockKind::OceanicMafic,
        })
        .unwrap();
    let mut bedrock = valid.bedrock_kinds().raw_values().to_vec();
    bedrock[0] = incompatible.raw();
    let forged = SphericalGeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::for_spherical(&sphere),
        BedrockKindField::new(bedrock).unwrap(),
        valid.fracture_intensity().to_vec(),
        valid.erosion_resistance().to_vec(),
        valid.relative_permeability().to_vec(),
        valid.metallic_mineral_potential().to_vec(),
        valid.geothermal_potential().to_vec(),
        valid.sedimentary_basin_potential().to_vec(),
    )
    .unwrap();
    assert!(matches!(
        forged.validate_against(&sphere, &tectonic, &mantle, &relief),
        Err(SphericalGeologicValidationError::InvalidGeologicFields(
            GeologicValidationError::BedrockCrustMismatch { .. }
        ))
    ));
}

#[test]
fn spherical_geology_wire_bounds_every_dense_sequence_before_validation() {
    let sphere = surface(6_371_000.0);
    let (tectonic, _, _) = upstream(&sphere);
    let encoded = serde_json::to_value(valid_snapshot(&sphere, &tectonic)).unwrap();
    for field in [
        "bedrock_kinds",
        "fracture_intensity",
        "erosion_resistance",
        "relative_permeability",
        "metallic_mineral_potential",
        "geothermal_potential",
        "sedimentary_basin_potential",
    ] {
        let mut oversized = encoded.clone();
        oversized[field] = serde_json::Value::Array(vec![
            serde_json::json!(0.0);
            MAX_SPHERICAL_CELL_COUNT as usize + 1
        ]);
        let error = serde_json::from_value::<SphericalGeologicSnapshot>(oversized)
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
        let error = serde_json::from_value::<SphericalGeologicSnapshot>(oversized)
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
    let (first_tectonic, first_mantle, first_relief) = upstream(&first);
    let (second_tectonic, second_mantle, second_relief) = upstream(&second);
    let snapshot = valid_snapshot(&first, &first_tectonic);

    assert!(matches!(
        snapshot.validate_against(&second, &second_tectonic, &second_mantle, &second_relief),
        Err(SphericalGeologicValidationError::SurfaceMismatch { .. })
    ));
    assert!(snapshot
        .validate_against(&first, &second_tectonic, &first_mantle, &first_relief)
        .is_err());
    assert!(snapshot
        .validate_against(&first, &first_tectonic, &second_mantle, &first_relief)
        .is_err());
    assert!(snapshot
        .validate_against(&first, &first_tectonic, &first_mantle, &second_relief)
        .is_err());
}
