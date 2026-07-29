use std::collections::BTreeMap;

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, GeologicArtifact, GeologicSpecArtifact,
    MantleArtifact, ReliefArtifact, RulePackSetArtifact, TectonicArtifact, TectonicSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::view::{
    prepare_cell_field, DisplayRangeMode, FieldCatalog, FieldDisplayState, FieldPayloadRef,
};
use sekai::world::fields::{FieldDomain, FieldPaletteHint, FieldValueType, MissingValuePolicy};
use sekai::world::natural::{
    bedrock_kind_field_id, boundary_kind_field_id, boundary_strength_field_id,
    crust_base_elevation_field_id, crust_kind_field_id, crust_thickness_field_id,
    elevation_field_id, erosion_resistance_field_id, fracture_intensity_field_id,
    geothermal_potential_field_id, land_ocean_field_id, mantle_heat_flow_field_id,
    metallic_mineral_potential_field_id, natural_field_registry, plate_id_field_id,
    plate_velocity_field_id, regional_offset_field_id, relative_permeability_field_id,
    sedimentary_basin_potential_field_id, tectonic_offset_field_id, volcanic_influence_field_id,
    volcanic_offset_field_id, GeologicSpec, NaturalFieldDisplayCache, NaturalFieldRegistryError,
    TectonicSpec, ELEVATION_MAX_M, ELEVATION_MIN_M, HEAT_FLOW_MAX_MW_M2, HEAT_FLOW_MIN_MW_M2,
    MAX_PLATE_COUNT, VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
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
    assert_eq!(registry.len(), 21);
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
    assert_eq!(elevation.display.palette(), FieldPaletteHint::Diverging);
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

    let tectonic_before = serde_json::to_vec(tectonic.as_ref()).unwrap();
    let mantle_before = serde_json::to_vec(mantle.as_ref()).unwrap();
    let relief_before = serde_json::to_vec(relief.as_ref()).unwrap();
    let geology_before = serde_json::to_vec(geology.as_ref()).unwrap();
    let mut state = FieldDisplayState::default();
    for id in [
        plate_id_field_id(),
        crust_kind_field_id(),
        elevation_field_id(),
        bedrock_kind_field_id(),
        geothermal_potential_field_id(),
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
}
