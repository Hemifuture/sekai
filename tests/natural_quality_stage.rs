use sekai::engine::{
    Artifact, BuildEngine, BuildOutcome, ExternalArtifacts, MemoryStageCache, Stage, StageInputs,
};
use sekai::generators::natural::{
    evaluate_spherical_foundation_quality, spherical_natural_foundation_graph,
    AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact, HydroErosionSpecArtifact,
    NaturalQualityArtifact, ReliefSpecArtifact, ResolvedWorldFormationArtifact,
    RulePackSetArtifact, SphericalHydroErosionArtifact, SphericalNaturalQualityStage,
    SphericalNaturalQualityStageInputs, SphericalReliefArtifact, SphericalTectonicArtifact,
    TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{SphericalSpaceArtifact, SphericalSurfaceArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::view::{PaletteId, SphericalFieldDisplayState};
use sekai::world::natural::{
    ClimateSpec, GeologicSpec, HydroErosionSpec, NaturalQualityReport, ReliefSpec, TectonicSpec,
    WorldFormationSpec,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

fn external() -> ExternalArtifacts {
    external_with_relief(ReliefSpec::default())
}

fn external_with_relief(relief: ReliefSpec) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(SphericalSpaceArtifact::new(SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        }))
        .unwrap();
    artifacts
        .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
        .unwrap();
    artifacts
        .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
        .unwrap();
    artifacts
        .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
        .unwrap();
    artifacts
        .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
        .unwrap();
    artifacts.insert(ReliefSpecArtifact::new(relief)).unwrap();
    artifacts
        .insert(WorldFormationSpecArtifact::new(
            WorldFormationSpec::default(),
        ))
        .unwrap();
    artifacts
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    artifacts
        .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
        .unwrap();
    artifacts
}

fn build_fixture() -> BuildOutcome {
    BuildEngine::new(spherical_natural_foundation_graph().unwrap())
        .build(RootSeed::new(42), external(), &mut MemoryStageCache::new())
        .unwrap()
}

fn evaluate(outcome: &BuildOutcome) -> NaturalQualityReport {
    let surface = outcome.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
    let formation = outcome
        .artifacts
        .get::<ResolvedWorldFormationArtifact>()
        .unwrap();
    let relief_spec = outcome.artifacts.get::<ReliefSpecArtifact>().unwrap();
    let tectonic = outcome
        .artifacts
        .get::<SphericalTectonicArtifact>()
        .unwrap();
    let relief = outcome.artifacts.get::<SphericalReliefArtifact>().unwrap();
    let hydro = outcome
        .artifacts
        .get::<SphericalHydroErosionArtifact>()
        .unwrap();

    evaluate_spherical_foundation_quality(
        surface.snapshot(),
        formation.formation(),
        relief_spec.spec(),
        tectonic.snapshot(),
        relief.snapshot(),
        hydro.snapshot(),
    )
    .unwrap()
}

#[test]
fn evaluator_reports_the_exact_p0_inventory_deterministically() {
    let outcome = build_fixture();
    let first = evaluate(&outcome);
    let second = evaluate(&outcome);

    let actual_ids = first
        .metrics()
        .iter()
        .map(|metric| {
            format!(
                "{}.{}.v{}",
                metric.id().namespace(),
                metric.id().name(),
                metric.id().version()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual_ids,
        [
            "hydrology.outlet-area-coverage.v1",
            "hydrology.river-segment-count.v1",
            "quality.non-finite-value-count.v1",
            "relief.actual-land-area-fraction.v1",
            "relief.land-crust-jaccard.v1",
            "relief.oceanic-emergent-area-fraction.v1",
            "relief.requested-land-area-fraction.v1",
            "tectonics.continental-area-fraction.v1",
            "tectonics.continental-retention.v1",
        ]
    );
    assert_eq!(first.surface_ref(), surface_ref(&outcome));

    let first_json = serde_json::to_vec(&first).unwrap();
    assert_eq!(first_json, serde_json::to_vec(&second).unwrap());
    let rendered = String::from_utf8(first_json).unwrap();
    for forbidden in ["renderer", "palette", "display"] {
        assert!(!rendered.contains(forbidden), "found {forbidden} in report");
    }
}

#[test]
fn evaluator_hash_is_independent_of_display_palette_state() {
    let outcome = build_fixture();
    let before = serde_json::to_vec(&evaluate(&outcome)).unwrap();

    let mut display = SphericalFieldDisplayState::default();
    display.set_palette_override(Some(PaletteId::Diverging));
    assert_eq!(display.palette_override(), Some(PaletteId::Diverging));

    let after = serde_json::to_vec(&evaluate(&outcome)).unwrap();
    assert_eq!(blake3::hash(&before), blake3::hash(&after));
}

#[test]
fn quality_stage_declares_exact_dependencies_and_artifact_contract() {
    let report = evaluate(&build_fixture());
    let artifact = NaturalQualityArtifact::new(report.clone());
    artifact.validate().unwrap();
    assert_eq!(
        NaturalQualityArtifact::KEY.as_str(),
        "world.natural-quality"
    );
    assert_eq!(artifact.report(), &report);

    let encoded = serde_json::to_value(&artifact).unwrap();
    let decoded: NaturalQualityArtifact = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, artifact);
    let mut invalid_report = encoded.clone();
    invalid_report["report"]["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<NaturalQualityArtifact>(invalid_report).is_err());
    let mut unknown = encoded;
    unknown["renderer"] = serde_json::json!("globe");
    assert!(serde_json::from_value::<NaturalQualityArtifact>(unknown).is_err());

    assert_eq!(
        SphericalNaturalQualityStageInputs::dependencies(),
        &[
            ResolvedWorldFormationArtifact::KEY,
            SphericalHydroErosionArtifact::KEY,
            SphericalReliefArtifact::KEY,
            ReliefSpecArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
            SphericalTectonicArtifact::KEY,
        ]
    );
    let stage = SphericalNaturalQualityStage;
    assert_eq!(stage.id().as_str(), "natural.spherical-quality");
    assert_eq!(stage.version(), 1);
    assert_eq!(stage.namespace(), "sekai.core");
}

#[test]
fn quality_stage_cache_tracks_science_but_ignores_palette_state() {
    let engine = BuildEngine::new(spherical_natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::with_max_entries(128).unwrap();
    let first = engine
        .build(RootSeed::new(42), external(), &mut cache)
        .unwrap();
    assert!(!quality_stage_report(&first).cache_hit());

    let mut display = SphericalFieldDisplayState::default();
    display.set_palette_override(Some(PaletteId::Categorical));
    let repeated = engine
        .build(RootSeed::new(42), external(), &mut cache)
        .unwrap();
    assert_eq!(display.palette_override(), Some(PaletteId::Categorical));
    assert!(quality_stage_report(&repeated).cache_hit());

    let changed = engine
        .build(
            RootSeed::new(42),
            external_with_relief(ReliefSpec {
                target_land_fraction: 0.45,
                ..ReliefSpec::default()
            }),
            &mut cache,
        )
        .unwrap();
    assert!(!quality_stage_report(&changed).cache_hit());
    assert_ne!(
        first.artifacts.hash::<NaturalQualityArtifact>().unwrap(),
        changed.artifacts.hash::<NaturalQualityArtifact>().unwrap()
    );
}

fn quality_stage_report(outcome: &BuildOutcome) -> &sekai::engine::StageReport {
    outcome
        .report
        .stages()
        .iter()
        .find(|stage| stage.stage_id() == "natural.spherical-quality")
        .expect("formal graph must publish the quality stage")
}

fn surface_ref(outcome: &BuildOutcome) -> sekai::world::spatial::SurfaceRef {
    sekai::world::spatial::SurfaceRef::for_spherical(
        outcome
            .artifacts
            .get::<SphericalSurfaceArtifact>()
            .unwrap()
            .snapshot(),
    )
}
