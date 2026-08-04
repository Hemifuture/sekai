use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::spatial::{
    SpatialCell, SpatialEdge, SpatialSnapshot, SurfaceGeometryKind, SurfaceRef, SurfaceRefError,
    Topology, SPATIAL_SCHEMA_V1, SPHERICAL_SURFACE_SCHEMA_V1,
};
use sekai::world::{
    BoundaryCondition, CellId, EdgeId, Meters, SphericalSpaceSpec, SquareMeters, WorldPoint,
    WorldRect,
};
use serde_json::json;

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn square_meters(value: f64) -> SquareMeters {
    SquareMeters::new(value).unwrap()
}

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(meters(x), meters(y))
}

fn cell(
    id: u32,
    site: (f64, f64),
    polygon: &[(f64, f64)],
    neighbors: &[u32],
    area: f64,
) -> SpatialCell {
    SpatialCell {
        id: CellId::from_raw(id),
        site: point(site.0, site.1),
        centroid: point(site.0, site.1),
        area: square_meters(area),
        polygon: polygon.iter().map(|&(x, y)| point(x, y)).collect(),
        neighbors: neighbors.iter().copied().map(CellId::from_raw).collect(),
    }
}

fn edge(id: u32, start: (f64, f64), end: (f64, f64), cells: [Option<u32>; 2]) -> SpatialEdge {
    let start = point(start.0, start.1);
    let end = point(end.0, end.1);
    let dx = end.x().get() - start.x().get();
    let dy = end.y().get() - start.y().get();
    SpatialEdge {
        id: EdgeId::from_raw(id),
        start,
        end,
        length: meters(dx.hypot(dy)),
        cells: cells.map(|cell| cell.map(CellId::from_raw)),
    }
}

fn planar_fixture(scale: f64) -> SpatialSnapshot {
    let area = scale * scale;
    let p = |x: f64, y: f64| (x * scale, y * scale);
    let cells = vec![
        cell(
            0,
            p(0.5, 0.5),
            &[p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)],
            &[1, 2],
            area,
        ),
        cell(
            1,
            p(1.5, 0.5),
            &[p(1.0, 0.0), p(2.0, 0.0), p(2.0, 1.0), p(1.0, 1.0)],
            &[0, 3],
            area,
        ),
        cell(
            2,
            p(0.5, 1.5),
            &[p(0.0, 1.0), p(1.0, 1.0), p(1.0, 2.0), p(0.0, 2.0)],
            &[0, 3],
            area,
        ),
        cell(
            3,
            p(1.5, 1.5),
            &[p(1.0, 1.0), p(2.0, 1.0), p(2.0, 2.0), p(1.0, 2.0)],
            &[1, 2],
            area,
        ),
    ];
    let edges = vec![
        edge(0, p(0.0, 0.0), p(1.0, 0.0), [Some(0), None]),
        edge(1, p(0.0, 1.0), p(0.0, 0.0), [Some(0), None]),
        edge(2, p(1.0, 0.0), p(2.0, 0.0), [Some(1), None]),
        edge(3, p(2.0, 0.0), p(2.0, 1.0), [Some(1), None]),
        edge(4, p(0.0, 2.0), p(0.0, 1.0), [Some(2), None]),
        edge(5, p(1.0, 2.0), p(0.0, 2.0), [Some(2), None]),
        edge(6, p(2.0, 1.0), p(2.0, 2.0), [Some(3), None]),
        edge(7, p(2.0, 2.0), p(1.0, 2.0), [Some(3), None]),
        edge(8, p(1.0, 0.0), p(1.0, 1.0), [Some(0), Some(1)]),
        edge(9, p(1.0, 1.0), p(0.0, 1.0), [Some(0), Some(2)]),
        edge(10, p(1.0, 1.0), p(2.0, 1.0), [Some(1), Some(3)]),
        edge(11, p(1.0, 1.0), p(1.0, 2.0), [Some(2), Some(3)]),
    ];
    SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        WorldRect::new(point(0.0, 0.0), point(2.0 * scale, 2.0 * scale)).unwrap(),
        BoundaryCondition::Closed,
        cells,
        edges,
    )
    .unwrap()
}

fn spherical_fixture(radius: f64) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: meters(radius),
        target_cell_count: 42,
    })
    .unwrap()
}

#[test]
fn planar_fingerprint_is_deterministic_without_changing_the_wire_shape() {
    let snapshot = planar_fixture(1.0);
    let first = snapshot.fingerprint();
    let second = snapshot.fingerprint();
    assert_eq!(first, second);
    assert_ne!(first, [0; 32]);

    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert!(encoded.get("fingerprint").is_none());
    let decoded: SpatialSnapshot = serde_json::from_value(encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.fingerprint(), first);
}

#[test]
fn equal_cardinality_different_planar_geometry_has_a_different_identity() {
    let first = planar_fixture(1.0);
    let second = planar_fixture(2.0);
    assert_eq!(first.cell_count(), second.cell_count());
    assert_eq!(first.edges().len(), second.edges().len());

    let first_ref = SurfaceRef::for_planar(&first);
    let second_ref = SurfaceRef::for_planar(&second);
    assert_ne!(first_ref.fingerprint(), second_ref.fingerprint());
    assert_ne!(first_ref, second_ref);
}

#[test]
fn spherical_identity_reuses_the_authoritative_surface_fingerprint() {
    let snapshot = spherical_fixture(6_371_000.0);
    let surface_ref = SurfaceRef::for_spherical(&snapshot);

    assert_eq!(
        surface_ref.geometry_kind(),
        SurfaceGeometryKind::SphericalV1
    );
    assert_eq!(surface_ref.geometry_schema(), SPHERICAL_SURFACE_SCHEMA_V1);
    assert_eq!(surface_ref.cell_count(), snapshot.cells().len() as u32);
    assert_eq!(surface_ref.edge_count(), snapshot.edges().len() as u32);
    assert_eq!(surface_ref.fingerprint(), snapshot.fingerprint());
}

#[test]
fn surface_identity_round_trips_with_explicit_stable_fields() {
    let surface_ref = SurfaceRef::for_planar(&planar_fixture(1.0));
    let value = serde_json::to_value(surface_ref).unwrap();

    assert_eq!(value["geometry_kind"], json!("planar_v1"));
    assert_eq!(value["geometry_schema"], json!(SPATIAL_SCHEMA_V1));
    assert_eq!(value["cell_count"], json!(4));
    assert_eq!(value["edge_count"], json!(12));
    assert_eq!(value["fingerprint"].as_array().unwrap().len(), 32);
    assert_eq!(
        serde_json::from_value::<SurfaceRef>(value).unwrap(),
        surface_ref
    );
}

#[test]
fn constructor_and_deserialization_reject_invalid_identity_values() {
    let valid = SurfaceRef::for_planar(&planar_fixture(1.0));
    assert!(matches!(
        SurfaceRef::new(SurfaceGeometryKind::PlanarV1, 1, 0, 12, [1; 32]),
        Err(SurfaceRefError::EmptyCells)
    ));
    assert!(matches!(
        SurfaceRef::new(SurfaceGeometryKind::PlanarV1, 1, 4, 0, [1; 32]),
        Err(SurfaceRefError::EmptyEdges)
    ));
    assert!(matches!(
        SurfaceRef::new(SurfaceGeometryKind::PlanarV1, 2, 4, 12, [1; 32]),
        Err(SurfaceRefError::UnsupportedGeometrySchema { .. })
    ));
    assert!(matches!(
        SurfaceRef::new(SurfaceGeometryKind::PlanarV1, 1, 4, 12, [0; 32]),
        Err(SurfaceRefError::ZeroFingerprint)
    ));

    for (field, invalid) in [
        ("geometry_schema", json!(2)),
        ("cell_count", json!(0)),
        ("edge_count", json!(0)),
        ("fingerprint", json!(vec![0; 32])),
    ] {
        let mut value = serde_json::to_value(valid).unwrap();
        value[field] = invalid;
        assert!(
            serde_json::from_value::<SurfaceRef>(value).is_err(),
            "{field}"
        );
    }

    let mut unknown = serde_json::to_value(valid).unwrap();
    unknown["projection"] = json!("mercator");
    assert!(serde_json::from_value::<SurfaceRef>(unknown).is_err());
}
