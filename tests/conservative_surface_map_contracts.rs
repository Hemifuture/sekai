use sekai::world::spatial::{
    ConservativeSurfaceMap, SurfaceGeometryKind, SurfaceOverlapWeight, SurfaceRef,
    TangentTransform, CONSERVATIVE_SURFACE_MAP_SCHEMA_V1, SPHERICAL_SURFACE_SCHEMA_V1,
};
use sekai::world::{CellId, MAX_SPHERICAL_CELL_COUNT};

fn surface_ref(fingerprint: u8) -> SurfaceRef {
    SurfaceRef::new(
        SurfaceGeometryKind::SphericalV1,
        SPHERICAL_SURFACE_SCHEMA_V1,
        3,
        3,
        [fingerprint; 32],
    )
    .unwrap()
}

fn identity_weight(source: u32, area_m2: f64) -> SurfaceOverlapWeight {
    SurfaceOverlapWeight::new(
        CellId::from_raw(source),
        area_m2,
        TangentTransform::identity(),
    )
    .unwrap()
}

fn identity_map() -> ConservativeSurfaceMap {
    ConservativeSurfaceMap::new(
        CONSERVATIVE_SURFACE_MAP_SCHEMA_V1,
        surface_ref(1),
        surface_ref(2),
        vec![1.0, 2.0, 3.0],
        vec![1.0, 2.0, 3.0],
        vec![0, 1, 2, 3],
        vec![
            identity_weight(0, 1.0),
            identity_weight(1, 2.0),
            identity_weight(2, 3.0),
        ],
        0,
        0.0,
    )
    .unwrap()
}

#[test]
fn identity_map_round_trips_with_canonical_rows_and_exact_stats() {
    let map = identity_map();
    map.validate().unwrap();
    assert_eq!(map.schema_version(), CONSERVATIVE_SURFACE_MAP_SCHEMA_V1);
    assert_eq!(map.source_ref(), surface_ref(1));
    assert_eq!(map.target_ref(), surface_ref(2));
    assert_eq!(map.source_cell_areas_m2(), &[1.0, 2.0, 3.0]);
    assert_eq!(map.target_cell_areas_m2(), &[1.0, 2.0, 3.0]);
    assert_eq!(map.target_row_offsets(), &[0, 1, 2, 3]);
    assert_eq!(map.overlap_count(), 3);
    assert_eq!(
        map.target_row(CellId::from_raw(1)).unwrap(),
        &[identity_weight(1, 2.0)]
    );
    assert!(map.target_row(CellId::from_raw(3)).is_none());
    assert_eq!(map.solve_stats().balance_iterations(), 0);
    assert_eq!(map.solve_stats().max_source_margin_relative_error(), 0.0);
    assert_eq!(map.solve_stats().max_target_margin_relative_error(), 0.0);
    assert_eq!(map.solve_stats().max_relative_geometric_adjustment(), 0.0);

    let json = serde_json::to_string(&map).unwrap();
    let decoded: ConservativeSurfaceMap = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, map);
    assert_eq!(serde_json::to_string(&decoded).unwrap(), json);
}

#[test]
fn primitives_reject_non_finite_out_of_range_or_non_positive_values() {
    assert_eq!(TangentTransform::identity().apply([2.0, -3.0]), [2.0, -3.0]);
    assert!(TangentTransform::new([f64::NAN, 0.0, 0.0, 1.0]).is_err());
    assert!(TangentTransform::new([1.01, 0.0, 0.0, 1.0]).is_err());
    assert!(
        SurfaceOverlapWeight::new(CellId::from_raw(0), 0.0, TangentTransform::identity()).is_err()
    );
    assert!(SurfaceOverlapWeight::new(
        CellId::from_raw(0),
        f64::INFINITY,
        TangentTransform::identity()
    )
    .is_err());
}

#[test]
fn map_rejects_cardinality_offsets_order_coverage_and_margin_contradictions() {
    let valid_weights = vec![
        identity_weight(0, 1.0),
        identity_weight(1, 2.0),
        identity_weight(2, 3.0),
    ];
    let build = |source_areas: Vec<f64>,
                 target_areas: Vec<f64>,
                 offsets: Vec<u32>,
                 weights: Vec<SurfaceOverlapWeight>| {
        ConservativeSurfaceMap::new(
            CONSERVATIVE_SURFACE_MAP_SCHEMA_V1,
            surface_ref(1),
            surface_ref(2),
            source_areas,
            target_areas,
            offsets,
            weights,
            0,
            0.0,
        )
    };

    assert!(build(
        vec![1.0, 2.0],
        vec![1.0, 2.0, 3.0],
        vec![0, 1, 2, 3],
        valid_weights.clone()
    )
    .is_err());
    assert!(build(
        vec![1.0, 2.0, 3.0],
        vec![1.0, 2.0, 3.0],
        vec![1, 2, 3, 3],
        valid_weights.clone()
    )
    .is_err());
    assert!(build(
        vec![1.0, 2.0, 3.0],
        vec![1.0, 2.0, 3.0],
        vec![0, 1, 1, 3],
        valid_weights.clone()
    )
    .is_err());
    assert!(build(
        vec![1.0, 2.0, 3.0],
        vec![1.0, 2.0, 3.0],
        vec![0, 2, 3, 4],
        vec![
            identity_weight(0, 0.5),
            identity_weight(0, 0.5),
            identity_weight(1, 2.0),
            identity_weight(2, 3.0),
        ]
    )
    .is_err());
    assert!(build(
        vec![1.0, 2.0, 3.0],
        vec![3.0, 0.5, 2.5],
        vec![0, 2, 3, 4],
        vec![
            identity_weight(2, 2.0),
            identity_weight(0, 1.0),
            identity_weight(1, 0.5),
            identity_weight(2, 2.5),
        ]
    )
    .is_err());
    assert!(build(
        vec![1.0, 2.0, 3.0],
        vec![1.1, 1.9, 3.0],
        vec![0, 1, 2, 3],
        valid_weights
    )
    .is_err());
}

#[test]
fn map_deserialization_rejects_unknown_schema_allocation_and_stale_stats() {
    let value = serde_json::to_value(identity_map()).unwrap();

    let mut wrong_schema = value.clone();
    wrong_schema["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<ConservativeSurfaceMap>(wrong_schema).is_err());

    let mut unknown = value.clone();
    unknown["nearest_neighbor_fallback"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ConservativeSurfaceMap>(unknown).is_err());

    let mut excessive = value.clone();
    excessive["source_ref"]["cell_count"] = serde_json::json!(MAX_SPHERICAL_CELL_COUNT + 1);
    assert!(serde_json::from_value::<ConservativeSurfaceMap>(excessive).is_err());

    let mut stale_stats = value.clone();
    stale_stats["solve_stats"]["max_source_margin_relative_error"] = serde_json::json!(0.5);
    assert!(serde_json::from_value::<ConservativeSurfaceMap>(stale_stats).is_err());

    let mut bad_offset = value.clone();
    bad_offset["target_row_offsets"][3] = serde_json::json!(2);
    assert!(serde_json::from_value::<ConservativeSurfaceMap>(bad_offset).is_err());

    let mut bad_source = value;
    bad_source["weights"][0]["source_cell"] = serde_json::json!(3);
    assert!(serde_json::from_value::<ConservativeSurfaceMap>(bad_source).is_err());
}
