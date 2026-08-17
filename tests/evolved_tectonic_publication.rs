use std::sync::OnceLock;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{EvolvedTectonicGenerationError, EvolvedTectonicGenerator};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    CrustKind, NaturalQualityProfile, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    TectonicSpec, WorldFormationPreset, CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};

fn bundle() -> &'static ProfileSurfaceBundle {
    static BUNDLE: OnceLock<ProfileSurfaceBundle> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap()
    })
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn generate(seed: u64) -> sekai::world::natural::EvolvedTectonicSnapshot {
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
    ));
    EvolvedTectonicGenerator::generate(bundle(), &TectonicSpec::default(), &formation(), &mut rng)
        .unwrap()
}

#[test]
fn publication_is_identity_bound_conservative_derived_and_repeatable() {
    let first = generate(42);
    let repeated = generate(42);
    let authority = bundle().authoritative_surface();
    let map = bundle().control_to_authoritative_map();

    first.validate_against(authority).unwrap();
    assert_eq!(first.surface_ref(), map.target_ref());
    assert_eq!(map.source_ref().cell_count(), 4_842);
    assert_eq!(map.target_ref().cell_count(), 20_252);
    assert_eq!(
        first.material().totals(),
        first.material_budget().final_authoritative()
    );
    assert!(first.material_budget().max_authority_relative_error() <= 1.0e-6);
    assert_eq!(
        first.lineage_budget().final_live_lineages() as usize,
        first.compatibility().plates().len()
    );
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );

    for (index, cell) in authority.cells().iter().enumerate() {
        let material = first.material();
        let represented_area = material.continental_reference_area_m2()[index]
            + material.oceanic_reference_area_m2()[index];
        assert!(
            (represented_area - cell.area.get()).abs() / cell.area.get() <= 1.0e-6,
            "cell {index} does not close its mapped area"
        );
        assert_eq!(
            first.compatibility().crust_kind(cell.id),
            material.compatibility_kind(index)
        );
        assert_eq!(
            first.compatibility().crust_thickness_for_cell(cell.id),
            material.compatibility_thickness_km(index)
        );
        match material.compatibility_kind(index).unwrap() {
            CrustKind::Continental => assert_eq!(
                first.compatibility().crust_age_myr()[index],
                CONTINENTAL_CRUST_AGE_SENTINEL_MYR
            ),
            CrustKind::Oceanic => {
                assert!(first.compatibility().crust_age_myr()[index] >= 0.0)
            }
        }
        let east = first.compatibility().crust_state().lineation_east()[index];
        let north = first.compatibility().crust_state().lineation_north()[index];
        let norm = east.hypot(north);
        assert!(norm == 0.0 || (norm - 1.0).abs() <= 1.0e-4);
    }
}

#[test]
fn publication_uses_the_retained_p1_map_and_records_category_ambiguity() {
    let snapshot = generate(97);
    let budget = snapshot.material_budget();

    assert!(budget.category_ambiguity_area_fraction().is_finite());
    assert!((0.0..=1.0).contains(&budget.category_ambiguity_area_fraction()));
    assert!(budget.categorical_area_quantization_m2() > 0.0);
    assert!(budget.max_control_relative_error() <= 1.0e-9);
    assert!(budget.max_authority_relative_error() <= 1.0e-6);
    assert!(snapshot
        .forcing()
        .boundary_distance_m()
        .iter()
        .all(|distance| distance.is_finite() && *distance >= 0.0));
}

#[test]
fn cancellation_is_atomic_before_control_evolution_or_authority_remap() {
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    let mut rng = StageRng::from_seed_with_cancellation(
        derive_stage_seed(
            RootSeed::new(31),
            StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
        ),
        &cancellation,
    );

    let error = EvolvedTectonicGenerator::generate(
        bundle(),
        &TectonicSpec::default(),
        &formation(),
        &mut rng,
    )
    .unwrap_err();
    assert_eq!(error, EvolvedTectonicGenerationError::Cancelled);
}

#[test]
fn publication_source_has_no_nearest_or_barycentric_material_fallback() {
    let source = include_str!("../src/generators/natural/spherical_tectonics/publication.rs");
    assert!(source.contains("remap_extensive_f64"));
    assert!(!source.contains("project_current_state("));
    assert!(!source.contains("walk_nearest_cell("));
    assert!(!source.contains("interpolate_dense_control_material("));
}

#[test]
#[ignore = "release-only Standard/High cooperative-cancellation publication paths"]
fn standard_and_high_publication_cancel_without_returning_partial_snapshots() {
    for profile in [NaturalQualityProfile::Standard, NaturalQualityProfile::High] {
        let profile_bundle = ProfileSurfaceBuilder::build(
            profile,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let cancellation = BuildCancellation::new();
        let worker_cancellation = cancellation.clone();
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = Arc::clone(&barrier);
        let worker = std::thread::spawn(move || {
            let mut rng = StageRng::from_seed_with_cancellation(
                derive_stage_seed(
                    RootSeed::new(73),
                    StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
                ),
                &worker_cancellation,
            );
            worker_barrier.wait();
            EvolvedTectonicGenerator::generate(
                &profile_bundle,
                &TectonicSpec::default(),
                &formation(),
                &mut rng,
            )
        });
        barrier.wait();
        std::thread::sleep(Duration::from_millis(10));
        cancellation.cancel();
        assert_eq!(
            worker.join().unwrap(),
            Err(EvolvedTectonicGenerationError::Cancelled),
            "{profile:?} returned a partial publication"
        );
    }
}
