use std::f64::consts::PI;

use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::spatial::{
    NaturalSurface, PlanarNaturalSurface, SpatialCell, SpatialEdge, SpatialSnapshot,
    SphericalNaturalSurface, SurfaceGeometryKind, SPATIAL_SCHEMA_V1,
};
use sekai::world::{
    BoundaryCondition, CellId, EdgeId, Meters, SphericalSpaceSpec, SquareMeters, WorldPoint,
    WorldRect,
};

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

fn planar_fixture(scale_x: f64, scale_y: f64) -> SpatialSnapshot {
    let area = scale_x * scale_y;
    let p = |x: f64, y: f64| (x * scale_x, y * scale_y);
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
        WorldRect::new(point(0.0, 0.0), point(2.0 * scale_x, 2.0 * scale_y)).unwrap(),
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
fn planar_adapter_preserves_current_euclidean_metrics_and_shape_coordinates() {
    let snapshot = planar_fixture(2.0, 1.0);
    let surface = PlanarNaturalSurface::new(&snapshot).unwrap();

    assert_eq!(
        surface.surface_ref().geometry_kind(),
        SurfaceGeometryKind::PlanarV1
    );
    assert!(!surface.is_closed());
    assert_eq!(surface.cell_count(), 4);
    assert_eq!(surface.edge_count(), 12);
    assert_eq!(surface.total_area().get(), 8.0);
    assert_eq!(surface.short_length_scale().get(), 2.0);
    assert_eq!(surface.long_length_scale().get(), 4.0);

    let cell = surface.cell(CellId::from_raw(0)).unwrap();
    assert_eq!(cell.id(), CellId::from_raw(0));
    assert_eq!(cell.area().get(), 2.0);
    assert_eq!(cell.shape_position(), [0.25, 0.125, 0.0]);
    assert!(surface.cell(CellId::from_raw(4)).is_none());

    let internal = surface.edge(EdgeId::from_raw(8)).unwrap();
    assert_eq!(
        internal.owners(),
        [Some(CellId::from_raw(0)), Some(CellId::from_raw(1))]
    );
    assert_eq!(internal.boundary_length().get(), 1.0);
    assert_eq!(internal.traversal_length().get(), 1.0);
    assert_eq!(internal.center_distance().unwrap().get(), 2.0);

    let boundary = surface.edge(EdgeId::from_raw(0)).unwrap();
    assert_eq!(boundary.owners(), [Some(CellId::from_raw(0)), None]);
    assert!(boundary.center_distance().is_none());
    assert!(surface.edge(EdgeId::from_raw(12)).is_none());
}

#[test]
fn planar_third_shape_coordinate_is_constant_and_cannot_change_distance_rankings() {
    let snapshot = planar_fixture(2.0, 1.0);
    let surface = PlanarNaturalSurface::new(&snapshot).unwrap();
    let positions = (0..surface.cell_count())
        .map(|index| {
            surface
                .cell(CellId::from_raw(index as u32))
                .unwrap()
                .shape_position()
        })
        .collect::<Vec<_>>();

    assert!(positions.iter().all(|position| position[2] == 0.0));
    for first in 0..positions.len() {
        for second in 0..positions.len() {
            let a = positions[first];
            let b = positions[second];
            let squared_2d = (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
            let squared_3d = squared_2d + (a[2] - b[2]).powi(2);
            assert_eq!(squared_3d, squared_2d);
        }
    }
}

#[test]
fn spherical_adapter_exposes_closed_authoritative_metrics_without_a_projection() {
    let radius = 6_371_000.0;
    let snapshot = spherical_fixture(radius);
    let surface = SphericalNaturalSurface::new(&snapshot).unwrap();

    assert_eq!(
        surface.surface_ref().geometry_kind(),
        SurfaceGeometryKind::SphericalV1
    );
    assert_eq!(surface.surface_ref().fingerprint(), snapshot.fingerprint());
    assert!(surface.is_closed());
    assert_eq!(surface.cell_count(), snapshot.cells().len());
    assert_eq!(surface.edge_count(), snapshot.edges().len());
    assert_eq!(surface.total_area(), snapshot.total_cell_area());
    assert_eq!(surface.short_length_scale().get(), PI * radius);
    assert_eq!(surface.long_length_scale().get(), PI * radius);

    for index in 0..surface.cell_count() {
        let metric = surface.cell(CellId::from_raw(index as u32)).unwrap();
        assert_eq!(metric.id().raw() as usize, index);
        assert!(metric.area().get() > 0.0);
        assert!(metric
            .shape_position()
            .into_iter()
            .all(|component| component.is_finite() && (0.0..=1.0).contains(&component)));
        let centroid = snapshot.cells()[index].centroid.components();
        let expected = centroid.map(|component| (component + 1.0) * 0.5);
        assert_eq!(metric.shape_position(), expected);
    }

    for index in 0..surface.edge_count() {
        let metric = surface.edge(EdgeId::from_raw(index as u32)).unwrap();
        assert_eq!(metric.id().raw() as usize, index);
        assert!(metric.owners().into_iter().all(|owner| owner.is_some()));
        assert!(metric.boundary_length().get() > 0.0);
        assert!(metric.traversal_length().get() > 0.0);
        assert_eq!(metric.traversal_length(), metric.center_distance().unwrap());
        assert_eq!(
            metric.center_distance().unwrap(),
            snapshot.edges()[index].center_distance
        );
    }

    let expected_area = 4.0 * PI * radius * radius;
    assert!((surface.total_area().get() - expected_area).abs() / expected_area <= 1.0e-10);
}

#[test]
fn spherical_local_frames_match_authoritative_records_without_allocating_geometry() {
    let snapshot = spherical_fixture(6_371_000.0);
    let surface = SphericalNaturalSurface::new(&snapshot).unwrap();

    for (index, authoritative) in snapshot.cells().iter().enumerate() {
        let frame = surface.cell_frame(CellId::from_raw(index as u32)).unwrap();
        assert_eq!(frame.id(), authoritative.id);
        assert_eq!(frame.radial(), authoritative.centroid);
    }
    assert!(surface
        .cell_frame(CellId::from_raw(snapshot.cells().len() as u32))
        .is_none());

    for (index, authoritative) in snapshot.edges().iter().enumerate() {
        let frame = surface.edge_frame(EdgeId::from_raw(index as u32)).unwrap();
        assert_eq!(frame.id(), authoritative.id);
        assert_eq!(frame.vertices(), authoritative.vertices);
        assert_eq!(frame.owners(), authoritative.cells);
        assert_eq!(frame.midpoint(), authoritative.midpoint);
        assert_eq!(frame.normal_from_first(), authoritative.normal_from_first);
        assert!(frame.midpoint().dot(frame.normal_from_first()).abs() <= 1.0e-12);
        assert!((frame.midpoint().norm() - 1.0).abs() <= 1.0e-12);
        assert!((frame.normal_from_first().norm() - 1.0).abs() <= 1.0e-12);
    }
    assert!(surface
        .edge_frame(EdgeId::from_raw(snapshot.edges().len() as u32))
        .is_none());
}

#[test]
fn public_adapter_construction_revalidates_untrusted_snapshots() {
    let planar = planar_fixture(1.0, 1.0);
    let mut planar_json = serde_json::to_value(&planar).unwrap();
    planar_json["cells"][0]["area"] = serde_json::json!(9.0);
    let invalid_planar: SpatialSnapshot = serde_json::from_value(planar_json).unwrap();
    assert!(PlanarNaturalSurface::new(&invalid_planar).is_err());

    let spherical = spherical_fixture(2.0);
    let mut spherical_json = serde_json::to_value(&spherical).unwrap();
    spherical_json["fingerprint"] = serde_json::json!(vec![0; 32]);
    let invalid_spherical = serde_json::from_value(spherical_json).unwrap();
    assert!(SphericalNaturalSurface::new(&invalid_spherical).is_err());
}
