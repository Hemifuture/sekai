use std::collections::BTreeSet;
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
        expected_tectonic_hash: "bc4b529c0537bf9626ce497404faf38b3265979b28b01dc91807cdf047092441",
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
        expected_tectonic_hash: "4b1d93335ae47c2e787e98e330ce48490814d73c247cba95379f9723c0faf48a",
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
        expected_tectonic_hash: "6d40a6b7fc39388dff393bf111728256af21192f0b689149766be7a1458f2f3f",
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
        expected_tectonic_hash: "3f347846f21c67acf6daa07e5f02ee58003d53f3ac38a91078821d69daa0d1fa",
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

#[test]
fn spherical_natural_scientific_and_deterministic_matrix() {
    let mut actual_hashes = Vec::with_capacity(CASES.len());
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
        assert_eq!(tectonic.plates().len(), usize::from(case.plate_count));

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
        let maximum_cell_area = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .fold(0.0, f64::max);
        let continental_area = surface
            .cells()
            .iter()
            .filter(|cell| tectonic.crust_kind(cell.id) == Some(CrustKind::Continental))
            .map(|cell| cell.area.get())
            .sum::<f64>();
        assert!(
            (continental_area - total_area * f64::from(case.continental_fraction)).abs()
                <= maximum_cell_area,
            "{}",
            case.name
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
            "matrix_case={} cells={} tectonic_hash={} mantle_hash={}",
            case.name,
            surface.cells().len(),
            tectonic_hash,
            mantle_hash
        );
        actual_hashes.push((case, tectonic_hash, mantle_hash));
    }

    for (case, tectonic_hash, mantle_hash) in actual_hashes {
        assert_eq!(tectonic_hash, case.expected_tectonic_hash, "{}", case.name);
        assert_eq!(mantle_hash, case.expected_mantle_hash, "{}", case.name);
    }
}
