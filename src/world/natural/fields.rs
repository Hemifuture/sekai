use std::collections::BTreeMap;

use thiserror::Error;

use super::{
    TectonicSnapshot, AIR_TEMPERATURE_MAX_C, AIR_TEMPERATURE_MIN_C, ANNUAL_PRECIPITATION_MAX_MM,
    CLIMATOLOGICAL_YEAR_SECONDS, CONTINENTAL_CRUST_MAX_THICKNESS_KM, CRUST_BASE_ELEVATION_MAX_M,
    CRUST_BASE_ELEVATION_MIN_M, ELEVATION_MAX_M, ELEVATION_MIN_M, HEAT_FLOW_MAX_MW_M2,
    HEAT_FLOW_MIN_MW_M2, MAX_DEPOSITION_THICKNESS_M, MAX_EROSION_DEPTH_M, MAX_LAKE_DEPTH_M,
    MAX_PLATE_COUNT, MAX_STRAHLER_ORDER, MIN_PLATE_COUNT, OCEANIC_CRUST_MIN_THICKNESS_KM,
    REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M, TECTONIC_OFFSET_MAX_M, TECTONIC_OFFSET_MIN_M,
    TEMPERATURE_SEASONALITY_MAX_C, VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
};
use crate::world::fields::{
    FieldDisplayMetadata, FieldDomain, FieldId, FieldPaletteHint, FieldRegistry,
    FieldRegistryBuilder, FieldSchema, FieldSchemaError, FieldUnit, FieldValueType,
    MissingValuePolicy, ValueRange,
};
use crate::world::MAX_DIMENSION_METERS;

const NAMESPACE: &str = "sekai.core.natural";
const UNIT_NAMESPACE: &str = "sekai.core.units";
const SCHEMA_VERSION: u16 = 1;
const MAX_DRAINAGE_AREA_KM2: f32 =
    (MAX_DIMENSION_METERS * MAX_DIMENSION_METERS / 1_000_000.0) as f32;
const MAX_MEAN_ANNUAL_DISCHARGE_M3_S: f32 =
    (MAX_DIMENSION_METERS * MAX_DIMENSION_METERS * (ANNUAL_PRECIPITATION_MAX_MM as f64 / 1_000.0)
        / CLIMATOLOGICAL_YEAR_SECONDS) as f32;

#[derive(Debug, Clone, Copy)]
struct NaturalFieldRegistryLimits {
    max_drainage_area_km2: f32,
    max_mean_annual_discharge_m3_s: f32,
}

/// Returns the stable plate-identifier field ID.
pub fn plate_id_field_id() -> FieldId {
    field_id("plate_id")
}

/// Returns the stable crust-category field ID.
pub fn crust_kind_field_id() -> FieldId {
    field_id("crust_kind")
}

/// Returns the stable crust-thickness field ID.
pub fn crust_thickness_field_id() -> FieldId {
    field_id("crust_thickness_km")
}

/// Returns the stable per-cell plate-velocity field ID.
pub fn plate_velocity_field_id() -> FieldId {
    field_id("plate_velocity")
}

/// Returns the stable edge boundary-category field ID.
pub fn boundary_kind_field_id() -> FieldId {
    field_id("boundary_kind")
}

/// Returns the stable edge boundary-strength field ID.
pub fn boundary_strength_field_id() -> FieldId {
    field_id("boundary_strength")
}

/// Returns the stable crust-base elevation field ID.
pub fn crust_base_elevation_field_id() -> FieldId {
    field_id("crust_base_elevation_m")
}

/// Returns the stable tectonic elevation contribution field ID.
pub fn tectonic_offset_field_id() -> FieldId {
    field_id("tectonic_offset_m")
}

/// Returns the stable regional elevation contribution field ID.
pub fn regional_offset_field_id() -> FieldId {
    field_id("regional_offset_m")
}

/// Returns the stable final elevation field ID.
pub fn elevation_field_id() -> FieldId {
    field_id("elevation_m")
}

/// Returns the stable land/ocean category field ID.
pub fn land_ocean_field_id() -> FieldId {
    field_id("land_ocean")
}

/// Returns the stable mantle heat-flow field ID.
pub fn mantle_heat_flow_field_id() -> FieldId {
    field_id("mantle_heat_flow_mw_m2")
}

/// Returns the stable mantle volcanic-influence field ID.
pub fn volcanic_influence_field_id() -> FieldId {
    field_id("volcanic_influence")
}

/// Returns the stable volcanic elevation contribution field ID.
pub fn volcanic_offset_field_id() -> FieldId {
    field_id("volcanic_offset_m")
}

/// Returns the stable surface-bedrock category field ID.
pub fn bedrock_kind_field_id() -> FieldId {
    field_id("bedrock_kind")
}

/// Returns the stable fracture-intensity field ID.
pub fn fracture_intensity_field_id() -> FieldId {
    field_id("fracture_intensity")
}

/// Returns the stable erosion-resistance field ID.
pub fn erosion_resistance_field_id() -> FieldId {
    field_id("erosion_resistance")
}

/// Returns the stable relative-permeability field ID.
pub fn relative_permeability_field_id() -> FieldId {
    field_id("relative_permeability")
}

/// Returns the stable metallic-mineral formation-potential field ID.
pub fn metallic_mineral_potential_field_id() -> FieldId {
    field_id("metallic_mineral_potential")
}

/// Returns the stable geothermal formation-potential field ID.
pub fn geothermal_potential_field_id() -> FieldId {
    field_id("geothermal_potential")
}

/// Returns the stable sedimentary-basin formation-potential field ID.
pub fn sedimentary_basin_potential_field_id() -> FieldId {
    field_id("sedimentary_basin_potential")
}

/// Returns the stable geographic-latitude field ID.
pub fn latitude_degrees_field_id() -> FieldId {
    field_id("latitude_degrees")
}

/// Returns the stable normalized maritime-influence field ID.
pub fn maritime_influence_field_id() -> FieldId {
    field_id("maritime_influence")
}

/// Returns the stable annual-mean preliminary prevailing-wind field ID.
pub fn preliminary_prevailing_wind_m_s_field_id() -> FieldId {
    field_id("preliminary_prevailing_wind_m_s")
}

/// Returns the stable preliminary annual-mean air-temperature field ID.
pub fn preliminary_mean_air_temperature_c_field_id() -> FieldId {
    field_id("preliminary_mean_air_temperature_c")
}

/// Returns the stable preliminary peak-to-trough temperature-seasonality field ID.
pub fn preliminary_temperature_seasonality_c_field_id() -> FieldId {
    field_id("preliminary_temperature_seasonality_c")
}

/// Returns the stable preliminary annual-precipitation field ID.
pub fn preliminary_annual_precipitation_mm_field_id() -> FieldId {
    field_id("preliminary_annual_precipitation_mm")
}

/// Returns the stable current post-process surface-elevation field ID.
pub fn surface_elevation_m_field_id() -> FieldId {
    field_id("surface_elevation_m")
}

/// Returns the stable fluvial-erosion depth field ID.
pub fn fluvial_erosion_depth_m_field_id() -> FieldId {
    field_id("fluvial_erosion_depth_m")
}

/// Returns the stable sediment-deposition thickness field ID.
pub fn sediment_deposition_thickness_m_field_id() -> FieldId {
    field_id("sediment_deposition_thickness_m")
}

/// Returns the stable current surface-water category field ID.
pub fn surface_water_kind_field_id() -> FieldId {
    field_id("surface_water_kind")
}

/// Returns the stable inland-lake depth field ID.
pub fn lake_depth_m_field_id() -> FieldId {
    field_id("lake_depth_m")
}

/// Returns the stable annual local-runoff field ID.
pub fn annual_local_runoff_mm_field_id() -> FieldId {
    field_id("annual_local_runoff_mm")
}

/// Returns the stable mean annual accumulated-discharge field ID.
pub fn mean_annual_discharge_m3_s_field_id() -> FieldId {
    field_id("mean_annual_discharge_m3_s")
}

/// Returns the stable accumulated drainage-area field ID.
pub fn drainage_area_km2_field_id() -> FieldId {
    field_id("drainage_area_km2")
}

/// Returns the stable Strahler stream-order field ID.
pub fn strahler_stream_order_field_id() -> FieldId {
    field_id("strahler_stream_order")
}

/// Builds the complete V1 natural-field registry for a validated plate cardinality.
pub fn natural_field_registry(
    plate_count: u16,
) -> Result<FieldRegistry, NaturalFieldRegistryError> {
    validate_plate_count(plate_count)?;
    build_natural_field_registry(
        plate_count,
        NaturalFieldRegistryLimits {
            max_drainage_area_km2: MAX_DRAINAGE_AREA_KM2,
            max_mean_annual_discharge_m3_s: MAX_MEAN_ANNUAL_DISCHARGE_M3_S,
        },
    )
}

/// Builds the complete V1 natural-field registry with sphere-area-safe limits.
pub fn spherical_natural_field_registry(
    plate_count: u16,
    total_surface_area_m2: f64,
) -> Result<FieldRegistry, NaturalFieldRegistryError> {
    validate_plate_count(plate_count)?;
    if !total_surface_area_m2.is_finite() || total_surface_area_m2 <= 0.0 {
        return Err(NaturalFieldRegistryError::InvalidTotalSurfaceArea {
            found: total_surface_area_m2,
        });
    }

    let max_drainage_area_km2 = total_surface_area_m2 / 1_000_000.0;
    let max_mean_annual_discharge_m3_s = total_surface_area_m2
        * (f64::from(ANNUAL_PRECIPITATION_MAX_MM) / 1_000.0)
        / CLIMATOLOGICAL_YEAR_SECONDS;
    if !max_drainage_area_km2.is_finite()
        || max_drainage_area_km2 > f64::from(f32::MAX)
        || !max_mean_annual_discharge_m3_s.is_finite()
        || max_mean_annual_discharge_m3_s > f64::from(f32::MAX)
    {
        return Err(NaturalFieldRegistryError::SphericalFieldRangeOverflow {
            total_surface_area_m2,
        });
    }

    build_natural_field_registry(
        plate_count,
        NaturalFieldRegistryLimits {
            max_drainage_area_km2: max_drainage_area_km2 as f32,
            max_mean_annual_discharge_m3_s: max_mean_annual_discharge_m3_s as f32,
        },
    )
}

fn build_natural_field_registry(
    plate_count: u16,
    limits: NaturalFieldRegistryLimits,
) -> Result<FieldRegistry, NaturalFieldRegistryError> {
    let mut builder = FieldRegistryBuilder::new();
    for schema in schemas(plate_count, limits)? {
        builder.register(schema)?;
    }
    Ok(builder.build()?)
}

fn validate_plate_count(plate_count: u16) -> Result<(), NaturalFieldRegistryError> {
    if !(MIN_PLATE_COUNT..=MAX_PLATE_COUNT).contains(&plate_count) {
        return Err(NaturalFieldRegistryError::PlateCountOutOfRange {
            found: plate_count,
            min: MIN_PLATE_COUNT,
            max: MAX_PLATE_COUNT,
        });
    }
    Ok(())
}

fn schemas(
    plate_count: u16,
    limits: NaturalFieldRegistryLimits,
) -> Result<Vec<FieldSchema>, FieldSchemaError> {
    let plate_id = plate_id_field_id();
    let crust_kind = crust_kind_field_id();
    let crust_thickness = crust_thickness_field_id();
    let plate_velocity = plate_velocity_field_id();
    let boundary_kind = boundary_kind_field_id();
    let boundary_strength = boundary_strength_field_id();
    let crust_base = crust_base_elevation_field_id();
    let tectonic_offset = tectonic_offset_field_id();
    let regional_offset = regional_offset_field_id();
    let elevation = elevation_field_id();
    let land_ocean = land_ocean_field_id();
    let mantle_heat_flow = mantle_heat_flow_field_id();
    let volcanic_influence = volcanic_influence_field_id();
    let volcanic_offset = volcanic_offset_field_id();
    let bedrock_kind = bedrock_kind_field_id();
    let fracture_intensity = fracture_intensity_field_id();
    let erosion_resistance = erosion_resistance_field_id();
    let relative_permeability = relative_permeability_field_id();
    let metallic_mineral_potential = metallic_mineral_potential_field_id();
    let geothermal_potential = geothermal_potential_field_id();
    let sedimentary_basin_potential = sedimentary_basin_potential_field_id();
    let latitude_degrees = latitude_degrees_field_id();
    let maritime_influence = maritime_influence_field_id();
    let prevailing_wind = preliminary_prevailing_wind_m_s_field_id();
    let mean_air_temperature = preliminary_mean_air_temperature_c_field_id();
    let temperature_seasonality = preliminary_temperature_seasonality_c_field_id();
    let annual_precipitation = preliminary_annual_precipitation_mm_field_id();
    let surface_elevation = surface_elevation_m_field_id();
    let fluvial_erosion_depth = fluvial_erosion_depth_m_field_id();
    let sediment_deposition_thickness = sediment_deposition_thickness_m_field_id();
    let surface_water_kind = surface_water_kind_field_id();
    let lake_depth = lake_depth_m_field_id();
    let annual_local_runoff = annual_local_runoff_mm_field_id();
    let mean_annual_discharge = mean_annual_discharge_m3_s_field_id();
    let drainage_area = drainage_area_km2_field_id();
    let strahler_stream_order = strahler_stream_order_field_id();

    Ok(vec![
        category_schema(
            plate_id.clone(),
            FieldDomain::Cells,
            (0..u32::from(plate_count))
                .map(|plate| {
                    (
                        plate,
                        format!("field.sekai.core.natural.plate_id.plate-{plate:02}"),
                    )
                })
                .collect(),
        )?,
        category_schema(
            crust_kind.clone(),
            FieldDomain::Cells,
            BTreeMap::from([
                (0, "field.sekai.core.natural.crust_kind.oceanic".into()),
                (1, "field.sekai.core.natural.crust_kind.continental".into()),
            ]),
        )?,
        scalar_schema(
            crust_thickness.clone(),
            FieldDomain::Cells,
            custom_unit("kilometer", "km"),
            OCEANIC_CRUST_MIN_THICKNESS_KM,
            CONTINENTAL_CRUST_MAX_THICKNESS_KM,
            FieldPaletteHint::Sequential,
            1,
            vec![crust_kind.clone()],
        )?,
        vector_schema(
            plate_velocity.clone(),
            custom_unit("centimeter-per-year", "cm/year"),
            vec![plate_id.clone()],
        )?,
        category_schema_with_dependencies(
            boundary_kind.clone(),
            FieldDomain::Edges,
            BTreeMap::from([
                (0, "field.sekai.core.natural.boundary_kind.none".into()),
                (1, "field.sekai.core.natural.boundary_kind.weak".into()),
                (
                    2,
                    "field.sekai.core.natural.boundary_kind.continental_collision".into(),
                ),
                (
                    3,
                    "field.sekai.core.natural.boundary_kind.subduction".into(),
                ),
                (
                    4,
                    "field.sekai.core.natural.boundary_kind.continental_rift".into(),
                ),
                (
                    5,
                    "field.sekai.core.natural.boundary_kind.oceanic_ridge".into(),
                ),
                (6, "field.sekai.core.natural.boundary_kind.transform".into()),
            ]),
            vec![
                plate_id.clone(),
                crust_kind.clone(),
                crust_thickness.clone(),
                plate_velocity.clone(),
            ],
        )?,
        scalar_schema(
            boundary_strength.clone(),
            FieldDomain::Edges,
            FieldUnit::Unitless,
            0.0,
            1.0,
            FieldPaletteHint::Sequential,
            2,
            vec![boundary_kind.clone(), plate_velocity],
        )?,
        scalar_schema(
            mantle_heat_flow.clone(),
            FieldDomain::Cells,
            custom_unit("milliwatt-per-square-meter", "mW/m²"),
            HEAT_FLOW_MIN_MW_M2,
            HEAT_FLOW_MAX_MW_M2,
            FieldPaletteHint::Sequential,
            1,
            Vec::new(),
        )?,
        scalar_schema(
            volcanic_influence.clone(),
            FieldDomain::Cells,
            FieldUnit::Unitless,
            0.0,
            1.0,
            FieldPaletteHint::Sequential,
            2,
            vec![mantle_heat_flow.clone()],
        )?,
        scalar_schema(
            crust_base.clone(),
            FieldDomain::Cells,
            custom_unit("meter", "m"),
            CRUST_BASE_ELEVATION_MIN_M,
            CRUST_BASE_ELEVATION_MAX_M,
            FieldPaletteHint::Diverging,
            0,
            vec![crust_kind.clone(), crust_thickness],
        )?,
        scalar_schema(
            tectonic_offset.clone(),
            FieldDomain::Cells,
            custom_unit("meter", "m"),
            TECTONIC_OFFSET_MIN_M,
            TECTONIC_OFFSET_MAX_M,
            FieldPaletteHint::Diverging,
            0,
            vec![boundary_kind.clone(), boundary_strength.clone()],
        )?,
        scalar_schema(
            volcanic_offset.clone(),
            FieldDomain::Cells,
            custom_unit("meter", "m"),
            VOLCANIC_OFFSET_MIN_M,
            VOLCANIC_OFFSET_MAX_M,
            FieldPaletteHint::Sequential,
            0,
            vec![volcanic_influence.clone()],
        )?,
        scalar_schema(
            regional_offset.clone(),
            FieldDomain::Cells,
            custom_unit("meter", "m"),
            REGIONAL_OFFSET_MIN_M,
            REGIONAL_OFFSET_MAX_M,
            FieldPaletteHint::Diverging,
            0,
            Vec::new(),
        )?,
        scalar_schema(
            elevation.clone(),
            FieldDomain::Cells,
            custom_unit("meter", "m"),
            ELEVATION_MIN_M,
            ELEVATION_MAX_M,
            FieldPaletteHint::Hypsometric,
            0,
            vec![
                crust_base,
                tectonic_offset.clone(),
                volcanic_offset,
                regional_offset,
            ],
        )?,
        category_schema_with_palette(
            land_ocean.clone(),
            FieldDomain::Cells,
            BTreeMap::from([
                (0, "field.sekai.core.natural.land_ocean.ocean".into()),
                (1, "field.sekai.core.natural.land_ocean.land".into()),
            ]),
            vec![elevation.clone()],
            FieldPaletteHint::LandOcean,
        )?,
        category_schema_with_dependencies(
            bedrock_kind.clone(),
            FieldDomain::Cells,
            BTreeMap::from([
                (
                    0,
                    "field.sekai.core.natural.bedrock_kind.oceanic_mafic".into(),
                ),
                (
                    1,
                    "field.sekai.core.natural.bedrock_kind.continental_crystalline".into(),
                ),
                (
                    2,
                    "field.sekai.core.natural.bedrock_kind.sedimentary".into(),
                ),
                (
                    3,
                    "field.sekai.core.natural.bedrock_kind.metamorphic".into(),
                ),
                (4, "field.sekai.core.natural.bedrock_kind.volcanic".into()),
            ]),
            vec![
                crust_kind,
                boundary_kind.clone(),
                volcanic_influence.clone(),
                elevation.clone(),
            ],
        )?,
        scalar_schema(
            fracture_intensity.clone(),
            FieldDomain::Cells,
            FieldUnit::Unitless,
            0.0,
            1.0,
            FieldPaletteHint::Sequential,
            2,
            vec![boundary_strength.clone(), volcanic_influence.clone()],
        )?,
        scalar_schema(
            erosion_resistance.clone(),
            FieldDomain::Cells,
            FieldUnit::Unitless,
            0.0,
            1.0,
            FieldPaletteHint::Sequential,
            2,
            vec![bedrock_kind.clone(), fracture_intensity.clone()],
        )?,
        scalar_schema(
            relative_permeability.clone(),
            FieldDomain::Cells,
            FieldUnit::Unitless,
            0.0,
            1.0,
            FieldPaletteHint::Sequential,
            2,
            vec![bedrock_kind.clone(), fracture_intensity.clone()],
        )?,
        scalar_schema(
            metallic_mineral_potential,
            FieldDomain::Cells,
            FieldUnit::Unitless,
            0.0,
            1.0,
            FieldPaletteHint::Sequential,
            2,
            vec![
                bedrock_kind.clone(),
                boundary_kind,
                boundary_strength,
                fracture_intensity.clone(),
                volcanic_influence,
            ],
        )?,
        scalar_schema(
            geothermal_potential,
            FieldDomain::Cells,
            FieldUnit::Unitless,
            0.0,
            1.0,
            FieldPaletteHint::Sequential,
            2,
            vec![mantle_heat_flow, fracture_intensity],
        )?,
        scalar_schema(
            sedimentary_basin_potential,
            FieldDomain::Cells,
            FieldUnit::Unitless,
            0.0,
            1.0,
            FieldPaletteHint::Sequential,
            2,
            vec![bedrock_kind, tectonic_offset, elevation.clone()],
        )?,
        scalar_schema(
            latitude_degrees.clone(),
            FieldDomain::Cells,
            custom_unit("degree", "°"),
            -90.0,
            90.0,
            FieldPaletteHint::Diverging,
            1,
            Vec::new(),
        )?,
        scalar_schema(
            maritime_influence.clone(),
            FieldDomain::Cells,
            FieldUnit::Unitless,
            0.0,
            1.0,
            FieldPaletteHint::Sequential,
            2,
            vec![land_ocean],
        )?,
        vector_schema(
            prevailing_wind.clone(),
            custom_unit("meter-per-second", "m/s"),
            vec![latitude_degrees.clone()],
        )?,
        scalar_schema(
            mean_air_temperature.clone(),
            FieldDomain::Cells,
            custom_unit("degree-celsius", "°C"),
            AIR_TEMPERATURE_MIN_C,
            AIR_TEMPERATURE_MAX_C,
            FieldPaletteHint::Diverging,
            1,
            vec![
                latitude_degrees.clone(),
                elevation.clone(),
                maritime_influence.clone(),
            ],
        )?,
        scalar_schema(
            temperature_seasonality,
            FieldDomain::Cells,
            custom_unit("degree-celsius", "°C"),
            0.0,
            TEMPERATURE_SEASONALITY_MAX_C,
            FieldPaletteHint::Sequential,
            1,
            vec![
                latitude_degrees,
                elevation.clone(),
                maritime_influence.clone(),
            ],
        )?,
        scalar_schema(
            annual_precipitation.clone(),
            FieldDomain::Cells,
            custom_unit("millimeter-per-year", "mm/year"),
            0.0,
            ANNUAL_PRECIPITATION_MAX_MM,
            FieldPaletteHint::Sequential,
            0,
            vec![
                mean_air_temperature,
                elevation.clone(),
                maritime_influence,
                prevailing_wind,
            ],
        )?,
        scalar_schema(
            fluvial_erosion_depth.clone(),
            FieldDomain::Cells,
            custom_unit("meter", "m"),
            0.0,
            MAX_EROSION_DEPTH_M,
            FieldPaletteHint::Sequential,
            1,
            vec![
                annual_precipitation.clone(),
                elevation.clone(),
                erosion_resistance,
                relative_permeability.clone(),
            ],
        )?,
        scalar_schema(
            sediment_deposition_thickness.clone(),
            FieldDomain::Cells,
            custom_unit("meter", "m"),
            0.0,
            MAX_DEPOSITION_THICKNESS_M,
            FieldPaletteHint::Sequential,
            1,
            vec![elevation.clone(), fluvial_erosion_depth.clone()],
        )?,
        scalar_schema(
            surface_elevation.clone(),
            FieldDomain::Cells,
            custom_unit("meter", "m"),
            ELEVATION_MIN_M,
            ELEVATION_MAX_M,
            FieldPaletteHint::Hypsometric,
            0,
            vec![
                elevation,
                fluvial_erosion_depth,
                sediment_deposition_thickness,
            ],
        )?,
        scalar_schema(
            lake_depth.clone(),
            FieldDomain::Cells,
            custom_unit("meter", "m"),
            0.0,
            MAX_LAKE_DEPTH_M,
            FieldPaletteHint::Sequential,
            1,
            vec![surface_elevation.clone()],
        )?,
        category_schema_with_dependencies(
            surface_water_kind.clone(),
            FieldDomain::Cells,
            BTreeMap::from([
                (
                    0,
                    "field.sekai.core.natural.surface_water_kind.dry_land".into(),
                ),
                (
                    1,
                    "field.sekai.core.natural.surface_water_kind.ocean".into(),
                ),
                (2, "field.sekai.core.natural.surface_water_kind.lake".into()),
            ]),
            vec![lake_depth, surface_elevation],
        )?,
        scalar_schema(
            annual_local_runoff.clone(),
            FieldDomain::Cells,
            custom_unit("millimeter-per-year", "mm/year"),
            0.0,
            ANNUAL_PRECIPITATION_MAX_MM,
            FieldPaletteHint::Sequential,
            0,
            vec![annual_precipitation, relative_permeability],
        )?,
        scalar_schema(
            mean_annual_discharge.clone(),
            FieldDomain::Cells,
            custom_unit("cubic-meter-per-second", "m³/s"),
            0.0,
            limits.max_mean_annual_discharge_m3_s,
            FieldPaletteHint::Sequential,
            2,
            vec![annual_local_runoff],
        )?,
        scalar_schema(
            drainage_area,
            FieldDomain::Cells,
            custom_unit("square-kilometer", "km²"),
            0.0,
            limits.max_drainage_area_km2,
            FieldPaletteHint::Sequential,
            1,
            Vec::new(),
        )?,
        category_schema_with_dependencies(
            strahler_stream_order,
            FieldDomain::Cells,
            std::iter::once((
                0,
                "field.sekai.core.natural.strahler_stream_order.none".into(),
            ))
            .chain((1..=u32::from(MAX_STRAHLER_ORDER)).map(|order| {
                (
                    order,
                    format!("field.sekai.core.natural.strahler_stream_order.order-{order:03}"),
                )
            }))
            .collect(),
            vec![mean_annual_discharge, surface_water_kind],
        )?,
    ])
}

fn field_id(name: &str) -> FieldId {
    FieldId::new(NAMESPACE, name, SCHEMA_VERSION).expect("engine-owned natural field ID is valid")
}

fn custom_unit(name: &str, symbol: &str) -> FieldUnit {
    FieldUnit::Custom {
        namespace: UNIT_NAMESPACE.into(),
        name: name.into(),
        symbol: symbol.into(),
    }
}

fn display(
    id: &FieldId,
    palette: FieldPaletteHint,
    decimal_places: u8,
) -> Result<FieldDisplayMetadata, FieldSchemaError> {
    FieldDisplayMetadata::new(
        format!("field.{}.{}", id.namespace(), id.name()),
        palette,
        decimal_places,
    )
}

fn category_schema(
    id: FieldId,
    domain: FieldDomain,
    category_labels: BTreeMap<u32, String>,
) -> Result<FieldSchema, FieldSchemaError> {
    category_schema_with_dependencies(id, domain, category_labels, Vec::new())
}

fn category_schema_with_dependencies(
    id: FieldId,
    domain: FieldDomain,
    category_labels: BTreeMap<u32, String>,
    dependencies: Vec<FieldId>,
) -> Result<FieldSchema, FieldSchemaError> {
    category_schema_with_palette(
        id,
        domain,
        category_labels,
        dependencies,
        FieldPaletteHint::Categorical,
    )
}

fn category_schema_with_palette(
    id: FieldId,
    domain: FieldDomain,
    category_labels: BTreeMap<u32, String>,
    dependencies: Vec<FieldId>,
    palette: FieldPaletteHint,
) -> Result<FieldSchema, FieldSchemaError> {
    Ok(FieldSchema {
        display: display(&id, palette, 0)?,
        id,
        domain,
        value_type: FieldValueType::CategoryU32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies,
        category_labels,
    })
}

fn scalar_schema(
    id: FieldId,
    domain: FieldDomain,
    unit: FieldUnit,
    min: f32,
    max: f32,
    palette: FieldPaletteHint,
    decimal_places: u8,
    dependencies: Vec<FieldId>,
) -> Result<FieldSchema, FieldSchemaError> {
    Ok(FieldSchema {
        display: display(&id, palette, decimal_places)?,
        id,
        domain,
        value_type: FieldValueType::ScalarF32,
        unit,
        valid_range: Some(ValueRange::new(min, max)?),
        missing: MissingValuePolicy::Forbidden,
        dependencies,
        category_labels: BTreeMap::new(),
    })
}

fn vector_schema(
    id: FieldId,
    unit: FieldUnit,
    dependencies: Vec<FieldId>,
) -> Result<FieldSchema, FieldSchemaError> {
    Ok(FieldSchema {
        display: display(&id, FieldPaletteHint::Vector, 1)?,
        id,
        domain: FieldDomain::Cells,
        value_type: FieldValueType::Vector2F32,
        unit,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies,
        category_labels: BTreeMap::new(),
    })
}

/// Derived, renderer-facing dense arrays not stored as authoritative world fields.
#[derive(Debug, Clone, PartialEq)]
pub struct NaturalFieldDisplayCache {
    plate_velocity_cm_per_year: Vec<[f32; 2]>,
    boundary_kind: Vec<u32>,
    boundary_strength: Vec<f32>,
}

impl NaturalFieldDisplayCache {
    /// Derives display arrays from one immutable tectonic snapshot.
    pub fn new(tectonic: &TectonicSnapshot) -> Self {
        let plate_velocity_cm_per_year = tectonic
            .cell_plates()
            .raw_values()
            .iter()
            .map(|&plate| {
                let velocity = tectonic.plates()[plate as usize]
                    .velocity
                    .components_mm_per_year();
                [f32::from(velocity[0]) / 10.0, f32::from(velocity[1]) / 10.0]
            })
            .collect();
        let boundary_kind = tectonic
            .boundaries()
            .iter()
            .map(|record| record.kind.raw())
            .collect();
        let boundary_strength = tectonic
            .boundaries()
            .iter()
            .map(|record| record.strength)
            .collect();
        Self {
            plate_velocity_cm_per_year,
            boundary_kind,
            boundary_strength,
        }
    }

    /// Returns the derived per-cell velocity vectors in centimeters per year.
    pub fn plate_velocity_cm_per_year(&self) -> &[[f32; 2]] {
        &self.plate_velocity_cm_per_year
    }

    /// Returns derived raw boundary-category values in edge order.
    pub fn boundary_kind(&self) -> &[u32] {
        &self.boundary_kind
    }

    /// Returns derived boundary strengths in edge order.
    pub fn boundary_strength(&self) -> &[f32] {
        &self.boundary_strength
    }
}

/// Errors returned while constructing the formal natural-field registry.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum NaturalFieldRegistryError {
    /// Dynamic plate labels were requested outside the supported plate range.
    #[error("plate count {found} is outside {min}..={max}")]
    PlateCountOutOfRange {
        /// The rejected plate count.
        found: u16,
        /// The inclusive lower bound.
        min: u16,
        /// The inclusive upper bound.
        max: u16,
    },
    /// A spherical registry was requested with a non-physical surface area.
    #[error("total surface area must be finite and positive, got {found} m²")]
    InvalidTotalSurfaceArea {
        /// The rejected total surface area in square meters.
        found: f64,
    },
    /// A sphere-derived field maximum cannot be represented by the V1 f32 schema.
    #[error(
        "sphere-derived natural field range exceeds f32 for surface area {total_surface_area_m2} m²"
    )]
    SphericalFieldRangeOverflow {
        /// The positive finite surface area whose derived range overflowed.
        total_surface_area_m2: f64,
    },
    /// One engine-owned field schema violated the generic schema contract.
    #[error("invalid natural field schema: {0}")]
    InvalidSchema(#[from] FieldSchemaError),
}
