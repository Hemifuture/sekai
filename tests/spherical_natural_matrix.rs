use std::collections::{BTreeSet, VecDeque};
use std::f64::consts::PI;

use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::{MantleGenerator, TectonicGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    CrustKind, GeologicSpec, MantleActivity, MantleFormationBias, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, TectonicActivity, TectonicSpec, WorldFormationPreset,
    MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

#[derive(Clone, Copy)]
struct MatrixCase {
    name: &'static str,
    radius_m: f64,
    target_cells: u32,
    seed: u64,
    plate_count: u16,
    tectonic_activity: TectonicActivity,
    preset: ResolvedWorldFormationPreset,
    continental_fraction: f32,
    mantle_activity: MantleActivity,
    mantle_bias: MantleFormationBias,
    expected_tectonic_hash: &'static str,
    expected_mantle_hash: &'static str,
}

const CASES: [MatrixCase; 4] = [
    MatrixCase {
        name: "minimum-radius-quiet",
        radius_m: 1.0,
        target_cells: 42,
        seed: 1,
        plate_count: 2,
        tectonic_activity: TectonicActivity::Quiet,
        preset: ResolvedWorldFormationPreset::Supercontinent,
        continental_fraction: 0.42,
        mantle_activity: MantleActivity::Quiet,
        mantle_bias: MantleFormationBias::Neutral,
        expected_tectonic_hash: "44c53eb1ad30e0e36a24dbf0ea51147ea7f6b84219591e802f54b7f3ae39ab2f",
        expected_mantle_hash: "3f7b966c4918d1d4a3edf0c94990c2ab1f0870e209e1803fd7e36378a2eda77d",
    },
    MatrixCase {
        name: "regional-great-island",
        radius_m: 1_000_000.0,
        target_cells: 92,
        seed: 0x0BAD_5EED,
        plate_count: 7,
        tectonic_activity: TectonicActivity::Moderate,
        preset: ResolvedWorldFormationPreset::GreatIsland,
        continental_fraction: 0.28,
        mantle_activity: MantleActivity::Active,
        mantle_bias: MantleFormationBias::Neutral,
        expected_tectonic_hash: "3dc46c071305ec8749d9fe63e013f9cfce851ff65b8f7e9b01017416123982d7",
        expected_mantle_hash: "6235cfbdb57d1bfbce12fa426916b7e4376191da13f80efeb2750e5802b047db",
    },
    MatrixCase {
        name: "earth-continents",
        radius_m: 6_371_000.0,
        target_cells: 162,
        seed: 42,
        plate_count: 12,
        tectonic_activity: TectonicActivity::Moderate,
        preset: ResolvedWorldFormationPreset::Continents,
        continental_fraction: 0.38,
        mantle_activity: MantleActivity::Moderate,
        mantle_bias: MantleFormationBias::Neutral,
        expected_tectonic_hash: "27769a61bed3d8074dadbb7a0c0e2fbd0b7325431ae2d053708ea891491a2785",
        expected_mantle_hash: "03a432dafeead07521176d659a29f904ef52efe34e239389adbb6602682e5cfa",
    },
    MatrixCase {
        name: "maximum-radius-volcanic",
        radius_m: 100_000_000.0,
        target_cells: 642,
        seed: u64::MAX - 17,
        plate_count: 64,
        tectonic_activity: TectonicActivity::Active,
        preset: ResolvedWorldFormationPreset::VolcanicIslands,
        continental_fraction: 0.16,
        mantle_activity: MantleActivity::Quiet,
        mantle_bias: MantleFormationBias::VolcanicIslands,
        expected_tectonic_hash: "6dfd128040ae81cc70ca7525b84d8a88ea3c8dff0e441a06499c16df35f28697",
        expected_mantle_hash: "6e5def0d9603031ce138043672e45f8487e7779e92ec5b8c5f0d62c2c118f673",
    },
];

fn requested_preset(preset: ResolvedWorldFormationPreset) -> WorldFormationPreset {
    match preset {
        ResolvedWorldFormationPreset::Continents => WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Archipelago => WorldFormationPreset::Archipelago,
        ResolvedWorldFormationPreset::Supercontinent => WorldFormationPreset::Supercontinent,
        ResolvedWorldFormationPreset::GreatIsland => WorldFormationPreset::GreatIsland,
        ResolvedWorldFormationPreset::VolcanicIslands => WorldFormationPreset::VolcanicIslands,
    }
}

fn stage_rng(seed: u64, name: &'static str) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(name, 1, "sekai.matrix"),
    ))
}

fn assert_plate_domains_connected(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    tectonic: &sekai::world::natural::SphericalTectonicSnapshot,
    case_name: &str,
) {
    for plate in tectonic.plates() {
        let expected = surface
            .cells()
            .iter()
            .filter(|cell| tectonic.plate_for_cell(cell.id) == Some(plate.id()))
            .count();
        let mut reached = vec![false; surface.cells().len()];
        let mut queue = VecDeque::from([plate.seed_cell()]);
        reached[plate.seed_cell().raw() as usize] = true;
        let mut actual = 0;
        while let Some(cell) = queue.pop_front() {
            actual += 1;
            for &edge_id in &surface.cell(cell).unwrap().boundary_edges {
                let edge = surface.edge(edge_id).unwrap();
                let neighbor = if edge.cells[0] == cell {
                    edge.cells[1]
                } else {
                    edge.cells[0]
                };
                let index = neighbor.raw() as usize;
                if !reached[index] && tectonic.plate_for_cell(neighbor) == Some(plate.id()) {
                    reached[index] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        assert_eq!(
            actual,
            expected,
            "{case_name}: disconnected {:?}",
            plate.id()
        );
    }
}

#[test]
fn spherical_natural_scientific_and_deterministic_matrix() {
    let mut actual_hashes = Vec::with_capacity(CASES.len());
    let mut saw_final_count_differ_from_initial = false;
    for case in CASES {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(case.radius_m).unwrap(),
            target_cell_count: case.target_cells,
        })
        .unwrap();
        let tectonic_spec = TectonicSpec {
            plate_count: case.plate_count,
            continental_crust_fraction: case.continental_fraction,
            activity: case.tectonic_activity,
            ..TectonicSpec::default()
        };
        let formation = ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            requested_preset(case.preset),
            case.preset,
        )
        .unwrap();
        let tectonic = TectonicGenerator::generate_spherical(
            &surface,
            &tectonic_spec,
            &formation,
            &mut stage_rng(case.seed, "matrix.tectonic"),
        )
        .unwrap();
        let tectonic_repeated = TectonicGenerator::generate_spherical(
            &surface,
            &tectonic_spec,
            &formation,
            &mut stage_rng(case.seed, "matrix.tectonic"),
        )
        .unwrap();
        assert_eq!(tectonic, tectonic_repeated, "{}", case.name);
        tectonic.validate_against(&surface).unwrap();
        assert!((2..=64).contains(&tectonic.plates().len()), "{}", case.name);
        saw_final_count_differ_from_initial |=
            tectonic.plates().len() != usize::from(case.plate_count);
        assert_plate_domains_connected(&surface, &tectonic, case.name);

        for cell in surface.cells() {
            let plate = tectonic.plate_for_cell(cell.id).unwrap();
            let velocity = tectonic.plates()[plate.raw() as usize]
                .rotation()
                .velocity_mm_per_year(surface.radius(), cell.centroid)
                .unwrap();
            let radial = cell.centroid.components();
            let dot = velocity[0] * radial[0] + velocity[1] * radial[1] + velocity[2] * radial[2];
            let speed =
                (velocity[0] * velocity[0] + velocity[1] * velocity[1] + velocity[2] * velocity[2])
                    .sqrt();
            assert!(dot.abs() <= 1.0e-8, "{}", case.name);
            assert!(
                speed <= MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR + 1.0e-9,
                "{}",
                case.name
            );
        }

        let total_area = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .sum::<f64>();
        let continental_area = surface
            .cells()
            .iter()
            .filter(|cell| tectonic.crust_kind(cell.id) == Some(CrustKind::Continental))
            .map(|cell| cell.area.get())
            .sum::<f64>();
        assert!(
            continental_area > 0.0 && continental_area < total_area,
            "{}: evolved crust lost one material class ({continental_area}/{total_area} m² continental)",
            case.name,
        );

        let geologic_spec = GeologicSpec {
            hotspot_count: 7,
            mantle_activity: case.mantle_activity,
            ..GeologicSpec::default()
        };
        let mantle = MantleGenerator::generate_spherical(
            &surface,
            &geologic_spec,
            case.mantle_bias,
            &mut stage_rng(case.seed, "matrix.mantle"),
        )
        .unwrap();
        let mantle_repeated = MantleGenerator::generate_spherical(
            &surface,
            &geologic_spec,
            case.mantle_bias,
            &mut stage_rng(case.seed, "matrix.mantle"),
        )
        .unwrap();
        assert_eq!(mantle, mantle_repeated, "{}", case.name);
        mantle.validate_against(&surface).unwrap();
        assert_eq!(
            mantle
                .hotspots()
                .iter()
                .map(|hotspot| hotspot.source_cell())
                .collect::<BTreeSet<_>>()
                .len(),
            mantle.hotspots().len(),
            "{}",
            case.name
        );
        for hotspot in mantle.hotspots() {
            assert!(hotspot.support_radius_m().get() <= PI * case.radius_m);
            assert_eq!(
                mantle.volcanic_influence()[hotspot.source_cell().raw() as usize],
                1.0
            );
        }

        let tectonic_hash = blake3::hash(&serde_json::to_vec(&tectonic).unwrap())
            .to_hex()
            .to_string();
        let mantle_hash = blake3::hash(&serde_json::to_vec(&mantle).unwrap())
            .to_hex()
            .to_string();
        eprintln!(
            "matrix_case={} cells={} initial_plates={} final_plates={} tectonic_hash={} mantle_hash={}",
            case.name,
            surface.cells().len(),
            case.plate_count,
            tectonic.plates().len(),
            tectonic_hash,
            mantle_hash
        );
        actual_hashes.push((case, tectonic_hash, mantle_hash));
    }

    assert!(
        saw_final_count_differ_from_initial,
        "the matrix never exercised an evolved final plate count"
    );

    for (case, tectonic_hash, mantle_hash) in actual_hashes {
        assert_eq!(tectonic_hash, case.expected_tectonic_hash, "{}", case.name);
        assert_eq!(mantle_hash, case.expected_mantle_hash, "{}", case.name);
    }
}
