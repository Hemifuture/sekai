use std::collections::BTreeSet;

use sekai::engine::{
    Artifact, BuildEngine, BuildOutcome, BuildReport, ExternalArtifacts, MemoryStageCache,
};
use sekai::generators::natural::{
    spherical_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
    GeologicSpecArtifact, HydroErosionSpecArtifact, NaturalQualityArtifact,
    NaturalQualityProfileArtifact, ReliefSpecArtifact, ResolvedWorldFormationArtifact,
    RulePackSetArtifact, SphericalGeologicArtifact, SphericalHydroErosionArtifact,
    SphericalMantleArtifact, SphericalPreliminaryClimateArtifact, SphericalReliefArtifact,
    SphericalTectonicArtifact, TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{SphericalSpaceArtifact, SphericalSurfaceArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    ClimateSpec, GeologicSpec, HydroErosionSpec, NaturalQualityProfile, ReliefSpec, TectonicSpec,
    WorldFormationPreset, WorldFormationSpec,
};
use sekai::world::spatial::audited_float_platform;
use sekai::world::spatial::SurfaceRef;
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

const ALL_STAGE_IDS: [&str; 17] = [
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
    "natural.spherical-quality",
];

const SPHERE_STAGE_IDS: [&str; 7] = [
    "natural.spherical-tectonics",
    "natural.spherical-mantle",
    "natural.spherical-relief",
    "natural.spherical-geology",
    "natural.spherical-preliminary-climate",
    "natural.spherical-hydro-erosion",
    "natural.spherical-quality",
];

const EXPECTED_GRAPH_HASHES: [(&str, &str); 9] = [
    (
        "surface",
        "213c897cc3af183bfb7a47c421d768e41f2993bd93d05f347ddf86fbb35500ec",
    ),
    (
        "tectonic",
        "06ec9a95a3e861ce03971d0426575b6139156f515622e54f4f76cc61b7c35a68",
    ),
    (
        "mantle",
        "c0213d96cfdad2bf5014eb56b6947642cac82f7124ff0ead3f2aa094dbce939e",
    ),
    (
        "relief",
        "5cbaf2e222257c77fa157df81f6d7fe415dce344d5fe2979232e6737426bd1fc",
    ),
    (
        "geology",
        "0174f20292208ee5f38ec1a3977f140ae86793e08930c1ce7f2b30fcb195fb71",
    ),
    (
        "climate",
        "8f9e0507183d559e87d16595f9709d7b6d34d221b7b0259a02d9618f0f09ec31",
    ),
    (
        "hydro",
        "b1513337fc0be091b82e7483e24ffc80275d9ef717c22594f1792b94a10fb6a0",
    ),
    (
        "quality",
        "66684802988168bd9a282f6531cd949d1615ea1282c61a1f35462b0404e4877f",
    ),
    (
        "result",
        "b0442d82c42ebc07d769b2f0cf9aa65e07596f68a84199968c10e446cb1c0f0a",
    ),
];

#[derive(Debug, Clone)]
struct Inputs {
    radius_m: f64,
    tectonic: TectonicSpec,
    geologic: GeologicSpec,
    climate: ClimateSpec,
    hydro: HydroErosionSpec,
    relief: ReliefSpec,
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
            relief: ReliefSpec::default(),
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
    Relief,
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
    if omit != Some(Omit::Relief) {
        artifacts
            .insert(ReliefSpecArtifact::new(inputs.relief.clone()))
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
    let relief = graph
        .descriptors()
        .iter()
        .find(|descriptor| descriptor.id().as_str() == "natural.spherical-relief")
        .unwrap();
    assert!(relief.dependencies().contains(&ReliefSpecArtifact::KEY));
}

#[test]
fn graph_requires_exactly_the_nine_approved_external_artifacts() {
    let inputs = Inputs::default();
    assert_eq!(external(&inputs, None).len(), 9);
    for omitted in [
        Omit::Space,
        Omit::Tectonic,
        Omit::Geologic,
        Omit::Climate,
        Omit::Hydro,
        Omit::Relief,
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
        .insert(NaturalQualityProfileArtifact::new(
            NaturalQualityProfile::Draft,
        ))
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
fn invalid_land_target_is_rejected_before_any_stage_runs() {
    let invalid = ReliefSpec {
        target_land_fraction: f32::NAN,
        ..ReliefSpec::default()
    };
    let error = ExternalArtifacts::new()
        .insert(ReliefSpecArtifact::new(invalid))
        .unwrap_err();

    assert!(error.to_string().contains("natural.invalid-relief-spec"));
}

#[test]
fn whole_graph_cross_validates_and_has_frozen_semantic_hashes() {
    let inputs = Inputs::default();
    let first = build(RootSeed::new(42), &inputs, &mut MemoryStageCache::new());
    let repeated = build(RootSeed::new(42), &inputs, &mut MemoryStageCache::new());

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
    let quality = first.artifacts.get::<NaturalQualityArtifact>().unwrap();

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
    quality.report().validate().unwrap();
    assert_eq!(
        quality.report().surface_ref(),
        SurfaceRef::for_spherical(surface.snapshot())
    );

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
        (
            "quality",
            hex(first
                .artifacts
                .hash::<NaturalQualityArtifact>()
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
            "quality" => repeated.artifacts.hash::<NaturalQualityArtifact>().unwrap(),
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
    if audited_float_platform() {
        assert_eq!(actual, EXPECTED_GRAPH_HASHES);
    } else {
        eprintln!("exact identity checks skipped: unaudited float platform");
    }
}

#[test]
fn whole_graph_accepts_an_evolved_final_plate_count() {
    let inputs = Inputs::default();
    let outcome = build(RootSeed::new(42), &inputs, &mut MemoryStageCache::new());
    let tectonic = outcome
        .artifacts
        .get::<SphericalTectonicArtifact>()
        .unwrap();

    assert_ne!(
        tectonic.snapshot().plates().len(),
        usize::from(inputs.tectonic.plate_count),
        "this graph fixture must exercise an evolved final plate count"
    );
    assert_eq!(outcome.report.stage_ids(), ALL_STAGE_IDS);
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
            "natural.spherical-quality",
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
            "natural.spherical-quality",
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
            "natural.spherical-quality",
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
            "natural.spherical-quality",
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
            "natural.spherical-quality",
        ],
    );

    let mut relief = Inputs::default();
    relief.relief.target_land_fraction = 0.55;
    exact_misses(
        &changed_build(relief, RootSeed::new(42)),
        &[
            "natural.spherical-relief",
            "natural.spherical-geology",
            "natural.spherical-preliminary-climate",
            "natural.spherical-hydro-erosion",
            "natural.spherical-quality",
        ],
    );
}

#[test]
fn land_target_changes_only_sea_level_and_downstream_land_classification() {
    fn land_fraction(outcome: &BuildOutcome) -> f64 {
        let surface = outcome.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
        let relief = outcome.artifacts.get::<SphericalReliefArtifact>().unwrap();
        let land_area = surface
            .snapshot()
            .cells()
            .iter()
            .zip(relief.snapshot().land_ocean().raw_values())
            .filter_map(|(cell, &kind)| (kind == 1).then_some(cell.area.get()))
            .sum::<f64>();
        land_area / surface.snapshot().total_cell_area().get()
    }

    let low_inputs = Inputs {
        relief: ReliefSpec {
            target_land_fraction: 0.25,
            ..ReliefSpec::default()
        },
        ..Inputs::default()
    };
    let high_inputs = Inputs {
        relief: ReliefSpec {
            target_land_fraction: 0.60,
            ..ReliefSpec::default()
        },
        ..Inputs::default()
    };
    let low = build(RootSeed::new(42), &low_inputs, &mut MemoryStageCache::new());
    let high = build(
        RootSeed::new(42),
        &high_inputs,
        &mut MemoryStageCache::new(),
    );

    for key in ["surface", "tectonic", "mantle"] {
        let equal = match key {
            "surface" => {
                low.artifacts.hash::<SphericalSurfaceArtifact>().unwrap()
                    == high.artifacts.hash::<SphericalSurfaceArtifact>().unwrap()
            }
            "tectonic" => {
                low.artifacts.hash::<SphericalTectonicArtifact>().unwrap()
                    == high.artifacts.hash::<SphericalTectonicArtifact>().unwrap()
            }
            "mantle" => {
                low.artifacts.hash::<SphericalMantleArtifact>().unwrap()
                    == high.artifacts.hash::<SphericalMantleArtifact>().unwrap()
            }
            _ => unreachable!(),
        };
        assert!(equal, "{key} changed with a relief-only author edit");
    }
    let low_relief = low.artifacts.get::<SphericalReliefArtifact>().unwrap();
    let high_relief = high.artifacts.get::<SphericalReliefArtifact>().unwrap();
    assert_eq!(
        low_relief.snapshot().crust_base_elevation_m(),
        high_relief.snapshot().crust_base_elevation_m()
    );
    assert_eq!(
        low_relief.snapshot().tectonic_offset_m(),
        high_relief.snapshot().tectonic_offset_m()
    );
    assert_eq!(
        low_relief.snapshot().volcanic_offset_m(),
        high_relief.snapshot().volcanic_offset_m()
    );
    assert_eq!(
        low_relief.snapshot().regional_offset_m(),
        high_relief.snapshot().regional_offset_m()
    );
    assert_eq!(
        low_relief.snapshot().elevation_m(),
        high_relief.snapshot().elevation_m()
    );
    assert!(low_relief.snapshot().sea_level_m() > high_relief.snapshot().sea_level_m());
    let low_actual = land_fraction(&low);
    let high_actual = land_fraction(&high);
    assert!(low_actual < high_actual);
    assert!((low_actual - 0.25).abs() <= 0.02, "low actual={low_actual}");
    assert!(
        (high_actual - 0.60).abs() <= 0.02,
        "high actual={high_actual}"
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
            "natural.spherical-quality",
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
