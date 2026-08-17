use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_evolved_tectonic_quality, EvolvedTectonicArtifact, EvolvedTectonicGenerationError,
    EvolvedTectonicGenerator,
};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    NaturalQualityProfile, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;

#[derive(Serialize)]
struct PerformanceEvidence {
    schema_version: u16,
    profiles: Vec<ProfilePerformance>,
}

#[derive(Serialize)]
struct ProfilePerformance {
    profile: NaturalQualityProfile,
    authoritative_cells: usize,
    control_cells: usize,
    conservative_map_overlaps: usize,
    bundle_build_micros: u128,
    generation_micros: Option<u128>,
    cancellation_latency_micros: Option<u128>,
    artifact_json_bytes: Option<usize>,
}

#[test]
#[ignore = "release-only Draft completion and Standard/High cancellation performance evidence"]
fn measure_evolved_tectonic_profiles() {
    let formation = formation();
    let mut records = Vec::new();
    for profile in [
        NaturalQualityProfile::Draft,
        NaturalQualityProfile::Standard,
        NaturalQualityProfile::High,
    ] {
        let bundle_started = Instant::now();
        let bundle = ProfileSurfaceBuilder::build(
            profile,
            Meters::new(RADIUS_M).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let bundle_elapsed = bundle_started.elapsed();
        assert!(
            bundle_elapsed <= Duration::from_secs(180),
            "{profile:?} P1 bundle took {bundle_elapsed:?}"
        );
        let authoritative_cells = bundle.authoritative_surface().cells().len();
        let control_cells = bundle.tectonic_control_surface().cells().len();
        let overlaps = bundle.control_to_authoritative_map().overlap_count();

        let (generation_micros, cancellation_latency_micros, artifact_json_bytes) =
            if profile == NaturalQualityProfile::Draft {
                let generation_started = Instant::now();
                let mut rng = StageRng::from_seed(derive_stage_seed(
                    RootSeed::new(42),
                    StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
                ));
                let snapshot = EvolvedTectonicGenerator::generate(
                    &bundle,
                    &TectonicSpec::default(),
                    &formation,
                    &mut rng,
                )
                .unwrap();
                let report =
                    evaluate_evolved_tectonic_quality(bundle.authoritative_surface(), &snapshot)
                        .unwrap();
                let artifact = EvolvedTectonicArtifact::new(snapshot, report);
                let bytes = serde_json::to_vec(&artifact).unwrap().len();
                let elapsed = generation_started.elapsed();
                assert!(
                    elapsed <= Duration::from_secs(120),
                    "Draft V5 generation took {elapsed:?}"
                );
                (Some(elapsed.as_micros()), None, Some(bytes))
            } else {
                let cancellation = BuildCancellation::new();
                let worker_cancellation = cancellation.clone();
                let started = Arc::new(Barrier::new(2));
                let worker_started = Arc::clone(&started);
                let worker_formation = formation.clone();
                let worker = std::thread::spawn(move || {
                    let mut rng = StageRng::from_seed_with_cancellation(
                        derive_stage_seed(
                            RootSeed::new(73),
                            StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
                        ),
                        &worker_cancellation,
                    );
                    worker_started.wait();
                    EvolvedTectonicGenerator::generate(
                        &bundle,
                        &TectonicSpec::default(),
                        &worker_formation,
                        &mut rng,
                    )
                });
                started.wait();
                std::thread::sleep(Duration::from_millis(10));
                let cancellation_started = Instant::now();
                cancellation.cancel();
                let result = worker.join().unwrap();
                let latency = cancellation_started.elapsed();
                assert_eq!(result, Err(EvolvedTectonicGenerationError::Cancelled));
                assert!(
                    latency <= Duration::from_secs(2),
                    "{profile:?} cancellation took {latency:?}"
                );
                (None, Some(latency.as_micros()), None)
            };

        eprintln!(
            "P2 performance profile={profile:?} authority={authoritative_cells} control={control_cells} overlaps={overlaps} bundle={bundle_elapsed:?} generation_us={generation_micros:?} cancellation_us={cancellation_latency_micros:?} artifact_bytes={artifact_json_bytes:?}"
        );
        records.push(ProfilePerformance {
            profile,
            authoritative_cells,
            control_cells,
            conservative_map_overlaps: overlaps,
            bundle_build_micros: bundle_elapsed.as_micros(),
            generation_micros,
            cancellation_latency_micros,
            artifact_json_bytes,
        });
    }
    let evidence = PerformanceEvidence {
        schema_version: 1,
        profiles: records,
    };
    let bytes = serde_json::to_vec_pretty(&evidence).unwrap();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p2");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("performance.json"), &bytes).unwrap();
    eprintln!(
        "P2 performance bytes={} hash={}",
        bytes.len(),
        blake3::hash(&bytes).to_hex()
    );
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}
