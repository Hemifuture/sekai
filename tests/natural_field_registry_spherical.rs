use std::f64::consts::PI;

use sekai::world::fields::{FieldId, FieldRegistry};
use sekai::world::natural::{
    drainage_area_km2_field_id, mean_annual_discharge_m3_s_field_id, natural_field_registry,
    spherical_natural_field_registry, NaturalFieldRegistryError, ANNUAL_PRECIPITATION_MAX_MM,
    CLIMATOLOGICAL_YEAR_SECONDS,
};

fn maximum(registry: &FieldRegistry, id: FieldId) -> f32 {
    registry.get(&id).unwrap().valid_range.unwrap().max()
}

#[test]
fn legacy_planar_registry_bytes_are_frozen_before_spherical_parameterization() {
    let registry = natural_field_registry(12).unwrap();
    let bytes = serde_json::to_vec(&registry).unwrap();
    let actual = blake3::hash(&bytes).to_hex().to_string();

    assert_eq!(
        actual,
        "7daf32cc8d7d00033b9bc541c8642bbe6482d30cb85ab99aa0f0a4cf18f9e740"
    );
}

#[test]
fn spherical_registry_ranges_cover_the_entire_physical_surface() {
    let radius_m = 100_000_000.0_f64;
    let total_surface_area_m2 = 4.0 * PI * radius_m * radius_m;
    let registry = spherical_natural_field_registry(12, total_surface_area_m2).unwrap();

    let physical_drainage_area_km2 = (total_surface_area_m2 / 1_000_000.0) as f32;
    let physical_discharge_m3_s = (total_surface_area_m2
        * (f64::from(ANNUAL_PRECIPITATION_MAX_MM) / 1_000.0)
        / CLIMATOLOGICAL_YEAR_SECONDS) as f32;

    assert_eq!(
        maximum(&registry, drainage_area_km2_field_id()),
        physical_drainage_area_km2
    );
    assert_eq!(
        maximum(&registry, mean_annual_discharge_m3_s_field_id()),
        physical_discharge_m3_s
    );
}

#[test]
fn spherical_registry_rejects_non_physical_surface_areas() {
    for found in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            spherical_natural_field_registry(12, found),
            Err(NaturalFieldRegistryError::InvalidTotalSurfaceArea { .. })
        ));
    }
}

#[test]
fn spherical_registry_rejects_unrepresentable_field_ranges() {
    let total_surface_area_m2 = f64::from(f32::MAX) * 1_000_000.0 * 2.0;

    assert!(matches!(
        spherical_natural_field_registry(12, total_surface_area_m2),
        Err(NaturalFieldRegistryError::SphericalFieldRangeOverflow { .. })
    ));
}
