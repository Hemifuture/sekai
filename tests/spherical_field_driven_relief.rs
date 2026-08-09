use serde::Serialize;
use serde_json::Value;

use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::{MantleGenerator, ReliefGenerator, TectonicGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BoundaryKind, CrustKind, GeologicSpec, LandOceanKind, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, SphericalMantleSnapshot, SphericalReliefSnapshot,
    SphericalTectonicSnapshot, TectonicSpec, WorldFormationPreset, COMPONENT_IDENTITY_TOLERANCE_M,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

const ROOT_SEED: u64 = 42;
const TARGET_CELL_COUNT: u32 = 2_562;

struct CurrentWorld {
    surface: SphericalSurfaceSnapshot,
    surface_geometry_before: Vec<u64>,
    tectonic: SphericalTectonicSnapshot,
    mantle: SphericalMantleSnapshot,
    relief: SphericalReliefSnapshot,
}

fn stage_rng(name: &'static str, version: u32) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(ROOT_SEED),
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

fn surface_geometry_bits(surface: &SphericalSurfaceSnapshot) -> Vec<u64> {
    let mut bits = Vec::with_capacity(surface.vertices().len() * 3 + surface.cells().len() * 6);
    for vertex in surface.vertices() {
        bits.extend(vertex.position.components().map(f64::to_bits));
    }
    for cell in surface.cells() {
        bits.extend(cell.site.components().map(f64::to_bits));
        bits.extend(cell.centroid.components().map(f64::to_bits));
    }
    bits
}

fn build_current_world() -> CurrentWorld {
    let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: TARGET_CELL_COUNT,
    })
    .unwrap();
    let surface_geometry_before = surface_geometry_bits(&surface);
    let formation = formation();
    let tectonic = TectonicGenerator::generate_spherical(
        &surface,
        &TectonicSpec::default(),
        &formation,
        &mut stage_rng("natural.spherical-tectonics", 2),
    )
    .unwrap();
    let mantle = MantleGenerator::generate_spherical(
        &surface,
        &GeologicSpec::default(),
        formation.mantle_bias(),
        &mut stage_rng("natural.spherical-mantle", 1),
    )
    .unwrap();
    let relief = ReliefGenerator::generate_spherical(
        &surface,
        &tectonic,
        &mantle,
        &mut stage_rng("natural.spherical-relief", 1),
        &mut Vec::new(),
    )
    .unwrap();
    CurrentWorld {
        surface,
        surface_geometry_before,
        tectonic,
        mantle,
        relief,
    }
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

fn assert_no_serialized_time_axis(value: &impl Serialize) {
    fn visit(value: &Value) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    assert!(
                        !matches!(
                            key.as_str(),
                            "history" | "timeline" | "time_slices" | "previous_state"
                        ),
                        "current-state artifact unexpectedly contains `{key}`"
                    );
                    visit(value);
                }
            }
            Value::Array(values) => values.iter().for_each(visit),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    visit(&serde_json::to_value(value).unwrap());
}

#[test]
fn field_driven_plates_and_crust_form_an_explainable_preliminary_heightmap() {
    let world = build_current_world();
    world.tectonic.validate_against(&world.surface).unwrap();
    world
        .relief
        .validate_against(&world.surface, &world.tectonic, &world.mantle)
        .unwrap();

    let elevations = world.relief.elevation_m().values();
    for (index, &elevation) in elevations.iter().enumerate() {
        let components = world.relief.crust_base_elevation_m().values()[index]
            + world.relief.tectonic_offset_m().values()[index]
            + world.relief.volcanic_offset_m().values()[index]
            + world.relief.regional_offset_m().values()[index];
        assert!(
            (elevation - components).abs() <= COMPONENT_IDENTITY_TOLERANCE_M,
            "cell {index} lost the four-component relief identity"
        );
    }

    let land = world
        .relief
        .land_ocean()
        .raw_values()
        .iter()
        .filter(|&&kind| kind == LandOceanKind::Land.raw())
        .count();
    let ocean = world.surface.cells().len() - land;
    assert!(land > 0 && ocean > 0, "land={land}, ocean={ocean}");

    let tectonic_offsets = world.relief.tectonic_offset_m().values();
    let mut convergent_uplift = Vec::new();
    let mut subduction_arc_above_trench = Vec::new();
    for edge in world.surface.edges() {
        let boundary = world.tectonic.boundaries()[edge.id.raw() as usize];
        let [first, second] = edge.cells;
        match boundary.kind {
            BoundaryKind::ContinentalCollision => convergent_uplift.push(
                (tectonic_offsets[first.raw() as usize] + tectonic_offsets[second.raw() as usize])
                    * 0.5,
            ),
            BoundaryKind::Subduction => {
                let subducting = boundary.subducting_plate.unwrap();
                let first_plate = world.tectonic.plate_for_cell(first).unwrap();
                let (trench, arc) = if first_plate == subducting {
                    (first, second)
                } else {
                    (second, first)
                };
                let difference =
                    tectonic_offsets[arc.raw() as usize] - tectonic_offsets[trench.raw() as usize];
                convergent_uplift.push(tectonic_offsets[arc.raw() as usize]);
                subduction_arc_above_trench.push(difference);
            }
            BoundaryKind::None
            | BoundaryKind::Weak
            | BoundaryKind::ContinentalRift
            | BoundaryKind::OceanicRidge
            | BoundaryKind::Transform => {}
        }
    }
    assert!(
        !convergent_uplift.is_empty() && convergent_uplift.iter().any(|&uplift| uplift > 0.0),
        "convergent boundaries never produce positive tectonic relief"
    );
    assert!(
        !subduction_arc_above_trench.is_empty() && median(subduction_arc_above_trench) > 0.0,
        "subduction arcs are not consistently above their trenches"
    );

    let mut continental_interior = Vec::new();
    let mut continental_interior_base = Vec::new();
    let mut oceanic = Vec::new();
    let mut oceanic_base = Vec::new();
    for cell in world.surface.cells() {
        let index = cell.id.raw() as usize;
        match world.tectonic.crust_kind(cell.id).unwrap() {
            CrustKind::Continental
                if cell.boundary_edges.iter().all(|edge| {
                    world.tectonic.boundaries()[edge.raw() as usize].kind == BoundaryKind::None
                }) =>
            {
                continental_interior.push(elevations[index]);
                continental_interior_base
                    .push(world.relief.crust_base_elevation_m().values()[index]);
            }
            CrustKind::Oceanic => {
                oceanic.push(elevations[index]);
                oceanic_base.push(world.relief.crust_base_elevation_m().values()[index]);
            }
            CrustKind::Continental => {}
        }
    }
    assert!(!continental_interior.is_empty() && !oceanic.is_empty());
    assert!(
        median(continental_interior_base) > median(oceanic_base),
        "crust-base synthesis must preserve the continental/oceanic height contrast"
    );
    assert!(
        median(continental_interior) > median(oceanic),
        "continental interiors must sit above oceanic crust in the preliminary heightmap"
    );

    let minimum = elevations.iter().copied().fold(f32::INFINITY, f32::min);
    let maximum = elevations.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        maximum - minimum >= 4_000.0,
        "preliminary height range is only {} m",
        maximum - minimum
    );
}

#[test]
fn current_snapshot_has_no_history_or_geometry_displacement_state() {
    let world = build_current_world();
    assert_no_serialized_time_axis(&world.tectonic);
    assert_no_serialized_time_axis(&world.relief);
    assert_eq!(
        surface_geometry_bits(&world.surface),
        world.surface_geometry_before,
        "heightmap generation must never displace the authoritative unit sphere"
    );
    assert!(world.surface.vertices().iter().all(|vertex| (vertex
        .position
        .components()
        .map(|value| value * value)
        .iter()
        .sum::<f64>()
        - 1.0)
        .abs()
        <= 16.0 * f64::EPSILON));
}
