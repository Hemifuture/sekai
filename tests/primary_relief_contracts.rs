use sekai::engine::BuildCancellation;
use sekai::generators::natural::{build_surface_water_geometry, solve_physical_sea_level};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    scaled_earth_ocean_inventory_m3, ElevationField, LandFractionConstraintStatus,
    NaturalQualityProfile, PrimaryReliefSnapshot, ReliefSpec, SeaLevelPolicy,
    SphericalReliefSnapshot, SurfaceWaterGeometry, PRIMARY_RELIEF_SCHEMA_V1, RELIEF_SCHEMA_V4,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::Meters;

fn surface() -> SphericalSurfaceSnapshot {
    ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap()
    .authoritative_surface()
    .clone()
}

fn valid_snapshot(surface: &SphericalSurfaceSnapshot) -> PrimaryReliefSnapshot {
    let count = surface.cells().len();
    let isostatic = vec![0.0; count];
    let dynamic = vec![0.0; count];
    let volcanic = vec![0.0; count];
    let passive = vec![0.0; count];
    let detail = vec![0.0; count];
    let elevation = vec![0.0; count];
    let areas = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .collect::<Vec<_>>();
    let inventory = scaled_earth_ocean_inventory_m3(areas.iter().sum()).unwrap();
    let solution = solve_physical_sea_level(surface, &elevation, inventory).unwrap();
    let elevation_field = ElevationField::from_values(elevation.clone()).unwrap();
    let land_ocean = solution.geometry().land_ocean().clone();
    let compatibility = SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::for_spherical(surface),
        solution.sea_level_m(),
        ElevationField::from_values(isostatic.clone()).unwrap(),
        ElevationField::from_values(dynamic.clone()).unwrap(),
        ElevationField::from_values(volcanic.clone()).unwrap(),
        ElevationField::from_values(
            passive
                .iter()
                .zip(&detail)
                .map(|(&margin, &regional)| margin + regional)
                .collect(),
        )
        .unwrap(),
        elevation_field,
        land_ocean,
    )
    .unwrap();
    let physical = solution
        .geometry()
        .global_land_area_fraction(surface)
        .unwrap();
    let tolerance = (surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .fold(0.0_f64, f64::max)
        / areas.iter().sum::<f64>())
    .max(0.02) as f32;

    PrimaryReliefSnapshot::new(
        PRIMARY_RELIEF_SCHEMA_V1,
        SurfaceRef::for_spherical(surface),
        compatibility,
        isostatic,
        dynamic,
        volcanic,
        passive,
        detail,
        elevation,
        inventory,
        solution.realized_water_volume_m3(),
        ReliefSpec::default().target_land_fraction,
        physical,
        tolerance,
        LandFractionConstraintStatus::Infeasible,
    )
    .unwrap()
}

fn water_geometry(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &PrimaryReliefSnapshot,
) -> SurfaceWaterGeometry {
    build_surface_water_geometry(
        surface,
        snapshot.elevation_m(),
        snapshot.sea_level_m(),
        &BuildCancellation::new(),
    )
    .unwrap()
}

#[test]
fn strict_primary_relief_roundtrips_and_cross_validates_physical_water() {
    let surface = surface();
    let snapshot = valid_snapshot(&surface);
    let geometry = water_geometry(&surface, &snapshot);
    snapshot
        .validate_against_surface(&surface, &geometry, &ReliefSpec::default())
        .unwrap();
    snapshot
        .validate_against_authoring(&surface, &ReliefSpec::default())
        .unwrap();

    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: PrimaryReliefSnapshot = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, snapshot);
    assert_eq!(
        decoded.constraint_status(),
        LandFractionConstraintStatus::Infeasible
    );
    assert_eq!(decoded.physical_land_fraction(), 0.0);
    assert!(decoded.water_volume_relative_error() <= 1.0e-6);
}

#[test]
fn strict_wire_rejects_unknown_schema_and_component_drift() {
    let surface = surface();
    let encoded = serde_json::to_value(valid_snapshot(&surface)).unwrap();

    let mut unknown = encoded.clone();
    unknown["surprise"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PrimaryReliefSnapshot>(unknown).is_err());

    let mut schema = encoded.clone();
    schema["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<PrimaryReliefSnapshot>(schema).is_err());

    let mut component = encoded;
    component["conditioned_regional_detail_m"][0] = serde_json::json!(25.0);
    assert!(serde_json::from_value::<PrimaryReliefSnapshot>(component).is_err());
}

#[test]
fn compatibility_mapping_cannot_diverge_from_causal_components() {
    let surface = surface();
    let mut encoded = serde_json::to_value(valid_snapshot(&surface)).unwrap();
    encoded["compatibility"]["regional_offset_m"][0] = serde_json::json!(5.0);
    encoded["compatibility"]["elevation_m"][0] = serde_json::json!(5.0);
    assert!(serde_json::from_value::<PrimaryReliefSnapshot>(encoded).is_err());
}

#[test]
fn surface_cross_validation_recomputes_area_weighted_constraint_status() {
    let surface = surface();
    let mut encoded = serde_json::to_value(valid_snapshot(&surface)).unwrap();
    encoded["physical_land_fraction"] = serde_json::json!(0.25);
    let stale: PrimaryReliefSnapshot = serde_json::from_value(encoded).unwrap();
    let geometry = water_geometry(&surface, &stale);
    assert!(stale
        .validate_against_surface(&surface, &geometry, &ReliefSpec::default())
        .is_err());
}

#[test]
fn target_policy_cross_validation_requires_the_authored_land_fraction_to_be_satisfied() {
    let surface = surface();
    let snapshot = valid_snapshot(&surface);
    let target_spec = ReliefSpec {
        sea_level_policy: SeaLevelPolicy::TargetLandFraction,
        ..ReliefSpec::default()
    };
    let geometry = water_geometry(&surface, &snapshot);

    assert!(snapshot
        .validate_against_surface(&surface, &geometry, &target_spec)
        .is_err());
    assert!(snapshot
        .validate_against_authoring(&surface, &target_spec)
        .is_err());
}
