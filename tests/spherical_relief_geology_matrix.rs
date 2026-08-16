use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::{
    GeologicGenerator, MantleGenerator, ReliefGenerator, TectonicGenerator,
};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BedrockKind, GeologicSpec, MantleActivity, MantleFormationBias, ReliefSpec,
    ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicActivity, TectonicSpec,
    WorldFormationPreset, COMPONENT_IDENTITY_TOLERANCE_M, ELEVATION_MAX_M,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
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
        expected_relief_hash: "fa85419891fe6f2f8c3c37b61da201b23886abbe2ec26368efc110c06b592d71",
        expected_geology_hash: "6aa14ee4096ac47d2e54811939a8f18fb47f792de4d5034af98a4870a4c90f76",
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
        expected_relief_hash: "f42e3a97cc75f11f4b6bf1f3f419974df454975238c9c8d11f439b0012934fed",
        expected_geology_hash: "d92f52b3f9150991ae39268ed6720b4726b6fe22146a0d13bba6aee634a7326a",
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
        expected_relief_hash: "073606a7ab2cdf726279796ead4cd8df70b02c9769535cd4553b717624684f5d",
        expected_geology_hash: "2b93538e11dea0c7b9bd271392adb5d5f6f5f18597daf6b41104b2ceca1ab412",
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
        expected_relief_hash: "f2e2e0d681668a14cdb517ca6f1f056fbf097890ad64615bf1893586fab6d445",
        expected_geology_hash: "4ee3b4650a98d4983acd617f0b6bba621e2376c778e78242a4a4a1671e4728c8",
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
            &ReliefSpec::default(),
            &mut stage_rng(case.seed, "matrix.relief"),
            &mut diagnostics,
        )
        .unwrap();
        let mut repeated_diagnostics = Vec::new();
        let relief_repeated = ReliefGenerator::generate_spherical(
            &surface,
            &tectonic,
            &mantle,
            &ReliefSpec::default(),
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
            let volcanic = relief.volcanic_offset_m().values()[index];
            assert!(
                volcanic > 0.0 || relief.elevation_m().values()[index] == ELEVATION_MAX_M,
                "{}: hotspot source lost its volcanic component without positive safety clamping",
                case.name,
            );
            assert_eq!(
                geology.bedrock_kind(hotspot.source_cell()),
                Some(BedrockKind::Volcanic)
            );
        }
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
