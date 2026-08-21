use std::collections::{BTreeMap, VecDeque};
use std::f64::consts::PI;
use std::path::PathBuf;

use image::{Rgba, RgbaImage};
use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    ClimateWorkDomainBuilder, EvolvedTectonicGenerator, GeologicSubstrateGenerator,
    GlobalCirculationArtifact, GlobalClimateForcingBuilder, PrimaryReliefGenerator,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    ClimateSpec, ClimateWorkDomainSnapshot, GeologicSpec, GlobalCirculationSnapshot,
    NaturalQualityProfile, PrimaryReliefSnapshot, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::{canonical_east_north_basis, SphericalSurfaceSnapshot};
use sekai::world::{Meters, RootSeed};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;
const WIDTH: u32 = 384;
const HEIGHT: u32 = 192;
const BACKGROUND: Rgba<u8> = Rgba([8, 12, 18, 255]);
const MONTHS: [usize; 2] = [0, 6];
const SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];

#[derive(Debug, Clone, Copy)]
enum AtlasField {
    Elevation,
    LowerWind,
    UpperWind,
    VerticalShear,
    SurfaceCurrent,
    AirTemperature,
    SeaSurfaceTemperature,
    ThermoclineTemperature,
    ThermoclineDepth,
    DeepOceanTemperature,
    SpecificHumidity,
    Precipitation,
    OrographicPrecipitation,
    LowerAtmosphereHeight,
    UpperAtmosphereHeight,
    SeaSurfaceHeight,
    ThermoclineHeight,
    SolverDiagnostic,
    RemapDiagnostic,
}

impl AtlasField {
    const ALL: [Self; 19] = [
        Self::Elevation,
        Self::LowerWind,
        Self::UpperWind,
        Self::VerticalShear,
        Self::SurfaceCurrent,
        Self::AirTemperature,
        Self::SeaSurfaceTemperature,
        Self::ThermoclineTemperature,
        Self::ThermoclineDepth,
        Self::DeepOceanTemperature,
        Self::SpecificHumidity,
        Self::Precipitation,
        Self::OrographicPrecipitation,
        Self::LowerAtmosphereHeight,
        Self::UpperAtmosphereHeight,
        Self::SeaSurfaceHeight,
        Self::ThermoclineHeight,
        Self::SolverDiagnostic,
        Self::RemapDiagnostic,
    ];

    const fn slug(self) -> &'static str {
        match self {
            Self::Elevation => "p3-elevation",
            Self::LowerWind => "near-surface-wind",
            Self::UpperWind => "upper-wind",
            Self::VerticalShear => "vertical-wind-shear",
            Self::SurfaceCurrent => "surface-ocean-current",
            Self::AirTemperature => "air-temperature",
            Self::SeaSurfaceTemperature => "sea-surface-temperature",
            Self::ThermoclineTemperature => "thermocline-temperature",
            Self::ThermoclineDepth => "thermocline-depth",
            Self::DeepOceanTemperature => "deep-ocean-temperature",
            Self::SpecificHumidity => "specific-humidity",
            Self::Precipitation => "precipitation",
            Self::OrographicPrecipitation => "orographic-precipitation",
            Self::LowerAtmosphereHeight => "lower-atmosphere-height-anomaly",
            Self::UpperAtmosphereHeight => "upper-atmosphere-height-anomaly",
            Self::SeaSurfaceHeight => "sea-surface-height-anomaly",
            Self::ThermoclineHeight => "thermocline-height-anomaly",
            Self::SolverDiagnostic => "solver-final-residual",
            Self::RemapDiagnostic => "remap-margin-error",
        }
    }
}

#[derive(Serialize)]
struct AtlasManifest {
    schema_version: u16,
    renderer: &'static str,
    months: [usize; 2],
    row_order: Vec<&'static str>,
    column_order: [&'static str; 4],
    images: Vec<AtlasImage>,
}

#[derive(Serialize)]
struct AtlasImage {
    seed: u64,
    path: String,
    bytes: usize,
    blake3: String,
    checkpoint_fingerprint: String,
    artifact_json_hash: String,
}

#[derive(Serialize)]
struct AtlasSeedMetadata {
    seed: u64,
    sea_level_m: f32,
    physical_land_fraction: f32,
    formation_cycles: u16,
    final_residual: f64,
    maximum_cfl: f64,
    dense_state_bytes: u64,
    forward_overlaps: u32,
    reverse_overlaps: u32,
    maximum_remap_margin_error: f64,
    checkpoint_fingerprint: String,
    artifact_json_bytes: usize,
    artifact_json_hash: String,
    quality_metric_count: usize,
}

struct AtlasWorld {
    relief: PrimaryReliefSnapshot,
    artifact: GlobalCirculationArtifact,
}

#[test]
#[ignore = "release-only 17-seed P4 seasonal map/globe diagnostic atlas"]
fn render_global_circulation_atlas() {
    let output = output_directory().join("atlas");
    if output.exists() {
        std::fs::remove_dir_all(&output).unwrap();
    }
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join("README.txt"),
        "P4 diagnostic atlas. Rows follow manifest.json; columns are January map/globe then July map/globe. Vector hue encodes tangent direction and brightness encodes speed. This raster-Voronoi sheet is evidence, not the P9 product renderer.\n",
    )
    .unwrap();

    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let domain = ClimateWorkDomainBuilder::build(
        surface,
        NaturalQualityProfile::Draft,
        &BuildCancellation::new(),
    )
    .unwrap();
    let formation = formation();
    let expected_artifact_hashes = load_evidence_artifact_hashes();
    let map = map_cell_raster(surface);
    let globe = globe_cell_raster(surface);
    let mut images = Vec::new();
    for seed in SEEDS {
        let world = generate_world(&bundle, &domain, &formation, seed);
        let climate = world.artifact.snapshot();
        let artifact_json = serde_json::to_vec(&world.artifact).unwrap();
        let artifact_json_hash = blake3::hash(&artifact_json).to_hex().to_string();
        assert_eq!(
            expected_artifact_hashes.get(&seed),
            Some(&artifact_json_hash),
            "atlas seed {seed} must render the exact quality-gated evidence product"
        );
        let checkpoint_fingerprint = hex(climate.checkpoint().fingerprint());
        let sheet = render_sheet(surface, &world.relief, climate, &map, &globe);
        let file_name = format!("seed-{seed:06}.png");
        let path = output.join(&file_name);
        sheet.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let remap = climate.remap_report();
        let max_remap = maximum_remap_error(climate);
        std::fs::write(
            output.join(format!("seed-{seed:06}.json")),
            serde_json::to_vec_pretty(&AtlasSeedMetadata {
                seed,
                sea_level_m: world.relief.sea_level_m(),
                physical_land_fraction: world.relief.physical_land_fraction(),
                formation_cycles: climate.solve_report().formation_years(),
                final_residual: climate.solve_report().final_residual(),
                maximum_cfl: climate.solve_report().maximum_cfl(),
                dense_state_bytes: climate.solve_report().dense_state_bytes(),
                forward_overlaps: remap.forward_overlap_count(),
                reverse_overlaps: remap.reverse_overlap_count(),
                maximum_remap_margin_error: max_remap,
                checkpoint_fingerprint: checkpoint_fingerprint.clone(),
                artifact_json_bytes: artifact_json.len(),
                artifact_json_hash: artifact_json_hash.clone(),
                quality_metric_count: world.artifact.quality_report().metrics().len(),
            })
            .unwrap(),
        )
        .unwrap();
        eprintln!(
            "P4 atlas seed={seed} path={} bytes={} hash={}",
            path.display(),
            bytes.len(),
            blake3::hash(&bytes).to_hex(),
        );
        images.push(AtlasImage {
            seed,
            path: file_name,
            bytes: bytes.len(),
            blake3: blake3::hash(&bytes).to_hex().to_string(),
            checkpoint_fingerprint,
            artifact_json_hash,
        });
    }
    let manifest = AtlasManifest {
        schema_version: 1,
        renderer: "bounded-raster-voronoi-seasonal-v1",
        months: MONTHS,
        row_order: AtlasField::ALL.into_iter().map(AtlasField::slug).collect(),
        column_order: [
            "january-equirectangular-map",
            "january-fixed-oblique-globe",
            "july-equirectangular-map",
            "july-fixed-oblique-globe",
        ],
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
    domain: &ClimateWorkDomainSnapshot,
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
    let forcing = GlobalClimateForcingBuilder::build(
        surface,
        &relief,
        &ClimateSpec::default(),
        domain,
        &BuildCancellation::new(),
    )
    .unwrap();
    let artifact = GlobalCirculationArtifact::generate(
        surface,
        domain,
        &forcing,
        &relief,
        &BuildCancellation::new(),
    )
    .unwrap();
    AtlasWorld { relief, artifact }
}

fn load_evidence_artifact_hashes() -> BTreeMap<u64, String> {
    let path = output_directory().join("evidence.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "run write_global_circulation_evidence before the atlas writer ({}): {error}",
            path.display()
        )
    });
    let evidence: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    evidence["seeds"]
        .as_array()
        .expect("P4 evidence seeds")
        .iter()
        .map(|seed| {
            (
                seed["seed"].as_u64().expect("evidence seed id"),
                seed["artifact_json_hash"]
                    .as_str()
                    .expect("evidence artifact hash")
                    .to_owned(),
            )
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn render_sheet(
    surface: &SphericalSurfaceSnapshot,
    relief: &PrimaryReliefSnapshot,
    climate: &GlobalCirculationSnapshot,
    map: &[usize],
    globe: &[usize],
) -> RgbaImage {
    let mut sheet =
        RgbaImage::from_pixel(WIDTH * 4, HEIGHT * AtlasField::ALL.len() as u32, BACKGROUND);
    for (row, field) in AtlasField::ALL.into_iter().enumerate() {
        let row_y = row as u32 * HEIGHT;
        for (month_column, month) in MONTHS.into_iter().enumerate() {
            let column_x = month_column as u32 * WIDTH * 2;
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let pixel = y as usize * WIDTH as usize + x as usize;
                    if map[pixel] != usize::MAX {
                        sheet.put_pixel(
                            column_x + x,
                            row_y + y,
                            field_color(field, map[pixel], month, surface, relief, climate),
                        );
                    }
                    if globe[pixel] != usize::MAX {
                        sheet.put_pixel(
                            column_x + WIDTH + x,
                            row_y + y,
                            field_color(field, globe[pixel], month, surface, relief, climate),
                        );
                    }
                }
            }
        }
    }
    sheet
}

fn field_color(
    field: AtlasField,
    cell: usize,
    month: usize,
    surface: &SphericalSurfaceSnapshot,
    relief: &PrimaryReliefSnapshot,
    climate: &GlobalCirculationSnapshot,
) -> Rgba<u8> {
    let fields = climate.fields();
    match field {
        AtlasField::Elevation => elevation_color(relief.elevation_m()[cell]),
        AtlasField::LowerWind => vector_color(
            fields.near_surface_wind_m_s().values()[cell][month],
            surface,
            cell,
            20.0,
        ),
        AtlasField::UpperWind => vector_color(
            fields.upper_wind_m_s().unwrap().values()[cell][month],
            surface,
            cell,
            20.0,
        ),
        AtlasField::VerticalShear => vector_color(
            fields.vertical_wind_shear_m_s().unwrap().values()[cell][month],
            surface,
            cell,
            8.0,
        ),
        AtlasField::SurfaceCurrent => vector_color(
            fields.surface_ocean_current_m_s().values()[cell][month],
            surface,
            cell,
            0.5,
        ),
        AtlasField::AirTemperature => temperature_color(
            fields.monthly_air_temperature_c().values()[cell][month],
            -45.0,
            35.0,
        ),
        AtlasField::SeaSurfaceTemperature => temperature_color(
            fields.monthly_sea_surface_temperature_c().values()[cell][month],
            -2.0,
            35.0,
        ),
        AtlasField::ThermoclineTemperature => temperature_color(
            fields.monthly_thermocline_temperature_c().unwrap().values()[cell][month],
            -5.0,
            30.0,
        ),
        AtlasField::ThermoclineDepth => sequential_color(
            fields.monthly_thermocline_depth_m().unwrap().values()[cell][month],
            0.0,
            1_500.0,
            Rgba([30, 49, 91, 255]),
            Rgba([102, 236, 214, 255]),
        ),
        AtlasField::DeepOceanTemperature => temperature_color(
            fields.monthly_deep_ocean_temperature_c().unwrap().values()[cell][month],
            -5.0,
            25.0,
        ),
        AtlasField::SpecificHumidity => sequential_color(
            fields.monthly_specific_humidity().values()[cell][month],
            0.0,
            0.03,
            Rgba([51, 40, 88, 255]),
            Rgba([106, 245, 221, 255]),
        ),
        AtlasField::Precipitation => sequential_color(
            fields.monthly_precipitation_mm_day().values()[cell][month],
            0.0,
            30.0,
            Rgba([42, 30, 59, 255]),
            Rgba([124, 213, 255, 255]),
        ),
        AtlasField::OrographicPrecipitation => sequential_color(
            fields.monthly_orographic_precipitation_mm_day().values()[cell][month],
            0.0,
            30.0,
            Rgba([42, 30, 59, 255]),
            Rgba([247, 222, 96, 255]),
        ),
        AtlasField::LowerAtmosphereHeight => signed_color(
            fields.monthly_lower_atmosphere_height_anomaly_m().values()[cell][month],
            800.0,
        ),
        AtlasField::UpperAtmosphereHeight => signed_color(
            fields
                .monthly_upper_atmosphere_height_anomaly_m()
                .unwrap()
                .values()[cell][month],
            800.0,
        ),
        AtlasField::SeaSurfaceHeight => signed_color(
            fields.monthly_sea_surface_height_anomaly_m().values()[cell][month],
            5.0,
        ),
        AtlasField::ThermoclineHeight => signed_color(
            fields
                .monthly_thermocline_height_anomaly_m()
                .unwrap()
                .values()[cell][month],
            200.0,
        ),
        AtlasField::SolverDiagnostic => {
            diagnostic_color(climate.solve_report().final_residual() / 0.25)
        }
        AtlasField::RemapDiagnostic => diagnostic_color(maximum_remap_error(climate) / 1.0e-10),
    }
}

fn maximum_remap_error(climate: &GlobalCirculationSnapshot) -> f64 {
    let report = climate.remap_report();
    [
        report.forward_source_margin_relative_error(),
        report.forward_target_margin_relative_error(),
        report.reverse_source_margin_relative_error(),
        report.reverse_target_margin_relative_error(),
    ]
    .into_iter()
    .fold(0.0, f64::max)
}

fn vector_color(
    vector: [f32; 3],
    surface: &SphericalSurfaceSnapshot,
    cell: usize,
    scale: f64,
) -> Rgba<u8> {
    let (east, north) = canonical_east_north_basis(surface.cells()[cell].centroid);
    let vector = vector.map(f64::from);
    let zonal = dot(vector, east);
    let meridional = dot(vector, north);
    let angle = meridional.atan2(zonal);
    let hue = (angle / std::f64::consts::TAU + 1.0).fract();
    let speed = zonal.hypot(meridional);
    let value = 0.18 + 0.82 * (speed / scale).clamp(0.0, 1.0).sqrt();
    hsv(hue, 0.82, value)
}

fn hsv(hue: f64, saturation: f64, value: f64) -> Rgba<u8> {
    let sector = hue * 6.0;
    let index = sector.floor() as u8 % 6;
    let fraction = sector - sector.floor();
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * fraction);
    let t = value * (1.0 - saturation * (1.0 - fraction));
    let (red, green, blue) = match index {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    Rgba([
        (255.0 * red).round() as u8,
        (255.0 * green).round() as u8,
        (255.0 * blue).round() as u8,
        255,
    ])
}

fn temperature_color(value: f32, minimum: f32, maximum: f32) -> Rgba<u8> {
    let t = ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0);
    if t < 0.5 {
        gradient(
            Rgba([25, 55, 137, 255]),
            Rgba([224, 236, 221, 255]),
            t * 2.0,
        )
    } else {
        gradient(
            Rgba([224, 236, 221, 255]),
            Rgba([188, 43, 36, 255]),
            (t - 0.5) * 2.0,
        )
    }
}

fn sequential_color(
    value: f32,
    minimum: f32,
    maximum: f32,
    low: Rgba<u8>,
    high: Rgba<u8>,
) -> Rgba<u8> {
    gradient(
        low,
        high,
        ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0),
    )
}

fn signed_color(value: f32, scale: f32) -> Rgba<u8> {
    let normalized = (value / scale).clamp(-1.0, 1.0);
    if normalized < 0.0 {
        gradient(
            Rgba([223, 230, 223, 255]),
            Rgba([35, 85, 185, 255]),
            -normalized,
        )
    } else {
        gradient(
            Rgba([223, 230, 223, 255]),
            Rgba([211, 58, 44, 255]),
            normalized,
        )
    }
}

fn diagnostic_color(fraction_of_limit: f64) -> Rgba<u8> {
    gradient(
        Rgba([42, 145, 88, 255]),
        Rgba([214, 62, 53, 255]),
        fraction_of_limit.clamp(0.0, 1.0) as f32,
    )
}

fn elevation_color(value: f32) -> Rgba<u8> {
    if value < 0.0 {
        gradient(
            Rgba([11, 29, 75, 255]),
            Rgba([73, 151, 188, 255]),
            ((value + 7_000.0) / 7_000.0).clamp(0.0, 1.0),
        )
    } else {
        gradient(
            Rgba([70, 118, 68, 255]),
            Rgba([244, 238, 219, 255]),
            (value / 6_000.0).clamp(0.0, 1.0),
        )
    }
}

fn gradient(low: Rgba<u8>, high: Rgba<u8>, amount: f32) -> Rgba<u8> {
    Rgba(std::array::from_fn(|component| {
        if component == 3 {
            255
        } else {
            (f32::from(low[component])
                + amount * (f32::from(high[component]) - f32::from(low[component])))
            .round()
            .clamp(0.0, 255.0) as u8
        }
    }))
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

fn rotate_oblique([x, y, z]: [f64; 3]) -> [f64; 3] {
    let yaw = -35.0_f64.to_radians();
    let pitch = 22.0_f64.to_radians();
    let yawed = [
        yaw.cos() * x - yaw.sin() * y,
        yaw.sin() * x + yaw.cos() * y,
        z,
    ];
    [
        yawed[0],
        pitch.cos() * yawed[1] - pitch.sin() * yawed[2],
        pitch.sin() * yawed[1] + pitch.cos() * yawed[2],
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

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn output_directory() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p4")
}

#[test]
fn atlas_paths_are_isolated_under_target() {
    assert!(output_directory().ends_with("target/natural-quality/p4"));
}
