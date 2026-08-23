use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_primary_relief_corpus_quality, evaluate_primary_relief_quality,
    EvolvedTectonicGenerator, GeologicSubstrateGenerator, PrimaryReliefGenerator,
    PrimaryReliefQualitySample,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    EvolvedTectonicSnapshot, GeologicSpec, GeologicSubstrateSnapshot, NaturalQualityProfile,
    PrimaryReliefSnapshot, QualityMetricStatus, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};

struct Fixture {
    bundle: ProfileSurfaceBundle,
    evolved: EvolvedTectonicSnapshot,
    substrate: GeologicSubstrateSnapshot,
    relief: PrimaryReliefSnapshot,
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn stage_rng(seed: u64, id: &'static str, version: u32) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(id, version, "sekai.core"),
    ))
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let evolved = EvolvedTectonicGenerator::generate(
            &bundle,
            &TectonicSpec::default(),
            &formation(),
            &mut stage_rng(42, "natural.evolved-tectonics", 5),
        )
        .unwrap();
        let substrate = GeologicSubstrateGenerator::generate(
            bundle.authoritative_surface(),
            &evolved,
            &GeologicSpec::default(),
            &formation(),
            &mut stage_rng(42, "natural.geologic-substrate", 1),
        )
        .unwrap();
        let relief = PrimaryReliefGenerator::generate(
            bundle.authoritative_surface(),
            &evolved,
            &substrate,
            &ReliefSpec::default(),
            &mut stage_rng(42, "natural.primary-relief", 1),
            &mut Vec::new(),
        )
        .unwrap();
        Fixture {
            bundle,
            evolved,
            substrate,
            relief,
        }
    })
}

#[test]
fn per_world_report_has_the_exact_locked_inventory_and_all_hard_gates_pass() {
    let fixture = fixture();
    let report = evaluate_primary_relief_quality(
        fixture.bundle.authoritative_surface(),
        &fixture.evolved,
        &fixture.substrate,
        &fixture.relief,
    )
    .unwrap();
    let names = report
        .metrics()
        .iter()
        .map(|metric| {
            assert_eq!(metric.id().namespace(), "sekai.primary-relief-v1");
            assert_eq!(metric.id().version(), 1);
            metric.id().name()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "coast-plate-boundary-overlap",
            "component-closure-max-error-m",
            "continental-ocean-median-separation-m",
            "convergent-positive-dynamic-fraction",
            "elevation-safety-violation-count",
            "hotspot-positive-construction-fraction",
            "maximum-plate-area-fraction",
            "non-finite-value-count",
            "old-young-ocean-depth-separation-m",
            "physical-land-area-fraction",
            "regional-detail-rms-ratio",
            "subduction-negative-dynamic-fraction",
            "upstream-p2-hard-failure-count",
            "water-inventory-ratio",
            "water-volume-relative-error",
        ]
    );
    let water_inventory = report
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == "water-inventory-ratio")
        .unwrap();
    assert_eq!(water_inventory.value(), Some(1.0));
    assert_eq!(water_inventory.bounds().min(), None);
    assert_eq!(water_inventory.bounds().max(), None);
    for hard in [
        "component-closure-max-error-m",
        "elevation-safety-violation-count",
        "maximum-plate-area-fraction",
        "non-finite-value-count",
        "upstream-p2-hard-failure-count",
        "water-volume-relative-error",
    ] {
        let metric = report
            .metrics()
            .iter()
            .find(|metric| metric.id().name() == hard)
            .unwrap();
        assert_eq!(metric.status(), QualityMetricStatus::Pass, "{hard}");
    }
}

#[test]
fn absent_hotspots_are_unavailable_instead_of_silently_passing() {
    let fixture = fixture();
    let no_hotspots = GeologicSpec {
        hotspot_count: 0,
        ..GeologicSpec::default()
    };
    let substrate = GeologicSubstrateGenerator::generate(
        fixture.bundle.authoritative_surface(),
        &fixture.evolved,
        &no_hotspots,
        &formation(),
        &mut stage_rng(73, "natural.geologic-substrate", 1),
    )
    .unwrap();
    let relief = PrimaryReliefGenerator::generate(
        fixture.bundle.authoritative_surface(),
        &fixture.evolved,
        &substrate,
        &ReliefSpec::default(),
        &mut stage_rng(73, "natural.primary-relief", 1),
        &mut Vec::new(),
    )
    .unwrap();
    let report = evaluate_primary_relief_quality(
        fixture.bundle.authoritative_surface(),
        &fixture.evolved,
        &substrate,
        &relief,
    )
    .unwrap();
    let metric = report
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == "hotspot-positive-construction-fraction")
        .unwrap();
    assert_eq!(metric.status(), QualityMetricStatus::Unavailable);
    assert!(metric.reason().is_some());
}

#[test]
fn corpus_report_contains_only_statistics_and_recomputes_from_raw_samples() {
    let fixture = fixture();
    let single = evaluate_primary_relief_quality(
        fixture.bundle.authoritative_surface(),
        &fixture.evolved,
        &fixture.substrate,
        &fixture.relief,
    )
    .unwrap();
    let samples = [
        PrimaryReliefQualitySample::new(&fixture.evolved, &fixture.substrate, &fixture.relief),
        PrimaryReliefQualitySample::new(&fixture.evolved, &fixture.substrate, &fixture.relief),
    ];
    let corpus =
        evaluate_primary_relief_corpus_quality(fixture.bundle.authoritative_surface(), &samples)
            .unwrap();
    assert_eq!(
        corpus
            .metrics()
            .iter()
            .map(|metric| metric.id().name())
            .collect::<Vec<_>>(),
        vec![
            "coast-plate-boundary-overlap",
            "continental-ocean-median-separation-m",
            "convergent-positive-dynamic-fraction",
            "hotspot-positive-construction-fraction",
            "old-young-ocean-depth-separation-m",
            "physical-land-area-fraction",
            "regional-detail-rms-ratio",
            "subduction-negative-dynamic-fraction",
            "water-inventory-ratio",
        ]
    );
    for metric in corpus.metrics() {
        let per_world = single
            .metrics()
            .iter()
            .find(|candidate| candidate.id().name() == metric.id().name())
            .unwrap();
        assert_eq!(metric.value(), per_world.value(), "{}", metric.id().name());
    }
    let water_inventory = corpus
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == "water-inventory-ratio")
        .unwrap();
    assert_eq!(water_inventory.bounds().min(), None);
    assert_eq!(water_inventory.bounds().max(), None);
    let physical_land = corpus
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == "physical-land-area-fraction")
        .unwrap();
    assert_eq!(physical_land.bounds().min(), None);
    assert_eq!(physical_land.bounds().max(), None);
}

#[test]
#[ignore = "fixed 17-seed P3 corpus; run explicitly in Release"]
fn fixed_p0_corpus_passes_every_locked_p3_statistical_gate() {
    const SEEDS: [u64; 17] = [
        42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
    ];
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let mut evolved = Vec::with_capacity(SEEDS.len());
    let mut substrates = Vec::with_capacity(SEEDS.len());
    let mut reliefs = Vec::with_capacity(SEEDS.len());
    for seed in SEEDS {
        let tectonic = EvolvedTectonicGenerator::generate(
            &bundle,
            &TectonicSpec::default(),
            &formation(),
            &mut stage_rng(seed, "natural.evolved-tectonics", 5),
        )
        .unwrap();
        let substrate = GeologicSubstrateGenerator::generate(
            surface,
            &tectonic,
            &GeologicSpec::default(),
            &formation(),
            &mut stage_rng(seed, "natural.geologic-substrate", 1),
        )
        .unwrap();
        let relief = PrimaryReliefGenerator::generate(
            surface,
            &tectonic,
            &substrate,
            &ReliefSpec::default(),
            &mut stage_rng(seed, "natural.primary-relief", 1),
            &mut Vec::new(),
        )
        .unwrap();
        evolved.push(tectonic);
        substrates.push(substrate);
        reliefs.push(relief);
    }
    let samples = (0..SEEDS.len())
        .map(|index| {
            PrimaryReliefQualitySample::new(&evolved[index], &substrates[index], &reliefs[index])
        })
        .collect::<Vec<_>>();
    let report = evaluate_primary_relief_corpus_quality(surface, &samples).unwrap();
    let mut continental_base = Vec::new();
    let mut ocean_base = Vec::new();
    let mut continental_dynamic = Vec::new();
    let mut ocean_dynamic = Vec::new();
    let mut continental_passive = Vec::new();
    let mut ocean_passive = Vec::new();
    let mut sea_levels = Vec::new();
    for index in 0..SEEDS.len() {
        sea_levels.push(f64::from(reliefs[index].sea_level_m()));
        for cell in 0..surface.cells().len() {
            let (base, dynamic, passive) = if substrates[index].crust_kind(cell)
                == Some(sekai::world::natural::CrustKind::Continental)
            {
                (
                    &mut continental_base,
                    &mut continental_dynamic,
                    &mut continental_passive,
                )
            } else {
                (&mut ocean_base, &mut ocean_dynamic, &mut ocean_passive)
            };
            base.push(f64::from(reliefs[index].isostatic_base_m()[cell]));
            dynamic.push(f64::from(reliefs[index].dynamic_tectonic_offset_m()[cell]));
            passive.push(f64::from(reliefs[index].passive_margin_offset_m()[cell]));
        }
    }
    for values in [
        &mut continental_base,
        &mut ocean_base,
        &mut continental_dynamic,
        &mut ocean_dynamic,
        &mut continental_passive,
        &mut ocean_passive,
        &mut sea_levels,
    ] {
        values.sort_by(f64::total_cmp);
    }
    eprintln!(
        "P3 components medians: continental base/dynamic/passive={:.2}/{:.2}/{:.2}; ocean={:.2}/{:.2}/{:.2}; sea={:.2}",
        median_sorted(&continental_base),
        median_sorted(&continental_dynamic),
        median_sorted(&continental_passive),
        median_sorted(&ocean_base),
        median_sorted(&ocean_dynamic),
        median_sorted(&ocean_passive),
        median_sorted(&sea_levels),
    );
    for metric in report.metrics() {
        eprintln!(
            "{}={:?} status={:?} n={} reason={:?}",
            metric.id().name(),
            metric.value(),
            metric.status(),
            metric.sample_count(),
            metric.reason()
        );
    }
    let failures = report
        .metrics()
        .iter()
        .filter(|metric| metric.status() != QualityMetricStatus::Pass)
        .map(|metric| metric.id().name())
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "P3 corpus failures: {failures:?}");
}

fn median_sorted(values: &[f64]) -> f64 {
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}
