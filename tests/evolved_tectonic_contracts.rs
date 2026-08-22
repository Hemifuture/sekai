use std::sync::OnceLock;

use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BoundaryRecord, CrustKind, CrustKindField, CrustMaterialResidual, CrustMaterialTotals,
    EvolvedTectonicSnapshot, NaturalQualityProfile, PlateIdField, SphericalCrustMaterialState,
    SphericalCrustState, SphericalOrogenyKind, SphericalPlate, SphericalPlateRotation,
    SphericalTectonicForcingState, SphericalTectonicLineageBudget, SphericalTectonicMaterialBudget,
    SphericalTectonicMaterialProcesses, SphericalTectonicSnapshot, TectonicMaterialAmount,
    CONTINENTAL_CRUST_AGE_SENTINEL_MYR, EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1,
    MAX_TECTONIC_AUTHORITY_RELATIVE_BUDGET_ERROR, NATURAL_RESOLUTION_PLAN_SCHEMA_V1,
    NO_OROGENY_AGE_SENTINEL_MYR, TECTONIC_SNAPSHOT_SCHEMA_V3,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef, UnitVector3};
use sekai::world::{CellId, Meters, PlateId, SphericalSpaceSpec};

const EARTH_RADIUS_M: f64 = 6_371_000.0;

fn surface() -> &'static SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(EARTH_RADIUS_M).unwrap(),
            target_cell_count: NaturalQualityProfile::Draft.authoritative_target_cell_count(),
        })
        .unwrap()
    })
}

fn compatibility_snapshot(surface: &SphericalSurfaceSnapshot) -> SphericalTectonicSnapshot {
    let cell_count = surface.cells().len();
    let mut kinds = vec![CrustKind::Oceanic; cell_count];
    kinds[0] = CrustKind::Continental;
    let mut thickness = vec![3.0; cell_count];
    thickness[0] = 20.0;
    let mut ages = vec![0.0; cell_count];
    ages[0] = CONTINENTAL_CRUST_AGE_SENTINEL_MYR;
    let crust = SphericalCrustState::new(
        CrustKindField::from_kinds(kinds),
        thickness,
        ages,
        vec![0.0; cell_count],
        vec![0.0; cell_count],
        vec![0.0; cell_count],
        vec![SphericalOrogenyKind::None; cell_count],
        vec![NO_OROGENY_AGE_SENTINEL_MYR; cell_count],
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

fn material_state(surface: &SphericalSurfaceSnapshot) -> SphericalCrustMaterialState {
    let cell_count = surface.cells().len();
    let mut continental_area = vec![0.0; cell_count];
    continental_area[0] = surface.cells()[0].area.get();
    let mut continental_volume = vec![0.0; cell_count];
    continental_volume[0] = continental_area[0] * 20_000.0;
    let mut oceanic_area = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .collect::<Vec<_>>();
    oceanic_area[0] = 0.0;
    let oceanic_volume = oceanic_area
        .iter()
        .map(|area| area * 3_000.0)
        .collect::<Vec<_>>();
    SphericalCrustMaterialState::new(
        continental_area,
        continental_volume,
        oceanic_area,
        oceanic_volume,
    )
    .unwrap()
}

fn evolved_snapshot() -> EvolvedTectonicSnapshot {
    let surface = surface();
    let material = material_state(surface);
    let totals = material.totals();
    let processes = SphericalTectonicMaterialProcesses::new(
        0.0,
        0.0,
        TectonicMaterialAmount::zero(),
        TectonicMaterialAmount::zero(),
        TectonicMaterialAmount::zero(),
        TectonicMaterialAmount::zero(),
        TectonicMaterialAmount::zero(),
    )
    .unwrap();
    let budget =
        SphericalTectonicMaterialBudget::new(totals, processes, totals, totals, 0.0, 0.0).unwrap();
    let forcing = SphericalTectonicForcingState::new(
        vec![0.0; surface.cells().len()],
        vec![0.0; surface.cells().len()],
        vec![0.0; surface.cells().len()],
        vec![0.0; surface.cells().len()],
        vec![NO_OROGENY_AGE_SENTINEL_MYR; surface.cells().len()],
    )
    .unwrap();
    EvolvedTectonicSnapshot::new(
        EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1,
        NaturalQualityProfile::Draft
            .resolve(&SphericalSpaceSpec {
                radius: Meters::new(EARTH_RADIUS_M).unwrap(),
                target_cell_count: NaturalQualityProfile::Draft.authoritative_target_cell_count(),
            })
            .unwrap(),
        compatibility_snapshot(surface),
        material,
        forcing,
        budget,
        SphericalTectonicLineageBudget::new(1, 0, 0, 1, 0, 0).unwrap(),
    )
    .unwrap()
}

#[test]
fn evolved_snapshot_round_trips_and_cross_validates_every_nested_identity() {
    let snapshot = evolved_snapshot();
    snapshot.validate().unwrap();
    snapshot.validate_against(surface()).unwrap();
    assert_eq!(
        snapshot.schema_version(),
        EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1
    );
    assert_eq!(
        snapshot.resolution_plan().schema_version(),
        NATURAL_RESOLUTION_PLAN_SCHEMA_V1
    );
    assert_eq!(
        snapshot.compatibility().surface_ref(),
        SurfaceRef::for_spherical(surface())
    );
    assert_eq!(snapshot.material().len(), surface().cells().len());
    assert_eq!(snapshot.forcing().len(), surface().cells().len());
    assert_eq!(
        snapshot.material_budget().final_authoritative(),
        snapshot.material().totals()
    );

    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(
        encoded["schema_version"],
        EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1
    );
    assert_eq!(
        encoded["compatibility"]["schema_version"],
        TECTONIC_SNAPSHOT_SCHEMA_V3
    );
    let decoded: EvolvedTectonicSnapshot = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, snapshot);
}

#[test]
fn evolved_wire_is_strict_and_recomputes_budget_and_lineage_evidence() {
    let snapshot = evolved_snapshot();
    let encoded = serde_json::to_value(snapshot).unwrap();

    let mut wrong_schema = encoded.clone();
    wrong_schema["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<EvolvedTectonicSnapshot>(wrong_schema).is_err());

    let mut unknown = encoded.clone();
    unknown["history"] = serde_json::json!([]);
    assert!(serde_json::from_value::<EvolvedTectonicSnapshot>(unknown).is_err());

    let mut nested_unknown = encoded.clone();
    nested_unknown["material"]["density"] = serde_json::json!([]);
    assert!(serde_json::from_value::<EvolvedTectonicSnapshot>(nested_unknown).is_err());

    let mut forged_residual = encoded.clone();
    forged_residual["material_budget"]["control_residual"]["continental_area_m2"] =
        serde_json::json!(1.0);
    assert!(serde_json::from_value::<EvolvedTectonicSnapshot>(forged_residual).is_err());

    let mut broken_lineage = encoded;
    broken_lineage["lineage_budget"]["final_live_lineages"] = serde_json::json!(2);
    assert!(serde_json::from_value::<EvolvedTectonicSnapshot>(broken_lineage).is_err());
}

#[test]
fn material_state_rejects_invalid_components_and_derives_compatibility_values() {
    let state = SphericalCrustMaterialState::new(
        vec![10.0, 4.0],
        vec![200_000.0, 80_000.0],
        vec![0.0, 6.0],
        vec![0.0, 18_000.0],
    )
    .unwrap();
    assert_eq!(state.compatibility_kind(0), Some(CrustKind::Continental));
    assert_eq!(state.compatibility_kind(1), Some(CrustKind::Oceanic));
    assert_eq!(state.compatibility_thickness_km(0), Some(20.0));
    assert_eq!(state.compatibility_thickness_km(1), Some(3.0));
    assert_eq!(
        state.totals(),
        CrustMaterialTotals::new(
            TectonicMaterialAmount::new(14.0, 280_000.0).unwrap(),
            TectonicMaterialAmount::new(6.0, 18_000.0).unwrap(),
        )
    );

    assert!(SphericalCrustMaterialState::new(vec![1.0], vec![0.0], vec![0.0], vec![0.0]).is_err());
    assert!(
        SphericalCrustMaterialState::new(vec![0.0], vec![1.0], vec![1.0], vec![3_000.0]).is_err()
    );
    assert!(
        SphericalCrustMaterialState::new(vec![f64::NAN], vec![0.0], vec![1.0], vec![3_000.0])
            .is_err()
    );
    assert!(SphericalCrustMaterialState::new(
        vec![1.0, 1.0],
        vec![20_000.0],
        vec![0.0, 0.0],
        vec![0.0, 0.0]
    )
    .is_err());
}

#[test]
fn forcing_contract_rejects_negative_non_finite_and_invalid_event_age() {
    let valid = SphericalTectonicForcingState::new(
        vec![0.6],
        vec![0.3],
        vec![42.0],
        vec![600_000.0],
        vec![0.0],
    )
    .unwrap();
    assert_eq!(valid.uplift_rate_mm_per_year(), &[0.6]);
    assert_eq!(valid.subsidence_rate_mm_per_year(), &[0.3]);
    assert_eq!(valid.shortening_rate_mm_per_year(), &[42.0]);
    assert_eq!(valid.boundary_distance_m(), &[600_000.0]);
    assert_eq!(valid.event_age_myr(), &[0.0]);

    assert!(SphericalTectonicForcingState::new(
        vec![-0.1],
        vec![0.0],
        vec![0.0],
        vec![0.0],
        vec![-1.0]
    )
    .is_err());
    assert!(SphericalTectonicForcingState::new(
        vec![0.0],
        vec![f32::INFINITY],
        vec![0.0],
        vec![0.0],
        vec![-1.0]
    )
    .is_err());
    assert!(SphericalTectonicForcingState::new(
        vec![0.0],
        vec![0.0],
        vec![0.0],
        vec![0.0],
        vec![-0.5]
    )
    .is_err());
    assert!(SphericalTectonicForcingState::new(
        vec![0.0, 0.0],
        vec![0.0],
        vec![0.0],
        vec![0.0],
        vec![-1.0]
    )
    .is_err());
}

#[test]
fn material_and_lineage_budgets_enforce_exact_equations_and_error_limits() {
    let initial = CrustMaterialTotals::new(
        TectonicMaterialAmount::new(30.0, 900_000.0).unwrap(),
        TectonicMaterialAmount::new(70.0, 490_000.0).unwrap(),
    );
    let processes = SphericalTectonicMaterialProcesses::new(
        3.0,
        0.0,
        TectonicMaterialAmount::zero(),
        TectonicMaterialAmount::new(4.0, 28_000.0).unwrap(),
        TectonicMaterialAmount::new(6.0, 42_000.0).unwrap(),
        TectonicMaterialAmount::new(1.0, 7_000.0).unwrap(),
        TectonicMaterialAmount::zero(),
    )
    .unwrap();
    let final_control = CrustMaterialTotals::new(
        TectonicMaterialAmount::new(33.0, 900_000.0).unwrap(),
        TectonicMaterialAmount::new(73.0, 511_000.0).unwrap(),
    );
    let final_authority = final_control;
    let budget = SphericalTectonicMaterialBudget::new(
        initial,
        processes,
        final_control,
        final_authority,
        0.25,
        0.01,
    )
    .unwrap();
    assert_eq!(budget.control_residual(), CrustMaterialResidual::zero());
    assert_eq!(
        budget.authority_remap_residual(),
        CrustMaterialResidual::zero()
    );
    assert_eq!(budget.max_control_relative_error(), 0.0);
    assert_eq!(budget.max_authority_relative_error(), 0.0);

    let excessive = CrustMaterialTotals::new(
        TectonicMaterialAmount::new(
            33.0 * (1.0 + MAX_TECTONIC_AUTHORITY_RELATIVE_BUDGET_ERROR * 2.0),
            900_000.0,
        )
        .unwrap(),
        final_control.oceanic(),
    );
    assert!(SphericalTectonicMaterialBudget::new(
        initial,
        processes,
        final_control,
        excessive,
        0.0,
        0.0,
    )
    .is_err());

    let lineage = SphericalTectonicLineageBudget::new(12, 7, 5, 14, 3, 2).unwrap();
    assert_eq!(lineage.final_live_lineages(), 14);
    assert!(SphericalTectonicLineageBudget::new(12, 7, 4, 14, 3, 2).is_err());
}
