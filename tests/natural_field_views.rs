use std::collections::BTreeMap;

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, GeologicSpecArtifact, ReliefArtifact,
    RulePackSetArtifact, TectonicArtifact, TectonicSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::view::{
    prepare_cell_field, DisplayRangeMode, FieldCatalog, FieldDisplayState, FieldPayloadRef,
};
use sekai::world::fields::{FieldDomain, FieldPaletteHint, FieldValueType, MissingValuePolicy};
use sekai::world::natural::{
    boundary_kind_field_id, boundary_strength_field_id, crust_base_elevation_field_id,
    crust_kind_field_id, crust_thickness_field_id, elevation_field_id, land_ocean_field_id,
    natural_field_registry, plate_id_field_id, plate_velocity_field_id, regional_offset_field_id,
    tectonic_offset_field_id, GeologicSpec, NaturalFieldDisplayCache, NaturalFieldRegistryError,
    TectonicSpec, ELEVATION_MAX_M, ELEVATION_MIN_M, MAX_PLATE_COUNT,
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
    assert_eq!(registry.len(), 11);
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
        ]
    );
    assert_eq!(
        schema(&first, land_ocean_field_id()).dependencies,
        vec![elevation_field_id()]
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

fn natural_artifacts() -> (
    std::sync::Arc<SpatialArtifact>,
    std::sync::Arc<TectonicArtifact>,
    std::sync::Arc<ReliefArtifact>,
) {
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
    (
        outcome.artifacts.get::<SpatialArtifact>().unwrap(),
        outcome.artifacts.get::<TectonicArtifact>().unwrap(),
        outcome.artifacts.get::<ReliefArtifact>().unwrap(),
    )
}

#[test]
fn borrowed_natural_payloads_match_every_registered_domain() {
    let (spatial, tectonic, relief) = natural_artifacts();
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
    let relief_before = serde_json::to_vec(relief.as_ref()).unwrap();
    let mut state = FieldDisplayState::default();
    for id in [
        plate_id_field_id(),
        crust_kind_field_id(),
        elevation_field_id(),
    ] {
        state.select_field(id);
        state.reconcile(&catalog, spatial.snapshot().cell_count());
    }
    assert_eq!(
        serde_json::to_vec(tectonic.as_ref()).unwrap(),
        tectonic_before
    );
    assert_eq!(serde_json::to_vec(relief.as_ref()).unwrap(), relief_before);
}
