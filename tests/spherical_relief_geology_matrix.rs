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
        expected_relief_hash: "c99258b0705b8a48cf9a72ca95acb5d5c01fdc085951ed46918814ae61f8d17f",
        expected_geology_hash: "c5ccd8f480946e532dec51709e68c62f7acb02c7af7de07d026918340b312d51",
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
        expected_relief_hash: "6d596d846915ef7a57a01a7e80a0cdd5541e3b19a544192470a9facaff027201",
        expected_geology_hash: "1c6a0012df3fe388772c737024c20c95ff6a49ff863486a914f8994216264594",
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
        expected_relief_hash: "77312515da4d079174d590b2a92321c5f6bc5e530c4eb66f554bbae022fd775e",
        expected_geology_hash: "7f989b571d4e3b936b514055c1859159258d6c165a9a68415c6d29062b9f0120",
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
        expected_relief_hash: "b63696be8df24982366e153db92fb4caeaf1af6d9ef8114adfc433fefd244798",
        expected_geology_hash: "f2e0c90eca739e96d6204de5575788773e900fe6dc62c065093a165df5cb0c26",
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
