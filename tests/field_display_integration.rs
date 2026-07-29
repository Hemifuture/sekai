use std::collections::BTreeMap;
use std::sync::Arc;

use sekai::view::{
    built_in_palette, prepare_cell_field, CellGeometrySource, DisplayPrepareError,
    DisplayRangeMode, DisplayRevision, DisplayRevisionClock, DisplayRevisions, DisplayStatusError,
    FieldDisplayResourceState, FieldView, LinearRgba, MeshCompleteness, PaletteId,
    PreparedCellField, PreparedCellMesh, PreparedDiagnosticMask, PreparedFieldDisplay,
    ResolvedDisplayRange,
};
use sekai::world::fields::{
    DomainSizes, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain, FieldId,
    FieldPaletteHint, FieldRegistryBuilder, FieldSchema, FieldUnit, FieldValueType,
    MissingValuePolicy, ValueRange,
};
use sekai::world::{CellId, Meters, WorldPoint, WorldRect};

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(Meters::new(x).unwrap(), Meters::new(y).unwrap())
}

struct FourCellGeometry {
    bounds: WorldRect,
    polygons: Vec<Vec<WorldPoint>>,
}

impl FourCellGeometry {
    fn new() -> Self {
        Self {
            bounds: WorldRect::new(point(0.0, 0.0), point(2.0, 2.0)).unwrap(),
            polygons: vec![
                vec![
                    point(0.0, 0.0),
                    point(1.0, 0.0),
                    point(1.0, 1.0),
                    point(0.0, 1.0),
                ],
                vec![
                    point(1.0, 0.0),
                    point(2.0, 0.0),
                    point(2.0, 1.0),
                    point(1.0, 1.0),
                ],
                vec![
                    point(0.0, 1.0),
                    point(1.0, 1.0),
                    point(1.0, 2.0),
                    point(0.0, 2.0),
                ],
                vec![
                    point(1.0, 1.0),
                    point(2.0, 1.0),
                    point(2.0, 2.0),
                    point(1.0, 2.0),
                ],
            ],
        }
    }
}

impl CellGeometrySource for FourCellGeometry {
    fn bounds(&self) -> WorldRect {
        self.bounds
    }

    fn cell_count(&self) -> usize {
        self.polygons.len()
    }

    fn polygon(&self, cell: CellId) -> Option<&[WorldPoint]> {
        self.polygons.get(cell.raw() as usize).map(Vec::as_slice)
    }
}

fn field_id(name: &str) -> FieldId {
    FieldId::new("test.packet", name, 1).unwrap()
}

fn prepared_scalar(values: &[f32]) -> PreparedCellField {
    let id = field_id("scalar");
    let schema = FieldSchema {
        id: id.clone(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::ScalarF32,
        unit: FieldUnit::Unitless,
        valid_range: Some(ValueRange::new(0.0, 1.0).unwrap()),
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new("field.test.scalar", FieldPaletteHint::Sequential, 2)
            .unwrap(),
    };
    let mut builder = FieldRegistryBuilder::new();
    builder.register(schema).unwrap();
    let registry = builder.build().unwrap();
    let mut fields = ExtensionFieldSet::new();
    fields
        .insert(
            &registry,
            id.clone(),
            FieldData::ScalarF32(values.to_vec()),
            &DomainSizes::new(values.len(), 0),
        )
        .unwrap();
    let view = FieldView::new(registry.get(&id).unwrap(), fields.get(&id).unwrap()).unwrap();
    prepare_cell_field(&view, values.len(), DisplayRangeMode::Schema).unwrap()
}

fn prepared_category(values: &[u32]) -> PreparedCellField {
    let id = field_id("category");
    let schema = FieldSchema {
        id: id.clone(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::CategoryU32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::from([
            (2, "field.test.category.two".into()),
            (9, "field.test.category.nine".into()),
        ]),
        display: FieldDisplayMetadata::new("field.test.category", FieldPaletteHint::Categorical, 0)
            .unwrap(),
    };
    let mut builder = FieldRegistryBuilder::new();
    builder.register(schema).unwrap();
    let registry = builder.build().unwrap();
    let mut fields = ExtensionFieldSet::new();
    fields
        .insert(
            &registry,
            id.clone(),
            FieldData::CategoryU32(values.to_vec()),
            &DomainSizes::new(values.len(), 0),
        )
        .unwrap();
    let view = FieldView::new(registry.get(&id).unwrap(), fields.get(&id).unwrap()).unwrap();
    prepare_cell_field(&view, values.len(), DisplayRangeMode::Data).unwrap()
}

fn four_cell_mesh() -> Arc<PreparedCellMesh> {
    Arc::new(
        PreparedCellMesh::build(&FourCellGeometry::new(), MeshCompleteness::RequireAll).unwrap(),
    )
}

fn palette(id: PaletteId) -> Arc<[LinearRgba]> {
    Arc::from(built_in_palette(id))
}

fn revisions(mesh: u64, field: u64, diagnostics: u64, palette: u64) -> DisplayRevisions {
    DisplayRevisions::new(
        DisplayRevision::new(mesh).unwrap(),
        DisplayRevision::new(field).unwrap(),
        DisplayRevision::new(diagnostics).unwrap(),
        DisplayRevision::new(palette).unwrap(),
    )
}

fn valid_render_packet() -> PreparedFieldDisplay {
    PreparedFieldDisplay::new(
        four_cell_mesh(),
        Arc::new(prepared_scalar(&[0.0, 0.25, 0.75, 1.0])),
        Arc::new(PreparedDiagnosticMask::empty(4)),
        palette(PaletteId::Sequential),
        revisions(1, 1, 1, 1),
        true,
    )
    .unwrap()
}

#[test]
fn render_packet_requires_matching_cell_lengths() {
    let mesh = four_cell_mesh();
    let field = Arc::new(prepared_scalar(&[0.0, 0.5, 1.0]));
    let diagnostics = Arc::new(PreparedDiagnosticMask::empty(4));

    assert!(matches!(
        PreparedFieldDisplay::new(
            mesh,
            field,
            diagnostics,
            palette(PaletteId::Sequential),
            revisions(1, 1, 1, 1),
            true,
        ),
        Err(DisplayPrepareError::CellCountMismatch {
            expected: 4,
            actual: 3,
        })
    ));
}

#[test]
fn failed_replacement_keeps_last_complete_packet() {
    let valid = Arc::new(valid_render_packet());
    let mut resource = FieldDisplayResourceState::new(valid.clone());
    let error = DisplayPrepareError::NoFiniteScalarValues {
        field: field_id("broken"),
    };

    resource.reject_prepare(error.clone());

    assert!(Arc::ptr_eq(resource.current().unwrap(), &valid));
    assert_eq!(resource.error(), Some(&DisplayStatusError::Prepare(error)));
}

#[test]
fn range_changes_keep_buffer_revisions_and_share_the_raw_field_arc() {
    let first = valid_render_packet();
    let second = first.with_display_range(ResolvedDisplayRange::new(-1.0, 1.0).unwrap());

    assert_eq!(first.revisions(), second.revisions());
    assert!(Arc::ptr_eq(first.mesh_arc(), second.mesh_arc()));
    assert!(Arc::ptr_eq(first.field_arc(), second.field_arc()));
    assert_eq!(second.display_range().unwrap().bounds(), (-1.0, 1.0));
}

#[test]
fn category_packets_ignore_scalar_range_changes_without_cloning_values() {
    let first = PreparedFieldDisplay::new(
        four_cell_mesh(),
        Arc::new(prepared_category(&[2, 9, 2, 9])),
        Arc::new(PreparedDiagnosticMask::empty(4)),
        palette(PaletteId::Categorical),
        revisions(1, 2, 1, 2),
        true,
    )
    .unwrap();
    let second = first.with_display_range(ResolvedDisplayRange::new(0.0, 1.0).unwrap());

    assert_eq!(second.display_range(), None);
    assert!(Arc::ptr_eq(first.field_arc(), second.field_arc()));
}

#[test]
fn packet_rejects_invalid_palettes_and_revisions_are_non_zero_monotonic() {
    assert_eq!(
        DisplayRevision::new(0),
        Err(DisplayPrepareError::ZeroRevision)
    );
    let mut clock = DisplayRevisionClock::default();
    assert_eq!(clock.issue().unwrap().get(), 1);
    assert_eq!(clock.issue().unwrap().get(), 2);

    let invalid_palette: Arc<[LinearRgba]> = Arc::from([LinearRgba::new(f32::NAN, 0.0, 0.0, 1.0)]);
    assert!(matches!(
        PreparedFieldDisplay::new(
            four_cell_mesh(),
            Arc::new(prepared_scalar(&[0.0, 0.25, 0.75, 1.0])),
            Arc::new(PreparedDiagnosticMask::empty(4)),
            invalid_palette,
            revisions(1, 1, 1, 1),
            true,
        ),
        Err(DisplayPrepareError::InvalidPalette)
    ));
}

#[test]
fn runtime_status_codes_are_validated_without_clearing_the_current_packet() {
    let valid = Arc::new(valid_render_packet());
    let mut resource = FieldDisplayResourceState::new(valid.clone());

    assert_eq!(
        resource.reject_runtime("Display GPU", "device lost"),
        Err(DisplayPrepareError::InvalidStatusCode)
    );
    assert_eq!(resource.error(), None);
    resource
        .reject_runtime("display.gpu", "device lost")
        .unwrap();
    assert!(Arc::ptr_eq(resource.current().unwrap(), &valid));
    assert_eq!(
        resource.error(),
        Some(&DisplayStatusError::Runtime {
            code: "display.gpu".into(),
            message: "device lost".into(),
        })
    );
}
