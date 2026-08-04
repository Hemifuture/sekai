use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::{
    GeologicGenerator, MantleGenerator, ReliefGenerator, TectonicGenerator,
};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BedrockKind, CrustKind, GeologicSpec, MantleFormationBias, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, SphericalGeologicSnapshot, SphericalMantleSnapshot,
    SphericalReliefSnapshot, SphericalTectonicSnapshot, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

fn surface(
    radius_m: f64,
    target_cell_count: u32,
) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn stage_rng(name: &'static str, seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(name, 1, "sekai.spherical-geology-tests"),
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

fn upstream(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    seed: u64,
) -> (
    SphericalTectonicSnapshot,
    SphericalMantleSnapshot,
    SphericalReliefSnapshot,
) {
    let tectonic = TectonicGenerator::generate_spherical(
        surface,
        &TectonicSpec::default(),
        &formation(),
        &mut stage_rng("spherical-geology-tectonics", seed),
    )
    .unwrap();
    let mantle = MantleGenerator::generate_spherical(
        surface,
        &GeologicSpec::default(),
        MantleFormationBias::Neutral,
        &mut stage_rng("spherical-geology-mantle", seed),
    )
    .unwrap();
    let mut diagnostics = Vec::new();
    let relief = ReliefGenerator::generate_spherical(
        surface,
        &tectonic,
        &mantle,
        &mut stage_rng("spherical-geology-relief", seed),
        &mut diagnostics,
    )
    .unwrap();
    (tectonic, mantle, relief)
}

fn generate(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
    mantle: &SphericalMantleSnapshot,
    relief: &SphericalReliefSnapshot,
    seed: u64,
) -> SphericalGeologicSnapshot {
    GeologicGenerator::generate_spherical(
        surface,
        tectonic,
        mantle,
        relief,
        &GeologicSpec::default(),
        &mut stage_rng("spherical-geology", seed),
    )
    .unwrap()
}

#[test]
fn spherical_geology_is_deterministic_bounded_seed_sensitive_and_surface_bound() {
    let sphere = surface(6_371_000.0, 642);
    let (tectonic, mantle, relief) = upstream(&sphere, 0x600D_F00D);
    let first = generate(&sphere, &tectonic, &mantle, &relief, 71);
    let repeated = generate(&sphere, &tectonic, &mantle, &relief, 71);
    let changed = generate(&sphere, &tectonic, &mantle, &relief, 72);

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );
    assert_ne!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&changed).unwrap()
    );
    first
        .validate_against(&sphere, &tectonic, &mantle, &relief)
        .unwrap();

    for index in 0..sphere.cells().len() {
        let cell = CellId::from_raw(index as u32);
        let kind = first.bedrock_kind(cell).unwrap();
        let crust = tectonic.crust_kind(cell).unwrap();
        match kind {
            BedrockKind::OceanicMafic => assert_eq!(crust, CrustKind::Oceanic),
            BedrockKind::ContinentalCrystalline | BedrockKind::Metamorphic => {
                assert_eq!(crust, CrustKind::Continental)
            }
            BedrockKind::Sedimentary | BedrockKind::Volcanic => {}
        }
        for value in [
            first.fracture_intensity()[index],
            first.erosion_resistance()[index],
            first.relative_permeability()[index],
            first.metallic_mineral_potential()[index],
            first.geothermal_potential()[index],
            first.sedimentary_basin_potential()[index],
        ] {
            assert!(value.is_finite() && (0.0..=1.0).contains(&value));
        }
    }
}

#[test]
fn mantle_sources_and_active_boundaries_drive_expected_material_responses() {
    let sphere = surface(6_371_000.0, 642);
    let (tectonic, mantle, relief) = upstream(&sphere, 0x0BAD_CAFE);
    let geology = generate(&sphere, &tectonic, &mantle, &relief, 83);

    for hotspot in mantle.hotspots() {
        let index = hotspot.source_cell().raw() as usize;
        assert_eq!(
            geology.bedrock_kind(hotspot.source_cell()),
            Some(BedrockKind::Volcanic)
        );
        assert!(geology.fracture_intensity()[index] >= 0.45);
        assert!(geology.geothermal_potential()[index] > 0.0);
        assert!(geology.metallic_mineral_potential()[index] > 0.5);
    }

    let boundary_cells = sphere
        .edges()
        .iter()
        .filter(|edge| {
            !matches!(
                tectonic.boundaries()[edge.id.raw() as usize].kind,
                sekai::world::natural::BoundaryKind::None
                    | sekai::world::natural::BoundaryKind::Weak
            )
        })
        .flat_map(|edge| edge.cells)
        .collect::<Vec<_>>();
    assert!(!boundary_cells.is_empty());
    let boundary_mean = boundary_cells
        .iter()
        .map(|cell| geology.fracture_intensity()[cell.raw() as usize] as f64)
        .sum::<f64>()
        / boundary_cells.len() as f64;
    let global_mean = geology
        .fracture_intensity()
        .iter()
        .map(|&value| f64::from(value))
        .sum::<f64>()
        / sphere.cells().len() as f64;
    assert!(
        boundary_mean > global_mean,
        "boundary={boundary_mean}, global={global_mean}"
    );
}

#[test]
fn equal_count_different_surface_and_each_mismatched_upstream_are_rejected() {
    let first_surface = surface(6_371_000.0, 162);
    let second_surface = surface(6_000_000.0, 162);
    let (first_tectonic, first_mantle, first_relief) = upstream(&first_surface, 101);
    let (second_tectonic, second_mantle, second_relief) = upstream(&second_surface, 101);
    let geology = generate(
        &first_surface,
        &first_tectonic,
        &first_mantle,
        &first_relief,
        103,
    );

    assert!(geology
        .validate_against(
            &second_surface,
            &second_tectonic,
            &second_mantle,
            &second_relief
        )
        .is_err());
    assert!(GeologicGenerator::generate_spherical(
        &first_surface,
        &second_tectonic,
        &first_mantle,
        &first_relief,
        &GeologicSpec::default(),
        &mut stage_rng("spherical-geology", 107),
    )
    .is_err());
    assert!(GeologicGenerator::generate_spherical(
        &first_surface,
        &first_tectonic,
        &second_mantle,
        &first_relief,
        &GeologicSpec::default(),
        &mut stage_rng("spherical-geology", 109),
    )
    .is_err());
    assert!(GeologicGenerator::generate_spherical(
        &first_surface,
        &first_tectonic,
        &first_mantle,
        &second_relief,
        &GeologicSpec::default(),
        &mut stage_rng("spherical-geology", 113),
    )
    .is_err());
}
