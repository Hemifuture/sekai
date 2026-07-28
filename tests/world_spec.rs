use sekai::world::{
    BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed, SpecError, TechnologyBaseline, WorldSpec,
    MAX_CELL_COUNT, MAX_DIMENSION_METERS, MIN_CELL_COUNT, MIN_DIMENSION_METERS,
    WORLD_SPEC_SCHEMA_V1,
};

fn valid_spec() -> WorldSpec {
    WorldSpec {
        schema_version: WORLD_SPEC_SCHEMA_V1,
        root_seed: RootSeed::new(42),
        space: PlanarSpaceSpec {
            width: Meters::new(20_000_000.0).unwrap(),
            height: Meters::new(10_000_000.0).unwrap(),
            target_cell_count: 80_000,
            boundary: BoundaryCondition::Closed,
        },
        technology: TechnologyBaseline::PreIndustrialMedieval,
    }
}

#[test]
fn accepts_the_supported_planar_spec() {
    assert!(valid_spec().validate().is_ok());
}

#[test]
fn rejects_unsupported_schema() {
    let mut spec = valid_spec();
    spec.schema_version = 2;
    assert_eq!(
        spec.validate(),
        Err(SpecError::UnsupportedSchema {
            found: 2,
            supported: WORLD_SPEC_SCHEMA_V1,
        })
    );
}

#[test]
fn accepts_inclusive_dimension_boundaries() {
    for dimension in [MIN_DIMENSION_METERS, MAX_DIMENSION_METERS] {
        let mut space = valid_spec().space;
        space.width = Meters::new(dimension).unwrap();
        space.height = Meters::new(dimension).unwrap();
        assert!(space.validate().is_ok());
    }
}

#[test]
fn rejects_dimensions_outside_the_safety_range() {
    for dimension in [MIN_DIMENSION_METERS / 2.0, MAX_DIMENSION_METERS * 2.0] {
        let mut space = valid_spec().space;
        space.width = Meters::new(dimension).unwrap();
        assert!(matches!(
            space.validate(),
            Err(SpecError::DimensionOutOfRange { .. })
        ));
    }
}

#[test]
fn accepts_inclusive_cell_count_boundaries() {
    for target_cell_count in [MIN_CELL_COUNT, MAX_CELL_COUNT] {
        let mut space = valid_spec().space;
        space.target_cell_count = target_cell_count;
        assert!(space.validate().is_ok());
    }
}

#[test]
fn rejects_cell_counts_outside_the_safety_range() {
    for target_cell_count in [MIN_CELL_COUNT - 1, MAX_CELL_COUNT + 1] {
        let mut space = valid_spec().space;
        space.target_cell_count = target_cell_count;
        assert!(matches!(
            space.validate(),
            Err(SpecError::CellCountOutOfRange { .. })
        ));
    }
}

#[test]
fn accepts_inclusive_aspect_ratio_boundaries() {
    for (width, height) in [(1.0, 16.0), (16.0, 1.0)] {
        let mut space = valid_spec().space;
        space.width = Meters::new(width).unwrap();
        space.height = Meters::new(height).unwrap();
        assert!(space.validate().is_ok());
    }
}

#[test]
fn planar_space_rejects_extreme_aspect_ratios_directly() {
    let mut space = valid_spec().space;
    space.width = Meters::new(1_000.0).unwrap();
    space.height = Meters::new(1.0).unwrap();
    assert!(matches!(
        space.validate(),
        Err(SpecError::AspectRatioOutOfRange { .. })
    ));
}
