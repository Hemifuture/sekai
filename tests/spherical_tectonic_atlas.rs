use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use image::{imageops, Rgba, RgbaImage};
use sekai::app::{build_spherical_external_artifacts, build_spherical_presentation_candidate};
use sekai::engine::{BuildEngine, MemoryStageCache};
use sekai::generators::natural::{
    spherical_natural_foundation_graph, SphericalHydroErosionArtifact, SphericalTectonicArtifact,
};
use sekai::generators::spatial::SphericalSurfaceArtifact;
use sekai::view::{
    DisplayRevisionClock, PreparedProjectedMap, ProjectionBounds, ProjectionPoint,
    SphericalFieldDisplayState,
};
use sekai::world::natural::{
    BoundaryKind, CrustKind, GeologicSpec, ReliefSpec, SphericalTectonicSnapshot, TectonicSpec,
    WorldFormationSpec,
};
use sekai::world::spatial::{canonical_east_north_basis, SphericalSurfaceSnapshot, UnitVector3};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const VIEW_WIDTH: u32 = 512;
const VIEW_HEIGHT: u32 = 256;
const BACKGROUND: Rgba<u8> = Rgba([12, 16, 22, 255]);
const EMPTY_CELL: u32 = u32::MAX;

#[derive(Debug, Clone, Copy)]
struct AtlasConfig {
    seeds: [u64; 17],
    target_cell_count: u32,
    render_map: bool,
    render_globe: bool,
}

impl AtlasConfig {
    fn formal_seed_matrix() -> Self {
        Self {
            seeds: [
                42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
            ],
            target_cell_count: 20_000,
            render_map: true,
            render_globe: true,
        }
    }
}

#[derive(Debug)]
struct AtlasError(String);

impl fmt::Display for AtlasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for AtlasError {}

#[derive(Debug, Clone, Copy)]
enum AtlasField {
    PlateOwner,
    CrustKind,
    CrustAge,
    TectonicElevation,
    FinalElevation,
    Boundary,
    Lineation,
}

impl AtlasField {
    const ALL: [Self; 7] = [
        Self::PlateOwner,
        Self::CrustKind,
        Self::CrustAge,
        Self::TectonicElevation,
        Self::FinalElevation,
        Self::Boundary,
        Self::Lineation,
    ];

    const fn slug(self) -> &'static str {
        match self {
            Self::PlateOwner => "plate-owner",
            Self::CrustKind => "crust-kind",
            Self::CrustAge => "crust-age",
            Self::TectonicElevation => "tectonic-elevation",
            Self::FinalElevation => "final-elevation",
            Self::Boundary => "boundary-kind-strength",
            Self::Lineation => "lineation",
        }
    }
}

struct CellRaster {
    width: u32,
    height: u32,
    cells: Vec<u32>,
    depth: Vec<f32>,
}

impl CellRaster {
    fn new(width: u32, height: u32) -> Self {
        let len = width as usize * height as usize;
        Self {
            width,
            height,
            cells: vec![EMPTY_CELL; len],
            depth: vec![f32::NEG_INFINITY; len],
        }
    }

    fn draw_triangle(&mut self, points: [[f32; 3]; 3], cell: u32, depth_test: bool) {
        let min_x = points
            .iter()
            .map(|point| point[0])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let max_x = points
            .iter()
            .map(|point| point[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.width.saturating_sub(1) as f32) as u32;
        let min_y = points
            .iter()
            .map(|point| point[1])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let max_y = points
            .iter()
            .map(|point| point[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.height.saturating_sub(1) as f32) as u32;
        if min_x > max_x || min_y > max_y {
            return;
        }

        let area = edge(points[0], points[1], points[2]);
        if !area.is_finite() || area.abs() <= f32::EPSILON {
            return;
        }
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let sample = [x as f32 + 0.5, y as f32 + 0.5, 0.0];
                let weights = [
                    edge(points[1], points[2], sample) / area,
                    edge(points[2], points[0], sample) / area,
                    edge(points[0], points[1], sample) / area,
                ];
                if weights.iter().any(|weight| *weight < -1.0e-5) {
                    continue;
                }
                let index = y as usize * self.width as usize + x as usize;
                let depth = weights[0] * points[0][2]
                    + weights[1] * points[1][2]
                    + weights[2] * points[2][2];
                if !depth_test || depth > self.depth[index] {
                    self.depth[index] = depth;
                    self.cells[index] = cell;
                }
            }
        }
    }
}

fn edge(first: [f32; 3], second: [f32; 3], point: [f32; 3]) -> f32 {
    (point[0] - first[0]) * (second[1] - first[1]) - (point[1] - first[1]) * (second[0] - first[0])
}

fn render_atlas(config: AtlasConfig, output: &Path) -> Result<(), AtlasError> {
    if output.exists() {
        std::fs::remove_dir_all(output).map_err(atlas_error)?;
    }
    std::fs::create_dir_all(output).map_err(atlas_error)?;
    std::fs::write(
        output.join("README.txt"),
        "Rows: plate owner, crust kind, crust age, tectonic elevation, final elevation, boundary kind/strength, lineation. Columns: Equal Earth map, undeformed unit globe.\n",
    )
    .map_err(atlas_error)?;

    for (seed_index, seed) in config.seeds.into_iter().enumerate() {
        let seed_dir = output.join(format!("seed-{seed:06}"));
        std::fs::create_dir_all(&seed_dir).map_err(atlas_error)?;
        let mut cache = MemoryStageCache::default();
        let space = SphericalSpaceSpec {
            radius: Meters::new(EARTH_RADIUS_M).map_err(atlas_error)?,
            target_cell_count: config.target_cell_count,
        };
        let formation = WorldFormationSpec::default();
        let tectonic_spec = TectonicSpec::default();
        let relief_spec = ReliefSpec::default();
        let geologic = GeologicSpec::default();
        let external = build_spherical_external_artifacts(
            &space,
            &formation,
            &tectonic_spec,
            &relief_spec,
            &geologic,
        )
        .map_err(atlas_error)?;
        let outcome = BuildEngine::new(spherical_natural_foundation_graph().map_err(atlas_error)?)
            .build(RootSeed::new(seed), external, &mut cache)
            .map_err(atlas_error)?;
        let surface = outcome
            .artifacts
            .get::<SphericalSurfaceArtifact>()
            .map_err(atlas_error)?;
        let tectonic = outcome
            .artifacts
            .get::<SphericalTectonicArtifact>()
            .map_err(atlas_error)?;
        let final_surface = outcome
            .artifacts
            .get::<SphericalHydroErosionArtifact>()
            .map_err(atlas_error)?;
        let candidate = build_spherical_presentation_candidate(
            RootSeed::new(seed),
            &space,
            &formation,
            &tectonic_spec,
            &relief_spec,
            &geologic,
            &mut cache,
            &SphericalFieldDisplayState::default(),
            &DisplayRevisionClock::default(),
        )
        .map_err(atlas_error)?;

        let map_raster = config
            .render_map
            .then(|| rasterize_map(candidate.map(), VIEW_WIDTH, VIEW_HEIGHT));
        let globe_raster = config.render_globe.then(|| {
            rasterize_globe(
                candidate.globe().vertices(),
                candidate.globe().indices(),
                VIEW_WIDTH,
                VIEW_HEIGHT,
            )
        });
        let mut sheet = RgbaImage::from_pixel(
            VIEW_WIDTH * u32::from(config.render_map) + VIEW_WIDTH * u32::from(config.render_globe),
            VIEW_HEIGHT * AtlasField::ALL.len() as u32,
            BACKGROUND,
        );
        for (row, field) in AtlasField::ALL.into_iter().enumerate() {
            let mut column = 0_u32;
            if let Some(raster) = map_raster.as_ref() {
                let mut image = colorize(
                    raster,
                    field,
                    tectonic.snapshot(),
                    final_surface
                        .snapshot()
                        .surface()
                        .surface_elevation_m()
                        .values(),
                );
                decorate_map(
                    &mut image,
                    field,
                    candidate.map(),
                    surface.snapshot(),
                    tectonic.snapshot(),
                );
                save_view(&seed_dir, field, "map", &image)?;
                imageops::overlay(
                    &mut sheet,
                    &image,
                    column.into(),
                    (row as u32 * VIEW_HEIGHT).into(),
                );
                column += VIEW_WIDTH;
            }
            if let Some(raster) = globe_raster.as_ref() {
                let mut image = colorize(
                    raster,
                    field,
                    tectonic.snapshot(),
                    final_surface
                        .snapshot()
                        .surface()
                        .surface_elevation_m()
                        .values(),
                );
                decorate_globe(&mut image, field, surface.snapshot(), tectonic.snapshot());
                save_view(&seed_dir, field, "globe", &image)?;
                imageops::overlay(
                    &mut sheet,
                    &image,
                    column.into(),
                    (row as u32 * VIEW_HEIGHT).into(),
                );
            }
        }
        sheet
            .save(seed_dir.join("contact-sheet.png"))
            .map_err(atlas_error)?;
        println!(
            "tectonic atlas {}/{}: seed {} -> {} cells, {} plates",
            seed_index + 1,
            config.seeds.len(),
            seed,
            surface.snapshot().cells().len(),
            tectonic.snapshot().plates().len()
        );
    }
    Ok(())
}

fn rasterize_map(map: &PreparedProjectedMap, width: u32, height: u32) -> CellRaster {
    let mut raster = CellRaster::new(width, height);
    for triangle in map.indices().chunks_exact(3) {
        let vertices = [
            map.vertices()[triangle[0] as usize],
            map.vertices()[triangle[1] as usize],
            map.vertices()[triangle[2] as usize],
        ];
        debug_assert!(vertices
            .iter()
            .all(|vertex| vertex.cell() == vertices[0].cell()));
        let points =
            vertices.map(|vertex| map_to_screen(vertex.position(), map.bounds(), width, height));
        raster.draw_triangle(points, vertices[0].cell().raw(), false);
    }
    raster
}

fn rasterize_globe(
    vertices: &[sekai::view::GlobeVertex],
    indices: &[u32],
    width: u32,
    height: u32,
) -> CellRaster {
    let mut raster = CellRaster::new(width, height);
    for triangle in indices.chunks_exact(3) {
        let source = [
            vertices[triangle[0] as usize],
            vertices[triangle[1] as usize],
            vertices[triangle[2] as usize],
        ];
        debug_assert!(source
            .iter()
            .all(|vertex| vertex.cell() == source[0].cell()));
        let rotated = source.map(|vertex| rotate_oblique(vertex.position()));
        let clipped = clip_front(&rotated);
        for side in 1..clipped.len().saturating_sub(1) {
            let points = [clipped[0], clipped[side], clipped[side + 1]]
                .map(|point| globe_to_screen(point, width, height));
            raster.draw_triangle(points, source[0].cell().raw(), true);
        }
    }
    raster
}

fn map_to_screen(
    point: ProjectionPoint,
    bounds: ProjectionBounds,
    width: u32,
    height: u32,
) -> [f32; 3] {
    [
        (((point.x() - bounds.min_x()) / (bounds.max_x() - bounds.min_x())) * f64::from(width))
            as f32,
        (((bounds.max_y() - point.y()) / (bounds.max_y() - bounds.min_y())) * f64::from(height))
            as f32,
        0.0,
    ]
}

fn rotate_oblique(point: [f32; 3]) -> [f32; 3] {
    let yaw = -25.0_f32.to_radians();
    let pitch = -58.0_f32.to_radians();
    let yawed = [
        point[0] * yaw.cos() - point[1] * yaw.sin(),
        point[0] * yaw.sin() + point[1] * yaw.cos(),
        point[2],
    ];
    [
        yawed[0],
        yawed[1] * pitch.cos() - yawed[2] * pitch.sin(),
        yawed[1] * pitch.sin() + yawed[2] * pitch.cos(),
    ]
}

fn clip_front(triangle: &[[f32; 3]; 3]) -> Vec<[f32; 3]> {
    let mut output = Vec::with_capacity(4);
    let mut previous = triangle[2];
    let mut previous_inside = previous[2] >= 0.0;
    for &current in triangle {
        let current_inside = current[2] >= 0.0;
        if previous_inside != current_inside {
            let amount = previous[2] / (previous[2] - current[2]);
            output.push(std::array::from_fn(|axis| {
                previous[axis] + (current[axis] - previous[axis]) * amount
            }));
        }
        if current_inside {
            output.push(current);
        }
        previous = current;
        previous_inside = current_inside;
    }
    output
}

fn globe_to_screen(point: [f32; 3], width: u32, height: u32) -> [f32; 3] {
    let radius = width.min(height) as f32 * 0.47;
    [
        width as f32 * 0.5 + point[0] * radius,
        height as f32 * 0.5 - point[1] * radius,
        point[2],
    ]
}

fn colorize(
    raster: &CellRaster,
    field: AtlasField,
    tectonic: &SphericalTectonicSnapshot,
    final_elevation_m: &[f32],
) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(raster.width, raster.height, BACKGROUND);
    for (pixel, &cell) in image.pixels_mut().zip(&raster.cells) {
        if cell == EMPTY_CELL {
            continue;
        }
        let index = cell as usize;
        *pixel = match field {
            AtlasField::PlateOwner => plate_color(tectonic.cell_plates().raw_values()[index]),
            AtlasField::CrustKind => match tectonic.crust_kinds().get(index) {
                Some(CrustKind::Continental) => Rgba([196, 151, 82, 255]),
                Some(CrustKind::Oceanic) => Rgba([41, 87, 126, 255]),
                None => Rgba([255, 0, 255, 255]),
            },
            AtlasField::CrustAge => crust_age_color(
                tectonic.crust_kinds().get(index),
                tectonic.crust_age_myr()[index],
            ),
            AtlasField::TectonicElevation => {
                elevation_color(tectonic.tectonic_elevation_m()[index])
            }
            AtlasField::FinalElevation => elevation_color(final_elevation_m[index]),
            AtlasField::Boundary => dim(match tectonic.crust_kinds().get(index) {
                Some(CrustKind::Continental) => Rgba([105, 84, 59, 255]),
                _ => Rgba([35, 55, 72, 255]),
            }),
            AtlasField::Lineation => dim(elevation_color(tectonic.tectonic_elevation_m()[index])),
        };
    }
    image
}

fn plate_color(plate: u32) -> Rgba<u8> {
    let mut hash = plate.wrapping_mul(0x9E37_79B9).wrapping_add(0x85EB_CA6B);
    hash ^= hash >> 16;
    let hue = (hash % 360) as f32;
    hsl(hue, 0.58, 0.55)
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> Rgba<u8> {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let x = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let match_value = lightness - chroma * 0.5;
    Rgba([
        ((red + match_value) * 255.0).round() as u8,
        ((green + match_value) * 255.0).round() as u8,
        ((blue + match_value) * 255.0).round() as u8,
        255,
    ])
}

fn crust_age_color(kind: Option<CrustKind>, age_myr: f32) -> Rgba<u8> {
    if kind == Some(CrustKind::Continental) {
        return Rgba([94, 72, 57, 255]);
    }
    let amount = (age_myr / 180.0).clamp(0.0, 1.0);
    gradient(Rgba([243, 190, 73, 255]), Rgba([31, 70, 132, 255]), amount)
}

fn elevation_color(elevation_m: f32) -> Rgba<u8> {
    if elevation_m < 0.0 {
        gradient(
            Rgba([15, 38, 82, 255]),
            Rgba([72, 154, 188, 255]),
            ((elevation_m + 6_500.0) / 6_500.0).clamp(0.0, 1.0),
        )
    } else if elevation_m < 1_500.0 {
        gradient(
            Rgba([84, 150, 82, 255]),
            Rgba([184, 164, 91, 255]),
            elevation_m / 1_500.0,
        )
    } else {
        gradient(
            Rgba([151, 110, 72, 255]),
            Rgba([245, 244, 238, 255]),
            ((elevation_m - 1_500.0) / 5_500.0).clamp(0.0, 1.0),
        )
    }
}

fn gradient(first: Rgba<u8>, second: Rgba<u8>, amount: f32) -> Rgba<u8> {
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

fn dim(color: Rgba<u8>) -> Rgba<u8> {
    Rgba([color[0] / 2, color[1] / 2, color[2] / 2, 255])
}

fn decorate_map(
    image: &mut RgbaImage,
    field: AtlasField,
    map: &PreparedProjectedMap,
    surface: &SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
) {
    match field {
        AtlasField::PlateOwner | AtlasField::Boundary => {
            for segment in map.edge_segments() {
                let record = tectonic.boundaries()[segment.edge().raw() as usize];
                if record.kind == BoundaryKind::None {
                    continue;
                }
                draw_line(
                    image,
                    screen_xy(map_to_screen(
                        segment.start(),
                        map.bounds(),
                        image.width(),
                        image.height(),
                    )),
                    screen_xy(map_to_screen(
                        segment.end(),
                        map.bounds(),
                        image.width(),
                        image.height(),
                    )),
                    boundary_color(record.kind),
                    boundary_radius(field, record.strength),
                );
            }
        }
        AtlasField::Lineation => draw_map_lineation(image, map, surface, tectonic),
        AtlasField::CrustKind
        | AtlasField::CrustAge
        | AtlasField::TectonicElevation
        | AtlasField::FinalElevation => {}
    }
}

fn decorate_globe(
    image: &mut RgbaImage,
    field: AtlasField,
    surface: &SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
) {
    match field {
        AtlasField::PlateOwner | AtlasField::Boundary => {
            for edge in surface.edges() {
                let record = tectonic.boundaries()[edge.id.raw() as usize];
                if record.kind == BoundaryKind::None {
                    continue;
                }
                let endpoints = edge.vertices.map(|vertex| {
                    surface
                        .vertex(vertex)
                        .expect("validated edge vertex exists")
                        .position
                });
                draw_globe_arc(
                    image,
                    endpoints,
                    boundary_color(record.kind),
                    boundary_radius(field, record.strength),
                );
            }
        }
        AtlasField::Lineation => draw_globe_lineation(image, surface, tectonic),
        AtlasField::CrustKind
        | AtlasField::CrustAge
        | AtlasField::TectonicElevation
        | AtlasField::FinalElevation => {}
    }
}

fn boundary_radius(field: AtlasField, strength: f32) -> i32 {
    if matches!(field, AtlasField::PlateOwner) {
        0
    } else {
        (strength.clamp(0.0, 1.0) * 2.0).round() as i32 + 1
    }
}

fn boundary_color(kind: BoundaryKind) -> Rgba<u8> {
    match kind {
        BoundaryKind::None => Rgba([0, 0, 0, 0]),
        BoundaryKind::Weak => Rgba([150, 150, 150, 255]),
        BoundaryKind::ContinentalCollision => Rgba([245, 240, 225, 255]),
        BoundaryKind::Subduction => Rgba([237, 67, 55, 255]),
        BoundaryKind::ContinentalRift => Rgba([244, 145, 48, 255]),
        BoundaryKind::OceanicRidge => Rgba([47, 221, 221, 255]),
        BoundaryKind::Transform => Rgba([213, 80, 221, 255]),
    }
}

fn draw_map_lineation(
    image: &mut RgbaImage,
    map: &PreparedProjectedMap,
    surface: &SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
) {
    for cell in surface.cells() {
        if !lineation_is_sampled(cell.id.raw()) {
            continue;
        }
        let index = cell.id.raw() as usize;
        let east_amount = tectonic.lineation_east()[index];
        let north_amount = tectonic.lineation_north()[index];
        if east_amount == 0.0 && north_amount == 0.0 {
            continue;
        }
        let Ok(center) = map.projection().forward(cell.centroid) else {
            continue;
        };
        let Ok(Some(direction)) = map.projection().map_local_vector(
            cell.centroid,
            [f64::from(east_amount), f64::from(north_amount)],
        ) else {
            continue;
        };
        let start = screen_xy(map_to_screen(
            center,
            map.bounds(),
            image.width(),
            image.height(),
        ));
        let direction = [direction.x() as f32, -direction.y() as f32];
        let length = direction[0].hypot(direction[1]);
        if length <= f32::EPSILON {
            continue;
        }
        let delta = [direction[0] / length * 4.0, direction[1] / length * 4.0];
        draw_line(
            image,
            [start[0] - delta[0], start[1] - delta[1]],
            [start[0] + delta[0], start[1] + delta[1]],
            Rgba([250, 225, 87, 255]),
            0,
        );
    }
}

fn draw_globe_lineation(
    image: &mut RgbaImage,
    surface: &SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
) {
    for cell in surface.cells() {
        if !lineation_is_sampled(cell.id.raw()) {
            continue;
        }
        let index = cell.id.raw() as usize;
        let components = [
            f64::from(tectonic.lineation_east()[index]),
            f64::from(tectonic.lineation_north()[index]),
        ];
        if components == [0.0, 0.0] {
            continue;
        }
        let center = rotate_oblique(cell.centroid.components().map(|value| value as f32));
        if center[2] <= 0.0 {
            continue;
        }
        let (east, north) = canonical_east_north_basis(cell.centroid);
        let tangent: [f64; 3] =
            std::array::from_fn(|axis| east[axis] * components[0] + north[axis] * components[1]);
        let Some(endpoint) = UnitVector3::new(
            cell.centroid.components()[0] + tangent[0] * 0.04,
            cell.centroid.components()[1] + tangent[1] * 0.04,
            cell.centroid.components()[2] + tangent[2] * 0.04,
        )
        .ok() else {
            continue;
        };
        let endpoint = rotate_oblique(endpoint.components().map(|value| value as f32));
        if endpoint[2] <= 0.0 {
            continue;
        }
        let start = screen_xy(globe_to_screen(center, image.width(), image.height()));
        let end = screen_xy(globe_to_screen(endpoint, image.width(), image.height()));
        let delta = [end[0] - start[0], end[1] - start[1]];
        draw_line(
            image,
            [start[0] - delta[0], start[1] - delta[1]],
            end,
            Rgba([250, 225, 87, 255]),
            0,
        );
    }
}

fn lineation_is_sampled(cell_id: u32) -> bool {
    // SplitMix64's finalizer gives a stable spatially uncorrelated display sample.
    // This changes only the review atlas: the authoritative lineation field remains intact.
    let mut value = u64::from(cell_id).wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (value ^ (value >> 31)) % 64 == 0
}

fn draw_globe_arc(
    image: &mut RgbaImage,
    endpoints: [UnitVector3; 2],
    color: Rgba<u8>,
    radius: i32,
) {
    let angle = endpoints[0].dot(endpoints[1]).clamp(-1.0, 1.0).acos();
    let divisions = ((angle / 0.02).ceil() as usize).clamp(2, 24);
    let mut previous = rotate_oblique(endpoints[0].components().map(|value| value as f32));
    for step in 1..=divisions {
        let amount = step as f64 / divisions as f64;
        let current = rotate_oblique(
            slerp(endpoints, amount)
                .components()
                .map(|value| value as f32),
        );
        if let Some([start, end]) = clip_front_segment(previous, current) {
            draw_line(
                image,
                screen_xy(globe_to_screen(start, image.width(), image.height())),
                screen_xy(globe_to_screen(end, image.width(), image.height())),
                color,
                radius,
            );
        }
        previous = current;
    }
}

fn slerp(endpoints: [UnitVector3; 2], amount: f64) -> UnitVector3 {
    let first = endpoints[0].components();
    let second = endpoints[1].components();
    let angle = endpoints[0].dot(endpoints[1]).clamp(-1.0, 1.0).acos();
    let sin_angle = angle.sin();
    let components: [f64; 3] = if sin_angle.abs() <= f64::EPSILON {
        std::array::from_fn(|axis| first[axis] + (second[axis] - first[axis]) * amount)
    } else {
        let first_weight = ((1.0 - amount) * angle).sin() / sin_angle;
        let second_weight = (amount * angle).sin() / sin_angle;
        std::array::from_fn(|axis| first[axis] * first_weight + second[axis] * second_weight)
    };
    UnitVector3::new(components[0], components[1], components[2])
        .expect("spherical edge interpolation remains nonzero")
}

fn clip_front_segment(first: [f32; 3], second: [f32; 3]) -> Option<[[f32; 3]; 2]> {
    if first[2] < 0.0 && second[2] < 0.0 {
        return None;
    }
    if first[2] >= 0.0 && second[2] >= 0.0 {
        return Some([first, second]);
    }
    let amount = first[2] / (first[2] - second[2]);
    let crossing = std::array::from_fn(|axis| first[axis] + (second[axis] - first[axis]) * amount);
    if first[2] >= 0.0 {
        Some([first, crossing])
    } else {
        Some([crossing, second])
    }
}

fn draw_line(image: &mut RgbaImage, start: [f32; 2], end: [f32; 2], color: Rgba<u8>, radius: i32) {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let steps = delta[0].abs().max(delta[1].abs()).ceil().max(1.0) as i32;
    for step in 0..=steps {
        let amount = step as f32 / steps as f32;
        let x = (start[0] + delta[0] * amount).round() as i32;
        let y = (start[1] + delta[1] * amount).round() as i32;
        for offset_y in -radius..=radius {
            for offset_x in -radius..=radius {
                if offset_x * offset_x + offset_y * offset_y > radius * radius {
                    continue;
                }
                let pixel = [x + offset_x, y + offset_y];
                if pixel[0] >= 0
                    && pixel[1] >= 0
                    && pixel[0] < image.width() as i32
                    && pixel[1] < image.height() as i32
                {
                    image.put_pixel(pixel[0] as u32, pixel[1] as u32, color);
                }
            }
        }
    }
}

fn screen_xy(point: [f32; 3]) -> [f32; 2] {
    [point[0], point[1]]
}

fn save_view(
    directory: &Path,
    field: AtlasField,
    view: &str,
    image: &RgbaImage,
) -> Result<(), AtlasError> {
    let path: PathBuf = directory.join(format!("{}-{view}.png", field.slug()));
    image.save(path).map_err(atlas_error)
}

fn atlas_error(error: impl fmt::Display) -> AtlasError {
    AtlasError(error.to_string())
}

#[test]
#[ignore = "manual dual-view tectonic atlas"]
fn render_spherical_tectonic_atlas() {
    let output = Path::new("target/spherical-tectonic-atlas");
    render_atlas(AtlasConfig::formal_seed_matrix(), output).unwrap();
}

#[test]
fn globe_atlas_projection_never_uses_height() {
    let source = [0.31, -0.44, 0.842];
    let projected = globe_to_screen(rotate_oblique(source), VIEW_WIDTH, VIEW_HEIGHT);
    let repeated = globe_to_screen(rotate_oblique(source), VIEW_WIDTH, VIEW_HEIGHT);
    assert_eq!(projected, repeated);
    assert!(projected.into_iter().all(f32::is_finite));
}

#[test]
fn lineation_evidence_uses_a_stable_readable_sample() {
    let first: Vec<_> = (0..10_000_u32)
        .filter(|id| lineation_is_sampled(*id))
        .collect();
    let second: Vec<_> = (0..10_000_u32)
        .filter(|id| lineation_is_sampled(*id))
        .collect();

    assert_eq!(first, second, "atlas sampling must be deterministic");
    assert!(
        (130..=190).contains(&first.len()),
        "expected roughly one readable glyph per sixty-four cells, got {}",
        first.len()
    );
}
