use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, BuildCancellation, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{
    continental_airy_elevation_m, dynamic_tectonic_response_m, oceanic_isostatic_elevation_m,
    parsons_sclater_ocean_depth_m, EvolvedTectonicGenerator, GeologicSubstrateGenerator,
    PrimaryReliefGenerationError, PrimaryReliefGenerator,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    EvolvedTectonicSnapshot, GeologicSpec, GeologicSubstrateSnapshot, NaturalQualityProfile,
    ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};

struct Fixture {
    bundle: ProfileSurfaceBundle,
    evolved: EvolvedTectonicSnapshot,
    substrate: GeologicSubstrateSnapshot,
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let mut tectonic_rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
        ));
        let evolved = EvolvedTectonicGenerator::generate(
            &bundle,
            &TectonicSpec::default(),
            &formation(),
            &mut tectonic_rng,
        )
        .unwrap();
        let mut substrate_rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("natural.geologic-substrate", 1, "sekai.core"),
        ));
        let substrate = GeologicSubstrateGenerator::generate(
            bundle.authoritative_surface(),
            &evolved,
            &GeologicSpec::default(),
            &formation(),
            &mut substrate_rng,
        )
        .unwrap();
        Fixture {
            bundle,
            evolved,
            substrate,
        }
    })
}

fn relief_rng(cancellation: Option<&BuildCancellation>) -> StageRng {
    let seed = derive_stage_seed(
        RootSeed::new(42),
        StageIdentity::new("natural.primary-relief", 1, "sekai.core"),
    );
    cancellation.map_or_else(
        || StageRng::from_seed(seed),
        |signal| StageRng::from_seed_with_cancellation(seed, signal),
    )
}

#[test]
fn density_aware_airy_balance_is_monotone_and_has_the_reference_freeboard() {
    assert!((continental_airy_elevation_m(35.0, 2_800.0) - 250.0).abs() < 1.0e-4);
    assert!(
        continental_airy_elevation_m(45.0, 2_800.0) > continental_airy_elevation_m(35.0, 2_800.0)
    );
    assert!(
        continental_airy_elevation_m(35.0, 2_750.0) > continental_airy_elevation_m(35.0, 2_950.0)
    );
}

#[test]
fn parsons_sclater_depth_and_oceanic_buoyancy_keep_physical_ordering() {
    assert_eq!(parsons_sclater_ocean_depth_m(0.0), 2_500.0);
    assert!(parsons_sclater_ocean_depth_m(20.0) < parsons_sclater_ocean_depth_m(80.0));
    assert!(parsons_sclater_ocean_depth_m(80.0) < parsons_sclater_ocean_depth_m(160.0));
    assert!(
        oceanic_isostatic_elevation_m(20.0, 7.0, 2_950.0)
            > oceanic_isostatic_elevation_m(100.0, 7.0, 2_950.0)
    );
    assert!(
        oceanic_isostatic_elevation_m(80.0, 9.0, 2_900.0)
            > oceanic_isostatic_elevation_m(80.0, 6.0, 3_000.0)
    );
}

#[test]
fn dynamic_response_preserves_accumulated_relief_and_present_forcing_signs() {
    assert_eq!(dynamic_tectonic_response_m(1_000.0, 0.0, 0.0), 650.0);
    assert_eq!(dynamic_tectonic_response_m(0.0, 1.0, 0.0), 250.0);
    assert_eq!(dynamic_tectonic_response_m(0.0, 0.0, 1.0), -250.0);
}

#[test]
fn generated_relief_closes_components_water_and_all_causal_supports() {
    let fixture = fixture();
    let surface = fixture.bundle.authoritative_surface();
    let mut diagnostics = Vec::<Diagnostic>::new();
    let relief = PrimaryReliefGenerator::generate(
        surface,
        &fixture.evolved,
        &fixture.substrate,
        &ReliefSpec::default(),
        &mut relief_rng(None),
        &mut diagnostics,
    )
    .unwrap();

    relief
        .validate_against(surface, &fixture.substrate, &ReliefSpec::default())
        .unwrap();
    assert!(relief.water_volume_relative_error() <= 1.0e-6);
    assert!(relief
        .passive_margin_offset_m()
        .iter()
        .any(|value| value.abs() > 0.0));
    assert!(relief
        .conditioned_regional_detail_m()
        .iter()
        .any(|value| value.abs() > 0.0));
    for hotspot in fixture.substrate.mantle().hotspots() {
        assert!(relief.volcanic_construction_m()[hotspot.source_cell().raw() as usize] > 0.0);
    }
    for index in 0..surface.cells().len() {
        let calculated = relief.isostatic_base_m()[index]
            + relief.dynamic_tectonic_offset_m()[index]
            + relief.volcanic_construction_m()[index]
            + relief.passive_margin_offset_m()[index]
            + relief.conditioned_regional_detail_m()[index];
        assert!((relief.elevation_m()[index] - calculated).abs() <= 0.01);
    }
}

#[test]
fn primary_relief_is_byte_deterministic() {
    let fixture = fixture();
    let mut first_diagnostics = Vec::new();
    let first = PrimaryReliefGenerator::generate(
        fixture.bundle.authoritative_surface(),
        &fixture.evolved,
        &fixture.substrate,
        &ReliefSpec::default(),
        &mut relief_rng(None),
        &mut first_diagnostics,
    )
    .unwrap();
    let mut second_diagnostics = Vec::new();
    let second = PrimaryReliefGenerator::generate(
        fixture.bundle.authoritative_surface(),
        &fixture.evolved,
        &fixture.substrate,
        &ReliefSpec::default(),
        &mut relief_rng(None),
        &mut second_diagnostics,
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert_eq!(first_diagnostics, second_diagnostics);
}

#[test]
fn primary_relief_honors_an_already_cancelled_stream() {
    let fixture = fixture();
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    let error = PrimaryReliefGenerator::generate(
        fixture.bundle.authoritative_surface(),
        &fixture.evolved,
        &fixture.substrate,
        &ReliefSpec::default(),
        &mut relief_rng(Some(&cancellation)),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert_eq!(error, PrimaryReliefGenerationError::Cancelled);
}
