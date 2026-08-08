use sekai::view::{SphericalMeshBudgets, SphericalMeshError};

#[test]
fn public_count_checks_distinguish_budgets_and_checked_u32_overflow() {
    let budgets = SphericalMeshBudgets::new(4, 8, 12, 16);
    assert!(matches!(
        budgets.check_counts(5, 0, 0, 0),
        Err(SphericalMeshError::CellBudgetExceeded { actual: 5, max: 4 })
    ));
    assert!(matches!(
        budgets.check_counts(0, 9, 0, 0),
        Err(SphericalMeshError::VertexBudgetExceeded { actual: 9, max: 8 })
    ));
    assert!(matches!(
        budgets.check_counts(0, 0, 13, 0),
        Err(SphericalMeshError::IndexBudgetExceeded {
            actual: 13,
            max: 12
        })
    ));
    assert!(matches!(
        budgets.check_counts(0, 0, 0, 17),
        Err(SphericalMeshError::EdgeSegmentBudgetExceeded {
            actual: 17,
            max: 16
        })
    ));

    #[cfg(target_pointer_width = "64")]
    {
        let count = u32::MAX as usize + 1;
        let unbounded = SphericalMeshBudgets::new(usize::MAX, usize::MAX, usize::MAX, usize::MAX);
        assert!(matches!(
            unbounded.check_counts(0, count, 0, 0),
            Err(SphericalMeshError::IntegerOverflow {
                context: "projected vertex count"
            })
        ));
    }
}

#[test]
fn default_budgets_cover_the_authoritative_spherical_limits() {
    let budgets = SphericalMeshBudgets::default();
    assert_eq!(budgets, SphericalMeshBudgets::DEFAULT);
    assert!(budgets.cells() >= sekai::world::MAX_SPHERICAL_CELL_COUNT as usize);
    assert!(budgets.edge_segments() >= sekai::world::MAX_SPHERICAL_EDGE_COUNT as usize * 2);
    assert!(budgets.vertices() >= budgets.cells());
    assert!(budgets.indices() >= budgets.vertices());
}
