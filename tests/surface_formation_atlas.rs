use std::collections::VecDeque;
use std::f64::consts::PI;
use std::path::PathBuf;

use image::{Rgba, RgbaImage};
use sekai::engine::{derive_stage_seed, BuildCancellation, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{
    ClimateWorkDomainBuilder, EvolvedTectonicGenerator, GeologicSubstrateGenerator,
    GlobalCirculationGenerator, GlobalClimateForcingBuilder, NaturalSurfaceFormationArtifact,
    PrimaryReliefGenerator, SurfaceFormationInputs,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    ClimateModelProfile, ClimateSpec, ClimateWorkDomainSnapshot, GeologicSpec, HydroErosionSpec,
    NaturalQualityProfile, NaturalSurfaceFormationSnapshot, PrimaryReliefSnapshot, ReliefSpec,
    ResolvedWorldFormation, ResolvedWorldFormationPreset, SurfaceWaterKind, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, RootSeed};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;
const WIDTH: u32 = 384;
const HEIGHT: u32 = 192;
const BACKGROUND: Rgba<u8> = Rgba([8, 12, 18, 255]);
const SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];

#[derive(Debug, Clone, Copy)]
enum AtlasField {
    PrimaryElevation,
    FinalElevation,
    ElevationChange,
    TectonicDisplacement,
    FluvialErosion,
    HillslopeErosion,
    HillslopeDeposition,
    RoutedSedimentDeposition,
    CoastalErosion,
    CoastalDeposition,
    IsostaticResponse,
    SurfaceWater,
    MeanAnnualDischarge,
    StrahlerOrder,
    SedimentThickness,
    DominantProvenance,
    DeltaPotential,
    FormationPrecipitation,
    ShelfDelivery,
}

impl AtlasField {
    const ALL: [Self; 19] = [
        Self::PrimaryElevation,
        Self::FinalElevation,
        Self::ElevationChange,
        Self::TectonicDisplacement,
        Self::FluvialErosion,
        Self::HillslopeErosion,
        Self::HillslopeDeposition,
        Self::RoutedSedimentDeposition,
        Self::CoastalErosion,
        Self::CoastalDeposition,
        Self::IsostaticResponse,
        Self::SurfaceWater,
        Self::MeanAnnualDischarge,
        Self::StrahlerOrder,
        Self::SedimentThickness,
        Self::DominantProvenance,
        Self::DeltaPotential,
        Self::FormationPrecipitation,
        Self::ShelfDelivery,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::PrimaryElevation => "primary-elevation-m",
            Self::FinalElevation => "final-elevation-m",
            Self::ElevationChange => "final-minus-primary-elevation-m",
            Self::TectonicDisplacement => "tectonic-displacement-m",
            Self::FluvialErosion => "fluvial-erosion-m",
            Self::HillslopeErosion => "hillslope-erosion-m",
            Self::HillslopeDeposition => "hillslope-deposition-m",
            Self::RoutedSedimentDeposition => "routed-sediment-deposition-m",
            Self::CoastalErosion => "coastal-erosion-m",
            Self::CoastalDeposition => "coastal-deposition-m",
            Self::IsostaticResponse => "isostatic-response-m",
            Self::SurfaceWater => "surface-water-class",
            Self::MeanAnnualDischarge => "mean-annual-discharge-m3-s",
            Self::StrahlerOrder => "strahler-order",
            Self::SedimentThickness => "sediment-thickness-m",
            Self::DominantProvenance => "dominant-sediment-source",
            Self::DeltaPotential => "delta-potential",
            Self::FormationPrecipitation => "formation-precipitation-mm-day",
            Self::ShelfDelivery => "shelf-delivery-kg",
        }
    }
}

#[derive(Serialize)]
struct AtlasManifest {
    schema_version: u16,
    profile: NaturalQualityProfile,
    width: u32,
    height: u32,
    columns: Vec<&'static str>,
    rows: Vec<&'static str>,
    seeds: Vec<u64>,
    note: &'static str,
}

#[test]
#[ignore = "release-only 17-seed P5 causal formation map/globe atlas"]
fn render_surface_formation_atlas() {
    let output = output_directory().join("atlas");
    if output.exists() {
        std::fs::remove_dir_all(&output).unwrap();
    }
    std::fs::create_dir_all(&output).unwrap();

    let cancellation = BuildCancellation::new();
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &cancellation,
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let domain =
        ClimateWorkDomainBuilder::build(surface, NaturalQualityProfile::Draft, &cancellation)
            .unwrap();
    let map = map_cell_raster(surface);
    let globe = globe_cell_raster(surface);

    for seed in SEEDS {
        let (relief, artifact) = generate_world(&bundle, &domain, seed);
        let sheet = render_sheet(&relief, artifact.snapshot(), &map, &globe);
        sheet
            .save(output.join(format!("seed-{seed:02}.png")))
            .unwrap();
        eprintln!("P5 atlas seed={seed} rendered");
    }

    let manifest = AtlasManifest {
        schema_version: 1,
        profile: NaturalQualityProfile::Draft,
        width: WIDTH,
        height: HEIGHT,
        columns: vec!["equirectangular-map", "oblique-globe"],
        rows: AtlasField::ALL.into_iter().map(AtlasField::label).collect(),
        seeds: SEEDS.to_vec(),
        note: "P5 diagnostic atlas. Each row is one causal formation field; columns are the \
               equirectangular map then the oblique globe. This raster-Voronoi sheet is \
               evidence, not the product renderer.",
    };
    std::fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn atlas_paths_are_isolated_under_target() {
    assert!(output_directory().ends_with("target/natural-quality/p5"));
}

fn render_sheet(
    relief: &PrimaryReliefSnapshot,
    snapshot: &NaturalSurfaceFormationSnapshot,
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
                        field_color(field, map[pixel], relief, snapshot),
                    );
                }
                if globe[pixel] != usize::MAX {
                    sheet.put_pixel(
                        WIDTH + x,
                        row_y + y,
                        field_color(field, globe[pixel], relief, snapshot),
                    );
                }
            }
        }
    }
    sheet
}

fn field_color(
    field: AtlasField,
    cell: usize,
    relief: &PrimaryReliefSnapshot,
    snapshot: &NaturalSurfaceFormationSnapshot,
) -> Rgba<u8> {
    let terrain = snapshot.terrain_fields();
    let components = terrain.elevation_components();
    let sediment = terrain.sediment();
    let hydrology = snapshot.hydrology();
    match field {
        AtlasField::PrimaryElevation => elevation_color(relief.elevation_m()[cell]),
        AtlasField::FinalElevation => elevation_color(terrain.final_elevation_m()[cell]),
        AtlasField::ElevationChange => signed_color(
            terrain.final_elevation_m()[cell] - components.primary_elevation_m()[cell],
            200.0,
        ),
        AtlasField::TectonicDisplacement => {
            signed_color(components.tectonic_displacement_m()[cell], 200.0)
        }
        AtlasField::FluvialErosion => {
            sequential_color(components.fluvial_erosion_m()[cell], 0.0, 200.0)
        }
        AtlasField::HillslopeErosion => {
            sequential_color(components.hillslope_erosion_m()[cell], 0.0, 200.0)
        }
        AtlasField::HillslopeDeposition => {
            sequential_color(components.hillslope_deposition_m()[cell], 0.0, 200.0)
        }
        AtlasField::RoutedSedimentDeposition => {
            sequential_color(components.routed_sediment_deposition_m()[cell], 0.0, 400.0)
        }
        AtlasField::CoastalErosion => {
            sequential_color(components.coastal_erosion_m()[cell], 0.0, 5.0)
        }
        AtlasField::CoastalDeposition => {
            sequential_color(components.coastal_deposition_m()[cell], 0.0, 400.0)
        }
        AtlasField::IsostaticResponse => {
            signed_color(components.isostatic_response_m()[cell], 100.0)
        }
        AtlasField::SurfaceWater => match hydrology.surface_water().get(cell) {
            Some(SurfaceWaterKind::Ocean) => Rgba([26, 62, 122, 255]),
            Some(SurfaceWaterKind::Lake) => Rgba([64, 168, 210, 255]),
            Some(SurfaceWaterKind::DryLand) => Rgba([120, 108, 84, 255]),
            None => BACKGROUND,
        },
        AtlasField::MeanAnnualDischarge => sequential_color(
            f64::from(hydrology.mean_annual_discharge_m3_s()[cell]).ln_1p() as f32,
            0.0,
            12.0,
        ),
        AtlasField::StrahlerOrder => sequential_color(
            hydrology.strahler_order().raw_values()[cell] as f32,
            0.0,
            8.0,
        ),
        AtlasField::SedimentThickness => {
            sequential_color(sediment.sediment_thickness_m()[cell], 0.0, 400.0)
        }
        AtlasField::DominantProvenance => provenance_color(sediment.provenance_fraction()[cell]),
        AtlasField::DeltaPotential => sequential_color(sediment.delta_potential()[cell], 0.0, 1.0),
        AtlasField::FormationPrecipitation => sequential_color(
            snapshot
                .formation_climate()
                .fields()
                .monthly_precipitation_mm_day()
                .values()[cell]
                .iter()
                .sum::<f32>()
                / 12.0,
            0.0,
            20.0,
        ),
        AtlasField::ShelfDelivery => sequential_color(
            sediment.shelf_delivery_kg()[cell].max(0.0).ln_1p() as f32,
            0.0,
            32.0,
        ),
    }
}

fn provenance_color(fractions: [f32; 5]) -> Rgba<u8> {
    let mut best = 0;
    for (index, value) in fractions.iter().enumerate() {
        if *value > fractions[best] {
            best = index;
        }
    }
    if fractions[best] <= 0.0 {
        return BACKGROUND;
    }
    match best {
        0 => Rgba([196, 84, 72, 255]),
        1 => Rgba([214, 168, 74, 255]),
        2 => Rgba([104, 176, 120, 255]),
        3 => Rgba([94, 128, 200, 255]),
        _ => Rgba([168, 116, 196, 255]),
    }
}

fn elevation_color(value: f32) -> Rgba<u8> {
    if value < 0.0 {
        gradient(
            Rgba([6, 18, 48, 255]),
            Rgba([48, 116, 176, 255]),
            ((value + 8_000.0) / 8_000.0).clamp(0.0, 1.0),
        )
    } else {
        gradient(
            Rgba([44, 96, 60, 255]),
            Rgba([238, 240, 244, 255]),
            (value / 5_000.0).clamp(0.0, 1.0),
        )
    }
}

fn sequential_color(value: f32, minimum: f32, maximum: f32) -> Rgba<u8> {
    let amount = ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0);
    gradient(Rgba([16, 22, 32, 255]), Rgba([250, 226, 128, 255]), amount)
}

fn signed_color(value: f32, scale: f32) -> Rgba<u8> {
    let amount = (value / scale).clamp(-1.0, 1.0);
    if amount >= 0.0 {
        gradient(Rgba([24, 26, 30, 255]), Rgba([236, 108, 84, 255]), amount)
    } else {
        gradient(Rgba([24, 26, 30, 255]), Rgba([84, 150, 236, 255]), -amount)
    }
}

fn gradient(low: Rgba<u8>, high: Rgba<u8>, amount: f32) -> Rgba<u8> {
    let blend = |first: u8, second: u8| {
        (f32::from(first) + (f32::from(second) - f32::from(first)) * amount).round() as u8
    };
    Rgba([
        blend(low.0[0], high.0[0]),
        blend(low.0[1], high.0[1]),
        blend(low.0[2], high.0[2]),
        255,
    ])
}

fn generate_world(
    bundle: &ProfileSurfaceBundle,
    domain: &ClimateWorkDomainSnapshot,
    seed: u64,
) -> (PrimaryReliefSnapshot, NaturalSurfaceFormationArtifact) {
    let cancellation = BuildCancellation::new();
    let surface = bundle.authoritative_surface();
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    let mut evolved_rng = stage_rng(seed, "natural.evolved-tectonics", 5);
    let evolved = EvolvedTectonicGenerator::generate(
        bundle,
        &TectonicSpec::default(),
        &formation,
        &mut evolved_rng,
    )
    .unwrap();
    let mut substrate_rng = stage_rng(seed, "natural.geologic-substrate", 1);
    let substrate = GeologicSubstrateGenerator::generate(
        surface,
        &evolved,
        &GeologicSpec::default(),
        &formation,
        &mut substrate_rng,
    )
    .unwrap();
    let mut relief_rng = stage_rng(seed, "natural.primary-relief", 1);
    let mut diagnostics = Vec::<Diagnostic>::new();
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
        &cancellation,
    )
    .unwrap();
    let initial_climate = GlobalCirculationGenerator::generate(
        surface,
        domain,
        &forcing,
        ClimateModelProfile::C2LayeredV1,
        &cancellation,
    )
    .unwrap();
    let artifact = NaturalSurfaceFormationArtifact::generate(
        SurfaceFormationInputs {
            surface,
            quality_profile: NaturalQualityProfile::Draft,
            tectonics: &evolved,
            substrate: &substrate,
            relief: &relief,
            domain,
            climate_spec: &ClimateSpec::default(),
            initial_climate: &initial_climate,
            formation_spec: &HydroErosionSpec::default(),
        },
        &cancellation,
    )
    .unwrap();
    (relief, artifact)
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

fn output_directory() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p5")
}
