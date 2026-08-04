use std::f64::consts::PI;

use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    Hotspot, SphericalMantleSnapshot, SphericalMantleValidationError, HEAT_FLOW_MAX_MW_M2,
    HEAT_FLOW_MIN_MW_M2, MANTLE_SNAPSHOT_SCHEMA_V2,
};
use sekai::world::spatial::{SurfaceGeometryKind, SurfaceRef, SPATIAL_SCHEMA_V1};
use sekai::world::{CellId, HotspotId, Meters, SphericalSpaceSpec};

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn spherical_surface(radius: f64) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: meters(radius),
        target_cell_count: 42,
    })
    .unwrap()
}

fn hotspot(id: u32, source: u32, support_radius_m: f64) -> Hotspot {
    Hotspot::new(
        HotspotId::from_raw(id),
        CellId::from_raw(source),
        800,
        meters(support_radius_m),
    )
    .unwrap()
}

fn valid_snapshot(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
) -> SphericalMantleSnapshot {
    let count = surface.cells().len();
    let mut heat = vec![65.0; count];
    let mut influence = vec![0.0; count];
    heat[0] = 220.0;
    influence[0] = 1.0;
    SphericalMantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        vec![hotspot(0, 0, 250_000.0)],
        heat,
        influence,
    )
    .unwrap()
}

#[test]
fn spherical_mantle_round_trips_with_one_exact_surface_identity() {
    let surface = spherical_surface(6_371_000.0);
    let snapshot = valid_snapshot(&surface);

    snapshot.validate().unwrap();
    snapshot.validate_against(&surface).unwrap();
    assert_eq!(snapshot.schema_version(), MANTLE_SNAPSHOT_SCHEMA_V2);
    assert_eq!(snapshot.surface_ref(), SurfaceRef::for_spherical(&surface));
    assert_eq!(snapshot.hotspots().len(), 1);
    assert_eq!(snapshot.heat_flow_mw_m2().len(), surface.cells().len());
    assert_eq!(snapshot.volcanic_influence().len(), surface.cells().len());

    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded["schema_version"], MANTLE_SNAPSHOT_SCHEMA_V2);
    assert_eq!(encoded["surface_ref"]["geometry_kind"], "spherical_v1");
    assert!(encoded.get("cell_count").is_none());
    let decoded: SphericalMantleSnapshot = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);

    let mut unknown_snapshot = encoded.clone();
    unknown_snapshot["projection"] = serde_json::json!("equirectangular");
    assert!(serde_json::from_value::<SphericalMantleSnapshot>(unknown_snapshot).is_err());
    let mut unknown_hotspot = encoded;
    unknown_hotspot["hotspots"][0]["longitude"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<SphericalMantleSnapshot>(unknown_hotspot).is_err());
}

#[test]
fn spherical_mantle_rejects_wrong_kind_duplicate_sources_and_dense_field_errors() {
    let surface = spherical_surface(6_371_000.0);
    let count = surface.cells().len();
    let planar_ref = SurfaceRef::new(
        SurfaceGeometryKind::PlanarV1,
        SPATIAL_SCHEMA_V1,
        count as u32,
        surface.edges().len() as u32,
        [9; 32],
    )
    .unwrap();
    let build = |surface_ref, hotspots: Vec<Hotspot>, heat: Vec<f32>, influence: Vec<f32>| {
        SphericalMantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V2,
            surface_ref,
            hotspots,
            heat,
            influence,
        )
    };

    assert!(matches!(
        build(
            planar_ref,
            vec![hotspot(0, 0, 1.0)],
            vec![65.0; count],
            vec![0.0; count]
        ),
        Err(SphericalMantleValidationError::InvalidSurfaceKind { .. })
    ));
    assert!(matches!(
        build(
            SurfaceRef::for_spherical(&surface),
            vec![hotspot(0, 0, 1.0), hotspot(1, 0, 1.0)],
            vec![65.0; count],
            vec![0.0; count]
        ),
        Err(SphericalMantleValidationError::DuplicateHotspotSourceCell { .. })
    ));
    assert!(matches!(
        build(
            SurfaceRef::for_spherical(&surface),
            vec![],
            vec![65.0; count - 1],
            vec![0.0; count]
        ),
        Err(SphericalMantleValidationError::FieldLengthMismatch { .. })
    ));

    for heat_value in [
        f32::NAN,
        HEAT_FLOW_MIN_MW_M2 - 1.0,
        HEAT_FLOW_MAX_MW_M2 + 1.0,
    ] {
        let mut heat = vec![65.0; count];
        heat[3] = heat_value;
        assert!(matches!(
            build(
                SurfaceRef::for_spherical(&surface),
                vec![],
                heat,
                vec![0.0; count]
            ),
            Err(SphericalMantleValidationError::HeatFlowOutOfRange { .. })
        ));
    }
    for influence_value in [f32::NAN, -0.01, 1.01] {
        let mut influence = vec![0.0; count];
        influence[3] = influence_value;
        assert!(matches!(
            build(
                SurfaceRef::for_spherical(&surface),
                vec![],
                vec![65.0; count],
                influence
            ),
            Err(SphericalMantleValidationError::VolcanicInfluenceOutOfRange { .. })
        ));
    }
}

#[test]
fn exact_surface_mismatch_and_support_beyond_half_circumference_are_rejected() {
    let surface = spherical_surface(6_371_000.0);
    let different_radius = spherical_surface(6_000_000.0);
    assert_eq!(surface.cells().len(), different_radius.cells().len());
    assert_eq!(surface.edges().len(), different_radius.edges().len());
    let snapshot = valid_snapshot(&surface);
    assert!(matches!(
        snapshot.validate_against(&different_radius),
        Err(SphericalMantleValidationError::SurfaceMismatch { .. })
    ));

    let over_half_circumference = SphericalMantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::for_spherical(&surface),
        vec![hotspot(0, 0, PI * surface.radius().get() + 1.0)],
        vec![65.0; surface.cells().len()],
        vec![0.0; surface.cells().len()],
    )
    .unwrap();
    assert!(matches!(
        over_half_circumference.validate_against(&surface),
        Err(SphericalMantleValidationError::SupportRadiusExceedsHemisphere { .. })
    ));
}
