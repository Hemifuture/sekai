use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sekai::view::{
    built_in_palette, prepare_cell_field, rasterize_reference, CellDiagnosticRef,
    CellGeometrySource, DiagnosticScope, DisplayRangeMode, DisplayRevisionClock, DisplayRevisions,
    FieldCatalog, MeshCompleteness, PaletteId, PreparedCellMesh, PreparedDiagnosticMask,
    PreparedFieldDisplay, ViewDiagnosticSeverity,
};
use sekai::world::fields::{
    DomainSizes, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain, FieldId,
    FieldPaletteHint, FieldRegistryBuilder, FieldSchema, FieldUnit, FieldValueType,
    MissingValuePolicy, ValueRange,
};
use sekai::world::{CellId, Meters, WorldPoint, WorldRect};

const GOLDEN_WIDTH: u32 = 128;
const GOLDEN_HEIGHT: u32 = 64;

#[test]
fn scalar_golden_matches() {
    assert_golden(
        "scalar.png",
        &scalar_packet(false),
        GOLDEN_WIDTH,
        GOLDEN_HEIGHT,
    );
}

#[test]
fn category_golden_matches() {
    assert_golden(
        "category.png",
        &category_packet(),
        GOLDEN_WIDTH,
        GOLDEN_HEIGHT,
    );
}

#[test]
fn diagnostic_golden_matches() {
    assert_golden(
        "diagnostic.png",
        &scalar_packet(true),
        GOLDEN_WIDTH,
        GOLDEN_HEIGHT,
    );
}

#[test]
#[ignore = "writes reviewed field-display golden PNGs"]
fn regenerate_field_goldens() {
    assert_eq!(
        std::env::var("SEKAI_UPDATE_FIELD_GOLDENS").as_deref(),
        Ok("1")
    );
    write_golden(
        "scalar.png",
        &scalar_packet(false),
        GOLDEN_WIDTH,
        GOLDEN_HEIGHT,
    );
    write_golden(
        "category.png",
        &category_packet(),
        GOLDEN_WIDTH,
        GOLDEN_HEIGHT,
    );
    write_golden(
        "diagnostic.png",
        &scalar_packet(true),
        GOLDEN_WIDTH,
        GOLDEN_HEIGHT,
    );
}

fn assert_golden(name: &str, packet: &PreparedFieldDisplay, width: u32, height: u32) {
    let actual = rasterize_reference(packet, width, height).unwrap();
    let expected = image::ImageReader::open(golden_path(name))
        .unwrap()
        .decode()
        .unwrap()
        .into_rgba8();
    let actual_hash = blake3::hash(actual.rgba8());
    let expected_hash = blake3::hash(expected.as_raw());
    assert_eq!(
        (actual.width(), actual.height()),
        expected.dimensions(),
        "{name}: dimension mismatch; actual={actual_hash}, expected={expected_hash}"
    );
    assert_eq!(
        actual.rgba8(),
        expected.as_raw(),
        "{name}: pixel mismatch; actual={actual_hash}, expected={expected_hash}"
    );
}

fn write_golden(name: &str, packet: &PreparedFieldDisplay, width: u32, height: u32) {
    let image = rasterize_reference(packet, width, height).unwrap();
    let path = golden_path(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::save_buffer_with_format(
        path,
        image.rgba8(),
        image.width(),
        image.height(),
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("field-display")
        .join(name)
}

fn scalar_packet(with_diagnostics: bool) -> PreparedFieldDisplay {
    let field_id = FieldId::new("test.golden", "scalar", 1).unwrap();
    let schema = FieldSchema {
        id: field_id.clone(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::ScalarF32,
        unit: FieldUnit::Unitless,
        valid_range: Some(ValueRange::new(0.0, 1.0).unwrap()),
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new(
            "field.test.golden.scalar",
            FieldPaletteHint::Sequential,
            2,
        )
        .unwrap(),
    };
    packet(
        schema,
        FieldData::ScalarF32(vec![0.0, 0.35, 0.7, 1.0]),
        PaletteId::Sequential,
        with_diagnostics.then_some(field_id),
    )
}

fn category_packet() -> PreparedFieldDisplay {
    let labels = BTreeMap::from([
        (10, "field.test.category.ten".into()),
        (20, "field.test.category.twenty".into()),
        (30, "field.test.category.thirty".into()),
        (40, "field.test.category.forty".into()),
    ]);
    let schema = FieldSchema {
        id: FieldId::new("test.golden", "category", 1).unwrap(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::CategoryU32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: labels,
        display: FieldDisplayMetadata::new(
            "field.test.golden.category",
            FieldPaletteHint::Categorical,
            0,
        )
        .unwrap(),
    };
    packet(
        schema,
        FieldData::CategoryU32(vec![10, 20, 30, 40]),
        PaletteId::Categorical,
        None,
    )
}

fn packet(
    schema: FieldSchema,
    data: FieldData,
    palette: PaletteId,
    diagnostic_field: Option<FieldId>,
) -> PreparedFieldDisplay {
    let field_id = schema.id.clone();
    let mut registry = FieldRegistryBuilder::new();
    registry.register(schema).unwrap();
    let registry = registry.build().unwrap();
    let mut fields = ExtensionFieldSet::new();
    fields
        .insert(&registry, field_id.clone(), data, &DomainSizes::new(4, 0))
        .unwrap();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
    let view = catalog.get(&field_id).unwrap().view().unwrap();
    let field = Arc::new(prepare_cell_field(view, 4, DisplayRangeMode::Schema).unwrap());
    let diagnostics = diagnostic_field.as_ref().map_or_else(
        || PreparedDiagnosticMask::empty(4),
        |diagnostic_field| {
            PreparedDiagnosticMask::build(
                4,
                [
                    CellDiagnosticRef {
                        severity: ViewDiagnosticSeverity::Info,
                        code: "test.info",
                        field_id: Some(diagnostic_field),
                        cell_id: Some(CellId::from_raw(1)),
                        message: "info",
                    },
                    CellDiagnosticRef {
                        severity: ViewDiagnosticSeverity::Warning,
                        code: "test.warning",
                        field_id: Some(diagnostic_field),
                        cell_id: Some(CellId::from_raw(2)),
                        message: "warning",
                    },
                    CellDiagnosticRef {
                        severity: ViewDiagnosticSeverity::Error,
                        code: "test.error",
                        field_id: Some(diagnostic_field),
                        cell_id: Some(CellId::from_raw(3)),
                        message: "error",
                    },
                ],
                Some(diagnostic_field),
                DiagnosticScope::SelectedField,
            )
            .unwrap()
        },
    );
    let mut clock = DisplayRevisionClock::default();
    let revisions = DisplayRevisions::new(
        clock.issue().unwrap(),
        clock.issue().unwrap(),
        clock.issue().unwrap(),
        clock.issue().unwrap(),
    );
    PreparedFieldDisplay::new(
        Arc::new(
            PreparedCellMesh::build(&FourCellGeometry::new(), MeshCompleteness::RequireAll)
                .unwrap(),
        ),
        field,
        Arc::new(diagnostics),
        Arc::from(built_in_palette(palette)),
        revisions,
        diagnostic_field.is_some(),
    )
    .unwrap()
}

struct FourCellGeometry {
    bounds: WorldRect,
    polygons: [Vec<WorldPoint>; 4],
}

impl FourCellGeometry {
    fn new() -> Self {
        Self {
            bounds: WorldRect::new(point(0.0, 0.0), point(2.0, 1.0)).unwrap(),
            polygons: [
                square(0.0, 0.0, 1.0, 0.5),
                square(1.0, 0.0, 2.0, 0.5),
                square(0.0, 0.5, 1.0, 1.0),
                square(1.0, 0.5, 2.0, 1.0),
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

fn square(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<WorldPoint> {
    vec![
        point(min_x, min_y),
        point(max_x, min_y),
        point(max_x, max_y),
        point(min_x, max_y),
    ]
}

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(Meters::new(x).unwrap(), Meters::new(y).unwrap())
}
