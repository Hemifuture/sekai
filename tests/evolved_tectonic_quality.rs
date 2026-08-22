use std::sync::OnceLock;

use sekai::engine::{
    derive_stage_seed, Artifact, BuildCancellation, Diagnostic, StageIdentity, StageRng,
};
use sekai::generators::natural::{
    evaluate_evolved_tectonic_corpus_quality, evaluate_evolved_tectonic_quality,
    EvolvedTectonicArtifact, EvolvedTectonicGenerator, MantleGenerator, ReliefGenerator,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    GeologicSpec, NaturalQualityProfile, NaturalQualityReport, QualityBounds, QualityMetric,
    QualityMetricStatus, ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    SphericalReliefSnapshot, TectonicSpec, WorldFormationPreset, NATURAL_QUALITY_REPORT_SCHEMA_V1,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};

fn bundle() -> &'static ProfileSurfaceBundle {
    static BUNDLE: OnceLock<ProfileSurfaceBundle> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap()
    })
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

#[test]
fn quality_report_is_surface_bound_versioned_and_covers_every_p2_gate() {
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(42),
        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
    ));
    let snapshot = EvolvedTectonicGenerator::generate(
        bundle(),
        &TectonicSpec {
            plate_count: 12,
            continental_crust_fraction: 0.38,
            ..TectonicSpec::default()
        },
        &formation(),
        &mut rng,
    )
    .unwrap();
    let report =
        evaluate_evolved_tectonic_quality(bundle().authoritative_surface(), &snapshot).unwrap();
    report.validate().unwrap();
    assert_eq!(report.surface_ref(), snapshot.surface_ref());

    let names = report
        .metrics()
        .iter()
        .map(|metric| {
            assert_eq!(metric.id().namespace(), "sekai.tectonics-v5");
            assert_eq!(metric.id().version(), 1);
            metric.id().name()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "authority-material-relative-error",
            "collision-causality-fraction",
            "continental-area-fraction",
            "continental-area-retention",
            "control-material-relative-error",
            "lineage-closure-error",
            "maximum-plate-area-fraction",
            "non-finite-value-count",
            "ocean-age-depth-spearman",
            "regular-triple-junction-angle-fraction",
            "remap-category-ambiguity-fraction",
            "subduction-causality-fraction",
            "transform-to-convergent-uplift-ratio",
        ]
    );
    assert!(report
        .metrics()
        .iter()
        .all(|metric| metric.status() != QualityMetricStatus::Fail));
    assert!(report.metrics().iter().all(|metric| {
        metric.value().is_none_or(f64::is_finite)
            && metric.bounds().min().is_none_or(f64::is_finite)
            && metric.bounds().max().is_none_or(f64::is_finite)
    }));

    let corpus =
        evaluate_evolved_tectonic_corpus_quality(bundle().authoritative_surface(), &[&snapshot])
            .unwrap();
    assert_eq!(corpus.surface_ref(), snapshot.surface_ref());
    assert_eq!(
        corpus
            .metrics()
            .iter()
            .map(|metric| metric.id().name())
            .collect::<Vec<_>>(),
        vec![
            "collision-causality-fraction",
            "continental-area-fraction",
            "ocean-age-depth-spearman",
            "regular-triple-junction-angle-fraction",
            "subduction-causality-fraction",
            "transform-to-convergent-uplift-ratio",
        ]
    );
}

#[test]
fn quality_evaluator_rejects_a_different_authoritative_surface() {
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(43),
        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
    ));
    let snapshot = EvolvedTectonicGenerator::generate(
        bundle(),
        &TectonicSpec::default(),
        &formation(),
        &mut rng,
    )
    .unwrap();
    let report =
        evaluate_evolved_tectonic_quality(bundle().authoritative_surface(), &snapshot).unwrap();
    // Per-world statuses are measurements, not gates: a report that records a
    // failing transform-to-convergent ratio is still valid evidence.
    let failing_metrics = report
        .metrics()
        .iter()
        .map(|metric| {
            if metric.id().name() == "transform-to-convergent-uplift-ratio" {
                QualityMetric::new(
                    metric.id().clone(),
                    QualityMetricStatus::Fail,
                    Some(0.75),
                    metric.sample_count().max(1),
                    QualityBounds::at_most(0.50).unwrap(),
                    None,
                )
                .unwrap()
            } else {
                metric.clone()
            }
        })
        .collect();
    EvolvedTectonicArtifact::new(snapshot.clone(), report)
        .validate()
        .unwrap();
    let failing_report = NaturalQualityReport::new(
        NATURAL_QUALITY_REPORT_SCHEMA_V1,
        snapshot.surface_ref(),
        failing_metrics,
    )
    .unwrap();
    assert_eq!(
        failing_report
            .metrics()
            .iter()
            .find(|metric| metric.id().name() == "transform-to-convergent-uplift-ratio")
            .unwrap()
            .status(),
        QualityMetricStatus::Fail
    );
    EvolvedTectonicArtifact::new(snapshot.clone(), failing_report)
        .validate()
        .unwrap();
    let empty_report = NaturalQualityReport::new(
        NATURAL_QUALITY_REPORT_SCHEMA_V1,
        snapshot.surface_ref(),
        Vec::new(),
    )
    .unwrap();
    assert!(EvolvedTectonicArtifact::new(snapshot.clone(), empty_report)
        .validate()
        .is_err());
    let other = sekai::generators::spatial::GeodesicVoronoiBuilder::build(
        &sekai::world::SphericalSpaceSpec {
            radius: Meters::new(6_371_100.0).unwrap(),
            target_cell_count: NaturalQualityProfile::Draft.authoritative_target_cell_count(),
        },
    )
    .unwrap();
    assert!(evaluate_evolved_tectonic_quality(&other, &snapshot).is_err());
}

#[test]
#[ignore = "release-only 17-seed legacy-relief compatibility harness; P3 must replace this gate"]
fn legacy_relief_coast_overlap_remains_below_the_p2_handoff_limit() {
    const SEEDS: [u64; 17] = [
        42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
    ];
    let surface = bundle().authoritative_surface();
    let formation = formation();
    let mut overlaps = Vec::new();
    for seed in SEEDS {
        let mut tectonic_rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
        ));
        let tectonic = EvolvedTectonicGenerator::generate(
            bundle(),
            &TectonicSpec::default(),
            &formation,
            &mut tectonic_rng,
        )
        .unwrap();
        let quality = evaluate_evolved_tectonic_quality(surface, &tectonic).unwrap();
        let failed = quality
            .metrics()
            .iter()
            .filter(|metric| metric.status() == QualityMetricStatus::Fail)
            .map(|metric| metric.id().name())
            .collect::<Vec<_>>();
        if !failed.is_empty() {
            eprintln!("P2 corpus-scoped per-world observations seed={seed}: {failed:?}");
        }
        let mut mantle_rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("natural.spherical-mantle", 1, "sekai.core"),
        ));
        let mantle = MantleGenerator::generate_spherical(
            surface,
            &GeologicSpec::default(),
            formation.mantle_bias(),
            &mut mantle_rng,
        )
        .unwrap();
        let mut relief_rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("natural.spherical-relief", 3, "sekai.core"),
        ));
        let relief = ReliefGenerator::generate_spherical(
            surface,
            tectonic.compatibility(),
            &mantle,
            &ReliefSpec::default(),
            &mut relief_rng,
            &mut Vec::<Diagnostic>::new(),
        )
        .unwrap();
        let overlap = one_cell_coast_plate_overlap(surface, tectonic.compatibility(), &relief);
        eprintln!("P2 legacy-relief coast overlap seed={seed}: {overlap:.6}");
        overlaps.push(overlap);
    }
    overlaps.sort_by(f64::total_cmp);
    let median = overlaps[overlaps.len() / 2];
    eprintln!("P2 legacy-relief coast overlap median={median:.6}: {overlaps:?}");

    // This is an explicit compatibility bridge, not a V5 graph dependency.
    // P3 must rerun and replace it using physical primary relief.
    assert!(median <= 0.35, "median coast/plate overlap was {median}");
}

fn one_cell_coast_plate_overlap(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    tectonic: &sekai::world::natural::SphericalTectonicSnapshot,
    relief: &SphericalReliefSnapshot,
) -> f64 {
    let mut boundary_cells = vec![false; surface.cells().len()];
    for edge in surface.edges() {
        if tectonic.plate_for_cell(edge.cells[0]) != tectonic.plate_for_cell(edge.cells[1]) {
            boundary_cells[edge.cells[0].raw() as usize] = true;
            boundary_cells[edge.cells[1].raw() as usize] = true;
        }
    }
    let mut coast_length = 0.0;
    let mut overlap_length = 0.0;
    for edge in surface.edges() {
        if relief.land_ocean_kind(edge.cells[0]) == relief.land_ocean_kind(edge.cells[1]) {
            continue;
        }
        coast_length += edge.length.get();
        if edge
            .cells
            .iter()
            .any(|cell| boundary_cells[cell.raw() as usize])
        {
            overlap_length += edge.length.get();
        }
    }
    overlap_length / coast_length
}
