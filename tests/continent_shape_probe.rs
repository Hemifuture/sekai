//! G4 stage-0 continental shape evidence (spec
//! `2026-10-08-g4-continental-geometry-physics-design.md` §7, §11).
//!
//! `probe_earth_reference_masks` turns a local ETOPO1 ice-surface grid into the
//! committed Earth fixtures; `probe_continent_shape_corpus` measures M1-M4 on
//! the published P2 crust-kind mask of the frozen seed corpus beside the
//! Earth reference and the equal-cap null model. Both are offline, ignored,
//! Release probes: the corpus property is statistical (testing-scope rule).

#[allow(dead_code)]
#[path = "support/continent_shape.rs"]
mod continent_shape;

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::OnceLock;

use continent_shape::{
    earth_fixture_directory, earth_fixture_path, encode_earth_mask, load_earth_mask,
    six_equal_caps_mask, CellGraph, EarthThreshold, ShapeMetrics,
};
use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::EvolvedTectonicGenerator;
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    CrustKind, NaturalQualityProfile, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    TectonicSpec, WorldFormationPreset, EARTH_WATER_REFERENCE_RADIUS_M,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, RootSeed};

/// The frozen eight-seed corpus of `tests/formation_seed_sweep.rs` (G4 §7.4).
const CORPUS_SEEDS: [u64; 8] = [
    14_500_982_625_797_044_708,
    11_728_661_556_832_718_318,
    12_307_902_622_372_036_327,
    4_153_584_564_816_370_805,
    7_499_274_601_121_372_060,
    5_744_981_634_611_769_885,
    5_828_067_323_727_643_059,
    7_755_195_920_647_622_459,
];

/// ETOPO1 grid-registered binary layout (`readme_etopo1_binary.txt`, NOAA NGDC):
/// 21601 x 10801 little-endian i16, first row at 90 N, first column at 180 W.
const ETOPO1_COLUMNS: usize = 21_601;
const ETOPO1_ROWS: usize = 10_801;
/// 10' sampling of the 1' grid, the diagnosis convention of G4 §7.2.
const ETOPO1_STRIDE: usize = 10;

fn profile_label(profile: NaturalQualityProfile) -> &'static str {
    match profile {
        NaturalQualityProfile::Draft => "draft",
        NaturalQualityProfile::Standard => "standard",
        NaturalQualityProfile::High => "high",
    }
}

fn profile_from_env() -> Vec<NaturalQualityProfile> {
    match std::env::var("SEKAI_SHAPE_PROFILE").as_deref() {
        Ok("draft") => vec![NaturalQualityProfile::Draft],
        Ok("standard") => vec![NaturalQualityProfile::Standard],
        _ => vec![
            NaturalQualityProfile::Draft,
            NaturalQualityProfile::Standard,
        ],
    }
}

fn bundle(profile: NaturalQualityProfile) -> &'static ProfileSurfaceBundle {
    static DRAFT: OnceLock<ProfileSurfaceBundle> = OnceLock::new();
    static STANDARD: OnceLock<ProfileSurfaceBundle> = OnceLock::new();
    let cell = match profile {
        NaturalQualityProfile::Draft => &DRAFT,
        _ => &STANDARD,
    };
    cell.get_or_init(|| {
        ProfileSurfaceBuilder::build(
            profile,
            Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap()
    })
}

/// Walking nearest-centroid locator: consecutive queries are spatially close,
/// so a greedy walk from the previous answer is O(1) amortised.
struct CellLocator<'a> {
    surface: &'a SphericalSurfaceSnapshot,
    neighbors: Vec<Vec<u32>>,
    last: usize,
}

impl<'a> CellLocator<'a> {
    fn new(surface: &'a SphericalSurfaceSnapshot) -> Self {
        let neighbors = surface
            .cells()
            .iter()
            .map(|cell| {
                surface
                    .cell_edges(cell.id)
                    .unwrap()
                    .iter()
                    .map(|&edge| surface.opposite_cell(cell.id, edge).unwrap().raw())
                    .collect()
            })
            .collect();
        Self {
            surface,
            neighbors,
            last: 0,
        }
    }

    fn locate(&mut self, point: [f64; 3]) -> usize {
        let score = |index: usize| {
            let centroid = self.surface.cells()[index].centroid.components();
            centroid[0] * point[0] + centroid[1] * point[1] + centroid[2] * point[2]
        };
        let mut best = self.last;
        let mut best_score = score(best);
        loop {
            let mut improved = false;
            for &neighbor in &self.neighbors[best] {
                let candidate = score(neighbor as usize);
                if candidate > best_score {
                    best = neighbor as usize;
                    best_score = candidate;
                    improved = true;
                }
            }
            if !improved {
                break;
            }
        }
        self.last = best;
        best
    }
}

fn geographic(latitude_deg: f64, longitude_deg: f64) -> [f64; 3] {
    let (latitude, longitude) = (latitude_deg.to_radians(), longitude_deg.to_radians());
    [
        latitude.cos() * longitude.cos(),
        latitude.cos() * longitude.sin(),
        latitude.sin(),
    ]
}

/// Cell means of the 10'-sampled ETOPO1 grid, each sample weighted by cos(lat)
/// (its share of sphere area).
fn etopo1_cell_means(surface: &SphericalSurfaceSnapshot, path: &PathBuf) -> Vec<f64> {
    let mut file = std::fs::File::open(path).expect("SEKAI_ETOPO1_PATH opens");
    assert_eq!(
        file.metadata().unwrap().len(),
        (ETOPO1_COLUMNS * ETOPO1_ROWS * 2) as u64,
        "ETOPO1 i2 grid-registered binary has 21601 x 10801 samples"
    );
    let mut locator = CellLocator::new(surface);
    let mut sums = vec![0.0_f64; surface.cells().len()];
    let mut weights = vec![0.0_f64; surface.cells().len()];
    let mut row_bytes = vec![0_u8; ETOPO1_COLUMNS * 2];
    for row in (0..ETOPO1_ROWS).step_by(ETOPO1_STRIDE) {
        file.seek(SeekFrom::Start((row * ETOPO1_COLUMNS * 2) as u64))
            .unwrap();
        file.read_exact(&mut row_bytes).unwrap();
        let latitude = 90.0 - row as f64 / 60.0;
        let weight = latitude.to_radians().cos();
        if weight <= 0.0 {
            continue;
        }
        // The last column repeats 180 W at 180 E; skip it.
        for column in (0..ETOPO1_COLUMNS - 1).step_by(ETOPO1_STRIDE) {
            let value = i16::from_le_bytes([row_bytes[2 * column], row_bytes[2 * column + 1]]);
            assert_ne!(value, -32_768, "ETOPO1 ice surface has no NODATA");
            let longitude = -180.0 + column as f64 / 60.0;
            let cell = locator.locate(geographic(latitude, longitude));
            sums[cell] += weight * f64::from(value);
            weights[cell] += weight;
        }
    }
    sums.iter()
        .zip(&weights)
        .map(|(&sum, &weight)| {
            assert!(weight > 0.0, "every cell receives 10' samples");
            sum / weight
        })
        .collect()
}

#[test]
#[ignore = "offline: needs SEKAI_ETOPO1_PATH (ETOPO1 ice-surface i2 binary), writes fixtures"]
fn probe_earth_reference_masks() {
    let path = PathBuf::from(
        std::env::var("SEKAI_ETOPO1_PATH").expect("SEKAI_ETOPO1_PATH names etopo1_ice_g_i2.bin"),
    );
    std::fs::create_dir_all(earth_fixture_directory()).unwrap();
    for profile in [
        NaturalQualityProfile::Draft,
        NaturalQualityProfile::Standard,
    ] {
        let surface = bundle(profile).authoritative_surface();
        let means = etopo1_cell_means(surface, &path);
        for threshold in [EarthThreshold::ShelfBreak, EarthThreshold::SeaLevel] {
            let mask = means
                .iter()
                .map(|&mean| mean > threshold.meters())
                .collect::<Vec<_>>();
            let bytes = encode_earth_mask(surface, &mask);
            let target = earth_fixture_path(profile_label(profile), threshold);
            std::fs::write(&target, &bytes).unwrap();
            println!(
                "{} {}: cells={} inside={} fraction={:.4} blake3={}",
                profile_label(profile),
                threshold.label(),
                mask.len(),
                mask.iter().filter(|&&inside| inside).count(),
                CellGraph::new(surface).metrics(&mask).area_fraction,
                blake3::hash(&bytes).to_hex()
            );
        }
        let surface_ref = sekai::world::spatial::SurfaceRef::for_spherical(surface);
        println!(
            "{} SurfaceRef fingerprint={} cells={} edges={}",
            profile_label(profile),
            blake3::Hash::from(surface_ref.fingerprint()).to_hex(),
            surface_ref.cell_count(),
            surface_ref.edge_count()
        );
    }
}

fn continents_mask(
    profile: NaturalQualityProfile,
    seed: u64,
    spec: &TectonicSpec,
) -> (Vec<bool>, f64) {
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
    ));
    let started = std::time::Instant::now();
    let snapshot =
        EvolvedTectonicGenerator::generate(bundle(profile), spec, &formation, &mut rng).unwrap();
    let elapsed = started.elapsed().as_secs_f64();
    let surface = bundle(profile).authoritative_surface();
    let compatibility = snapshot.compatibility();
    let mask = surface
        .cells()
        .iter()
        .map(|cell| compatibility.crust_kind(cell.id) == Some(CrustKind::Continental))
        .collect();
    (mask, elapsed)
}

fn print_metrics(label: &str, metrics: ShapeMetrics) {
    println!(
        "{label}: M1={:.4} M2={:.4} M3={:.4} M4={:.4} major={} area={:.4}",
        metrics.six_cap_iou,
        metrics.major_area_cv,
        metrics.max_elongation,
        metrics.box_dimension,
        metrics.major_blocks,
        metrics.area_fraction
    );
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        0.5 * (values[middle - 1] + values[middle])
    } else {
        values[middle]
    }
}

/// Writes an equirectangular PNG of a mask (land white, ocean dark) for the
/// user's visual comparison when `SEKAI_SHAPE_PNG_DIR` is set.
fn write_mask_png(surface: &SphericalSurfaceSnapshot, mask: &[bool], name: &str) {
    let Some(directory) = std::env::var_os("SEKAI_SHAPE_PNG_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    let (width, height) = (1_024_u32, 512_u32);
    let mut locator = CellLocator::new(surface);
    let mut image = image::GrayImage::new(width, height);
    for y in 0..height {
        let latitude = 90.0 - (f64::from(y) + 0.5) * 180.0 / f64::from(height);
        for x in 0..width {
            let longitude = -180.0 + (f64::from(x) + 0.5) * 360.0 / f64::from(width);
            let cell = locator.locate(geographic(latitude, longitude));
            image.put_pixel(x, y, image::Luma([if mask[cell] { 235 } else { 40 }]));
        }
    }
    image.save(directory.join(format!("{name}.png"))).unwrap();
}

#[test]
#[ignore = "release-only G4 stage-0 corpus probe: M1-M4 of 8 seeds beside Earth and the null model"]
fn probe_continent_shape_corpus() {
    let spec = TectonicSpec {
        continental_crust_fraction: ResolvedWorldFormationPreset::Continents
            .recommended_continental_crust_fraction(),
        ..TectonicSpec::default()
    };
    let tag = std::env::var("SEKAI_SHAPE_TAG").unwrap_or_else(|_| "probe".to_owned());
    for profile in profile_from_env() {
        let label = profile_label(profile);
        let surface = bundle(profile).authoritative_surface();
        let graph = CellGraph::new(surface);
        println!(
            "== {label}: cells={} activity={:?} plates={} fraction={}",
            surface.cells().len(),
            spec.activity,
            spec.plate_count,
            spec.continental_crust_fraction
        );
        for threshold in [EarthThreshold::ShelfBreak, EarthThreshold::SeaLevel] {
            match load_earth_mask(surface, &earth_fixture_path(label, threshold)) {
                Ok(mask) => {
                    print_metrics(
                        &format!("{label} earth {}", threshold.label()),
                        graph.metrics(&mask),
                    );
                    write_mask_png(
                        surface,
                        &mask,
                        &format!("{label}-earth-{}", threshold.label()),
                    );
                }
                Err(error) => {
                    println!("{label} earth {}: unavailable ({error})", threshold.label())
                }
            }
        }
        let null = six_equal_caps_mask(surface, f64::from(spec.continental_crust_fraction));
        print_metrics(&format!("{label} null six caps"), graph.metrics(&null));
        let mut columns: [Vec<f64>; 4] = Default::default();
        for seed in CORPUS_SEEDS {
            let (mask, elapsed) = continents_mask(profile, seed, &spec);
            let metrics = graph.metrics(&mask);
            print_metrics(&format!("{label} seed {seed} p2={elapsed:.3}s"), metrics);
            write_mask_png(surface, &mask, &format!("{tag}-{label}-{seed}"));
            columns[0].push(metrics.six_cap_iou);
            columns[1].push(metrics.major_area_cv);
            columns[2].push(metrics.max_elongation);
            columns[3].push(metrics.box_dimension);
        }
        let [m1, m2, m3, m4] = columns.map(median);
        println!("{label} median: M1={m1:.4} M2={m2:.4} M3={m3:.4} M4={m4:.4}");
    }
}

/// The helper must reproduce the analytic null-model limits (G4 §7.3: M1 = 1,
/// M2 = 0, M3 = 1, M4 = 1) on the equal-cap mask up to grid discretisation.
/// The bounds are coarse implementation-error checks, not quality gates:
/// a wrong cap radius, a broken Lloyd step or a mis-signed slope moves the
/// values far outside them.
#[test]
fn shape_metrics_recover_the_equal_cap_null_model() {
    let surface = bundle(NaturalQualityProfile::Draft).authoritative_surface();
    let graph = CellGraph::new(surface);
    let metrics = graph.metrics(&six_equal_caps_mask(surface, 0.38));
    assert_eq!(metrics.major_blocks, 6, "{metrics:?}");
    assert!(metrics.six_cap_iou > 0.95, "{metrics:?}");
    assert!(metrics.major_area_cv < 0.02, "{metrics:?}");
    assert!(metrics.max_elongation < 1.1, "{metrics:?}");
    assert!((metrics.box_dimension - 1.0).abs() < 0.2, "{metrics:?}");
}
