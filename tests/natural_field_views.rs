use std::collections::BTreeMap;

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicArtifact,
    GeologicSpecArtifact, HydroErosionArtifact, HydroErosionSpecArtifact, MantleArtifact,
    PreliminaryClimateArtifact, ReliefArtifact, RulePackSetArtifact, TectonicArtifact,
    TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::view::{
    prepare_cell_field, DisplayRangeMode, FieldCatalog, FieldDisplayState, FieldPayloadRef,
};
use sekai::world::fields::{FieldDomain, FieldPaletteHint, FieldValueType, MissingValuePolicy};
use sekai::world::natural::{
    annual_local_runoff_mm_field_id, bedrock_kind_field_id, boundary_kind_field_id,
    boundary_strength_field_id, crust_base_elevation_field_id, crust_kind_field_id,
    crust_thickness_field_id, drainage_area_km2_field_id, elevation_field_id,
    erosion_resistance_field_id, fluvial_erosion_depth_m_field_id, fracture_intensity_field_id,
    geothermal_potential_field_id, lake_depth_m_field_id, land_ocean_field_id,
    latitude_degrees_field_id, mantle_heat_flow_field_id, maritime_influence_field_id,
    mean_annual_discharge_m3_s_field_id, metallic_mineral_potential_field_id,
    natural_field_registry, plate_id_field_id, plate_velocity_field_id,
    preliminary_annual_precipitation_mm_field_id, preliminary_mean_air_temperature_c_field_id,
    preliminary_prevailing_wind_m_s_field_id, preliminary_temperature_seasonality_c_field_id,
    regional_offset_field_id, relative_permeability_field_id,
    sediment_deposition_thickness_m_field_id, sedimentary_basin_potential_field_id,
    strahler_stream_order_field_id, surface_elevation_m_field_id, surface_water_kind_field_id,
    tectonic_offset_field_id, volcanic_influence_field_id, volcanic_offset_field_id, ClimateSpec,
    GeologicSpec, HydroErosionSpec, NaturalFieldDisplayCache, NaturalFieldRegistryError,
    TectonicSpec, WorldFormationSpec, AIR_TEMPERATURE_MAX_C, AIR_TEMPERATURE_MIN_C,
    ANNUAL_PRECIPITATION_MAX_MM, ELEVATION_MAX_M, ELEVATION_MIN_M, HEAT_FLOW_MAX_MW_M2,
    HEAT_FLOW_MIN_MW_M2, MAX_DEPOSITION_THICKNESS_M, MAX_EROSION_DEPTH_M, MAX_LAKE_DEPTH_M,
    MAX_PLATE_COUNT, MAX_STRAHLER_ORDER, TEMPERATURE_SEASONALITY_MAX_C, VOLCANIC_OFFSET_MAX_M,
    VOLCANIC_OFFSET_MIN_M,
};
use sekai::world::spatial::Topology;
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

const NATURAL_NAMESPACE: &str = "sekai.core.natural";

fn registry() -> sekai::world::fields::FieldRegistry {
    natural_field_registry(12).unwrap()
}

fn schema(
    registry: &sekai::world::fields::FieldRegistry,
    id: sekai::world::fields::FieldId,
) -> &sekai::world::fields::FieldSchema {
    registry.get(&id).unwrap()
}

#[test]
fn schema_registry_contains_the_exact_formal_natural_fields() {
    let registry = registry();
    assert_eq!(registry.len(), 36);
    let expected = [
        (
            plate_id_field_id(),
            "plate_id",
            FieldDomain::Cells,
            FieldValueType::CategoryU32,
        ),
        (
            crust_kind_field_id(),
            "crust_kind",
            FieldDomain::Cells,
            FieldValueType::CategoryU32,
        ),
        (
            crust_thickness_field_id(),
            "crust_thickness_km",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            plate_velocity_field_id(),
            "plate_velocity",
            FieldDomain::Cells,
            FieldValueType::Vector2F32,
        ),
        (
            boundary_kind_field_id(),
            "boundary_kind",
            FieldDomain::Edges,
            FieldValueType::CategoryU32,
        ),
        (
            boundary_strength_field_id(),
            "boundary_strength",
            FieldDomain::Edges,
            FieldValueType::ScalarF32,
        ),
        (
            crust_base_elevation_field_id(),
            "crust_base_elevation_m",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            tectonic_offset_field_id(),
            "tectonic_offset_m",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            regional_offset_field_id(),
            "regional_offset_m",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            elevation_field_id(),
            "elevation_m",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            land_ocean_field_id(),
            "land_ocean",
            FieldDomain::Cells,
            FieldValueType::CategoryU32,
        ),
        (
            mantle_heat_flow_field_id(),
            "mantle_heat_flow_mw_m2",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            volcanic_influence_field_id(),
            "volcanic_influence",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            volcanic_offset_field_id(),
            "volcanic_offset_m",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            bedrock_kind_field_id(),
            "bedrock_kind",
            FieldDomain::Cells,
            FieldValueType::CategoryU32,
        ),
        (
            fracture_intensity_field_id(),
            "fracture_intensity",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            erosion_resistance_field_id(),
            "erosion_resistance",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            relative_permeability_field_id(),
            "relative_permeability",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            metallic_mineral_potential_field_id(),
            "metallic_mineral_potential",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            geothermal_potential_field_id(),
            "geothermal_potential",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            sedimentary_basin_potential_field_id(),
            "sedimentary_basin_potential",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            latitude_degrees_field_id(),
            "latitude_degrees",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            maritime_influence_field_id(),
            "maritime_influence",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            preliminary_prevailing_wind_m_s_field_id(),
            "preliminary_prevailing_wind_m_s",
            FieldDomain::Cells,
            FieldValueType::Vector2F32,
        ),
        (
            preliminary_mean_air_temperature_c_field_id(),
            "preliminary_mean_air_temperature_c",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            preliminary_temperature_seasonality_c_field_id(),
            "preliminary_temperature_seasonality_c",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            preliminary_annual_precipitation_mm_field_id(),
            "preliminary_annual_precipitation_mm",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            surface_elevation_m_field_id(),
            "surface_elevation_m",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            fluvial_erosion_depth_m_field_id(),
            "fluvial_erosion_depth_m",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            sediment_deposition_thickness_m_field_id(),
            "sediment_deposition_thickness_m",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            surface_water_kind_field_id(),
            "surface_water_kind",
            FieldDomain::Cells,
            FieldValueType::CategoryU32,
        ),
        (
            lake_depth_m_field_id(),
            "lake_depth_m",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            annual_local_runoff_mm_field_id(),
            "annual_local_runoff_mm",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            mean_annual_discharge_m3_s_field_id(),
            "mean_annual_discharge_m3_s",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            drainage_area_km2_field_id(),
            "drainage_area_km2",
            FieldDomain::Cells,
            FieldValueType::ScalarF32,
        ),
        (
            strahler_stream_order_field_id(),
            "strahler_stream_order",
            FieldDomain::Cells,
            FieldValueType::CategoryU32,
        ),
    ];

    for (id, name, domain, value_type) in expected {
        assert_eq!(id.namespace(), NATURAL_NAMESPACE);
        assert_eq!(id.name(), name);
        assert_eq!(id.version(), 1);
        let schema = schema(&registry, id);
        assert_eq!(schema.domain, domain);
        assert_eq!(schema.value_type, value_type);
        assert_eq!(schema.missing, MissingValuePolicy::Forbidden);
    }
}

#[test]
fn hydro_erosion_schemas_have_semantic_ranges_palettes_and_complete_categories() {
    let registry = registry();
    for (id, max, decimals) in [
        (fluvial_erosion_depth_m_field_id(), MAX_EROSION_DEPTH_M, 1),
        (
            sediment_deposition_thickness_m_field_id(),
            MAX_DEPOSITION_THICKNESS_M,
            1,
        ),
        (lake_depth_m_field_id(), MAX_LAKE_DEPTH_M, 1),
        (
            annual_local_runoff_mm_field_id(),
            ANNUAL_PRECIPITATION_MAX_MM,
            0,
        ),
    ] {
        let schema = schema(&registry, id);
        assert_eq!(
            schema.unit.symbol(),
            if decimals == 0 { "mm/year" } else { "m" }
        );
        assert_eq!(
            schema.valid_range.map(|range| (range.min(), range.max())),
            Some((0.0, max))
        );
        assert_eq!(schema.display.palette(), FieldPaletteHint::Sequential);
        assert_eq!(schema.display.decimal_places(), decimals);
    }

    let surface = schema(&registry, surface_elevation_m_field_id());
    assert_eq!(surface.unit.symbol(), "m");
    assert_eq!(
        surface.valid_range.map(|range| (range.min(), range.max())),
        Some((ELEVATION_MIN_M, ELEVATION_MAX_M))
    );
    assert_eq!(surface.display.palette(), FieldPaletteHint::Hypsometric);
    assert_eq!(surface.display.decimal_places(), 0);

    for (id, unit, decimals) in [
        (mean_annual_discharge_m3_s_field_id(), "m³/s", 2),
        (drainage_area_km2_field_id(), "km²", 1),
    ] {
        let schema = schema(&registry, id);
        assert_eq!(schema.unit.symbol(), unit);
        let range = schema.valid_range.expect("hydrology scalar has a range");
        assert_eq!(range.min(), 0.0);
        assert!(range.max().is_finite() && range.max() > 0.0);
        assert_eq!(schema.display.palette(), FieldPaletteHint::Sequential);
        assert_eq!(schema.display.decimal_places(), decimals);
    }

    assert_eq!(
        schema(&registry, surface_water_kind_field_id()).category_labels,
        BTreeMap::from([
            (
                0,
                "field.sekai.core.natural.surface_water_kind.dry_land".into()
            ),
            (
                1,
                "field.sekai.core.natural.surface_water_kind.ocean".into()
            ),
            (2, "field.sekai.core.natural.surface_water_kind.lake".into()),
        ])
    );
    let orders = &schema(&registry, strahler_stream_order_field_id()).category_labels;
    assert_eq!(orders.len(), usize::from(MAX_STRAHLER_ORDER) + 1);
    assert_eq!(
        orders.get(&0).map(String::as_str),
        Some("field.sekai.core.natural.strahler_stream_order.none")
    );
    assert_eq!(
        orders
            .get(&u32::from(MAX_STRAHLER_ORDER))
            .map(String::as_str),
        Some("field.sekai.core.natural.strahler_stream_order.order-255")
    );
}

#[test]
fn schema_units_ranges_labels_and_palettes_are_semantic() {
    let registry = registry();
    assert_eq!(
        schema(&registry, crust_thickness_field_id()).unit.symbol(),
        "km"
    );
    assert_eq!(
        schema(&registry, plate_velocity_field_id()).unit.symbol(),
        "cm/year"
    );
    for id in [
        crust_base_elevation_field_id(),
        tectonic_offset_field_id(),
        volcanic_offset_field_id(),
        regional_offset_field_id(),
        elevation_field_id(),
    ] {
        assert_eq!(schema(&registry, id).unit.symbol(), "m");
    }
    let elevation = schema(&registry, elevation_field_id());
    let range = elevation.valid_range.unwrap();
    assert_eq!(
        (range.min(), range.max()),
        (ELEVATION_MIN_M, ELEVATION_MAX_M)
    );
    assert_eq!(elevation.display.palette(), FieldPaletteHint::Hypsometric);
    let heat = schema(&registry, mantle_heat_flow_field_id());
    assert_eq!(heat.unit.symbol(), "mW/m²");
    assert_eq!(
        heat.valid_range.map(|range| (range.min(), range.max())),
        Some((HEAT_FLOW_MIN_MW_M2, HEAT_FLOW_MAX_MW_M2))
    );
    let volcanic_offset = schema(&registry, volcanic_offset_field_id());
    assert_eq!(
        volcanic_offset
            .valid_range
            .map(|range| (range.min(), range.max())),
        Some((VOLCANIC_OFFSET_MIN_M, VOLCANIC_OFFSET_MAX_M))
    );
    for id in [
        volcanic_influence_field_id(),
        fracture_intensity_field_id(),
        erosion_resistance_field_id(),
        relative_permeability_field_id(),
        metallic_mineral_potential_field_id(),
        geothermal_potential_field_id(),
        sedimentary_basin_potential_field_id(),
    ] {
        let schema = schema(&registry, id);
        assert_eq!(schema.unit.symbol(), "");
        assert_eq!(
            schema.valid_range.map(|range| (range.min(), range.max())),
            Some((0.0, 1.0))
        );
        assert_eq!(schema.display.palette(), FieldPaletteHint::Sequential);
    }
    assert_eq!(
        schema(&registry, plate_velocity_field_id())
            .display
            .palette(),
        FieldPaletteHint::Vector
    );
    let latitude = schema(&registry, latitude_degrees_field_id());
    assert_eq!(latitude.unit.symbol(), "°");
    assert_eq!(
        latitude.valid_range.map(|range| (range.min(), range.max())),
        Some((-90.0, 90.0))
    );
    assert_eq!(latitude.display.palette(), FieldPaletteHint::Diverging);
    assert_eq!(latitude.display.decimal_places(), 1);
    let maritime = schema(&registry, maritime_influence_field_id());
    assert_eq!(maritime.unit.symbol(), "");
    assert_eq!(
        maritime.valid_range.map(|range| (range.min(), range.max())),
        Some((0.0, 1.0))
    );
    assert_eq!(maritime.display.palette(), FieldPaletteHint::Sequential);
    assert_eq!(
        schema(&registry, preliminary_prevailing_wind_m_s_field_id())
            .unit
            .symbol(),
        "m/s"
    );
    for id in [
        preliminary_mean_air_temperature_c_field_id(),
        preliminary_temperature_seasonality_c_field_id(),
    ] {
        assert_eq!(schema(&registry, id).unit.symbol(), "°C");
    }
    assert_eq!(
        schema(&registry, preliminary_mean_air_temperature_c_field_id())
            .valid_range
            .map(|range| (range.min(), range.max())),
        Some((AIR_TEMPERATURE_MIN_C, AIR_TEMPERATURE_MAX_C))
    );
    assert_eq!(
        schema(&registry, preliminary_temperature_seasonality_c_field_id())
            .valid_range
            .map(|range| (range.min(), range.max())),
        Some((0.0, TEMPERATURE_SEASONALITY_MAX_C))
    );
    let precipitation = schema(&registry, preliminary_annual_precipitation_mm_field_id());
    assert_eq!(precipitation.unit.symbol(), "mm/year");
    assert_eq!(
        precipitation
            .valid_range
            .map(|range| (range.min(), range.max())),
        Some((0.0, ANNUAL_PRECIPITATION_MAX_MM))
    );
    assert_eq!(
        schema(&registry, land_ocean_field_id()).category_labels,
        BTreeMap::from([
            (0, "field.sekai.core.natural.land_ocean.ocean".into()),
            (1, "field.sekai.core.natural.land_ocean.land".into()),
        ])
    );
    assert_eq!(
        schema(&registry, bedrock_kind_field_id()).category_labels,
        BTreeMap::from([
            (
                0,
                "field.sekai.core.natural.bedrock_kind.oceanic_mafic".into()
            ),
            (
                1,
                "field.sekai.core.natural.bedrock_kind.continental_crystalline".into()
            ),
            (
                2,
                "field.sekai.core.natural.bedrock_kind.sedimentary".into()
            ),
            (
                3,
                "field.sekai.core.natural.bedrock_kind.metamorphic".into()
            ),
            (4, "field.sekai.core.natural.bedrock_kind.volcanic".into()),
        ])
    );
    assert_eq!(
        schema(&registry, boundary_kind_field_id())
            .category_labels
            .len(),
        7
    );
    assert_eq!(
        schema(&registry, plate_id_field_id()).category_labels.len(),
        12
    );
}

#[test]
fn schema_dependencies_are_closed_acyclic_and_stably_serialized() {
    let first = registry();
    let second = registry();
    assert_eq!(
        schema(&first, plate_velocity_field_id()).dependencies,
        vec![plate_id_field_id()]
    );
    assert_eq!(
        schema(&first, crust_base_elevation_field_id()).dependencies,
        vec![crust_kind_field_id(), crust_thickness_field_id()]
    );
    assert_eq!(
        schema(&first, elevation_field_id()).dependencies,
        vec![
            crust_base_elevation_field_id(),
            regional_offset_field_id(),
            tectonic_offset_field_id(),
            volcanic_offset_field_id(),
        ]
    );
    assert_eq!(
        schema(&first, land_ocean_field_id()).dependencies,
        vec![elevation_field_id()]
    );
    assert_eq!(
        schema(&first, volcanic_influence_field_id()).dependencies,
        vec![mantle_heat_flow_field_id()]
    );
    assert_eq!(
        schema(&first, volcanic_offset_field_id()).dependencies,
        vec![volcanic_influence_field_id()]
    );
    assert_eq!(
        schema(&first, fracture_intensity_field_id()).dependencies,
        vec![boundary_strength_field_id(), volcanic_influence_field_id()]
    );
    assert_eq!(
        schema(&first, erosion_resistance_field_id()).dependencies,
        vec![bedrock_kind_field_id(), fracture_intensity_field_id()]
    );
    assert_eq!(
        schema(&first, relative_permeability_field_id()).dependencies,
        vec![bedrock_kind_field_id(), fracture_intensity_field_id()]
    );
    assert_eq!(
        schema(&first, geothermal_potential_field_id()).dependencies,
        vec![fracture_intensity_field_id(), mantle_heat_flow_field_id()]
    );
    assert!(schema(&first, latitude_degrees_field_id())
        .dependencies
        .is_empty());
    assert_eq!(
        schema(&first, maritime_influence_field_id()).dependencies,
        vec![land_ocean_field_id()]
    );
    assert_eq!(
        schema(&first, preliminary_prevailing_wind_m_s_field_id()).dependencies,
        vec![latitude_degrees_field_id()]
    );
    for id in [
        preliminary_mean_air_temperature_c_field_id(),
        preliminary_temperature_seasonality_c_field_id(),
    ] {
        assert_eq!(
            schema(&first, id).dependencies,
            vec![
                elevation_field_id(),
                latitude_degrees_field_id(),
                maritime_influence_field_id(),
            ]
        );
    }
    assert_eq!(
        schema(&first, preliminary_annual_precipitation_mm_field_id()).dependencies,
        vec![
            elevation_field_id(),
            maritime_influence_field_id(),
            preliminary_mean_air_temperature_c_field_id(),
            preliminary_prevailing_wind_m_s_field_id(),
        ]
    );
    assert_eq!(
        schema(&first, fluvial_erosion_depth_m_field_id()).dependencies,
        vec![
            elevation_field_id(),
            erosion_resistance_field_id(),
            preliminary_annual_precipitation_mm_field_id(),
            relative_permeability_field_id(),
        ]
    );
    assert_eq!(
        schema(&first, sediment_deposition_thickness_m_field_id()).dependencies,
        vec![elevation_field_id(), fluvial_erosion_depth_m_field_id(),]
    );
    assert_eq!(
        schema(&first, surface_elevation_m_field_id()).dependencies,
        vec![
            elevation_field_id(),
            fluvial_erosion_depth_m_field_id(),
            sediment_deposition_thickness_m_field_id(),
        ]
    );
    assert_eq!(
        schema(&first, lake_depth_m_field_id()).dependencies,
        vec![surface_elevation_m_field_id()]
    );
    assert_eq!(
        schema(&first, surface_water_kind_field_id()).dependencies,
        vec![lake_depth_m_field_id(), surface_elevation_m_field_id()]
    );
    assert_eq!(
        schema(&first, annual_local_runoff_mm_field_id()).dependencies,
        vec![
            preliminary_annual_precipitation_mm_field_id(),
            relative_permeability_field_id(),
        ]
    );
    assert_eq!(
        schema(&first, mean_annual_discharge_m3_s_field_id()).dependencies,
        vec![annual_local_runoff_mm_field_id()]
    );
    assert!(schema(&first, drainage_area_km2_field_id())
        .dependencies
        .is_empty());
    assert_eq!(
        schema(&first, strahler_stream_order_field_id()).dependencies,
        vec![
            mean_annual_discharge_m3_s_field_id(),
            surface_water_kind_field_id(),
        ]
    );
    assert!(first.iter().all(|(_, schema)| [
        "monthly",
        "receiver",
        "drainage_surface",
        "flood_rank",
        "normalized"
    ]
    .iter()
    .all(|forbidden| !schema.id.name().contains(forbidden))));
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    let decoded: sekai::world::fields::FieldRegistry =
        serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    assert_eq!(decoded, first);
}

#[test]
fn schema_plate_labels_are_bounded_by_the_validated_maximum() {
    let maximum = natural_field_registry(MAX_PLATE_COUNT).unwrap();
    assert_eq!(
        schema(&maximum, plate_id_field_id()).category_labels.len(),
        MAX_PLATE_COUNT as usize
    );
    assert!(matches!(
        natural_field_registry(MAX_PLATE_COUNT + 1),
        Err(NaturalFieldRegistryError::PlateCountOutOfRange { .. })
    ));
}

struct NaturalArtifacts {
    spatial: std::sync::Arc<SpatialArtifact>,
    tectonic: std::sync::Arc<TectonicArtifact>,
    mantle: std::sync::Arc<MantleArtifact>,
    relief: std::sync::Arc<ReliefArtifact>,
    geology: std::sync::Arc<GeologicArtifact>,
    climate: std::sync::Arc<PreliminaryClimateArtifact>,
    hydro_erosion: std::sync::Arc<HydroErosionArtifact>,
}

fn natural_artifacts() -> NaturalArtifacts {
    let mut external = ExternalArtifacts::new();
    external
        .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
            width: Meters::new(1_000_000.0).unwrap(),
            height: Meters::new(600_000.0).unwrap(),
            target_cell_count: 128,
            boundary: BoundaryCondition::Closed,
        }))
        .unwrap();
    external
        .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
        .unwrap();
    external
        .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
        .unwrap();
    external
        .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
        .unwrap();
    external
        .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
        .unwrap();
    external
        .insert(WorldFormationSpecArtifact::new(
            WorldFormationSpec::default(),
        ))
        .unwrap();
    external
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    external
        .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
        .unwrap();
    let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(RootSeed::new(42), external, &mut MemoryStageCache::new())
        .unwrap();
    NaturalArtifacts {
        spatial: outcome.artifacts.get::<SpatialArtifact>().unwrap(),
        tectonic: outcome.artifacts.get::<TectonicArtifact>().unwrap(),
        mantle: outcome.artifacts.get::<MantleArtifact>().unwrap(),
        relief: outcome.artifacts.get::<ReliefArtifact>().unwrap(),
        geology: outcome.artifacts.get::<GeologicArtifact>().unwrap(),
        climate: outcome
            .artifacts
            .get::<PreliminaryClimateArtifact>()
            .unwrap(),
        hydro_erosion: outcome.artifacts.get::<HydroErosionArtifact>().unwrap(),
    }
}

#[test]
fn borrowed_natural_payloads_match_every_registered_domain() {
    let NaturalArtifacts {
        spatial,
        tectonic,
        mantle,
        relief,
        geology,
        climate,
        hydro_erosion,
    } = natural_artifacts();
    let registry = natural_field_registry(tectonic.snapshot().plates().len() as u16).unwrap();
    let cache = NaturalFieldDisplayCache::new(tectonic.snapshot());
    let payloads = vec![
        (
            plate_id_field_id(),
            FieldPayloadRef::CategoryU32(tectonic.snapshot().cell_plates().raw_values()),
        ),
        (
            crust_kind_field_id(),
            FieldPayloadRef::CategoryU32(tectonic.snapshot().crust_kinds().raw_values()),
        ),
        (
            crust_thickness_field_id(),
            FieldPayloadRef::ScalarF32(tectonic.snapshot().crust_thickness_km()),
        ),
        (
            plate_velocity_field_id(),
            FieldPayloadRef::Vector2F32(cache.plate_velocity_cm_per_year()),
        ),
        (
            boundary_kind_field_id(),
            FieldPayloadRef::CategoryU32(cache.boundary_kind()),
        ),
        (
            boundary_strength_field_id(),
            FieldPayloadRef::ScalarF32(cache.boundary_strength()),
        ),
        (
            crust_base_elevation_field_id(),
            FieldPayloadRef::ScalarF32(relief.snapshot().crust_base_elevation_m().values()),
        ),
        (
            tectonic_offset_field_id(),
            FieldPayloadRef::ScalarF32(relief.snapshot().tectonic_offset_m().values()),
        ),
        (
            regional_offset_field_id(),
            FieldPayloadRef::ScalarF32(relief.snapshot().regional_offset_m().values()),
        ),
        (
            elevation_field_id(),
            FieldPayloadRef::ScalarF32(relief.snapshot().elevation_m().values()),
        ),
        (
            land_ocean_field_id(),
            FieldPayloadRef::CategoryU32(relief.snapshot().land_ocean().raw_values()),
        ),
        (
            mantle_heat_flow_field_id(),
            FieldPayloadRef::ScalarF32(mantle.snapshot().heat_flow_mw_m2()),
        ),
        (
            volcanic_influence_field_id(),
            FieldPayloadRef::ScalarF32(mantle.snapshot().volcanic_influence()),
        ),
        (
            volcanic_offset_field_id(),
            FieldPayloadRef::ScalarF32(relief.snapshot().volcanic_offset_m().values()),
        ),
        (
            bedrock_kind_field_id(),
            FieldPayloadRef::CategoryU32(geology.snapshot().bedrock_kinds().raw_values()),
        ),
        (
            fracture_intensity_field_id(),
            FieldPayloadRef::ScalarF32(geology.snapshot().fracture_intensity()),
        ),
        (
            erosion_resistance_field_id(),
            FieldPayloadRef::ScalarF32(geology.snapshot().erosion_resistance()),
        ),
        (
            relative_permeability_field_id(),
            FieldPayloadRef::ScalarF32(geology.snapshot().relative_permeability()),
        ),
        (
            metallic_mineral_potential_field_id(),
            FieldPayloadRef::ScalarF32(geology.snapshot().metallic_mineral_potential()),
        ),
        (
            geothermal_potential_field_id(),
            FieldPayloadRef::ScalarF32(geology.snapshot().geothermal_potential()),
        ),
        (
            sedimentary_basin_potential_field_id(),
            FieldPayloadRef::ScalarF32(geology.snapshot().sedimentary_basin_potential()),
        ),
        (
            latitude_degrees_field_id(),
            FieldPayloadRef::ScalarF32(climate.snapshot().latitude_degrees()),
        ),
        (
            maritime_influence_field_id(),
            FieldPayloadRef::ScalarF32(climate.snapshot().maritime_influence()),
        ),
        (
            preliminary_prevailing_wind_m_s_field_id(),
            FieldPayloadRef::Vector2F32(climate.snapshot().prevailing_wind_m_s()),
        ),
        (
            preliminary_mean_air_temperature_c_field_id(),
            FieldPayloadRef::ScalarF32(climate.snapshot().mean_annual_air_temperature_c()),
        ),
        (
            preliminary_temperature_seasonality_c_field_id(),
            FieldPayloadRef::ScalarF32(climate.snapshot().temperature_seasonality_c()),
        ),
        (
            preliminary_annual_precipitation_mm_field_id(),
            FieldPayloadRef::ScalarF32(climate.snapshot().annual_precipitation_mm()),
        ),
        (
            surface_elevation_m_field_id(),
            FieldPayloadRef::ScalarF32(
                hydro_erosion
                    .snapshot()
                    .surface()
                    .surface_elevation_m()
                    .values(),
            ),
        ),
        (
            fluvial_erosion_depth_m_field_id(),
            FieldPayloadRef::ScalarF32(hydro_erosion.snapshot().surface().erosion_depth_m()),
        ),
        (
            sediment_deposition_thickness_m_field_id(),
            FieldPayloadRef::ScalarF32(hydro_erosion.snapshot().surface().deposition_thickness_m()),
        ),
        (
            surface_water_kind_field_id(),
            FieldPayloadRef::CategoryU32(
                hydro_erosion
                    .snapshot()
                    .hydrology()
                    .surface_water()
                    .raw_values(),
            ),
        ),
        (
            lake_depth_m_field_id(),
            FieldPayloadRef::ScalarF32(hydro_erosion.snapshot().hydrology().lake_depth_m()),
        ),
        (
            annual_local_runoff_mm_field_id(),
            FieldPayloadRef::ScalarF32(
                hydro_erosion
                    .snapshot()
                    .hydrology()
                    .annual_local_runoff_mm(),
            ),
        ),
        (
            mean_annual_discharge_m3_s_field_id(),
            FieldPayloadRef::ScalarF32(
                hydro_erosion
                    .snapshot()
                    .hydrology()
                    .mean_annual_discharge_m3_s(),
            ),
        ),
        (
            drainage_area_km2_field_id(),
            FieldPayloadRef::ScalarF32(hydro_erosion.snapshot().hydrology().drainage_area_km2()),
        ),
        (
            strahler_stream_order_field_id(),
            FieldPayloadRef::CategoryU32(
                hydro_erosion
                    .snapshot()
                    .hydrology()
                    .strahler_order()
                    .raw_values(),
            ),
        ),
    ];
    let catalog = FieldCatalog::from_payloads(&registry, payloads).unwrap();

    assert_eq!(catalog.entries().len(), registry.len());
    for entry in catalog.entries() {
        let view = entry
            .view()
            .expect("every formal natural field is produced");
        let expected = match entry.schema().domain {
            FieldDomain::Cells => spatial.snapshot().cell_count(),
            FieldDomain::Edges => spatial.snapshot().edges().len(),
            other => panic!("unexpected natural field domain {other:?}"),
        };
        assert_eq!(view.len(), expected);
    }
    assert_eq!(
        catalog
            .get(&plate_id_field_id())
            .unwrap()
            .view()
            .unwrap()
            .category_values()
            .unwrap()
            .as_ptr(),
        tectonic.snapshot().cell_plates().raw_values().as_ptr()
    );
    assert_eq!(
        catalog
            .get(&elevation_field_id())
            .unwrap()
            .view()
            .unwrap()
            .scalar_values()
            .unwrap()
            .as_ptr(),
        relief.snapshot().elevation_m().values().as_ptr()
    );
    assert_eq!(
        catalog
            .get(&plate_velocity_field_id())
            .unwrap()
            .view()
            .unwrap()
            .vector_values()
            .unwrap()
            .as_ptr(),
        cache.plate_velocity_cm_per_year().as_ptr()
    );
    assert_eq!(
        catalog
            .get(&mantle_heat_flow_field_id())
            .unwrap()
            .view()
            .unwrap()
            .scalar_values()
            .unwrap()
            .as_ptr(),
        mantle.snapshot().heat_flow_mw_m2().as_ptr()
    );
    assert_eq!(
        catalog
            .get(&volcanic_offset_field_id())
            .unwrap()
            .view()
            .unwrap()
            .scalar_values()
            .unwrap()
            .as_ptr(),
        relief.snapshot().volcanic_offset_m().values().as_ptr()
    );
    assert_eq!(
        catalog
            .get(&bedrock_kind_field_id())
            .unwrap()
            .view()
            .unwrap()
            .category_values()
            .unwrap()
            .as_ptr(),
        geology.snapshot().bedrock_kinds().raw_values().as_ptr()
    );
    for (id, source) in [
        (
            latitude_degrees_field_id(),
            climate.snapshot().latitude_degrees(),
        ),
        (
            maritime_influence_field_id(),
            climate.snapshot().maritime_influence(),
        ),
        (
            preliminary_mean_air_temperature_c_field_id(),
            climate.snapshot().mean_annual_air_temperature_c(),
        ),
        (
            preliminary_temperature_seasonality_c_field_id(),
            climate.snapshot().temperature_seasonality_c(),
        ),
        (
            preliminary_annual_precipitation_mm_field_id(),
            climate.snapshot().annual_precipitation_mm(),
        ),
    ] {
        assert_eq!(
            catalog
                .get(&id)
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            source.as_ptr()
        );
    }
    assert_eq!(
        catalog
            .get(&preliminary_prevailing_wind_m_s_field_id())
            .unwrap()
            .view()
            .unwrap()
            .vector_values()
            .unwrap()
            .as_ptr(),
        climate.snapshot().prevailing_wind_m_s().as_ptr()
    );
    for (id, source) in [
        (
            surface_elevation_m_field_id(),
            hydro_erosion
                .snapshot()
                .surface()
                .surface_elevation_m()
                .values(),
        ),
        (
            fluvial_erosion_depth_m_field_id(),
            hydro_erosion.snapshot().surface().erosion_depth_m(),
        ),
        (
            sediment_deposition_thickness_m_field_id(),
            hydro_erosion.snapshot().surface().deposition_thickness_m(),
        ),
        (
            lake_depth_m_field_id(),
            hydro_erosion.snapshot().hydrology().lake_depth_m(),
        ),
        (
            annual_local_runoff_mm_field_id(),
            hydro_erosion
                .snapshot()
                .hydrology()
                .annual_local_runoff_mm(),
        ),
        (
            mean_annual_discharge_m3_s_field_id(),
            hydro_erosion
                .snapshot()
                .hydrology()
                .mean_annual_discharge_m3_s(),
        ),
        (
            drainage_area_km2_field_id(),
            hydro_erosion.snapshot().hydrology().drainage_area_km2(),
        ),
    ] {
        assert_eq!(
            catalog
                .get(&id)
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            source.as_ptr()
        );
    }
    assert_eq!(
        catalog
            .get(&surface_water_kind_field_id())
            .unwrap()
            .view()
            .unwrap()
            .category_values()
            .unwrap()
            .as_ptr(),
        hydro_erosion
            .snapshot()
            .hydrology()
            .surface_water()
            .raw_values()
            .as_ptr()
    );
    assert_eq!(
        catalog
            .get(&strahler_stream_order_field_id())
            .unwrap()
            .view()
            .unwrap()
            .category_values()
            .unwrap()
            .as_ptr(),
        hydro_erosion
            .snapshot()
            .hydrology()
            .strahler_order()
            .raw_values()
            .as_ptr()
    );
    for (id, source) in [
        (
            fracture_intensity_field_id(),
            geology.snapshot().fracture_intensity(),
        ),
        (
            erosion_resistance_field_id(),
            geology.snapshot().erosion_resistance(),
        ),
        (
            relative_permeability_field_id(),
            geology.snapshot().relative_permeability(),
        ),
        (
            metallic_mineral_potential_field_id(),
            geology.snapshot().metallic_mineral_potential(),
        ),
        (
            geothermal_potential_field_id(),
            geology.snapshot().geothermal_potential(),
        ),
        (
            sedimentary_basin_potential_field_id(),
            geology.snapshot().sedimentary_basin_potential(),
        ),
    ] {
        assert_eq!(
            catalog
                .get(&id)
                .unwrap()
                .view()
                .unwrap()
                .scalar_values()
                .unwrap()
                .as_ptr(),
            source.as_ptr()
        );
    }

    let edge_view = catalog
        .get(&boundary_kind_field_id())
        .unwrap()
        .view()
        .unwrap();
    assert!(edge_view.cell_fill_kind().is_err());
    let elevation_view = catalog.get(&elevation_field_id()).unwrap().view().unwrap();
    let prepared = prepare_cell_field(
        elevation_view,
        spatial.snapshot().cell_count(),
        DisplayRangeMode::Schema,
    )
    .unwrap();
    assert_eq!(prepared.len(), spatial.snapshot().cell_count());
    for id in [
        latitude_degrees_field_id(),
        maritime_influence_field_id(),
        preliminary_mean_air_temperature_c_field_id(),
        preliminary_temperature_seasonality_c_field_id(),
        preliminary_annual_precipitation_mm_field_id(),
        surface_elevation_m_field_id(),
        fluvial_erosion_depth_m_field_id(),
        sediment_deposition_thickness_m_field_id(),
        surface_water_kind_field_id(),
        lake_depth_m_field_id(),
        annual_local_runoff_mm_field_id(),
        mean_annual_discharge_m3_s_field_id(),
        drainage_area_km2_field_id(),
        strahler_stream_order_field_id(),
    ] {
        assert!(catalog
            .get(&id)
            .unwrap()
            .view()
            .unwrap()
            .cell_fill_kind()
            .is_ok());
    }
    assert!(catalog
        .get(&preliminary_prevailing_wind_m_s_field_id())
        .unwrap()
        .view()
        .unwrap()
        .cell_fill_kind()
        .is_err());

    let tectonic_before = serde_json::to_vec(tectonic.as_ref()).unwrap();
    let mantle_before = serde_json::to_vec(mantle.as_ref()).unwrap();
    let relief_before = serde_json::to_vec(relief.as_ref()).unwrap();
    let geology_before = serde_json::to_vec(geology.as_ref()).unwrap();
    let climate_before = serde_json::to_vec(climate.as_ref()).unwrap();
    let hydro_erosion_before = serde_json::to_vec(hydro_erosion.as_ref()).unwrap();
    let mut state = FieldDisplayState::default();
    for id in [
        plate_id_field_id(),
        crust_kind_field_id(),
        elevation_field_id(),
        bedrock_kind_field_id(),
        geothermal_potential_field_id(),
        preliminary_mean_air_temperature_c_field_id(),
        preliminary_annual_precipitation_mm_field_id(),
        surface_elevation_m_field_id(),
        surface_water_kind_field_id(),
        strahler_stream_order_field_id(),
    ] {
        state.select_field(id);
        state.reconcile(&catalog, spatial.snapshot().cell_count());
    }
    assert_eq!(
        serde_json::to_vec(tectonic.as_ref()).unwrap(),
        tectonic_before
    );
    assert_eq!(serde_json::to_vec(mantle.as_ref()).unwrap(), mantle_before);
    assert_eq!(serde_json::to_vec(relief.as_ref()).unwrap(), relief_before);
    assert_eq!(
        serde_json::to_vec(geology.as_ref()).unwrap(),
        geology_before
    );
    assert_eq!(
        serde_json::to_vec(climate.as_ref()).unwrap(),
        climate_before
    );
    assert_eq!(
        serde_json::to_vec(hydro_erosion.as_ref()).unwrap(),
        hydro_erosion_before
    );
}
