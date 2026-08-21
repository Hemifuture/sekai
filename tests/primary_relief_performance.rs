use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use sekai::engine::{derive_stage_seed, BuildCancellation, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_primary_relief_quality, EvolvedTectonicGenerationError, EvolvedTectonicGenerator,
    GeologicSubstrateGenerator, PrimaryReliefArtifact, PrimaryReliefGenerator,
};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    GeologicSpec, NaturalQualityProfile, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
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
    evolved_generation_micros: Option<u128>,
    substrate_generation_micros: Option<u128>,
    primary_relief_generation_micros: Option<u128>,
    quality_evaluation_micros: Option<u128>,
    total_pipeline_micros: Option<u128>,
    cancellation_phase: Option<&'static str>,
    cancellation_latency_micros: Option<u128>,
    artifact_json_bytes: Option<usize>,
}

#[test]
#[ignore = "release-only Draft completion and Standard/High P3-pipeline cancellation evidence"]
fn measure_primary_relief_profiles() {
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

        let record = if profile == NaturalQualityProfile::Draft {
            let pipeline_started = Instant::now();
            let evolved_started = Instant::now();
            let mut evolved_rng = stage_rng(42, "natural.evolved-tectonics", 5, None);
            let evolved = EvolvedTectonicGenerator::generate(
                &bundle,
                &TectonicSpec::default(),
                &formation,
                &mut evolved_rng,
            )
            .unwrap();
            let evolved_elapsed = evolved_started.elapsed();
            assert!(
                evolved_elapsed <= Duration::from_secs(120),
                "Draft evolved tectonics took {evolved_elapsed:?}"
            );

            let substrate_started = Instant::now();
            let mut substrate_rng = stage_rng(42, "natural.geologic-substrate", 1, None);
            let substrate = GeologicSubstrateGenerator::generate(
                bundle.authoritative_surface(),
                &evolved,
                &GeologicSpec::default(),
                &formation,
                &mut substrate_rng,
            )
            .unwrap();
            let substrate_elapsed = substrate_started.elapsed();
            assert!(
                substrate_elapsed <= Duration::from_secs(30),
                "Draft geologic substrate took {substrate_elapsed:?}"
            );

            let relief_started = Instant::now();
            let mut relief_rng = stage_rng(42, "natural.primary-relief", 1, None);
            let mut diagnostics = Vec::<Diagnostic>::new();
            let relief = PrimaryReliefGenerator::generate(
                bundle.authoritative_surface(),
                &evolved,
                &substrate,
                &ReliefSpec::default(),
                &mut relief_rng,
                &mut diagnostics,
            )
            .unwrap();
            let relief_elapsed = relief_started.elapsed();
            assert!(
                relief_elapsed <= Duration::from_secs(30),
                "Draft primary relief took {relief_elapsed:?}"
            );

            let quality_started = Instant::now();
            let report = evaluate_primary_relief_quality(
                bundle.authoritative_surface(),
                &evolved,
                &substrate,
                &relief,
            )
            .unwrap();
            let quality_elapsed = quality_started.elapsed();
            let artifact_bytes =
                serde_json::to_vec(&PrimaryReliefArtifact::new(relief, report)).unwrap();
            let pipeline_elapsed = pipeline_started.elapsed();
            assert!(
                pipeline_elapsed <= Duration::from_secs(180),
                "Draft complete P3 pipeline took {pipeline_elapsed:?}"
            );
            ProfilePerformance {
                profile,
                authoritative_cells,
                control_cells,
                conservative_map_overlaps: overlaps,
                bundle_build_micros: bundle_elapsed.as_micros(),
                evolved_generation_micros: Some(evolved_elapsed.as_micros()),
                substrate_generation_micros: Some(substrate_elapsed.as_micros()),
                primary_relief_generation_micros: Some(relief_elapsed.as_micros()),
                quality_evaluation_micros: Some(quality_elapsed.as_micros()),
                total_pipeline_micros: Some(pipeline_elapsed.as_micros()),
                cancellation_phase: None,
                cancellation_latency_micros: None,
                artifact_json_bytes: Some(artifact_bytes.len()),
            }
        } else {
            let cancellation = BuildCancellation::new();
            let worker_cancellation = cancellation.clone();
            let started = Arc::new(Barrier::new(2));
            let worker_started = Arc::clone(&started);
            let worker_formation = formation.clone();
            let worker = std::thread::spawn(move || {
                let mut rng = stage_rng(
                    73,
                    "natural.evolved-tectonics",
                    5,
                    Some(&worker_cancellation),
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
                "{profile:?} P3 pipeline cancellation took {latency:?}"
            );
            ProfilePerformance {
                profile,
                authoritative_cells,
                control_cells,
                conservative_map_overlaps: overlaps,
                bundle_build_micros: bundle_elapsed.as_micros(),
                evolved_generation_micros: None,
                substrate_generation_micros: None,
                primary_relief_generation_micros: None,
                quality_evaluation_micros: None,
                total_pipeline_micros: None,
                cancellation_phase: Some("evolved-tectonics-upstream"),
                cancellation_latency_micros: Some(latency.as_micros()),
                artifact_json_bytes: None,
            }
        };

        eprintln!(
            "P3 performance profile={profile:?} authority={} control={} bundle_us={} total_us={:?} cancel_us={:?}",
            record.authoritative_cells,
            record.control_cells,
            record.bundle_build_micros,
            record.total_pipeline_micros,
            record.cancellation_latency_micros,
        );
        records.push(record);
    }
    let evidence = PerformanceEvidence {
        schema_version: 1,
        profiles: records,
    };
    let bytes = serde_json::to_vec_pretty(&evidence).unwrap();
    let directory = output_directory();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("performance.json"), &bytes).unwrap();
    eprintln!(
        "P3 performance bytes={} hash={}",
        bytes.len(),
        blake3::hash(&bytes).to_hex()
    );
}

fn stage_rng(
    seed: u64,
    name: &'static str,
    version: u32,
    cancellation: Option<&BuildCancellation>,
) -> StageRng {
    let seed = derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(name, version, "sekai.core"),
    );
    cancellation.map_or_else(
        || StageRng::from_seed(seed),
        |signal| StageRng::from_seed_with_cancellation(seed, signal),
    )
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn output_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p3")
}

#[test]
fn performance_evidence_path_is_isolated_under_target() {
    assert!(output_directory().ends_with("target/natural-quality/p3"));
}
