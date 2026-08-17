use sekai::engine::{BuildEngine, BuildOutcome, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    evaluate_spherical_foundation_quality, spherical_natural_foundation_graph,
    AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact, HydroErosionSpecArtifact,
    ReliefSpecArtifact, ResolvedWorldFormationArtifact, RulePackSetArtifact,
    SphericalHydroErosionArtifact, SphericalReliefArtifact, SphericalTectonicArtifact,
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
    artifacts
        .insert(ReliefSpecArtifact::new(ReliefSpec::default()))
        .unwrap();
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

fn surface_ref(outcome: &BuildOutcome) -> sekai::world::spatial::SurfaceRef {
    sekai::world::spatial::SurfaceRef::for_spherical(
        outcome
            .artifacts
            .get::<SphericalSurfaceArtifact>()
            .unwrap()
            .snapshot(),
    )
}
