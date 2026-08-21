use sekai::world::natural::{
    CrustMaterialTotals, SphericalTectonicMaterialBudget, SphericalTectonicMaterialProcesses,
    TectonicMaterialAmount,
};

fn amount(area_m2: f64, volume_m3: f64) -> TectonicMaterialAmount {
    TectonicMaterialAmount::new(area_m2, volume_m3).unwrap()
}

#[test]
fn named_material_sources_and_sinks_close_the_public_budget_equations() {
    let initial = CrustMaterialTotals::new(amount(100.0, 4_000_000.0), amount(200.0, 1_400_000.0));
    let processes = SphericalTectonicMaterialProcesses::new(
        10.0,
        TectonicMaterialAmount::zero(),
        amount(20.0, 140_000.0),
        amount(10.0, 70_000.0),
        amount(5.0, 35_000.0),
        amount(3.0, 21_000.0),
    )
    .unwrap();
    let final_control =
        CrustMaterialTotals::new(amount(110.0, 4_000_000.0), amount(192.0, 1_344_000.0));
    let budget = SphericalTectonicMaterialBudget::new(
        initial,
        processes,
        final_control,
        final_control,
        0.0,
        0.0,
    )
    .unwrap();

    assert_eq!(budget.control_residual().continental_area_m2(), 0.0);
    assert_eq!(budget.control_residual().continental_volume_m3(), 0.0);
    assert_eq!(budget.control_residual().oceanic_area_m2(), 0.0);
    assert_eq!(budget.control_residual().oceanic_volume_m3(), 0.0);
    assert_eq!(budget.max_control_relative_error(), 0.0);
    assert_eq!(budget.max_authority_relative_error(), 0.0);
}

#[test]
fn unnamed_loss_cannot_be_hidden_in_a_material_budget() {
    let initial = CrustMaterialTotals::new(amount(100.0, 4_000_000.0), amount(200.0, 1_400_000.0));
    let processes = SphericalTectonicMaterialProcesses::new(
        0.0,
        TectonicMaterialAmount::zero(),
        TectonicMaterialAmount::zero(),
        TectonicMaterialAmount::zero(),
        TectonicMaterialAmount::zero(),
        TectonicMaterialAmount::zero(),
    )
    .unwrap();
    let silently_lost =
        CrustMaterialTotals::new(amount(90.0, 3_600_000.0), amount(200.0, 1_400_000.0));

    assert!(SphericalTectonicMaterialBudget::new(
        initial,
        processes,
        silently_lost,
        silently_lost,
        0.0,
        0.0,
    )
    .is_err());
}
