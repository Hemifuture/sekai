use std::collections::VecDeque;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::EvolvedTectonicGenerator;
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    CrustKind, NaturalQualityProfile, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    SphericalTectonicLineageBudget, SphericalTectonicMaterialBudget, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
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
    PlateOwner,
    ContinentalMaterial,
    OceanAge,
    Uplift,
    Subsidence,
    Shortening,
    BoundaryDistance,
    AccumulatedElevation,
}

impl AtlasField {
    const ALL: [Self; 8] = [
        Self::PlateOwner,
        Self::ContinentalMaterial,
        Self::OceanAge,
        Self::Uplift,
        Self::Subsidence,
        Self::Shortening,
        Self::BoundaryDistance,
        Self::AccumulatedElevation,
    ];

    const fn slug(self) -> &'static str {
        match self {
            Self::PlateOwner => "plate-owner",
            Self::ContinentalMaterial => "continental-material-fraction",
            Self::OceanAge => "ocean-age",
            Self::Uplift => "uplift-rate",
            Self::Subsidence => "subsidence-rate",
            Self::Shortening => "shortening-rate",
            Self::BoundaryDistance => "boundary-distance",
            Self::AccumulatedElevation => "accumulated-tectonic-elevation",
        }
    }
}

#[derive(Serialize)]
struct AtlasManifest {
    schema_version: u16,
    renderer: &'static str,
    rows: Vec<&'static str>,
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
    plate_count: usize,
    material_budget: SphericalTectonicMaterialBudget,
    lineage_budget: SphericalTectonicLineageBudget,
}

#[test]
#[ignore = "release-only 17-seed P2 material/plate/forcing diagnostic atlas"]
fn render_evolved_tectonic_atlas() {
    let output = output_directory().join("atlas");
    if output.exists() {
        std::fs::remove_dir_all(&output).unwrap();
    }
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join("README.txt"),
        "P2 diagnostic atlas. Each row is listed in manifest.json; columns are equirectangular map and fixed oblique globe. The bounded raster-Voronoi renderer is diagnostic evidence, not the P9 product renderer.\n",
    )
    .unwrap();

    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let formation = formation();
    let mut images = Vec::new();
    for seed in SEEDS {
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
        ));
        let snapshot = EvolvedTectonicGenerator::generate(
            &bundle,
            &TectonicSpec::default(),
            &formation,
            &mut rng,
        )
        .unwrap();
        let sheet = render_sheet(bundle.authoritative_surface(), &snapshot);
        let file_name = format!("seed-{seed:06}.png");
        let path = output.join(&file_name);
        sheet.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(
            output.join(format!("seed-{seed:06}.json")),
            serde_json::to_vec_pretty(&AtlasSeedMetadata {
                seed,
                plate_count: snapshot.compatibility().plates().len(),
                material_budget: *snapshot.material_budget(),
                lineage_budget: *snapshot.lineage_budget(),
            })
            .unwrap(),
        )
        .unwrap();
        eprintln!(
            "P2 atlas seed={seed} path={} bytes={} hash={}",
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
        rows: AtlasField::ALL.into_iter().map(AtlasField::slug).collect(),
        images,
    };
    std::fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn render_sheet(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &sekai::world::natural::EvolvedTectonicSnapshot,
) -> RgbaImage {
    let mut sheet =
        RgbaImage::from_pixel(WIDTH * 2, HEIGHT * AtlasField::ALL.len() as u32, BACKGROUND);
    let map = map_cell_raster(surface);
    let globe = globe_cell_raster(surface);

    for (row, field) in AtlasField::ALL.into_iter().enumerate() {
        let row_y = row as u32 * HEIGHT;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let pixel = y as usize * WIDTH as usize + x as usize;
                if map[pixel] != usize::MAX {
                    sheet.put_pixel(x, row_y + y, field_color(field, map[pixel], snapshot));
                }
                if globe[pixel] != usize::MAX {
                    sheet.put_pixel(
                        WIDTH + x,
                        row_y + y,
                        field_color(field, globe[pixel], snapshot),
                    );
                }
            }
        }
    }
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
    snapshot: &sekai::world::natural::EvolvedTectonicSnapshot,
) -> Rgba<u8> {
    let tectonic = snapshot.compatibility();
    let material = snapshot.material();
    let forcing = snapshot.forcing();
    match field {
        AtlasField::PlateOwner => plate_color(tectonic.cell_plates().raw_values()[index]),
        AtlasField::ContinentalMaterial => {
            let continental = material.continental_reference_area_m2()[index];
            let total = continental + material.oceanic_reference_area_m2()[index];
            gradient(
                Rgba([28, 74, 121, 255]),
                Rgba([206, 157, 82, 255]),
                (continental / total) as f32,
            )
        }
        AtlasField::OceanAge => {
            if tectonic.crust_kinds().get(index) == Some(CrustKind::Continental) {
                Rgba([92, 71, 58, 255])
            } else {
                gradient(
                    Rgba([246, 201, 84, 255]),
                    Rgba([26, 57, 128, 255]),
                    (tectonic.crust_age_myr()[index] / 180.0).clamp(0.0, 1.0),
                )
            }
        }
        AtlasField::Uplift => forcing_color(
            forcing.uplift_rate_mm_per_year()[index],
            Rgba([248, 82, 55, 255]),
        ),
        AtlasField::Subsidence => forcing_color(
            forcing.subsidence_rate_mm_per_year()[index],
            Rgba([36, 132, 224, 255]),
        ),
        AtlasField::Shortening => forcing_color(
            forcing.shortening_rate_mm_per_year()[index],
            Rgba([211, 74, 224, 255]),
        ),
        AtlasField::BoundaryDistance => gradient(
            Rgba([244, 240, 204, 255]),
            Rgba([16, 26, 42, 255]),
            (forcing.boundary_distance_m()[index] / 2_000_000.0).clamp(0.0, 1.0),
        ),
        AtlasField::AccumulatedElevation => elevation_color(tectonic.tectonic_elevation_m()[index]),
    }
}

fn forcing_color(value: f32, active: Rgba<u8>) -> Rgba<u8> {
    let amount = (value / 10.0).clamp(0.0, 1.0).sqrt();
    gradient(Rgba([18, 22, 29, 255]), active, amount)
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

fn plate_color(plate: u32) -> Rgba<u8> {
    let mut hash = plate.wrapping_mul(0x9E37_79B9).wrapping_add(0x85EB_CA6B);
    hash ^= hash >> 16;
    Rgba([
        70 + (hash & 0x7f) as u8,
        70 + ((hash >> 8) & 0x7f) as u8,
        70 + ((hash >> 16) & 0x7f) as u8,
        255,
    ])
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
        .join("p2")
}

#[test]
fn atlas_palette_and_projection_are_deterministic_and_finite() {
    assert_eq!(plate_color(7), plate_color(7));
    assert_ne!(plate_color(7), plate_color(8));
    for point in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        assert!(rotate_oblique(point).into_iter().all(f64::is_finite));
    }
}
