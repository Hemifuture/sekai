use std::f64::consts::PI;

use sekai::world::fields::{FieldId, FieldRegistry, FieldValueType};
use sekai::world::natural::{
    circulation_annual_evaporation_mm_field_id, circulation_annual_precipitation_mm_field_id,
    coastal_deposition_rate_m_per_year_field_id, coastal_erosion_rate_m_per_year_field_id,
    drainage_area_km2_field_id, equilibrium_adjustment_m_field_id,
    fluvial_erosion_rate_m_per_year_field_id, hillslope_deposition_rate_m_per_year_field_id,
    hillslope_erosion_rate_m_per_year_field_id, isostatic_response_rate_m_per_year_field_id,
    mean_annual_discharge_m3_s_field_id, natural_field_registry, primary_relief_m_field_id,
    routed_sediment_deposition_rate_m_per_year_field_id, spherical_formation_field_registry,
    spherical_natural_field_registry, tectonic_displacement_rate_m_per_year_field_id,
    NaturalFieldRegistryError, ANNUAL_PRECIPITATION_MAX_MM, CLIMATOLOGICAL_YEAR_SECONDS,
};

fn maximum(registry: &FieldRegistry, id: FieldId) -> f32 {
    registry.get(&id).unwrap().valid_range.unwrap().max()
}

#[test]
fn formation_registry_bytes_are_frozen_with_p4_budget_fields() {
    let radius_m = 6_371_000.0_f64;
    let registry = spherical_formation_field_registry(12, 4.0 * PI * radius_m * radius_m).unwrap();
    let actual = blake3::hash(&serde_json::to_vec(&registry).unwrap())
        .to_hex()
        .to_string();

    let r4_current_state_fields = [
        (primary_relief_m_field_id(), "primary_relief_m", "m", false),
        (
            equilibrium_adjustment_m_field_id(),
            "equilibrium_adjustment_m",
            "m",
            false,
        ),
        (
            tectonic_displacement_rate_m_per_year_field_id(),
            "tectonic_displacement_rate_m_per_year",
            "m/year",
            true,
        ),
        (
            fluvial_erosion_rate_m_per_year_field_id(),
            "fluvial_erosion_rate_m_per_year",
            "m/year",
            true,
        ),
        (
            hillslope_erosion_rate_m_per_year_field_id(),
            "hillslope_erosion_rate_m_per_year",
            "m/year",
            true,
        ),
        (
            hillslope_deposition_rate_m_per_year_field_id(),
            "hillslope_deposition_rate_m_per_year",
            "m/year",
            true,
        ),
        (
            routed_sediment_deposition_rate_m_per_year_field_id(),
            "routed_sediment_deposition_rate_m_per_year",
            "m/year",
            true,
        ),
        (
            coastal_erosion_rate_m_per_year_field_id(),
            "coastal_erosion_rate_m_per_year",
            "m/year",
            true,
        ),
        (
            coastal_deposition_rate_m_per_year_field_id(),
            "coastal_deposition_rate_m_per_year",
            "m/year",
            true,
        ),
        (
            isostatic_response_rate_m_per_year_field_id(),
            "isostatic_response_rate_m_per_year",
            "m/year",
            true,
        ),
    ];
    assert_eq!(registry.len(), 29);
    for (field, expected_name, expected_unit, is_unbounded) in r4_current_state_fields {
        assert_eq!(field.name(), expected_name);
        let schema = registry
            .get(&field)
            .unwrap_or_else(|| panic!("R4 field {expected_name} is missing"));
        assert_eq!(schema.value_type, FieldValueType::ScalarF32);
        assert_eq!(schema.unit.symbol(), expected_unit);
        assert_eq!(
            schema.display.label_key(),
            format!("field.sekai.core.natural.{expected_name}"),
            "the registry key must remain the localization lookup key"
        );
        assert_eq!(schema.valid_range.is_none(), is_unbounded);
    }

    for field in [
        circulation_annual_evaporation_mm_field_id(),
        circulation_annual_precipitation_mm_field_id(),
    ] {
        assert!(
            registry.get(&field).unwrap().valid_range.is_none(),
            "raw P4 water totals must use their measured data range rather than an empirical envelope"
        );
    }

    assert_eq!(
        actual,
        "be7c169b774d82d8600e936c67215aeb3bd600217fce9eaabf41ee2234e341db"
    );
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
