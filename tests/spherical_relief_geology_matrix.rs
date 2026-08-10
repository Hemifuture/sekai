use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::{
    GeologicGenerator, MantleGenerator, ReliefGenerator, TectonicGenerator,
};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BedrockKind, GeologicSpec, MantleActivity, MantleFormationBias, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, TectonicActivity, TectonicSpec, WorldFormationPreset,
    COMPONENT_IDENTITY_TOLERANCE_M, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
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
    expected_relief_hash: &'static str,
    expected_geology_hash: &'static str,
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
        expected_relief_hash: "944e89741891b82daa1a45cae0200d98affc22331b48e7d67df425d738f2ece9",
        expected_geology_hash: "a127a34bfc494d87de3a18d2cfb9b9c3330d39eb875e30d50d82348c13cfac30",
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
        expected_relief_hash: "57f1b48e3766d52522ef4c25d3799bc3f32b9f494a096fb2da26ac1bc41c9e0e",
        expected_geology_hash: "012de66daac4c30fda92306d85f79746ec91fc8fcabf055266a7fb8e1a1b1010",
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
        expected_relief_hash: "eab7caf29b0434412c1b7881031ab686f51d20c5cc4ebcf3ab2550bac518064b",
        expected_geology_hash: "693fe0e592c6773100d2989b879c11d4107934125e31e92b6177c94a336af506",
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
        expected_relief_hash: "a8bf2767174e19ddded108bfaeed41db8ad58c0eb82665d038a5aa18826a311b",
        expected_geology_hash: "990ca66f113ff61982b3e38d634e8746a80bb507522d0c541001927678eaa502",
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
        StageIdentity::new(name, 1, "sekai.relief-geology-matrix"),
    ))
}

#[test]
fn spherical_relief_and_geology_scientific_deterministic_matrix() {
    let mut actual_hashes = Vec::new();
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

        let mut diagnostics = Vec::new();
        let relief = ReliefGenerator::generate_spherical(
            &surface,
            &tectonic,
            &mantle,
            &mut stage_rng(case.seed, "matrix.relief"),
            &mut diagnostics,
        )
        .unwrap();
        let mut repeated_diagnostics = Vec::new();
        let relief_repeated = ReliefGenerator::generate_spherical(
            &surface,
            &tectonic,
            &mantle,
            &mut stage_rng(case.seed, "matrix.relief"),
            &mut repeated_diagnostics,
        )
        .unwrap();
        assert_eq!(relief, relief_repeated, "{}", case.name);
        assert_eq!(diagnostics, repeated_diagnostics, "{}", case.name);
        relief
            .validate_against(&surface, &tectonic, &mantle)
            .unwrap();

        let geology = GeologicGenerator::generate_spherical(
            &surface,
            &tectonic,
            &mantle,
            &relief,
            &geologic_spec,
            &mut stage_rng(case.seed, "matrix.geology"),
        )
        .unwrap();
        let geology_repeated = GeologicGenerator::generate_spherical(
            &surface,
            &tectonic,
            &mantle,
            &relief,
            &geologic_spec,
            &mut stage_rng(case.seed, "matrix.geology"),
        )
        .unwrap();
        assert_eq!(geology, geology_repeated, "{}", case.name);
        geology
            .validate_against(&surface, &tectonic, &mantle, &relief)
            .unwrap();

        for index in 0..surface.cells().len() {
            let calculated = relief.crust_base_elevation_m().values()[index]
                + relief.tectonic_offset_m().values()[index]
                + relief.volcanic_offset_m().values()[index]
                + relief.regional_offset_m().values()[index];
            assert!(
                (relief.elevation_m().values()[index] - calculated).abs()
                    <= COMPONENT_IDENTITY_TOLERANCE_M,
                "{}",
                case.name
            );
            if mantle.volcanic_influence()[index] <= 0.0 {
                assert_eq!(relief.volcanic_offset_m().values()[index], 0.0);
            }
            for value in [
                geology.fracture_intensity()[index],
                geology.erosion_resistance()[index],
                geology.relative_permeability()[index],
                geology.metallic_mineral_potential()[index],
                geology.geothermal_potential()[index],
                geology.sedimentary_basin_potential()[index],
            ] {
                assert!(value.is_finite() && (0.0..=1.0).contains(&value));
            }
        }
        for hotspot in mantle.hotspots() {
            let index = hotspot.source_cell().raw() as usize;
            assert!(relief.volcanic_offset_m().values()[index] > 0.0);
            assert_eq!(
                geology.bedrock_kind(hotspot.source_cell()),
                Some(BedrockKind::Volcanic)
            );
        }
        let weighted_regional_mean = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                f64::from(relief.regional_offset_m().values()[index]) * cell.area.get()
            })
            .sum::<f64>()
            / surface.total_cell_area().get();
        assert!(weighted_regional_mean.abs() < 0.05, "{}", case.name);

        let relief_hash = blake3::hash(&serde_json::to_vec(&relief).unwrap())
            .to_hex()
            .to_string();
        let geology_hash = blake3::hash(&serde_json::to_vec(&geology).unwrap())
            .to_hex()
            .to_string();
        eprintln!(
            "matrix_case={} cells={} relief_hash={} geology_hash={}",
            case.name,
            surface.cells().len(),
            relief_hash,
            geology_hash
        );
        actual_hashes.push((case, relief_hash, geology_hash));
    }

    for (case, relief_hash, geology_hash) in actual_hashes {
        assert_eq!(relief_hash, case.expected_relief_hash, "{}", case.name);
        assert_eq!(geology_hash, case.expected_geology_hash, "{}", case.name);
    }
}
