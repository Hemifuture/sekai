use std::collections::BTreeSet;

use sekai::engine::{
    Artifact, BuildEngine, BuildOutcome, BuildReport, ExternalArtifacts, MemoryStageCache,
};
use sekai::generators::natural::{
    spherical_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
    GeologicSpecArtifact, HydroErosionSpecArtifact, ResolvedWorldFormationArtifact,
    RulePackSetArtifact, SphericalGeologicArtifact, SphericalHydroErosionArtifact,
    SphericalMantleArtifact, SphericalPreliminaryClimateArtifact, SphericalReliefArtifact,
    SphericalTectonicArtifact, TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{
    PlanarSpaceArtifact, SphericalSpaceArtifact, SphericalSurfaceArtifact,
};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    ClimateSpec, GeologicSpec, HydroErosionSpec, TectonicSpec, WorldFormationPreset,
    WorldFormationSpec,
};
use sekai::world::spatial::SurfaceRef;
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed, SphericalSpaceSpec};

const ALL_STAGE_IDS: [&str; 16] = [
    "natural.resolve-climate-rules",
    "natural.project-climate-input",
    "natural.resolve-geologic-rules",
    "natural.project-geologic-input",
    "natural.resolve-hydro-erosion-rules",
    "natural.project-hydro-erosion-input",
    "natural.resolve-tectonic-rules",
    "natural.project-tectonic-input",
    "natural.resolve-world-formation",
    "spatial.spherical-voronoi",
    "natural.spherical-mantle",
    "natural.spherical-tectonics",
    "natural.spherical-relief",
    "natural.spherical-geology",
    "natural.spherical-preliminary-climate",
    "natural.spherical-hydro-erosion",
];

const SPHERE_STAGE_IDS: [&str; 6] = [
    "natural.spherical-tectonics",
    "natural.spherical-mantle",
    "natural.spherical-relief",
    "natural.spherical-geology",
    "natural.spherical-preliminary-climate",
    "natural.spherical-hydro-erosion",
];

const EXPECTED_GRAPH_HASHES: [(&str, &str); 8] = [
    (
        "surface",
        "213c897cc3af183bfb7a47c421d768e41f2993bd93d05f347ddf86fbb35500ec",
    ),
    (
        "tectonic",
        "36b99b6e4aa060c7d2cb1f2ebd904be16d378275a9c9419fd26ffade41d3b972",
    ),
    (
        "mantle",
        "c0213d96cfdad2bf5014eb56b6947642cac82f7124ff0ead3f2aa094dbce939e",
    ),
    (
        "relief",
        "5f0f0e644616c8281d39e0313fe05bc668418541ca9e33e8771ba4a98bd11ba4",
    ),
    (
        "geology",
        "04bd2c7e7ef3ab6f38e7ab7c8da9c14c135cd3344a4365f7af83d08ceb0b06e8",
    ),
    (
        "climate",
        "c7c000a858b8cfa96eb9e00e2f6da878744ec4418ea94539e198de091130f82b",
    ),
    (
        "hydro",
        "7053d1259a37ba66286b5df943006499b6b630f394c90d7c89fac9d460522538",
    ),
    (
        "result",
        "dd47d1619ee159dfc7a7f76bb285f35a84117091cf47325dc5bfddf6d7b79452",
    ),
];

#[derive(Debug, Clone)]
struct Inputs {
    radius_m: f64,
    tectonic: TectonicSpec,
    geologic: GeologicSpec,
    climate: ClimateSpec,
    hydro: HydroErosionSpec,
    formation: WorldFormationSpec,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            radius_m: 6_371_000.0,
            tectonic: TectonicSpec::default(),
            geologic: GeologicSpec::default(),
            climate: ClimateSpec::default(),
            hydro: HydroErosionSpec::default(),
            formation: WorldFormationSpec::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Omit {
    Space,
    Tectonic,
    Geologic,
    Climate,
    Hydro,
    Formation,
    Rules,
    Constraints,
}

fn external(inputs: &Inputs, omit: Option<Omit>) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    if omit != Some(Omit::Space) {
        artifacts
            .insert(SphericalSpaceArtifact::new(SphericalSpaceSpec {
                radius: Meters::new(inputs.radius_m).unwrap(),
                target_cell_count: 162,
            }))
            .unwrap();
    }
    if omit != Some(Omit::Tectonic) {
        artifacts
            .insert(TectonicSpecArtifact::new(inputs.tectonic.clone()))
            .unwrap();
    }
    if omit != Some(Omit::Geologic) {
        artifacts
            .insert(GeologicSpecArtifact::new(inputs.geologic.clone()))
            .unwrap();
    }
    if omit != Some(Omit::Climate) {
        artifacts
            .insert(ClimateSpecArtifact::new(inputs.climate.clone()))
            .unwrap();
    }
    if omit != Some(Omit::Hydro) {
        artifacts
            .insert(HydroErosionSpecArtifact::new(inputs.hydro.clone()))
            .unwrap();
    }
    if omit != Some(Omit::Formation) {
        artifacts
            .insert(WorldFormationSpecArtifact::new(inputs.formation.clone()))
            .unwrap();
    }
    if omit != Some(Omit::Rules) {
        artifacts
            .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
            .unwrap();
    }
    if omit != Some(Omit::Constraints) {
        artifacts
            .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
            .unwrap();
    }
    artifacts
}

fn build(root_seed: RootSeed, inputs: &Inputs, cache: &mut MemoryStageCache) -> BuildOutcome {
    BuildEngine::new(spherical_natural_foundation_graph().unwrap())
        .build(root_seed, external(inputs, None), cache)
        .unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn exact_misses(report: &BuildReport, expected_misses: &[&str]) {
    let expected = expected_misses.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(report.stages().len(), ALL_STAGE_IDS.len());
    for stage in report.stages() {
        assert_eq!(
            stage.cache_hit(),
            !expected.contains(stage.stage_id()),
            "unexpected cache state for {}",
            stage.stage_id()
        );
    }
}

#[test]
fn graph_declares_the_authoritative_sphere_path() {
    let graph = spherical_natural_foundation_graph().unwrap();
    assert_eq!(graph.stage_ids(), ALL_STAGE_IDS);
    for stage_id in SPHERE_STAGE_IDS {
        let descriptor = graph
            .descriptors()
            .iter()
            .find(|descriptor| descriptor.id().as_str() == stage_id)
            .unwrap();
        assert!(descriptor
            .dependencies()
            .contains(&SphericalSurfaceArtifact::KEY));
        assert!(descriptor
            .dependencies()
            .iter()
            .all(|key| { !matches!(key.as_str(), "spatial.planar-spec" | "world.spatial") }));
    }
}

#[test]
fn graph_requires_exactly_the_eight_approved_external_artifacts() {
    let inputs = Inputs::default();
    assert_eq!(external(&inputs, None).len(), 8);
    for omitted in [
        Omit::Space,
        Omit::Tectonic,
        Omit::Geologic,
        Omit::Climate,
        Omit::Hydro,
        Omit::Formation,
        Omit::Rules,
        Omit::Constraints,
    ] {
        let failure = BuildEngine::new(spherical_natural_foundation_graph().unwrap())
            .build(
                RootSeed::new(42),
                external(&inputs, Some(omitted)),
                &mut MemoryStageCache::new(),
            )
            .unwrap_err();
        assert_eq!(
            failure.report.diagnostics()[0].code(),
            "engine.external-artifact",
            "wrong failure for {omitted:?}"
        );
        assert!(failure.report.stage_ids().is_empty());
    }

    let mut extra = external(&inputs, None);
    extra
        .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
            width: Meters::new(1_000_000.0).unwrap(),
            height: Meters::new(600_000.0).unwrap(),
            target_cell_count: 128,
            boundary: BoundaryCondition::Closed,
        }))
        .unwrap();
    let failure = BuildEngine::new(spherical_natural_foundation_graph().unwrap())
        .build(RootSeed::new(42), extra, &mut MemoryStageCache::new())
        .unwrap_err();
    assert_eq!(
        failure.report.diagnostics()[0].code(),
        "engine.external-artifact-set"
    );
    assert!(failure.report.stage_ids().is_empty());
}

#[test]
fn whole_graph_cross_validates_and_has_frozen_semantic_hashes() {
    let first = build(
        RootSeed::new(42),
        &Inputs::default(),
        &mut MemoryStageCache::new(),
    );
    let repeated = build(
        RootSeed::new(42),
        &Inputs::default(),
        &mut MemoryStageCache::new(),
    );

    let surface = first.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
    let formation = first
        .artifacts
        .get::<ResolvedWorldFormationArtifact>()
        .unwrap();
    let tectonic = first.artifacts.get::<SphericalTectonicArtifact>().unwrap();
    let mantle = first.artifacts.get::<SphericalMantleArtifact>().unwrap();
    let relief = first.artifacts.get::<SphericalReliefArtifact>().unwrap();
    let geology = first.artifacts.get::<SphericalGeologicArtifact>().unwrap();
    let climate = first
        .artifacts
        .get::<SphericalPreliminaryClimateArtifact>()
        .unwrap();
    let hydro = first
        .artifacts
        .get::<SphericalHydroErosionArtifact>()
        .unwrap();

    surface.snapshot().validate().unwrap();
    formation.formation().validate().unwrap();
    tectonic
        .snapshot()
        .validate_against(surface.snapshot())
        .unwrap();
    mantle
        .snapshot()
        .validate_against(surface.snapshot())
        .unwrap();
    relief
        .snapshot()
        .validate_against(surface.snapshot(), tectonic.snapshot(), mantle.snapshot())
        .unwrap();
    geology
        .snapshot()
        .validate_against(
            surface.snapshot(),
            tectonic.snapshot(),
            mantle.snapshot(),
            relief.snapshot(),
        )
        .unwrap();
    climate
        .snapshot()
        .validate_against(surface.snapshot(), relief.snapshot())
        .unwrap();
    hydro
        .snapshot()
        .validate_against(
            surface.snapshot(),
            relief.snapshot(),
            geology.snapshot(),
            climate.snapshot(),
        )
        .unwrap();

    let hashes = [
        (
            "surface",
            hex(first
                .artifacts
                .hash::<SphericalSurfaceArtifact>()
                .unwrap()
                .as_bytes()),
        ),
        (
            "tectonic",
            hex(first
                .artifacts
                .hash::<SphericalTectonicArtifact>()
                .unwrap()
                .as_bytes()),
        ),
        (
            "mantle",
            hex(first
                .artifacts
                .hash::<SphericalMantleArtifact>()
                .unwrap()
                .as_bytes()),
        ),
        (
            "relief",
            hex(first
                .artifacts
                .hash::<SphericalReliefArtifact>()
                .unwrap()
                .as_bytes()),
        ),
        (
            "geology",
            hex(first
                .artifacts
                .hash::<SphericalGeologicArtifact>()
                .unwrap()
                .as_bytes()),
        ),
        (
            "climate",
            hex(first
                .artifacts
                .hash::<SphericalPreliminaryClimateArtifact>()
                .unwrap()
                .as_bytes()),
        ),
        (
            "hydro",
            hex(first
                .artifacts
                .hash::<SphericalHydroErosionArtifact>()
                .unwrap()
                .as_bytes()),
        ),
    ];
    let result_hash = hex(first.report.result_hash().unwrap().as_bytes());
    println!(
        "spherical_graph_golden {} result={result_hash}",
        hashes
            .iter()
            .map(|(name, hash)| format!("{name}={hash}"))
            .collect::<Vec<_>>()
            .join(" ")
    );

    for (name, hash) in &hashes {
        let repeated_hash = match *name {
            "surface" => repeated
                .artifacts
                .hash::<SphericalSurfaceArtifact>()
                .unwrap(),
            "tectonic" => repeated
                .artifacts
                .hash::<SphericalTectonicArtifact>()
                .unwrap(),
            "mantle" => repeated
                .artifacts
                .hash::<SphericalMantleArtifact>()
                .unwrap(),
            "relief" => repeated
                .artifacts
                .hash::<SphericalReliefArtifact>()
                .unwrap(),
            "geology" => repeated
                .artifacts
                .hash::<SphericalGeologicArtifact>()
                .unwrap(),
            "climate" => repeated
                .artifacts
                .hash::<SphericalPreliminaryClimateArtifact>()
                .unwrap(),
            "hydro" => repeated
                .artifacts
                .hash::<SphericalHydroErosionArtifact>()
                .unwrap(),
            _ => unreachable!(),
        };
        assert_eq!(*hash, hex(repeated_hash.as_bytes()));
    }
    assert_eq!(first.report.result_hash(), repeated.report.result_hash());

    let actual = hashes
        .iter()
        .map(|(name, hash)| (*name, hash.as_str()))
        .chain(std::iter::once(("result", result_hash.as_str())))
        .collect::<Vec<_>>();
    assert_eq!(actual, EXPECTED_GRAPH_HASHES);
}

#[test]
fn cache_invalidation_matches_each_independent_input() {
    fn changed_build(inputs: Inputs, root_seed: RootSeed) -> BuildReport {
        let baseline = Inputs::default();
        let engine = BuildEngine::new(spherical_natural_foundation_graph().unwrap());
        let mut cache = MemoryStageCache::with_max_entries(128).unwrap();
        engine
            .build(RootSeed::new(42), external(&baseline, None), &mut cache)
            .unwrap();
        engine
            .build(root_seed, external(&inputs, None), &mut cache)
            .unwrap()
            .report
    }

    exact_misses(
        &changed_build(Inputs::default(), RootSeed::new(43)),
        &ALL_STAGE_IDS,
    );

    let mut formation = Inputs::default();
    formation.formation.preset = WorldFormationPreset::Archipelago;
    exact_misses(
        &changed_build(formation, RootSeed::new(42)),
        &[
            "natural.resolve-world-formation",
            "natural.spherical-mantle",
            "natural.spherical-tectonics",
            "natural.spherical-relief",
            "natural.spherical-geology",
            "natural.spherical-preliminary-climate",
            "natural.spherical-hydro-erosion",
        ],
    );

    let mut tectonic = Inputs::default();
    tectonic.tectonic.plate_count = 10;
    exact_misses(
        &changed_build(tectonic, RootSeed::new(42)),
        &[
            "natural.resolve-tectonic-rules",
            "natural.project-tectonic-input",
            "natural.spherical-tectonics",
            "natural.spherical-relief",
            "natural.spherical-geology",
            "natural.spherical-preliminary-climate",
            "natural.spherical-hydro-erosion",
        ],
    );

    let mut geologic = Inputs::default();
    geologic.geologic.hotspot_count = 6;
    exact_misses(
        &changed_build(geologic, RootSeed::new(42)),
        &[
            "natural.resolve-geologic-rules",
            "natural.project-geologic-input",
            "natural.spherical-mantle",
            "natural.spherical-relief",
            "natural.spherical-geology",
            "natural.spherical-preliminary-climate",
            "natural.spherical-hydro-erosion",
        ],
    );

    let mut climate = Inputs::default();
    climate.climate.temperature_offset_deci_c = 10;
    exact_misses(
        &changed_build(climate, RootSeed::new(42)),
        &[
            "natural.resolve-climate-rules",
            "natural.project-climate-input",
            "natural.spherical-preliminary-climate",
            "natural.spherical-hydro-erosion",
        ],
    );

    let mut hydro = Inputs::default();
    hydro.hydro.erosion_strength_permille = 900;
    exact_misses(
        &changed_build(hydro, RootSeed::new(42)),
        &[
            "natural.resolve-hydro-erosion-rules",
            "natural.project-hydro-erosion-input",
            "natural.spherical-hydro-erosion",
        ],
    );
}

#[test]
fn cache_isolates_same_count_surfaces_with_different_radii() {
    let engine = BuildEngine::new(spherical_natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::with_max_entries(128).unwrap();
    let first = engine
        .build(
            RootSeed::new(42),
            external(&Inputs::default(), None),
            &mut cache,
        )
        .unwrap();
    exact_misses(&first.report, &ALL_STAGE_IDS);
    let repeated = engine
        .build(
            RootSeed::new(42),
            external(&Inputs::default(), None),
            &mut cache,
        )
        .unwrap();
    exact_misses(&repeated.report, &[]);

    let changed = Inputs {
        radius_m: 7_000_000.0,
        ..Inputs::default()
    };
    let different = engine
        .build(RootSeed::new(42), external(&changed, None), &mut cache)
        .unwrap();
    exact_misses(
        &different.report,
        &[
            "spatial.spherical-voronoi",
            "natural.spherical-mantle",
            "natural.spherical-tectonics",
            "natural.spherical-relief",
            "natural.spherical-geology",
            "natural.spherical-preliminary-climate",
            "natural.spherical-hydro-erosion",
        ],
    );

    let first_surface = first.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
    let different_surface = different
        .artifacts
        .get::<SphericalSurfaceArtifact>()
        .unwrap();
    assert_eq!(
        first_surface.snapshot().cells().len(),
        different_surface.snapshot().cells().len()
    );
    assert_eq!(
        first_surface.snapshot().edges().len(),
        different_surface.snapshot().edges().len()
    );
    assert_ne!(
        SurfaceRef::for_spherical(first_surface.snapshot()),
        SurfaceRef::for_spherical(different_surface.snapshot())
    );
}
