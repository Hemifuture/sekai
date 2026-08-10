use sekai::engine::{derive_stage_seed, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::ReliefGenerator;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BoundaryRecord, CrustKind, CrustKindField, LandOceanKind, PlateIdField, SphericalCrustState,
    SphericalMantleSnapshot, SphericalOrogenyKind, SphericalPlate, SphericalPlateRotation,
    SphericalReliefSnapshot, SphericalTectonicSnapshot, CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
    MANTLE_SNAPSHOT_SCHEMA_V2, NO_OROGENY_AGE_SENTINEL_MYR, TECTONIC_SNAPSHOT_SCHEMA_V3,
};
use sekai::world::spatial::{SurfaceRef, UnitVector3};
use sekai::world::{CellId, Meters, PlateId, RootSeed, SphericalSpaceSpec};

const TRENCH_CELL: usize = 4;
const UPLIFT_CELL: usize = 5;
const OROGEN_CELL: usize = 6;
const SUBMERGED_CONTINENT_CELL: usize = 8;
const EMERGENT_OCEAN_CELL: usize = 9;

fn surface() -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 42,
    })
    .unwrap()
}

fn stage_rng(seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("tectonic-heightmap-test", 1, "sekai.test"),
    ))
}

fn set_continental(
    index: usize,
    thickness: f32,
    elevation: f32,
    kinds: &mut [CrustKind],
    thickness_km: &mut [f32],
    age_myr: &mut [f32],
    tectonic_elevation_m: &mut [f32],
) {
    kinds[index] = CrustKind::Continental;
    thickness_km[index] = thickness;
    age_myr[index] = CONTINENTAL_CRUST_AGE_SENTINEL_MYR;
    tectonic_elevation_m[index] = elevation;
}

fn tectonic_fixture(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    orogeny_age_myr: f32,
    orogeny_lineation: [f32; 2],
) -> SphericalTectonicSnapshot {
    let cell_count = surface.cells().len();
    let mut kinds = vec![CrustKind::Oceanic; cell_count];
    let mut thickness_km = vec![7.0; cell_count];
    let mut age_myr = vec![48.0; cell_count];
    let mut tectonic_elevation_m = vec![-4_000.0; cell_count];
    let mut lineation_east = vec![0.0; cell_count];
    let mut lineation_north = vec![0.0; cell_count];
    let mut orogeny_kind = vec![SphericalOrogenyKind::None; cell_count];
    let mut orogeny_age = vec![NO_OROGENY_AGE_SENTINEL_MYR; cell_count];

    set_continental(
        0,
        20.0,
        -1_500.0,
        &mut kinds,
        &mut thickness_km,
        &mut age_myr,
        &mut tectonic_elevation_m,
    );
    set_continental(
        1,
        60.0,
        -1_500.0,
        &mut kinds,
        &mut thickness_km,
        &mut age_myr,
        &mut tectonic_elevation_m,
    );
    age_myr[2] = 0.0;
    tectonic_elevation_m[2] = -1_000.0;
    age_myr[3] = 196.0;
    tectonic_elevation_m[3] = -6_000.0;
    age_myr[TRENCH_CELL] = 96.0;
    tectonic_elevation_m[TRENCH_CELL] = -8_000.0;
    set_continental(
        UPLIFT_CELL,
        42.0,
        3_000.0,
        &mut kinds,
        &mut thickness_km,
        &mut age_myr,
        &mut tectonic_elevation_m,
    );
    lineation_east[UPLIFT_CELL] = 1.0;
    orogeny_kind[UPLIFT_CELL] = SphericalOrogenyKind::Andean;
    orogeny_age[UPLIFT_CELL] = 12.0;
    set_continental(
        OROGEN_CELL,
        48.0,
        4_500.0,
        &mut kinds,
        &mut thickness_km,
        &mut age_myr,
        &mut tectonic_elevation_m,
    );
    lineation_east[OROGEN_CELL] = orogeny_lineation[0];
    lineation_north[OROGEN_CELL] = orogeny_lineation[1];
    orogeny_kind[OROGEN_CELL] = SphericalOrogenyKind::Himalayan;
    orogeny_age[OROGEN_CELL] = orogeny_age_myr;
    set_continental(
        SUBMERGED_CONTINENT_CELL,
        35.0,
        -1_200.0,
        &mut kinds,
        &mut thickness_km,
        &mut age_myr,
        &mut tectonic_elevation_m,
    );
    age_myr[EMERGENT_OCEAN_CELL] = 0.0;
    tectonic_elevation_m[EMERGENT_OCEAN_CELL] = 1_200.0;

    let crust = SphericalCrustState::new(
        CrustKindField::from_kinds(kinds),
        thickness_km,
        age_myr,
        tectonic_elevation_m,
        lineation_east,
        lineation_north,
        orogeny_kind,
        orogeny_age,
    )
    .unwrap();
    let rotation =
        SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
    SphericalTectonicSnapshot::new(
        TECTONIC_SNAPSHOT_SCHEMA_V3,
        SurfaceRef::for_spherical(surface),
        vec![SphericalPlate::new(
            PlateId::from_raw(0),
            CellId::from_raw(0),
            rotation,
        )],
        PlateIdField::from_ids(vec![PlateId::from_raw(0); cell_count]),
        crust,
        vec![BoundaryRecord::none(); surface.edges().len()],
        Vec::new(),
    )
    .unwrap()
}

fn neutral_mantle(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
) -> SphericalMantleSnapshot {
    SphericalMantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        Vec::new(),
        vec![80.0; surface.cells().len()],
        vec![0.0; surface.cells().len()],
    )
    .unwrap()
}

fn generate(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
    mantle: &SphericalMantleSnapshot,
    seed: u64,
) -> (SphericalReliefSnapshot, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let relief = ReliefGenerator::generate_spherical(
        surface,
        tectonic,
        mantle,
        &mut stage_rng(seed),
        &mut diagnostics,
    )
    .unwrap();
    (relief, diagnostics)
}

#[test]
fn current_crust_state_drives_coarse_height_and_not_a_kind_mask() {
    let surface = surface();
    let tectonic = tectonic_fixture(&surface, 8.0, [1.0, 0.0]);
    let mantle = neutral_mantle(&surface);
    let (relief, _) = generate(&surface, &tectonic, &mantle, 71);
    let base = relief.crust_base_elevation_m().values();
    let offset = relief.tectonic_offset_m().values();

    assert!(
        base[1] > base[0],
        "thicker continental crust did not raise isostatic base"
    );
    assert!(
        base[3] < base[2],
        "older oceanic crust did not thermally subside"
    );
    for index in 0..surface.cells().len() {
        assert!(
            (base[index] + offset[index] - tectonic.tectonic_elevation_m()[index]).abs() <= 0.5,
            "cell {index} no longer explains the evolved coarse tectonic height"
        );
    }
    assert!(relief.elevation_m().values()[TRENCH_CELL] < 0.0);
    assert!(relief.elevation_m().values()[UPLIFT_CELL] > 0.0);
    assert!(relief.regional_offset_m().values()[OROGEN_CELL] > 0.0);
    assert_eq!(relief.sea_level_m().to_bits(), 0.0_f32.to_bits());
    assert_eq!(
        relief.land_ocean_kind(CellId::from_raw(SUBMERGED_CONTINENT_CELL as u32)),
        Some(LandOceanKind::Ocean)
    );
    assert_eq!(
        relief.land_ocean_kind(CellId::from_raw(EMERGENT_OCEAN_CELL as u32)),
        Some(LandOceanKind::Land)
    );
}

#[test]
fn lineation_age_and_only_the_detail_seed_control_bounded_regional_detail() {
    let surface = surface();
    let mantle = neutral_mantle(&surface);
    let young = tectonic_fixture(&surface, 8.0, [1.0, 0.0]);
    let old = tectonic_fixture(&surface, 320.0, [1.0, 0.0]);
    let rotated = tectonic_fixture(&surface, 8.0, [0.0, 1.0]);
    let young_before = young.clone();
    let (first, _) = generate(&surface, &young, &mantle, 91);
    let (changed_seed, _) = generate(&surface, &young, &mantle, 92);
    let (old_relief, _) = generate(&surface, &old, &mantle, 91);
    let (rotated_relief, _) = generate(&surface, &rotated, &mantle, 91);

    assert_eq!(
        young, young_before,
        "height synthesis mutated current tectonic state"
    );
    assert_eq!(
        first.crust_base_elevation_m(),
        changed_seed.crust_base_elevation_m()
    );
    assert_eq!(first.tectonic_offset_m(), changed_seed.tectonic_offset_m());
    assert_ne!(first.regional_offset_m(), changed_seed.regional_offset_m());
    assert!(
        first.regional_offset_m().values()[OROGEN_CELL]
            > old_relief.regional_offset_m().values()[OROGEN_CELL],
        "old orogeny did not attenuate directed detail"
    );
    assert_ne!(
        first.regional_offset_m().values()[OROGEN_CELL].to_bits(),
        rotated_relief.regional_offset_m().values()[OROGEN_CELL].to_bits(),
        "rotating lineation did not rotate the directed detail response"
    );
    for relief in [&first, &changed_seed] {
        assert!(relief.elevation_m().values()[TRENCH_CELL] < 0.0);
        assert!(relief.elevation_m().values()[UPLIFT_CELL] > 0.0);
    }
}
