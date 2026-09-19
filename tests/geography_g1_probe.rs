//! G1 continental-crust-on-plates probe (spec 2026-08-26-g1-continental-crust-on-plates-design).
//!
//! Diagnostic writer, not a gate. Run explicitly:
//! `cargo test --release --test geography_g1_probe probe_g1_continental_crust_on_plates -- --ignored --nocapture`

mod support;

use std::path::PathBuf;
use std::time::Instant;

use sekai::engine::BuildCancellation;
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    CrustKind, EvolvedTectonicSnapshot, GeologicSubstrateSnapshot, LandOceanKind,
    NaturalQualityProfile, PrimaryReliefSnapshot, ResolvedWorldFormationPreset, TectonicSpec,
    EARTH_WATER_REFERENCE_RADIUS_M,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::Meters;
use serde::Serialize;
use support::global_circulation::try_build_primary_relief_for;

const PRIMARY_SEED: u64 = 42;
const FALLBACK_SEED: u64 = 3;
const PRESETS: [ResolvedWorldFormationPreset; 5] = [
    ResolvedWorldFormationPreset::Continents,
    ResolvedWorldFormationPreset::Archipelago,
    ResolvedWorldFormationPreset::Supercontinent,
    ResolvedWorldFormationPreset::GreatIsland,
    ResolvedWorldFormationPreset::VolcanicIslands,
];

#[derive(Debug, Clone, Serialize)]
struct GeographyG1World {
    preset: String,
    seed: u64,
    elapsed_ms: u128,
    error: Option<String>,
    report: Option<GeographyG1Report>,
}

#[derive(Debug, Clone, Serialize)]
struct GeographyG1Report {
    preset: String,
    seed: u64,
    elapsed_ms: u128,
    mixed_plates: usize,
    cross_plate_continental_edges: usize,
    continental_area_fraction: f64,
    continental_inundation_share: f64,
}

#[test]
#[ignore]
fn probe_g1_continental_crust_on_plates() {
    let cancellation = BuildCancellation::new();
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
        &cancellation,
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let mut worlds = Vec::with_capacity(PRESETS.len());
    for preset in PRESETS {
        let tectonic_spec = TectonicSpec {
            continental_crust_fraction: preset.recommended_continental_crust_fraction(),
            ..TectonicSpec::default()
        };
        let started = Instant::now();
        let (seed, outcome) =
            match try_build_primary_relief_for(&bundle, PRIMARY_SEED, preset, &tectonic_spec) {
                Ok(ok) => (PRIMARY_SEED, Ok(ok)),
                Err(primary) if preset == ResolvedWorldFormationPreset::Archipelago => {
                    match try_build_primary_relief_for(
                        &bundle,
                        FALLBACK_SEED,
                        preset,
                        &tectonic_spec,
                    ) {
                        Ok(ok) => (FALLBACK_SEED, Ok(ok)),
                        Err(fallback) => (
                            FALLBACK_SEED,
                            Err(format!(
                                "seed {PRIMARY_SEED}: {primary}; seed {FALLBACK_SEED}: {fallback}"
                            )),
                        ),
                    }
                }
                Err(error) => (PRIMARY_SEED, Err(error)),
            };
        let elapsed_ms = started.elapsed().as_millis();
        match outcome {
            Ok((evolved, substrate, relief)) => {
                let report = measure_world(
                    preset, seed, elapsed_ms, surface, &evolved, &substrate, &relief,
                );
                println!(
                    "\n== G1 {preset:?} seed={seed} {elapsed_ms} ms mixed={} cross={} inund={:.3} ==",
                    report.mixed_plates,
                    report.cross_plate_continental_edges,
                    report.continental_inundation_share
                );
                worlds.push(GeographyG1World {
                    preset: format!("{preset:?}"),
                    seed,
                    elapsed_ms,
                    error: None,
                    report: Some(report),
                });
            }
            Err(error) => {
                println!("\n== G1 {preset:?} seed={seed} FAILED after {elapsed_ms} ms ==\n{error}");
                worlds.push(GeographyG1World {
                    preset: format!("{preset:?}"),
                    seed,
                    elapsed_ms,
                    error: Some(error),
                    report: None,
                });
            }
        }
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("g1");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("continental-crust-on-plates.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&worlds).unwrap()).unwrap();
    println!("wrote {}", path.display());
    assert_eq!(worlds.len(), PRESETS.len());
    assert!(
        worlds.iter().any(|world| world.report.is_some()),
        "G1 corpus produced no measurable P2+P3 worlds"
    );
}

fn measure_world(
    preset: ResolvedWorldFormationPreset,
    seed: u64,
    elapsed_ms: u128,
    surface: &SphericalSurfaceSnapshot,
    evolved: &EvolvedTectonicSnapshot,
    substrate: &GeologicSubstrateSnapshot,
    relief: &PrimaryReliefSnapshot,
) -> GeographyG1Report {
    let n = surface.cells().len();
    let areas: Vec<f64> = surface.cells().iter().map(|cell| cell.area.get()).collect();
    let total_area: f64 = areas.iter().sum();
    let kinds: Vec<CrustKind> = (0..n)
        .map(|index| substrate.crust_kind(index).expect("every cell has crust"))
        .collect();
    let land: Vec<LandOceanKind> = (0..n)
        .map(|index| {
            relief
                .land_ocean()
                .get(index)
                .expect("every cell has land/ocean")
        })
        .collect();
    let plates: Vec<u32> = (0..n)
        .map(|index| {
            evolved
                .compatibility()
                .cell_plates()
                .get(index)
                .expect("every cell has a plate")
                .raw()
        })
        .collect();

    let mut continental_area = 0.0;
    let mut inundated = 0.0;
    let mut plate_flags =
        vec![(false, false); plates.iter().copied().max().unwrap_or(0) as usize + 1];
    for index in 0..n {
        let slot = &mut plate_flags[plates[index] as usize];
        match kinds[index] {
            CrustKind::Continental => {
                slot.0 = true;
                continental_area += areas[index];
                if land[index] == LandOceanKind::Ocean {
                    inundated += areas[index];
                }
            }
            CrustKind::Oceanic => slot.1 = true,
        }
    }
    let mixed_plates = plate_flags
        .iter()
        .filter(|&&(continental, oceanic)| continental && oceanic)
        .count();
    let cross_plate_continental_edges = surface
        .edges()
        .iter()
        .filter(|edge| {
            let first = edge.cells[0].raw() as usize;
            let second = edge.cells[1].raw() as usize;
            kinds[first] == CrustKind::Continental
                && kinds[second] == CrustKind::Continental
                && plates[first] != plates[second]
        })
        .count();

    GeographyG1Report {
        preset: format!("{preset:?}"),
        seed,
        elapsed_ms,
        mixed_plates,
        cross_plate_continental_edges,
        continental_area_fraction: if total_area > 0.0 {
            continental_area / total_area
        } else {
            f64::NAN
        },
        continental_inundation_share: if continental_area > 0.0 {
            inundated / continental_area
        } else {
            f64::NAN
        },
    }
}
