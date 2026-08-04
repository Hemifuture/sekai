use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    ElevationField, LandOceanField, MonthlyScalarField, MonthlyVector3Field,
    SphericalClimateValidationError, SphericalPreliminaryClimateSnapshot, SphericalReliefSnapshot,
    AIR_TEMPERATURE_MAX_C, CLIMATE_MONTH_COUNT, PRELIMINARY_CLIMATE_SCHEMA_V2, RELIEF_SCHEMA_V4,
    WIND_COMPONENT_MAX_M_S,
};
use sekai::world::spatial::{SurfaceGeometryKind, SurfaceRef, SPATIAL_SCHEMA_V1};
use sekai::world::{
    CellId, Meters, SphericalSpaceSpec, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT,
};

fn surface(radius_m: f64) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
        target_cell_count: 42,
    })
    .unwrap()
}

fn relief(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    elevation: impl Fn(usize) -> f32,
) -> SphericalReliefSnapshot {
    let values = (0..surface.cells().len())
        .map(elevation)
        .collect::<Vec<_>>();
    let zero = vec![0.0; values.len()];
    let final_elevation = ElevationField::from_values(values.clone()).unwrap();
    SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::for_spherical(surface),
        0.0,
        ElevationField::from_values(values).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero).unwrap(),
        final_elevation.clone(),
        LandOceanField::classify(&final_elevation, 0.0),
    )
    .unwrap()
}

fn tangent_east(radial: [f64; 3], speed: f32) -> [f32; 3] {
    let length = radial[0].hypot(radial[1]);
    if length <= f64::EPSILON {
        [0.0; 3]
    } else {
        [
            (-radial[1] / length) as f32 * speed,
            (radial[0] / length) as f32 * speed,
            0.0,
        ]
    }
}

fn valid_snapshot(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
) -> SphericalPreliminaryClimateSnapshot {
    let count = surface.cells().len();
    let latitude = surface
        .cells()
        .iter()
        .map(|cell| cell.centroid.components()[2].asin().to_degrees() as f32)
        .collect::<Vec<_>>();
    let temperature = (0..count)
        .map(|cell| std::array::from_fn(|month| 4.0 + cell as f32 * 0.01 + month as f32))
        .collect::<Vec<_>>();
    let precipitation = (0..count)
        .map(|cell| std::array::from_fn(|month| 20.0 + cell as f32 * 0.01 + month as f32))
        .collect::<Vec<_>>();
    let wind = surface
        .cells()
        .iter()
        .map(|cell| {
            std::array::from_fn(|month| {
                tangent_east(cell.centroid.components(), 4.0 + month as f32 * 0.1)
            })
        })
        .collect::<Vec<_>>();
    let mean_temperature = temperature
        .iter()
        .map(|months| months.iter().sum::<f32>() / CLIMATE_MONTH_COUNT as f32)
        .collect();
    let seasonality = temperature
        .iter()
        .map(|months| {
            months.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - months.iter().copied().fold(f32::INFINITY, f32::min)
        })
        .collect();
    let annual_precipitation = precipitation
        .iter()
        .map(|months| months.iter().sum())
        .collect();
    let prevailing_wind = wind
        .iter()
        .map(|months| {
            let sum = months.iter().fold([0.0_f32; 3], |sum, value| {
                [sum[0] + value[0], sum[1] + value[1], sum[2] + value[2]]
            });
            sum.map(|component| component / CLIMATE_MONTH_COUNT as f32)
        })
        .collect();

    SphericalPreliminaryClimateSnapshot::new(
        PRELIMINARY_CLIMATE_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        latitude,
        vec![0.0; count],
        MonthlyScalarField::from_values(temperature).unwrap(),
        MonthlyScalarField::from_values(precipitation).unwrap(),
        MonthlyVector3Field::from_values(wind).unwrap(),
        mean_temperature,
        seasonality,
        annual_precipitation,
        prevailing_wind,
    )
    .unwrap()
}

#[test]
fn spherical_climate_round_trips_with_exact_surface_identity_and_zero_copy_fields() {
    let sphere = surface(6_371_000.0);
    let land = relief(&sphere, |_| 100.0);
    let snapshot = valid_snapshot(&sphere);

    snapshot.validate().unwrap();
    snapshot.validate_against(&sphere, &land).unwrap();
    assert_eq!(snapshot.schema_version(), PRELIMINARY_CLIMATE_SCHEMA_V2);
    assert_eq!(snapshot.surface_ref(), SurfaceRef::for_spherical(&sphere));
    assert_eq!(snapshot.cell_count(), sphere.cells().len() as u32);
    assert_eq!(snapshot.latitude_degrees().len(), sphere.cells().len());
    assert_eq!(
        snapshot.maritime_influence(),
        vec![0.0; sphere.cells().len()]
    );
    assert_eq!(
        snapshot.monthly_air_temperature_c().len(),
        sphere.cells().len()
    );
    assert_eq!(
        snapshot.monthly_precipitation_mm().len(),
        sphere.cells().len()
    );
    assert_eq!(snapshot.monthly_wind_m_s().len(), sphere.cells().len());
    assert_eq!(
        snapshot.mean_annual_air_temperature_c().len(),
        sphere.cells().len()
    );
    assert_eq!(
        snapshot.temperature_seasonality_c(),
        vec![11.0; sphere.cells().len()]
    );
    assert_eq!(
        snapshot.annual_precipitation_mm().len(),
        sphere.cells().len()
    );
    assert_eq!(snapshot.prevailing_wind_m_s().len(), sphere.cells().len());
    assert!(snapshot.wind_m_s(CellId::from_raw(0), 0).is_some());
    assert_eq!(snapshot.wind_m_s(CellId::from_raw(0), 12), None);

    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded["schema_version"], PRELIMINARY_CLIMATE_SCHEMA_V2);
    assert_eq!(encoded["surface_ref"]["geometry_kind"], "spherical_v1");
    assert!(encoded.get("cell_count").is_none());
    let decoded: SphericalPreliminaryClimateSnapshot =
        serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);
    assert_eq!(
        serde_json::to_vec(&decoded).unwrap(),
        serde_json::to_vec(&snapshot).unwrap()
    );

    let mut unknown = encoded;
    unknown["longitude_degrees"] = serde_json::json!([0.0]);
    assert!(serde_json::from_value::<SphericalPreliminaryClimateSnapshot>(unknown).is_err());
}

#[test]
fn spherical_climate_rejects_schema_kind_lengths_ranges_and_summary_drift() {
    let sphere = surface(6_371_000.0);
    let valid = valid_snapshot(&sphere);
    let encoded = serde_json::to_value(&valid).unwrap();

    let mut wrong_schema = encoded.clone();
    wrong_schema["schema_version"] = serde_json::json!(1);
    assert!(serde_json::from_value::<SphericalPreliminaryClimateSnapshot>(wrong_schema).is_err());

    let planar_ref = SurfaceRef::new(
        SurfaceGeometryKind::PlanarV1,
        SPATIAL_SCHEMA_V1,
        sphere.cells().len() as u32,
        sphere.edges().len() as u32,
        [3; 32],
    )
    .unwrap();
    let mut wrong_kind = encoded.clone();
    wrong_kind["surface_ref"] = serde_json::to_value(planar_ref).unwrap();
    assert!(serde_json::from_value::<SphericalPreliminaryClimateSnapshot>(wrong_kind).is_err());

    let mut short = encoded.clone();
    short["monthly_precipitation_mm"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(serde_json::from_value::<SphericalPreliminaryClimateSnapshot>(short).is_err());

    let mut bad_temperature = encoded.clone();
    bad_temperature["monthly_air_temperature_c"][0][0] =
        serde_json::json!(AIR_TEMPERATURE_MAX_C + 1.0);
    assert!(
        serde_json::from_value::<SphericalPreliminaryClimateSnapshot>(bad_temperature).is_err()
    );

    let mut bad_wind = encoded.clone();
    bad_wind["monthly_wind_m_s"][0][0] =
        serde_json::json!([WIND_COMPONENT_MAX_M_S + 1.0, 0.0, 0.0]);
    assert!(serde_json::from_value::<SphericalPreliminaryClimateSnapshot>(bad_wind).is_err());

    let mut excessive_speed = encoded.clone();
    excessive_speed["monthly_wind_m_s"][0][0] = serde_json::json!([60.0, 60.0, 0.0]);
    let months = excessive_speed["monthly_wind_m_s"][0].as_array().unwrap();
    let sum = months.iter().fold([0.0_f32; 3], |mut sum, value| {
        for component in 0..3 {
            sum[component] += value[component].as_f64().unwrap() as f32;
        }
        sum
    });
    excessive_speed["prevailing_wind_m_s"][0] =
        serde_json::json!(sum.map(|component| component / CLIMATE_MONTH_COUNT as f32));
    assert!(
        serde_json::from_value::<SphericalPreliminaryClimateSnapshot>(excessive_speed).is_err()
    );

    let mut bad_summary = encoded;
    bad_summary["annual_precipitation_mm"][0] = serde_json::json!(999.0);
    assert!(serde_json::from_value::<SphericalPreliminaryClimateSnapshot>(bad_summary).is_err());
}

#[test]
fn spherical_climate_rejects_surface_allocations_beyond_schema_budgets() {
    let sphere = surface(6_371_000.0);
    let encoded = serde_json::to_value(valid_snapshot(&sphere)).unwrap();

    for (field, found) in [
        ("cell_count", MAX_SPHERICAL_CELL_COUNT + 1),
        ("edge_count", MAX_SPHERICAL_EDGE_COUNT + 1),
    ] {
        let mut oversized = encoded.clone();
        oversized["surface_ref"][field] = serde_json::json!(found);
        let error = serde_json::from_value::<SphericalPreliminaryClimateSnapshot>(oversized)
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeds spherical limit"), "{error}");
    }
}

#[test]
fn cross_validation_enforces_latitude_tangency_maritime_and_exact_relief_surface() {
    let first = surface(6_371_000.0);
    let second = surface(6_000_000.0);
    let land = relief(&first, |_| 100.0);
    let second_land = relief(&second, |_| 100.0);
    let climate = valid_snapshot(&first);

    assert!(matches!(
        climate.validate_against(&second, &second_land),
        Err(SphericalClimateValidationError::SurfaceMismatch { .. })
    ));

    let mut wrong_latitude = serde_json::to_value(&climate).unwrap();
    wrong_latitude["latitude_degrees"][0] = serde_json::json!(climate.latitude_degrees()[0] + 1.0);
    let wrong_latitude: SphericalPreliminaryClimateSnapshot =
        serde_json::from_value(wrong_latitude).unwrap();
    assert!(matches!(
        wrong_latitude.validate_against(&first, &land),
        Err(SphericalClimateValidationError::LatitudeMismatch { .. })
    ));

    let mut radial_wind = serde_json::to_value(&climate).unwrap();
    let radial = first.cells()[0]
        .centroid
        .components()
        .map(|component| component as f32);
    radial_wind["monthly_wind_m_s"][0][0] = serde_json::json!(radial);
    let monthly = radial_wind["monthly_wind_m_s"][0].as_array().unwrap();
    let sum = monthly.iter().fold([0.0_f32; 3], |mut sum, value| {
        for component in 0..3 {
            sum[component] += value[component].as_f64().unwrap() as f32;
        }
        sum
    });
    radial_wind["prevailing_wind_m_s"][0] =
        serde_json::json!(sum.map(|component| component / CLIMATE_MONTH_COUNT as f32));
    let radial_wind: SphericalPreliminaryClimateSnapshot =
        serde_json::from_value(radial_wind).unwrap();
    assert!(matches!(
        radial_wind.validate_against(&first, &land),
        Err(SphericalClimateValidationError::WindNotTangent { .. })
    ));

    let ocean = relief(&first, |index| if index == 0 { -100.0 } else { 100.0 });
    assert!(matches!(
        climate.validate_against(&first, &ocean),
        Err(SphericalClimateValidationError::OceanMaritimeMismatch { .. })
    ));

    let mut maritime_land = serde_json::to_value(&climate).unwrap();
    maritime_land["maritime_influence"][0] = serde_json::json!(0.5);
    let maritime_land: SphericalPreliminaryClimateSnapshot =
        serde_json::from_value(maritime_land).unwrap();
    assert!(matches!(
        maritime_land.validate_against(&first, &land),
        Err(SphericalClimateValidationError::AllLandMaritimeMismatch { .. })
    ));
}
