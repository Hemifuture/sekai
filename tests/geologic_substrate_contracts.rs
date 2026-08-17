use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::EvolvedTectonicGenerator;
use sekai::generators::spatial::{GeodesicVoronoiBuilder, ProfileSurfaceBuilder};
use sekai::world::natural::{
    BedrockKind, BedrockKindField, CrustKind, CrustKindField, GeologicSubstrateSnapshot,
    NaturalQualityProfile, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    SedimentSourceKind, SedimentSourceKindField, SphericalMantleSnapshot, TectonicSpec,
    WorldFormationPreset, GEOLOGIC_SUBSTRATE_SCHEMA_V1, MANTLE_SNAPSHOT_SCHEMA_V2,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::SurfaceRef;
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec, MAX_SPHERICAL_CELL_COUNT};

fn surface() -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 42,
    })
    .unwrap()
}

fn valid_snapshot() -> GeologicSubstrateSnapshot {
    let surface = surface();
    let count = surface.cells().len();
    let mantle = SphericalMantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::for_spherical(&surface),
        Vec::new(),
        vec![65.0; count],
        vec![0.0; count],
    )
    .unwrap();
    GeologicSubstrateSnapshot::new(
        GEOLOGIC_SUBSTRATE_SCHEMA_V1,
        SurfaceRef::for_spherical(&surface),
        mantle,
        CrustKindField::from_kinds(vec![CrustKind::Continental; count]),
        vec![35.0; count],
        vec![-1.0; count],
        vec![2_800.0; count],
        BedrockKindField::from_kinds(vec![BedrockKind::ContinentalCrystalline; count]),
        vec![0.25; count],
        vec![0.30; count],
        vec![0.20; count],
        SedimentSourceKindField::from_kinds(vec![SedimentSourceKind::Felsic; count]),
    )
    .unwrap()
}

#[test]
fn substrate_round_trips_with_strict_surface_bound_fields() {
    let surface = surface();
    let snapshot = valid_snapshot();
    snapshot.validate().unwrap();
    snapshot.validate_against_surface(&surface).unwrap();
    assert_eq!(snapshot.schema_version(), GEOLOGIC_SUBSTRATE_SCHEMA_V1);
    assert_eq!(snapshot.surface_ref(), SurfaceRef::for_spherical(&surface));
    assert_eq!(snapshot.cell_count() as usize, surface.cells().len());
    assert_eq!(snapshot.crust_kind(0), Some(CrustKind::Continental));
    assert_eq!(
        snapshot.bedrock_kind(0),
        Some(BedrockKind::ContinentalCrystalline)
    );
    assert_eq!(
        snapshot.sediment_source(0),
        Some(SedimentSourceKind::Felsic)
    );
    assert_eq!(snapshot.crust_density_kg_m3()[0], 2_800.0);
    assert_eq!(snapshot.heat_flow_mw_m2()[0], 65.0);

    let encoded = serde_json::to_value(&snapshot).unwrap();
    let decoded: GeologicSubstrateSnapshot = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);

    let mut unknown = encoded;
    unknown["visual_palette"] = serde_json::json!("terrain");
    assert!(serde_json::from_value::<GeologicSubstrateSnapshot>(unknown).is_err());
}

#[test]
fn sediment_sources_have_stable_strict_raw_codes() {
    let kinds = [
        SedimentSourceKind::Felsic,
        SedimentSourceKind::Mafic,
        SedimentSourceKind::Volcaniclastic,
        SedimentSourceKind::Sedimentary,
        SedimentSourceKind::Metamorphic,
    ];
    let field = SedimentSourceKindField::from_kinds(kinds.to_vec());
    assert_eq!(field.raw_values(), &[0, 1, 2, 3, 4]);
    for (raw, kind) in kinds.into_iter().enumerate() {
        assert_eq!(SedimentSourceKind::try_from_raw(raw as u32).unwrap(), kind);
        assert_eq!(field.get(raw), Some(kind));
    }
    assert!(SedimentSourceKind::try_from_raw(5).is_err());
    assert!(SedimentSourceKindField::from_raw(vec![0, 5]).is_err());
}

#[test]
fn substrate_rejects_bad_schema_lengths_density_and_sentinels() {
    let encoded = serde_json::to_value(valid_snapshot()).unwrap();

    let mut schema = encoded.clone();
    schema["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<GeologicSubstrateSnapshot>(schema).is_err());

    let mut short = encoded.clone();
    short["crust_density_kg_m3"].as_array_mut().unwrap().pop();
    assert!(serde_json::from_value::<GeologicSubstrateSnapshot>(short).is_err());

    for density in [2_499.0, 3_201.0, f32::NAN] {
        let mut invalid = encoded.clone();
        invalid["crust_density_kg_m3"][0] = serde_json::json!(density);
        assert!(serde_json::from_value::<GeologicSubstrateSnapshot>(invalid).is_err());
    }

    let mut continental_age = encoded.clone();
    continental_age["ocean_age_myr"][0] = serde_json::json!(10.0);
    assert!(serde_json::from_value::<GeologicSubstrateSnapshot>(continental_age).is_err());
}

#[test]
fn substrate_wire_bounds_dense_allocations_before_validation() {
    let mut encoded = serde_json::to_value(valid_snapshot()).unwrap();
    encoded["crust_density_kg_m3"] =
        serde_json::Value::Array(vec![
            serde_json::json!(2_800.0);
            MAX_SPHERICAL_CELL_COUNT as usize + 1
        ]);
    let error = serde_json::from_value::<GeologicSubstrateSnapshot>(encoded)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&format!("at most {MAX_SPHERICAL_CELL_COUNT} elements")),
        "{error}"
    );
}

#[test]
fn substrate_cross_validation_recomputes_every_copied_tectonic_fact() {
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(42),
        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
    ));
    let evolved =
        EvolvedTectonicGenerator::generate(&bundle, &TectonicSpec::default(), &formation, &mut rng)
            .unwrap();
    let count = surface.cells().len();
    let mantle = SphericalMantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        Vec::new(),
        vec![65.0; count],
        vec![0.0; count],
    )
    .unwrap();
    let bedrock = evolved
        .compatibility()
        .crust_kinds()
        .raw_values()
        .iter()
        .map(|&raw| match CrustKind::try_from_raw(raw).unwrap() {
            CrustKind::Continental => BedrockKind::ContinentalCrystalline,
            CrustKind::Oceanic => BedrockKind::OceanicMafic,
        })
        .collect::<Vec<_>>();
    let density = (0..count)
        .map(|index| {
            sekai::world::natural::effective_crust_density_kg_m3(
                evolved.material().continental_volume_m3()[index],
                evolved.material().oceanic_volume_m3()[index],
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let substrate = GeologicSubstrateSnapshot::new(
        GEOLOGIC_SUBSTRATE_SCHEMA_V1,
        SurfaceRef::for_spherical(surface),
        mantle,
        evolved.compatibility().crust_kinds().clone(),
        evolved.compatibility().crust_thickness_km().to_vec(),
        evolved.compatibility().crust_age_myr().to_vec(),
        density,
        BedrockKindField::from_kinds(bedrock.clone()),
        vec![0.25; count],
        vec![0.30; count],
        vec![0.20; count],
        SedimentSourceKindField::from_kinds(
            bedrock
                .into_iter()
                .map(sekai::world::natural::sediment_source_for_bedrock)
                .collect(),
        ),
    )
    .unwrap();
    substrate.validate_against(surface, &evolved).unwrap();

    let mut encoded = serde_json::to_value(substrate).unwrap();
    encoded["crust_thickness_km"][0] =
        serde_json::json!(evolved.compatibility().crust_thickness_km()[0] + 0.25);
    let changed: GeologicSubstrateSnapshot = serde_json::from_value(encoded).unwrap();
    assert!(changed.validate_against(surface, &evolved).is_err());
}
