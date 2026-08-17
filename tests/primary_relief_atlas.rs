use std::collections::VecDeque;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use sekai::engine::{derive_stage_seed, BuildCancellation, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{
    EvolvedTectonicGenerator, GeologicSubstrateGenerator, PrimaryReliefGenerator,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    BedrockKind, GeologicSpec, LandOceanKind, NaturalQualityProfile, ReliefSpec,
    ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, RootSeed};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;
const WIDTH: u32 = 512;
const HEIGHT: u32 = 256;
const BACKGROUND: Rgba<u8> = Rgba([9, 13, 19, 255]);
const SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];

#[derive(Debug, Clone, Copy)]
enum AtlasField {
    CrustDensity,
    BedrockLithology,
    IsostaticBase,
    DynamicTectonic,
    VolcanicConstruction,
    PassiveMargin,
    RegionalDetail,
    Elevation,
    PhysicalWater,
}

impl AtlasField {
    const ALL: [Self; 9] = [
        Self::CrustDensity,
        Self::BedrockLithology,
        Self::IsostaticBase,
        Self::DynamicTectonic,
        Self::VolcanicConstruction,
        Self::PassiveMargin,
        Self::RegionalDetail,
        Self::Elevation,
        Self::PhysicalWater,
    ];

    const fn slug(self) -> &'static str {
        match self {
            Self::CrustDensity => "crust-density",
            Self::BedrockLithology => "bedrock-lithology",
            Self::IsostaticBase => "isostatic-base",
            Self::DynamicTectonic => "dynamic-tectonic-response",
            Self::VolcanicConstruction => "volcanic-construction",
            Self::PassiveMargin => "passive-margin",
            Self::RegionalDetail => "conditioned-regional-detail",
            Self::Elevation => "primary-elevation",
            Self::PhysicalWater => "physical-water",
        }
    }
}

#[derive(Serialize)]
struct AtlasManifest {
    schema_version: u16,
    renderer: &'static str,
    row_order: Vec<&'static str>,
    column_order: [&'static str; 2],
    images: Vec<AtlasImage>,
}

#[derive(Serialize)]
struct AtlasImage {
    seed: u64,
    path: String,
    bytes: usize,
    blake3: String,
}

#[derive(Serialize)]
struct AtlasSeedMetadata {
    seed: u64,
    sea_level_m: f32,
    physical_land_fraction: f32,
    requested_land_fraction: f32,
    water_volume_relative_error: f64,
    diagnostics: usize,
}

struct AtlasWorld {
    substrate: sekai::world::natural::GeologicSubstrateSnapshot,
    relief: sekai::world::natural::PrimaryReliefSnapshot,
    diagnostics: Vec<Diagnostic>,
}

#[test]
#[ignore = "release-only 17-seed P3 substrate/relief/water diagnostic atlas"]
fn render_primary_relief_atlas() {
    let output = output_directory().join("atlas");
    if output.exists() {
        std::fs::remove_dir_all(&output).unwrap();
    }
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join("README.txt"),
        "P3 diagnostic atlas. Rows follow manifest.json; columns are equirectangular map and fixed oblique globe. Colors expose causal components and physical water classification. This bounded raster-Voronoi renderer is diagnostic evidence, not the P9 product renderer.\n",
    )
    .unwrap();

    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let formation = formation();
    let map = map_cell_raster(bundle.authoritative_surface());
    let globe = globe_cell_raster(bundle.authoritative_surface());
    let mut images = Vec::new();
    for seed in SEEDS {
        let world = generate_world(&bundle, &formation, seed);
        let sheet = render_sheet(
            bundle.authoritative_surface(),
            &world.substrate,
            &world.relief,
            &map,
            &globe,
        );
        let file_name = format!("seed-{seed:06}.png");
        let path = output.join(&file_name);
        sheet.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(
            output.join(format!("seed-{seed:06}.json")),
            serde_json::to_vec_pretty(&AtlasSeedMetadata {
                seed,
                sea_level_m: world.relief.sea_level_m(),
                physical_land_fraction: world.relief.physical_land_fraction(),
                requested_land_fraction: world.relief.requested_land_fraction(),
                water_volume_relative_error: world.relief.water_volume_relative_error(),
                diagnostics: world.diagnostics.len(),
            })
            .unwrap(),
        )
        .unwrap();
        eprintln!(
            "P3 atlas seed={seed} path={} bytes={} hash={}",
            path.display(),
            bytes.len(),
            blake3::hash(&bytes).to_hex(),
        );
        images.push(AtlasImage {
            seed,
            path: file_name,
            bytes: bytes.len(),
            blake3: blake3::hash(&bytes).to_hex().to_string(),
        });
    }
    let manifest = AtlasManifest {
        schema_version: 1,
        renderer: "bounded-raster-voronoi-v1",
        row_order: AtlasField::ALL.into_iter().map(AtlasField::slug).collect(),
        column_order: ["equirectangular-map", "fixed-oblique-globe"],
        images,
    };
    std::fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn generate_world(
    bundle: &ProfileSurfaceBundle,
    formation: &ResolvedWorldFormation,
    seed: u64,
) -> AtlasWorld {
    let surface = bundle.authoritative_surface();
    let mut evolved_rng = stage_rng(seed, "natural.evolved-tectonics", 5);
    let evolved = EvolvedTectonicGenerator::generate(
        bundle,
        &TectonicSpec::default(),
        formation,
        &mut evolved_rng,
    )
    .unwrap();
    let mut substrate_rng = stage_rng(seed, "natural.geologic-substrate", 1);
    let substrate = GeologicSubstrateGenerator::generate(
        surface,
        &evolved,
        &GeologicSpec::default(),
        formation,
        &mut substrate_rng,
    )
    .unwrap();
    let mut relief_rng = stage_rng(seed, "natural.primary-relief", 1);
    let mut diagnostics = Vec::new();
    let relief = PrimaryReliefGenerator::generate(
        surface,
        &evolved,
        &substrate,
        &ReliefSpec::default(),
        &mut relief_rng,
        &mut diagnostics,
    )
    .unwrap();
    AtlasWorld {
        substrate,
        relief,
        diagnostics,
    }
}

fn render_sheet(
    surface: &SphericalSurfaceSnapshot,
    substrate: &sekai::world::natural::GeologicSubstrateSnapshot,
    relief: &sekai::world::natural::PrimaryReliefSnapshot,
    map: &[usize],
    globe: &[usize],
) -> RgbaImage {
    let mut sheet =
        RgbaImage::from_pixel(WIDTH * 2, HEIGHT * AtlasField::ALL.len() as u32, BACKGROUND);
    for (row, field) in AtlasField::ALL.into_iter().enumerate() {
        let row_y = row as u32 * HEIGHT;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let pixel = y as usize * WIDTH as usize + x as usize;
                if map[pixel] != usize::MAX {
                    sheet.put_pixel(
                        x,
                        row_y + y,
                        field_color(field, map[pixel], substrate, relief),
                    );
                }
                if globe[pixel] != usize::MAX {
                    sheet.put_pixel(
                        WIDTH + x,
                        row_y + y,
                        field_color(field, globe[pixel], substrate, relief),
                    );
                }
            }
        }
    }
    debug_assert_eq!(surface.cells().len(), substrate.cell_count() as usize);
    sheet
}

fn map_cell_raster(surface: &SphericalSurfaceSnapshot) -> Vec<usize> {
    let seeds = surface
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let [x, y, z] = cell.centroid.components();
            let longitude = y.atan2(x);
            let latitude = z.clamp(-1.0, 1.0).asin();
            let pixel_x = ((longitude / (2.0 * PI) + 0.5) * f64::from(WIDTH))
                .floor()
                .clamp(0.0, f64::from(WIDTH - 1)) as u32;
            let pixel_y = ((0.5 - latitude / PI) * f64::from(HEIGHT))
                .floor()
                .clamp(0.0, f64::from(HEIGHT - 1)) as u32;
            (pixel_x, pixel_y, index)
        })
        .collect::<Vec<_>>();
    flood_cells(&seeds, &vec![true; (WIDTH * HEIGHT) as usize], true)
}

fn globe_cell_raster(surface: &SphericalSurfaceSnapshot) -> Vec<usize> {
    let radius = f64::from(HEIGHT) * 0.47;
    let center_x = f64::from(WIDTH) * 0.5;
    let center_y = f64::from(HEIGHT) * 0.5;
    let mut inside = vec![false; (WIDTH * HEIGHT) as usize];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            inside[y as usize * WIDTH as usize + x as usize] =
                (f64::from(x) - center_x).hypot(f64::from(y) - center_y) <= radius;
        }
    }
    let seeds = surface
        .cells()
        .iter()
        .enumerate()
        .filter_map(|(index, cell)| {
            let point = rotate_oblique(cell.centroid.components());
            if point[2] < 0.0 {
                return None;
            }
            let pixel_x = (center_x + point[0] * radius)
                .round()
                .clamp(0.0, f64::from(WIDTH - 1)) as u32;
            let pixel_y = (center_y - point[1] * radius)
                .round()
                .clamp(0.0, f64::from(HEIGHT - 1)) as u32;
            Some((pixel_x, pixel_y, index))
        })
        .collect::<Vec<_>>();
    flood_cells(&seeds, &inside, false)
}

fn flood_cells(seeds: &[(u32, u32, usize)], inside: &[bool], wrap_x: bool) -> Vec<usize> {
    let mut cells = vec![usize::MAX; (WIDTH * HEIGHT) as usize];
    let mut distances = vec![u32::MAX; cells.len()];
    let mut queue = VecDeque::new();
    for &(x, y, cell) in seeds {
        let pixel = y as usize * WIDTH as usize + x as usize;
        if inside[pixel] && (distances[pixel] != 0 || cell < cells[pixel]) {
            distances[pixel] = 0;
            cells[pixel] = cell;
            queue.push_back(pixel);
        }
    }
    while let Some(pixel) = queue.pop_front() {
        let x = (pixel % WIDTH as usize) as i32;
        let y = (pixel / WIDTH as usize) as i32;
        let distance = distances[pixel].saturating_add(1);
        let cell = cells[pixel];
        for [mut next_x, next_y] in [[x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]] {
            if wrap_x {
                next_x = next_x.rem_euclid(WIDTH as i32);
            }
            if next_x < 0 || next_y < 0 || next_x >= WIDTH as i32 || next_y >= HEIGHT as i32 {
                continue;
            }
            let next = next_y as usize * WIDTH as usize + next_x as usize;
            if !inside[next] {
                continue;
            }
            if distance < distances[next] || (distance == distances[next] && cell < cells[next]) {
                distances[next] = distance;
                cells[next] = cell;
                queue.push_back(next);
            }
        }
    }
    cells
}

fn field_color(
    field: AtlasField,
    index: usize,
    substrate: &sekai::world::natural::GeologicSubstrateSnapshot,
    relief: &sekai::world::natural::PrimaryReliefSnapshot,
) -> Rgba<u8> {
    match field {
        AtlasField::CrustDensity => gradient(
            Rgba([234, 211, 146, 255]),
            Rgba([30, 55, 94, 255]),
            ((substrate.crust_density_kg_m3()[index] - 2_700.0) / 400.0).clamp(0.0, 1.0),
        ),
        AtlasField::BedrockLithology => match substrate.bedrock_kind(index).unwrap() {
            BedrockKind::OceanicMafic => Rgba([42, 70, 102, 255]),
            BedrockKind::ContinentalCrystalline => Rgba([184, 146, 104, 255]),
            BedrockKind::Sedimentary => Rgba([215, 190, 119, 255]),
            BedrockKind::Metamorphic => Rgba([139, 92, 153, 255]),
            BedrockKind::Volcanic => Rgba([96, 49, 45, 255]),
        },
        AtlasField::IsostaticBase => elevation_color(relief.isostatic_base_m()[index]),
        AtlasField::DynamicTectonic => {
            signed_component_color(relief.dynamic_tectonic_offset_m()[index], 5_000.0)
        }
        AtlasField::VolcanicConstruction => gradient(
            Rgba([20, 23, 28, 255]),
            Rgba([243, 101, 48, 255]),
            (relief.volcanic_construction_m()[index] / 4_000.0)
                .clamp(0.0, 1.0)
                .sqrt(),
        ),
        AtlasField::PassiveMargin => {
            signed_component_color(relief.passive_margin_offset_m()[index], 1_200.0)
        }
        AtlasField::RegionalDetail => {
            signed_component_color(relief.conditioned_regional_detail_m()[index], 900.0)
        }
        AtlasField::Elevation => elevation_color(relief.elevation_m()[index]),
        AtlasField::PhysicalWater => match relief.land_ocean().get(index).unwrap() {
            LandOceanKind::Land => gradient(
                Rgba([74, 124, 67, 255]),
                Rgba([238, 231, 209, 255]),
                ((relief.elevation_m()[index] - relief.sea_level_m()) / 4_000.0).clamp(0.0, 1.0),
            ),
            LandOceanKind::Ocean => gradient(
                Rgba([16, 35, 83, 255]),
                Rgba([59, 148, 190, 255]),
                (1.0 - (relief.sea_level_m() - relief.elevation_m()[index]) / 6_000.0)
                    .clamp(0.0, 1.0),
            ),
        },
    }
}

fn signed_component_color(value: f32, scale: f32) -> Rgba<u8> {
    if value < 0.0 {
        gradient(
            Rgba([24, 27, 32, 255]),
            Rgba([46, 116, 201, 255]),
            (-value / scale).clamp(0.0, 1.0).sqrt(),
        )
    } else {
        gradient(
            Rgba([24, 27, 32, 255]),
            Rgba([232, 102, 49, 255]),
            (value / scale).clamp(0.0, 1.0).sqrt(),
        )
    }
}

fn elevation_color(value: f32) -> Rgba<u8> {
    if value < 0.0 {
        gradient(
            Rgba([12, 31, 79, 255]),
            Rgba([86, 151, 184, 255]),
            ((value + 7_000.0) / 7_000.0).clamp(0.0, 1.0),
        )
    } else {
        gradient(
            Rgba([75, 116, 67, 255]),
            Rgba([245, 239, 222, 255]),
            (value / 6_000.0).clamp(0.0, 1.0),
        )
    }
}

fn gradient(first: Rgba<u8>, second: Rgba<u8>, amount: f32) -> Rgba<u8> {
    let amount = amount.clamp(0.0, 1.0);
    Rgba(std::array::from_fn(|channel| {
        if channel == 3 {
            255
        } else {
            (f32::from(first[channel])
                + (f32::from(second[channel]) - f32::from(first[channel])) * amount)
                .round() as u8
        }
    }))
}

fn rotate_oblique([x, y, z]: [f64; 3]) -> [f64; 3] {
    let yaw = -25.0_f64.to_radians();
    let pitch = -58.0_f64.to_radians();
    let yawed = [
        x * yaw.cos() - y * yaw.sin(),
        x * yaw.sin() + y * yaw.cos(),
        z,
    ];
    [
        yawed[0],
        yawed[1] * pitch.cos() - yawed[2] * pitch.sin(),
        yawed[1] * pitch.sin() + yawed[2] * pitch.cos(),
    ]
}

fn stage_rng(seed: u64, name: &'static str, version: u32) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(name, version, "sekai.core"),
    ))
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn output_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p3")
}

#[test]
fn atlas_palette_projection_and_row_order_are_deterministic() {
    assert_eq!(AtlasField::ALL.len(), 9);
    assert_eq!(AtlasField::ALL[8].slug(), "physical-water");
    assert_ne!(
        signed_component_color(-500.0, 1_000.0),
        signed_component_color(500.0, 1_000.0)
    );
    for point in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        assert!(rotate_oblique(point).into_iter().all(f64::is_finite));
    }
}
