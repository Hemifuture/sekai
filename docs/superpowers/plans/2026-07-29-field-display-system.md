# Independent Field Display System V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a renderer-neutral, GPU-backed field observer that displays validated cell scalar and category fields, diagnostics, legends, and selected-cell values without coupling world generation to egui or wgpu.

**Architecture:** `view` borrows validated `world` contracts and prepares immutable display data; `gpu::field` owns reusable wgpu buffers and rendering; `ui::field` owns egui controls; `app` alone adapts the legacy terrain demo and build diagnostics. The renderer indexes one raw field value per cell from stable mesh cell IDs, so range and palette changes do not rebuild geometry or field arrays.

**Tech Stack:** Rust 1.85, serde, thiserror, egui/eframe 0.31, wgpu 24 through eframe, bytemuck, image 0.25 and pollster 0.4 as test-only dependencies, BLAKE3, native and wasm32 targets.

## Global Constraints

- Follow `docs/superpowers/specs/2026-07-29-field-display-system-design.md`.
- `src/view` may import only `std` and `crate::world`; it must not import engine, generators, terrain, models, app, ui, gpu, egui, eframe, wgpu, image, or bytemuck.
- `src/world`, `src/engine`, and `src/generators` must not import `crate::view`, egui, eframe, or wgpu.
- `src/gpu/field` and `src/ui/field` must not import engine, generators, terrain, or legacy models.
- `app` is the only composition root allowed to adapt `MapSystem`, `BuildReport`, egui state, and GPU resources.
- Do not create `WorldSnapshot`, history, events, time controls, vector/network/entity placeholder APIs, editing commands, storage, or town integrations.
- V1 map fill supports only `FieldDomain::Cells` with `ScalarF32` or `CategoryU32`; all other domain/type pairs remain inspectable and return an explicit unsupported-fill result.
- World schemas retain semantic palette hints only. RGBA values, display ranges, normalized values, revisions, and GPU buffers never enter `world`.
- `MAX_DISPLAY_CELLS = 200_000`, `MAX_DISPLAY_VERTICES = 6_000_000`, and `MAX_DISPLAY_INDICES = 12_000_000`.
- Static frames may update small canvas uniforms, but must not rebuild or upload geometry, field values, diagnostic masks, or palettes.
- A failed prepare or upload keeps the last complete render packet; never publish partial display state.
- Preserve the existing app, Delaunay/Voronoi overlays, terrain generation behavior, native build, wasm build, and Trunk bundle.
- Use deterministic ordered collections and checked integer/byte conversions in serialized or externally observable display preparation.
- Use TDD for every task: demonstrate the intended failing test before writing implementation.

---

## Target File Map

### Renderer-neutral view

- `src/view/mod.rs` — public exports only.
- `src/view/field.rs` — borrowed field values, catalog, support matrix, and errors.
- `src/view/palette.rs` — display ranges, built-in palettes, CPU reference color sampling, and prepared cell field values.
- `src/view/diagnostics.rs` — engine-neutral diagnostic references and per-cell masks.
- `src/view/state.rs` — field selection, display preferences, reconciliation, and value formatting.
- `src/view/mesh.rs` — normalized cell mesh, budgets, legacy-compatible geometry source, and cell picking.
- `src/view/prepared.rs` — revisions and atomically validated render packets.
- `src/view/reference.rs` — deterministic CPU triangle rasterizer used only by golden verification.

### GPU

- `src/gpu/field/mod.rs` — private GPU exports.
- `src/gpu/field/planner.rs` — pure revision-to-upload planning.
- `src/gpu/field/renderer.rs` — reusable buffers, checked uploads, uniforms, and indexed draw.
- `src/gpu/field/callback.rs` — egui-wgpu callback reading prepared resources.
- `assets/shaders/field_fill.wgsl` — generic scalar/category/diagnostic shader.

### UI and app adapter

- `src/ui/field/mod.rs` — field UI exports.
- `src/ui/field/controls.rs` — field, range, palette, and diagnostic controls.
- `src/ui/field/inspector.rs` — legend and selected-cell inspector.
- `src/app/legacy_display.rs` — private one-way adapter from current `MapSystem` outputs.
- `src/app.rs` — composition, renderer resource creation, generation result capture, and panels.
- `src/ui/canvas/canvas.rs` — carries the prepared display resource.
- `src/ui/canvas/widget_impl.rs` — replaces the old height callback with field callback.
- `src/resource/mod.rs` — field renderer and prepared display resource aliases.
- `src/gpu/mod.rs`, `src/ui/mod.rs`, `src/lib.rs` — module declarations.

### Removed legacy fill path

- `src/gpu/heightmap/heightmap_callback.rs`
- `src/gpu/heightmap/heightmap_renderer.rs`
- `src/gpu/heightmap/mod.rs`
- `assets/shaders/heightmap.wgsl`

### Tests and CI

- `tests/field_view_contracts.rs`
- `tests/field_display_palette.rs`
- `tests/field_display_diagnostics.rs`
- `tests/field_display_mesh.rs`
- `tests/field_display_integration.rs`
- `tests/field_display_golden.rs`
- `tests/golden/field-display/scalar.png`
- `tests/golden/field-display/category.png`
- `tests/golden/field-display/diagnostic.png`
- `.github/workflows/rust.yml`
- `Cargo.toml`, `Cargo.lock`

---

### Task 1: Add Borrowed Field Views and a Stable Catalog

**Files:**

- Modify: `src/world/fields/data.rs`
- Create: `src/view/mod.rs`
- Create: `src/view/field.rs`
- Modify: `src/lib.rs`
- Create: `tests/field_view_contracts.rs`

**Interfaces:**

- Consumes: `FieldRegistry::iter`, `ExtensionFieldSet::get`, `FieldSchema`, `FieldData`, `FieldDomain`, `FieldValueType`, and `StableIdKind`.
- Produces:

```rust
pub enum FieldValue {
    Scalar(f32),
    Category(u32),
    Boolean(bool),
    Vector2([f32; 2]),
    StableId { target: StableIdKind, value: u32 },
}

pub enum CellFillKind {
    Scalar,
    Category,
}

pub struct FieldView<'a>;
pub struct FieldCatalogEntry<'a>;
pub struct FieldCatalog<'a>;

impl<'a> FieldView<'a> {
    pub fn new(
        schema: &'a FieldSchema,
        data: &'a FieldData,
    ) -> Result<Self, FieldViewError>;
    pub fn schema(&self) -> &'a FieldSchema;
    pub fn len(&self) -> usize;
    pub fn value(&self, index: usize) -> Option<FieldValue>;
    pub fn scalar_values(&self) -> Option<&'a [f32]>;
    pub fn category_values(&self) -> Option<&'a [u32]>;
    pub fn vector_values(&self) -> Option<&'a [[f32; 2]]>;
    pub fn stable_id_values(&self) -> Option<(StableIdKind, &'a [u32])>;
    pub fn cell_fill_kind(&self) -> Result<CellFillKind, FieldViewError>;
}

impl<'a> FieldCatalog<'a> {
    pub fn from_extension_fields(
        registry: &'a FieldRegistry,
        fields: &'a ExtensionFieldSet,
    ) -> Result<Self, FieldViewError>;
    pub fn entries(&self) -> &[FieldCatalogEntry<'a>];
    pub fn get(&self, id: &FieldId) -> Option<&FieldCatalogEntry<'a>>;
    pub fn first_renderable(&self) -> Option<&FieldCatalogEntry<'a>>;
}
```

`FieldCatalogEntry` exposes `schema()` and `view() -> Option<&FieldView>`. A missing payload is a valid entry with `view() == None`.

- [ ] **Step 1: Expose safe payload cardinality**

Add public read-only methods to `FieldData`:

```rust
impl FieldData {
    pub fn len(&self) -> usize {
        match self {
            Self::ScalarF32(values) => values.len(),
            Self::CategoryU32(values) => values.len(),
            Self::Boolean(values) => values.len(),
            Self::Vector2F32(values) => values.len(),
            Self::StableIdU32 { values, .. } => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
```

Keep the payload collections private behind the existing enum variants; do not add mutation methods.

- [ ] **Step 2: Write failing catalog and value-access tests**

Create `tests/field_view_contracts.rs` with fixtures for five field types. The key assertions must be:

```rust
#[test]
fn catalog_is_id_ordered_and_keeps_missing_registered_fields() {
    let (registry, fields) = fixture_with_one_missing_payload();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
    let ids: Vec<_> = catalog
        .entries()
        .iter()
        .map(|entry| entry.schema().id.clone())
        .collect();

    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert!(catalog
        .get(&field_id("missing"))
        .unwrap()
        .view()
        .is_none());
}

#[test]
fn field_view_reads_every_supported_payload_without_copying() {
    let (registry, fields) = complete_fixture();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();

    assert_eq!(
        catalog.get(&field_id("scalar")).unwrap().view().unwrap().value(1),
        Some(FieldValue::Scalar(0.75))
    );
    assert_eq!(
        catalog.get(&field_id("category")).unwrap().view().unwrap().value(0),
        Some(FieldValue::Category(7))
    );
    assert_eq!(
        catalog.get(&field_id("boolean")).unwrap().view().unwrap().value(1),
        Some(FieldValue::Boolean(false))
    );
    assert_eq!(
        catalog.get(&field_id("vector")).unwrap().view().unwrap().value(0),
        Some(FieldValue::Vector2([1.0, -2.0]))
    );
    assert_eq!(
        catalog.get(&field_id("stable")).unwrap().view().unwrap().value(1),
        Some(FieldValue::StableId {
            target: StableIdKind::Cell,
            value: 0,
        })
    );
}

#[test]
fn cell_fill_support_matrix_is_explicit() {
    let (registry, fields) = complete_fixture();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();

    assert_eq!(
        catalog.get(&field_id("scalar")).unwrap().view().unwrap().cell_fill_kind(),
        Ok(CellFillKind::Scalar)
    );
    assert_eq!(
        catalog.get(&field_id("category")).unwrap().view().unwrap().cell_fill_kind(),
        Ok(CellFillKind::Category)
    );
    assert!(matches!(
        catalog.get(&field_id("vector")).unwrap().view().unwrap().cell_fill_kind(),
        Err(FieldViewError::UnsupportedCellFill { .. })
    ));
}
```

Construct schemas through `FieldRegistryBuilder` and payloads through `ExtensionFieldSet::insert`; do not bypass validation in fixtures.

- [ ] **Step 3: Run the focused test and observe the missing module failure**

Run:

```powershell
cargo test --test field_view_contracts
```

Expected: compilation fails because `sekai::view` does not exist.

- [ ] **Step 4: Implement the minimal borrowed contracts**

In `field.rs`, validate that schema value type and `FieldData` variant agree in `FieldView::new`. Define:

```rust
#[derive(Debug, Clone, PartialEq, Error)]
pub enum FieldViewError {
    #[error("field {field:?} payload type does not match {expected:?}")]
    TypeMismatch {
        field: FieldId,
        expected: FieldValueType,
    },
    #[error("field {field:?} with domain {domain:?} and type {value_type:?} cannot fill cells in display V1")]
    UnsupportedCellFill {
        field: FieldId,
        domain: FieldDomain,
        value_type: FieldValueType,
    },
}
```

`FieldCatalog::from_extension_fields` must iterate the registry, not the payload set. When data exists, call `FieldView::new`; when absent, retain the schema with no view. `first_renderable` returns the first entry whose view returns a successful `cell_fill_kind`.

Export the module from `lib.rs`:

```rust
/// Renderer-neutral, read-only world presentation contracts.
pub mod view;
```

- [ ] **Step 5: Verify Task 1**

Run:

```powershell
cargo test --test field_view_contracts
cargo test --test field_contracts
cargo fmt --all -- --check
cargo clippy --lib --tests -- -D warnings
```

Expected: all commands exit 0.

- [ ] **Step 6: Commit Task 1**

```powershell
git add src/world/fields/data.rs src/view/mod.rs src/view/field.rs src/lib.rs tests/field_view_contracts.rs
git commit -m "feat: add borrowed field view catalog"
```

---

### Task 2: Resolve Display Ranges, Palettes, and Prepared Cell Values

**Files:**

- Create: `src/view/palette.rs`
- Modify: `src/view/mod.rs`
- Create: `tests/field_display_palette.rs`

**Interfaces:**

- Consumes: `FieldView`, `CellFillKind`, `FieldSchema.valid_range`, `FieldPaletteHint`, `ValueRange`, and category labels.
- Produces:

```rust
pub struct LinearRgba([f32; 4]);

pub enum PaletteId {
    Sequential,
    Diverging,
    Categorical,
}

pub enum DisplayRangeMode {
    Schema,
    Data,
    Manual(ValueRange),
}

pub struct ResolvedDisplayRange {
    min: f32,
    max: f32,
}

impl ResolvedDisplayRange {
    pub fn new(min: f32, max: f32) -> Result<Self, DisplayPrepareError>;
    pub fn bounds(self) -> (f32, f32);
}

pub enum DisplayPrepareError {
    InvalidRange,
    MissingSchemaRange { field: FieldId },
    NoFiniteScalarValues { field: FieldId },
    CellCountMismatch { expected: usize, actual: usize },
    UnsupportedCellFill { field: FieldId },
    UnknownCategory { field: FieldId, key: u32 },
}

pub enum PreparedFieldKind {
    Scalar,
    Category,
}

pub struct PreparedCellField {
    field_id: FieldId,
    kind: PreparedFieldKind,
    raw_values: Vec<u32>,
    source_range: Option<ResolvedDisplayRange>,
    display_range: Option<ResolvedDisplayRange>,
    category_keys: Vec<u32>,
}

pub fn prepare_cell_field(
    field: &FieldView<'_>,
    expected_cells: usize,
    range_mode: DisplayRangeMode,
) -> Result<PreparedCellField, DisplayPrepareError>;

pub fn scalar_color(
    raw_value: f32,
    range: ResolvedDisplayRange,
    palette: &[LinearRgba],
) -> LinearRgba;

pub fn category_color(compact_index: u32, palette: &[LinearRgba]) -> LinearRgba;
```

- [ ] **Step 1: Write failing range and palette tests**

Create tests covering exact endpoint and constant behavior:

```rust
#[test]
fn schema_data_and_manual_ranges_resolve_explicitly() {
    let field = scalar_field_view(&[-2.0, 0.0, 8.0], Some((-10.0, 10.0)));

    assert_eq!(
        resolve_display_range(&field, DisplayRangeMode::Schema).unwrap().bounds(),
        (-10.0, 10.0)
    );
    assert_eq!(
        resolve_display_range(&field, DisplayRangeMode::Data).unwrap().bounds(),
        (-2.0, 8.0)
    );
    assert_eq!(
        resolve_display_range(
            &field,
            DisplayRangeMode::Manual(ValueRange::new(-1.0, 1.0).unwrap())
        )
        .unwrap()
        .bounds(),
        (-1.0, 1.0)
    );
}

#[test]
fn constant_scalar_fields_sample_the_palette_midpoint() {
    let range = ResolvedDisplayRange::new(4.0, 4.0).unwrap();
    let palette = built_in_palette(PaletteId::Sequential);
    assert_eq!(scalar_color(4.0, range, palette), sample_palette(palette, 0.5));
}

#[test]
fn prepared_scalars_store_ieee_bits_and_categories_store_sorted_compact_indices() {
    let scalar = prepare_cell_field(
        &scalar_field_view(&[0.25, 0.75], Some((0.0, 1.0))),
        2,
        DisplayRangeMode::Schema,
    )
    .unwrap();
    assert_eq!(scalar.raw_values(), &[0.25_f32.to_bits(), 0.75_f32.to_bits()]);

    let category = prepare_cell_field(
        &category_field_view(&[9, 4, 9], &[(4, "cat.four"), (9, "cat.nine")]),
        3,
        DisplayRangeMode::Data,
    )
    .unwrap();
    assert_eq!(category.category_keys(), &[4, 9]);
    assert_eq!(category.raw_values(), &[1, 0, 1]);
}

#[test]
fn field_prepare_rejects_length_and_unsupported_fill_without_partial_output() {
    assert!(matches!(
        prepare_cell_field(&scalar_field_view(&[1.0], None), 2, DisplayRangeMode::Data),
        Err(DisplayPrepareError::CellCountMismatch { expected: 2, actual: 1 })
    ));
    assert!(matches!(
        prepare_cell_field(&vector_field_view(&[[1.0, 0.0]]), 1, DisplayRangeMode::Data),
        Err(DisplayPrepareError::UnsupportedCellFill { .. })
    ));
}
```

- [ ] **Step 2: Run the focused test and confirm missing symbols**

Run:

```powershell
cargo test --test field_display_palette
```

Expected: compilation fails because palette and prepared-field types do not exist.

- [ ] **Step 3: Implement validated ranges**

`ResolvedDisplayRange::new` rejects non-finite or reversed bounds. Equal bounds are valid and normalize every equal value to `0.5`.

`resolve_display_range` rules:

```rust
match mode {
    DisplayRangeMode::Schema => schema
        .valid_range
        .map(ResolvedDisplayRange::from)
        .ok_or(DisplayPrepareError::MissingSchemaRange { field }),
    DisplayRangeMode::Data => finite_min_max(values)
        .ok_or(DisplayPrepareError::NoFiniteScalarValues { field }),
    DisplayRangeMode::Manual(range) => Ok(range.into()),
}
```

Although validated extension scalar values are finite, keep `finite_min_max` defensive for legacy adapter inputs and direct unit tests.

- [ ] **Step 4: Implement exact built-in palette behavior**

Use immutable built-in linear-RGBA tables. Define at least five sequential stops, five diverging stops, and twelve categorical colors. `sample_palette` clamps `t` to `0..=1`, maps to adjacent stops, and linearly interpolates each channel.

Category preparation:

1. collect schema category keys in their existing `BTreeMap` order;
2. build a `BTreeMap<u32, u32>` from key to compact index;
3. translate every payload key;
4. reject any missing mapping as `UnknownCategory`;
5. `category_color` uses `compact_index % palette.len()`.

Do not store RGBA in `FieldSchema`.

- [ ] **Step 5: Verify Task 2**

Run:

```powershell
cargo test --test field_display_palette
cargo test --test field_view_contracts
cargo fmt --all -- --check
cargo clippy --lib --tests -- -D warnings
```

Expected: all commands exit 0.

- [ ] **Step 6: Commit Task 2**

```powershell
git add src/view/palette.rs src/view/mod.rs tests/field_display_palette.rs
git commit -m "feat: prepare deterministic field colors"
```

---

### Task 3: Add Display State, Diagnostics, and Value Inspection

**Files:**

- Create: `src/view/diagnostics.rs`
- Create: `src/view/state.rs`
- Modify: `src/view/mod.rs`
- Create: `tests/field_display_diagnostics.rs`

**Interfaces:**

- Consumes: `FieldCatalog`, `FieldView`, `FieldDisplayMetadata`, `FieldUnit`, `CellId`, and `FieldId`.
- Produces:

```rust
pub enum ViewDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

pub struct CellDiagnosticRef<'a> {
    pub severity: ViewDiagnosticSeverity,
    pub code: &'a str,
    pub field_id: Option<&'a FieldId>,
    pub cell_id: Option<CellId>,
    pub message: &'a str,
}

pub struct OwnedViewDiagnostic {
    pub severity: ViewDiagnosticSeverity,
    pub code: String,
    pub field_id: Option<FieldId>,
    pub cell_id: Option<CellId>,
    pub message: String,
}

impl OwnedViewDiagnostic {
    pub fn as_ref(&self) -> CellDiagnosticRef<'_>;
}

pub struct PreparedDiagnosticMask {
    cells: Vec<u32>,
}

pub enum DiagnosticScope {
    SelectedField,
    AllFields,
}

pub struct FieldDisplayState {
    selected_field: Option<FieldId>,
    range_mode: DisplayRangeMode,
    palette_override: Option<PaletteId>,
    diagnostics_enabled: bool,
    diagnostic_scope: DiagnosticScope,
    selected_cell: Option<CellId>,
}

pub struct FormattedFieldValue {
    pub raw: FieldValue,
    pub text: String,
    pub unit: String,
    pub category_label_key: Option<String>,
}
```

- [ ] **Step 1: Write failing diagnostic and state tests**

Create:

```rust
#[test]
fn diagnostic_mask_keeps_highest_severity_and_respects_field_scope() {
    let selected = field_id("elevation");
    let other = field_id("rainfall");
    let diagnostics = [
        diagnostic(ViewDiagnosticSeverity::Info, Some(&selected), Some(0)),
        diagnostic(ViewDiagnosticSeverity::Warning, Some(&selected), Some(0)),
        diagnostic(ViewDiagnosticSeverity::Error, Some(&other), Some(1)),
        diagnostic(ViewDiagnosticSeverity::Warning, None, Some(2)),
    ];

    let selected_mask = PreparedDiagnosticMask::build(
        3,
        diagnostics,
        Some(&selected),
        DiagnosticScope::SelectedField,
    )
    .unwrap();
    assert_eq!(selected_mask.cells(), &[2, 0, 2]);

    let all_mask = PreparedDiagnosticMask::build(
        3,
        diagnostics,
        Some(&selected),
        DiagnosticScope::AllFields,
    )
    .unwrap();
    assert_eq!(all_mask.cells(), &[2, 3, 2]);
}

#[test]
fn state_reconciliation_uses_first_renderable_field_and_clears_invalid_cell() {
    let (registry, fields) = fixture_with_renderable_fields();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
    let mut state = FieldDisplayState::default();
    state.select_field(field_id("removed"));
    state.select_cell(CellId::from_raw(99));

    state.reconcile(&catalog, 4);

    assert_eq!(
        state.selected_field(),
        Some(&catalog.first_renderable().unwrap().schema().id)
    );
    assert_eq!(state.selected_cell(), None);
}

#[test]
fn inspector_uses_schema_precision_units_and_category_labels() {
    let field = custom_unit_scalar_view(12.3456, 2, "m");
    let formatted = format_field_value(&field, 0).unwrap();
    assert_eq!(formatted.text, "12.35");
    assert_eq!(formatted.unit, "m");

    let category = labeled_category_view(7, "biome.temperate_forest");
    let formatted = format_field_value(&category, 0).unwrap();
    assert_eq!(
        formatted.category_label_key.as_deref(),
        Some("biome.temperate_forest")
    );
}
```

Diagnostic mask encoding is exact: `0=None`, `1=Info`, `2=Warning`, `3=Error`.
Extend `DisplayPrepareError` with
`DiagnosticCellOutOfRange { cell: CellId, cell_count: usize }`.

- [ ] **Step 2: Run the focused test and confirm missing modules**

Run:

```powershell
cargo test --test field_display_diagnostics
```

Expected: compilation fails for missing diagnostic and state symbols.

- [ ] **Step 3: Implement engine-neutral diagnostics**

`PreparedDiagnosticMask::build` validates every cell ID against `cell_count`. Global diagnostics with no cell remain available to UI lists but do not set a cell mask. Under `SelectedField`, include:

- diagnostics with no field ID;
- diagnostics whose field ID equals the selected field.

Use numeric maximum for severity merging. Return `DiagnosticCellOutOfRange` instead of indexing unchecked.

- [ ] **Step 4: Implement deterministic state reconciliation and formatting**

State defaults:

```rust
Self {
    selected_field: None,
    range_mode: DisplayRangeMode::Data,
    palette_override: None,
    diagnostics_enabled: true,
    diagnostic_scope: DiagnosticScope::SelectedField,
    selected_cell: None,
}
```

When selection changes:

- choose schema range when present, otherwise data range;
- clear an incompatible palette override;
- keep the selected cell if it remains below `cell_count`.

Formatting:

- scalar uses exactly `display.decimal_places()`;
- category uses the schema localization key;
- boolean is `"true"` or `"false"`;
- vector is `"[x, y]"` with schema precision;
- stable ID is the decimal raw ID;
- custom units use their symbol; unitless uses an empty string.

- [ ] **Step 5: Verify Task 3**

Run:

```powershell
cargo test --test field_display_diagnostics
cargo test --test field_display_palette
cargo test --test field_view_contracts
cargo fmt --all -- --check
cargo clippy --lib --tests -- -D warnings
```

Expected: all commands exit 0.

- [ ] **Step 6: Commit Task 3**

```powershell
git add src/view/diagnostics.rs src/view/state.rs src/view/mod.rs tests/field_display_diagnostics.rs
git commit -m "feat: add field diagnostics and inspection state"
```

---

### Task 4: Prepare Bounded Cell Meshes and Deterministic Picking

**Files:**

- Create: `src/view/mesh.rs`
- Modify: `src/view/mod.rs`
- Create: `tests/field_display_mesh.rs`

**Interfaces:**

- Consumes: `SpatialSnapshot`, `Topology`, `WorldRect`, `WorldPoint`, and stable `CellId`.
- Produces:

```rust
pub const MAX_DISPLAY_CELLS: usize = 200_000;
pub const MAX_DISPLAY_VERTICES: usize = 6_000_000;
pub const MAX_DISPLAY_INDICES: usize = 12_000_000;

pub trait CellGeometrySource {
    fn bounds(&self) -> WorldRect;
    fn cell_count(&self) -> usize;
    fn polygon(&self, cell: CellId) -> Option<&[WorldPoint]>;
}

pub enum MeshCompleteness {
    RequireAll,
    AllowMissing,
}

pub struct DisplayVertex {
    pub position: [f32; 2],
    pub cell: u32,
}

pub struct PreparedCellMesh {
    bounds: WorldRect,
    local_extent: [f32; 2],
    cell_count: usize,
    vertices: Vec<DisplayVertex>,
    indices: Vec<u32>,
    picker: CellPicker,
}

impl PreparedCellMesh {
    pub fn build(
        source: &impl CellGeometrySource,
        completeness: MeshCompleteness,
    ) -> Result<Self, DisplayPrepareError>;
    pub fn pick_normalized(&self, normalized: [f32; 2]) -> Option<CellId>;
    pub fn pick_local(&self, local: [f32; 2]) -> Option<CellId>;
}
```

Implement `CellGeometrySource` for `SpatialSnapshot` through the public `Topology` methods.

- [ ] **Step 1: Write failing mesh tests**

Use a four-cell `SpatialSnapshot` fixture with world bounds offset from zero:

```rust
#[test]
fn spatial_snapshot_builds_stable_normalized_mesh() {
    let snapshot = four_cell_fixture_with_bounds(1_000_000.0, -2_000_000.0);
    let first = PreparedCellMesh::build(&snapshot, MeshCompleteness::RequireAll).unwrap();
    let second = PreparedCellMesh::build(&snapshot, MeshCompleteness::RequireAll).unwrap();

    assert_eq!(first.vertices(), second.vertices());
    assert_eq!(first.indices(), second.indices());
    assert_eq!(first.cell_count(), 4);
    assert!(first
        .vertices()
        .iter()
        .all(|vertex| (0.0..=1.0).contains(&vertex.position[0])
            && (0.0..=1.0).contains(&vertex.position[1])));
}

#[test]
fn mesh_keeps_cell_ids_aligned_with_field_indices() {
    let mesh = PreparedCellMesh::build(
        &four_cell_fixture(),
        MeshCompleteness::RequireAll,
    )
    .unwrap();
    for triangle in mesh.indices().chunks_exact(3) {
        let cells: Vec<_> = triangle
            .iter()
            .map(|index| mesh.vertices()[*index as usize].cell)
            .collect();
        assert_eq!(cells[0], cells[1]);
        assert_eq!(cells[1], cells[2]);
    }
}

#[test]
fn picker_returns_exact_cells_and_none_outside_bounds() {
    let mesh = PreparedCellMesh::build(
        &four_cell_fixture(),
        MeshCompleteness::RequireAll,
    )
    .unwrap();
    assert_eq!(
        mesh.pick_normalized([0.25, 0.25]),
        Some(CellId::from_raw(0))
    );
    assert_eq!(
        mesh.pick_normalized([0.75, 0.75]),
        Some(CellId::from_raw(3))
    );
    assert_eq!(mesh.pick_normalized([-0.01, 0.5]), None);
    assert_eq!(
        mesh.pick_local([mesh.local_extent()[0] * 0.25, mesh.local_extent()[1] * 0.25]),
        Some(CellId::from_raw(0))
    );
}

#[test]
fn mesh_rejects_missing_geometry_and_budget_overflow() {
    let missing = missing_cell_source();
    assert!(matches!(
        PreparedCellMesh::build(&missing, MeshCompleteness::RequireAll),
        Err(DisplayPrepareError::MissingCellGeometry { cell }) if cell == CellId::from_raw(1)
    ));

    let oversized = oversized_polygon_source(MAX_DISPLAY_VERTICES + 1);
    assert!(matches!(
        PreparedCellMesh::build(&oversized, MeshCompleteness::RequireAll),
        Err(DisplayPrepareError::VertexBudgetExceeded { .. })
    ));
}
```

- [ ] **Step 2: Run the focused test and confirm missing mesh types**

Run:

```powershell
cargo test --test field_display_mesh
```

Expected: compilation fails because `PreparedCellMesh` is not defined.

- [ ] **Step 3: Implement checked normalization and triangulation**

For every present polygon:

1. check cell count and cumulative vertices before allocation;
2. require at least three finite points;
3. normalize in `f64`:

```rust
let x = (point.x().get() - bounds.min().x().get()) / bounds.width().get();
let y = (point.y().get() - bounds.min().y().get()) / bounds.height().get();
```

4. reject normalized coordinates outside `-1e-6..=1.0+1e-6`;
5. append one `DisplayVertex` per polygon vertex with the source `CellId`;
6. append fan indices `(base, base + i, base + i + 1)`;
7. check every `usize -> u32` conversion;
8. check global vertex and index budgets before mutation.

`AllowMissing` skips only absent polygons; it still rejects malformed present polygons and preserves the original cell count.
Store `local_extent = [bounds.width, bounds.height]` after checked finite
`f64 -> f32` conversion. Vertices remain normalized; never add the potentially
large world origin back into GPU coordinates. Extend `DisplayPrepareError` with
explicit invalid-bounds, malformed-polygon, coordinate-conversion, cell-budget,
vertex-budget, index-budget, and integer-overflow variants.

- [ ] **Step 4: Implement bounded deterministic picking**

Create a square bin grid with:

```rust
let side = (cell_count as f64).sqrt().ceil().clamp(1.0, 512.0) as usize;
```

Insert each cell ID into every bin intersected by its normalized AABB, in ascending cell order. Deduplicate each bucket and retain ascending IDs. `pick_normalized`:

1. rejects points outside `0..=1`;
2. computes one bin;
3. tests candidates in ascending order with a half-open ray crossing rule;
4. returns the first containing cell.

`pick_local` rejects non-finite or out-of-extent coordinates, divides by
`local_extent`, and delegates to `pick_normalized`.

This makes shared-boundary selection deterministic.

- [ ] **Step 5: Verify Task 4**

Run:

```powershell
cargo test --test field_display_mesh
cargo test --test spatial_contracts
cargo fmt --all -- --check
cargo clippy --lib --tests -- -D warnings
```

Expected: all commands exit 0.

- [ ] **Step 6: Commit Task 4**

```powershell
git add src/view/mesh.rs src/view/mod.rs tests/field_display_mesh.rs
git commit -m "feat: prepare bounded field display meshes"
```

---

### Task 5: Publish Atomic Render Packets with Explicit Revisions

**Files:**

- Create: `src/view/prepared.rs`
- Modify: `src/view/mod.rs`
- Extend: `tests/field_display_integration.rs`

**Interfaces:**

- Consumes: `PreparedCellMesh`, `PreparedCellField`, `PreparedDiagnosticMask`, `ResolvedDisplayRange`, and built-in palette tables.
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRevision(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRevisions {
    pub mesh: DisplayRevision,
    pub field: DisplayRevision,
    pub diagnostics: DisplayRevision,
    pub palette: DisplayRevision,
}

pub struct DisplayRevisionClock {
    next: u64,
}

pub struct PreparedFieldDisplay {
    mesh: Arc<PreparedCellMesh>,
    field: Arc<PreparedCellField>,
    diagnostics: Arc<PreparedDiagnosticMask>,
    palette: Arc<[LinearRgba]>,
    revisions: DisplayRevisions,
    diagnostics_enabled: bool,
}

impl PreparedFieldDisplay {
    pub fn mesh(&self) -> &PreparedCellMesh;
    pub fn field(&self) -> &PreparedCellField;
    pub fn diagnostics(&self) -> &PreparedDiagnosticMask;
    pub fn palette(&self) -> &[LinearRgba];
    pub fn revisions(&self) -> DisplayRevisions;
    pub fn with_display_range(&self, range: ResolvedDisplayRange) -> Self;
}

pub enum DisplayStatusError {
    Prepare(DisplayPrepareError),
    Runtime { code: String, message: String },
}

pub struct FieldDisplayResourceState {
    current: Option<Arc<PreparedFieldDisplay>>,
    error: Option<DisplayStatusError>,
}
```

- [ ] **Step 1: Write failing atomicity and revision tests**

Create `tests/field_display_integration.rs`:

```rust
#[test]
fn render_packet_requires_matching_cell_lengths() {
    let mesh = Arc::new(four_cell_mesh());
    let field = Arc::new(prepared_scalar(&[0.0, 0.5, 1.0]));
    let diagnostics = Arc::new(PreparedDiagnosticMask::empty(4));

    assert!(matches!(
        PreparedFieldDisplay::new(
            mesh,
            field,
            diagnostics,
            sequential_palette(),
            revisions(1, 1, 1, 1),
            true,
        ),
        Err(DisplayPrepareError::CellCountMismatch { expected: 4, actual: 3 })
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
    assert_eq!(
        resource.error(),
        Some(&DisplayStatusError::Prepare(error))
    );
}

#[test]
fn range_changes_do_not_change_any_buffer_revision() {
    let first = valid_render_packet();
    let second =
        first.with_display_range(ResolvedDisplayRange::new(-1.0, 1.0).unwrap());

    assert_eq!(first.revisions(), second.revisions());
}
```

- [ ] **Step 2: Run the focused test and confirm missing prepared types**

Run:

```powershell
cargo test --test field_display_integration
```

Expected: compilation fails because prepared packet types do not exist.

- [ ] **Step 3: Implement non-zero monotonic revisions**

`DisplayRevision::new` rejects zero. `DisplayRevisionClock` starts at one and uses checked addition; overflow returns `DisplayPrepareError::RevisionOverflow`.
Extend `DisplayPrepareError` with `RevisionOverflow`, `ZeroRevision`, and
`InvalidPalette` variants.

The revision is reporting and invalidation state only. Do not serialize it or include it in world/build hashes.

- [ ] **Step 4: Implement all-or-nothing packet validation**

`PreparedFieldDisplay::new` checks:

- mesh cell count equals field value count;
- mesh cell count equals diagnostic mask length;
- palette is non-empty and every color component is finite in `0..=1`;
- scalar packets have a resolved display range;
- category packets have no scalar display range requirement;
- all revisions are non-zero.

`FieldDisplayResourceState::replace` receives a fully constructed packet and clears an obsolete status error. `reject_prepare` stores `DisplayStatusError::Prepare` without clearing `current`. `reject_runtime(code, message)` validates a lowercase machine code and stores `DisplayStatusError::Runtime` without making `view` depend on a GPU error type.

- [ ] **Step 5: Verify Task 5**

Run:

```powershell
cargo test --test field_display_integration
cargo test --test field_display_mesh
cargo test --test field_display_palette
cargo test --test field_display_diagnostics
cargo fmt --all -- --check
cargo clippy --lib --tests -- -D warnings
```

Expected: all commands exit 0.

- [ ] **Step 6: Commit Task 5**

```powershell
git add src/view/prepared.rs src/view/mod.rs tests/field_display_integration.rs
git commit -m "feat: publish atomic field render packets"
```

---

### Task 6: Implement the Reusable GPU Cell Field Renderer

**Files:**

- Create: `src/gpu/field/mod.rs`
- Create: `src/gpu/field/planner.rs`
- Create: `src/gpu/field/renderer.rs`
- Create: `assets/shaders/field_fill.wgsl`
- Modify: `src/gpu/mod.rs`

**Interfaces:**

- Consumes: `PreparedFieldDisplay`, `DisplayRevisions`, `DisplayVertex`, `CanvasUniforms`, wgpu device/queue/render pass.
- Produces:

```rust
pub struct UploadPlan {
    pub mesh: bool,
    pub field: bool,
    pub diagnostics: bool,
    pub palette: bool,
}

pub struct RendererUploadStats {
    pub geometry_uploads: u64,
    pub field_uploads: u64,
    pub diagnostic_uploads: u64,
    pub palette_uploads: u64,
    pub uniform_updates: u64,
    pub uploaded_bytes: u64,
}

pub struct CellFieldRenderer;

impl CellFieldRenderer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self;
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &PreparedFieldDisplay,
        canvas: &CanvasUniforms,
    ) -> Result<(), FieldRenderError>;
    pub fn render(&self, pass: &mut wgpu::RenderPass<'static>);
    pub fn stats(&self) -> &RendererUploadStats;
}
```

- [ ] **Step 1: Write failing pure upload-planner unit tests**

In `planner.rs` add tests before implementation:

```rust
#[test]
fn identical_revisions_upload_nothing() {
    let revisions = revisions(4, 5, 6, 7);
    assert_eq!(
        UploadPlan::between(Some(revisions), revisions),
        UploadPlan::none()
    );
}

#[test]
fn palette_revision_change_uploads_only_palette_data() {
    let current = revisions(4, 5, 6, 7);
    let next = revisions(4, 5, 6, 8);
    assert_eq!(
        UploadPlan::between(Some(current), next),
        UploadPlan {
            mesh: false,
            field: false,
            diagnostics: false,
            palette: true,
        }
    );
}

#[test]
fn mesh_change_forces_all_indexed_inputs_to_upload() {
    let current = revisions(1, 2, 3, 4);
    let next = revisions(5, 2, 3, 4);
    let plan = UploadPlan::between(Some(current), next);
    assert!(plan.mesh);
    assert!(plan.field);
    assert!(plan.diagnostics);
    assert!(plan.palette);
}
```

Mesh replacement forces dependent buffers because the cell cardinality may change.

- [ ] **Step 2: Run the library test and confirm missing GPU field module**

Run:

```powershell
cargo test --lib gpu::field
```

Expected: compilation fails because `gpu::field` is not declared.

- [ ] **Step 3: Implement GPU layouts and checked buffer growth**

Use a GPU-only interleaved vertex:

```rust
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuCellVertex {
    position: [f32; 2],
    cell: u32,
    padding: u32,
}
```

Pack scalar values as `f32::to_bits()` and category values as compact indices into one `u32` storage buffer.

Create storage buffers with at least 16 bytes. A helper:

```rust
fn checked_buffer_bytes<T>(len: usize) -> Result<u64, FieldRenderError> {
    len.checked_mul(std::mem::size_of::<T>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(FieldRenderError::BufferSizeOverflow)
}
```

Grow capacity to `required.next_power_of_two()` after checked arithmetic, capped by the spec budgets. Reuse existing buffers when capacity suffices.

- [ ] **Step 4: Implement shader and uniforms**

Bindings:

1. `array<GpuCellVertex>` storage;
2. `array<u32>` raw field storage;
3. `array<u32>` diagnostic mask storage;
4. `array<vec4<f32>>` palette storage;
5. uniform containing canvas transform, local extent, display min/max, field kind, base palette length, diagnostics enabled, diagnostic color offsets, and padding.

The vertex shader must:

```wgsl
let vertex = vertices[vertex_index];
let local_position = vertex.position * uniforms.local_extent;
let raw = field_values[vertex.cell];
var color: vec4<f32>;
if uniforms.field_kind == 0u {
    let value = bitcast<f32>(raw);
    let width = uniforms.display_max - uniforms.display_min;
    var t = 0.5;
    if width > 0.0 {
        t = clamp((value - uniforms.display_min) / width, 0.0, 1.0);
    }
    color = sample_palette(t);
} else {
    color = palette[raw % uniforms.palette_len];
}
color = apply_diagnostic(color, diagnostics[vertex.cell], uniforms.diagnostics_enabled);
```

Feed `local_position` through the existing `CanvasUniforms` transform. The
canvas camera therefore operates in origin-shifted local coordinates and never
receives the large world minimum.

Use the exact `LinearRgba` diagnostic constants from `view::palette`. Build one
combined palette buffer: base palette entries first, then Info, Warning, and
Error at offsets `palette_len`, `palette_len + 1`, and `palette_len + 2`.
`apply_diagnostic` indexes those suffix entries; WGSL contains no independent
diagnostic color literals.

- [ ] **Step 5: Implement revision-based uploads and stats**

On successful `prepare`:

- calculate `UploadPlan`;
- preflight every size conversion, capacity growth, replacement buffer, uniform,
  and counter increment before issuing the first queue write;
- update only selected buffers after the entire preflight succeeds;
- update canvas/field uniform every call;
- commit precomputed counter values;
- record revisions only after all writes have been issued;
- keep the previous revisions and drawable state when preflight fails.

`render` binds the index buffer as `Uint32` and calls `draw_indexed(0..index_count, 0, 0..1)`.

- [ ] **Step 6: Verify Task 6**

Run:

```powershell
cargo test --lib gpu::field
cargo check --all-features
cargo fmt --all -- --check
cargo clippy --lib --tests -- -D warnings
```

Expected: planner tests pass and the shader-backed renderer compiles.

- [ ] **Step 7: Commit Task 6**

```powershell
git add src/gpu/field src/gpu/mod.rs assets/shaders/field_fill.wgsl
git commit -m "feat: render generic GPU cell fields"
```

---

### Task 7: Add the Field Callback, Controls, Legend, and Inspector

**Files:**

- Create: `src/gpu/field/callback.rs`
- Modify: `src/gpu/field/mod.rs`
- Create: `src/ui/field/mod.rs`
- Create: `src/ui/field/controls.rs`
- Create: `src/ui/field/inspector.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/resource/mod.rs`
- Modify: `src/ui/canvas/canvas.rs`
- Modify: `src/ui/canvas/widget_impl.rs`
- Add unit tests beside `src/ui/field/controls.rs` and `src/ui/field/inspector.rs`

**Interfaces:**

- Consumes: `FieldCatalog`, `FieldDisplayState`, `FieldDisplayResourceState`, `CellFieldRenderer`, and `CanvasStateResource`.
- Produces:

```rust
pub type FieldRendererResource = Resource<CellFieldRenderer>;
pub type FieldDisplayResource = Resource<FieldDisplayResourceState>;
pub type FieldViewerStateResource = Resource<FieldDisplayState>;

pub enum FieldControlAction {
    SelectField(FieldId),
    SetRangeMode(DisplayRangeMode),
    SetPaletteOverride(Option<PaletteId>),
    SetDiagnosticsEnabled(bool),
    SetDiagnosticScope(DiagnosticScope),
}

pub struct FieldFillCallback;
pub fn show_field_controls(
    ui: &mut egui::Ui,
    catalog: &FieldCatalog<'_>,
    state: &FieldDisplayState,
) -> Vec<FieldControlAction>;
pub fn show_field_inspector(
    ui: &mut egui::Ui,
    catalog: &FieldCatalog<'_>,
    state: &FieldDisplayState,
    diagnostics: &[CellDiagnosticRef<'_>],
);
```

- [ ] **Step 1: Write failing pure compatibility and UI action tests**

Test the non-egui decision helpers:

```rust
#[test]
fn palette_choices_match_field_schema_hint() {
    assert_eq!(
        compatible_palettes(FieldPaletteHint::Sequential),
        &[PaletteId::Sequential]
    );
    assert_eq!(
        compatible_palettes(FieldPaletteHint::Diverging),
        &[PaletteId::Diverging]
    );
    assert_eq!(
        compatible_palettes(FieldPaletteHint::Categorical),
        &[PaletteId::Categorical]
    );
}

#[test]
fn missing_and_unsupported_fields_have_explicit_status_text() {
    assert_eq!(
        field_status_text(&missing_entry()),
        "已注册，但当前快照没有字段数据"
    );
    assert_eq!(
        field_status_text(&vector_cell_entry()),
        "V1 可检查该字段，但不支持单元格填色"
    );
}
```

Run one egui context test that invokes both controls and inspector with a four-cell fixture and asserts no panic:

```rust
#[test]
fn controls_and_inspector_render_fixture_without_mutating_fields() {
    let before = serde_json::to_vec(&fields).unwrap();
    let ctx = egui::Context::default();
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            show_field_controls(ui, &catalog, &state);
            show_field_inspector(ui, &catalog, &state, &diagnostics);
        });
    });
    assert_eq!(serde_json::to_vec(&fields).unwrap(), before);
}
```

The test fixture helper owns `registry`, `fields`, `state`, and diagnostics for
the full `egui::Context::run` call; the borrowed catalog never escapes it.

- [ ] **Step 2: Run UI library tests and confirm missing module**

Run:

```powershell
cargo test --lib ui::field
```

Expected: compilation fails because `ui::field` does not exist.

- [ ] **Step 3: Implement callback without rebuilding display data**

`FieldFillCallback::prepare`:

1. reads `FieldDisplayResourceState.current`;
2. returns without drawing when absent;
3. calls `CellFieldRenderer::prepare` with the immutable packet and current canvas uniform;
4. maps `FieldRenderError` to `DisplayStatusError::Runtime` with code `display.gpu` rather than unwrapping.

Clone the packet `Arc` while holding the display-resource read lock, then drop
that lock before mutating the renderer. If rendering preparation fails, drop
the renderer lock before taking the display-resource write lock used to publish
the runtime status. This fixes lock order and avoids callback deadlocks.

It must not access `MapSystem`, `TerrainGenerator`, `FieldRegistry`, or `ExtensionFieldSet`.

`paint` only calls renderer `render`.

- [ ] **Step 4: Implement controls as actions**

The controls:

- list catalog entries in stable order;
- use label key with full `namespace.name@version` fallback;
- disable map-fill selection for missing/unsupported entries while leaving them inspectable;
- expose only compatible palettes;
- expose Schema range only when `valid_range.is_some()`;
- emit actions instead of mutating world data.

No control invokes a generator.

- [ ] **Step 5: Implement legend and selected-cell inspector**

The legend displays:

- scalar display min, midpoint, max and unit;
- category entries in sorted key order, first 256 only, plus `共 N 项，仅显示前 256 项`;
- current schema range and current display range;
- selected cell raw/formatted value;
- matching diagnostics ordered Error, Warning, Info, then code.

Do not render a provenance panel in V1.

- [ ] **Step 6: Connect the callback to Canvas**

Add `FieldDisplayResource` and `FieldViewerStateResource` to `Canvas::new`. In
`widget_impl.rs`, place `FieldFillCallback` at the former height-fill z-order.
Give the canvas `Sense::click_and_drag`; on a primary click, transform the
pointer from screen coordinates through `CanvasState::to_canvas`, call
`packet.mesh().pick_local`, and store the result through
`FieldViewerStateResource`. Keep Delaunay, Voronoi, and points callbacks
unchanged.

At this task the display resource may be empty; Task 8 supplies legacy data.

- [ ] **Step 7: Verify Task 7**

Run:

```powershell
cargo test --lib ui::field
cargo test --lib gpu::field
cargo check --all-features
cargo fmt --all -- --check
cargo clippy --lib --tests -- -D warnings
```

Expected: all commands exit 0 and an empty display resource is a supported state.

- [ ] **Step 8: Commit Task 7**

```powershell
git add src/gpu/field src/ui/field src/ui/mod.rs src/resource/mod.rs src/ui/canvas
git commit -m "feat: add field viewer controls and callback"
```

---

### Task 8: Adapt Legacy Height and Plate Outputs, Then Remove the Duplicate Fill Path

**Files:**

- Create: `src/app/legacy_display.rs`
- Modify: `src/app.rs`
- Modify: `src/resource/mod.rs`
- Modify: `src/gpu/mod.rs`
- Delete: `src/gpu/heightmap/heightmap_callback.rs`
- Delete: `src/gpu/heightmap/heightmap_renderer.rs`
- Delete: `src/gpu/heightmap/mod.rs`
- Delete: `assets/shaders/heightmap.wgsl`
- Extend: `tests/field_display_integration.rs`

**Interfaces:**

- Consumes: current `MapSystem.voronoi`, `MapSystem.bounds`, generated `Vec<u8>` heights, generated `Vec<u16>` plate IDs, and new view preparation APIs.
- Produces:

```rust
struct LegacyTerrainDisplay {
    registry: FieldRegistry,
    fields: ExtensionFieldSet,
    mesh: Arc<PreparedCellMesh>,
    diagnostics: Vec<OwnedViewDiagnostic>,
}

impl LegacyTerrainDisplayAdapter {
    fn build(
        bounds: WorldRect,
        voronoi: &IndexedVoronoiDiagram,
        heights: &[u8],
        plate_ids: &[u16],
    ) -> Result<LegacyTerrainDisplay, LegacyDisplayError>;
}
```

The adapter remains private under `app`.

- [ ] **Step 1: Write failing legacy adapter tests**

In `legacy_display.rs`:

```rust
#[test]
fn adapter_exposes_height_and_plate_fields_with_valid_schemas() {
    let (bounds, voronoi) = four_cell_legacy_geometry();
    let heights = vec![0, 64, 128, 255];
    let plates = vec![2, 2, 9, 9];
    let display =
        LegacyTerrainDisplayAdapter::build(bounds, &voronoi, &heights, &plates).unwrap();
    let catalog = FieldCatalog::from_extension_fields(&display.registry, &display.fields).unwrap();

    let elevation = catalog.get(&legacy_elevation_id()).unwrap().view().unwrap();
    assert_eq!(elevation.scalar_values(), Some(&[0.0, 64.0, 128.0, 255.0][..]));
    assert_eq!(elevation.schema().valid_range.unwrap().min(), 0.0);
    assert_eq!(elevation.schema().valid_range.unwrap().max(), 255.0);

    let plate = catalog.get(&legacy_plate_id()).unwrap().view().unwrap();
    assert_eq!(plate.category_values(), Some(&[2, 2, 9, 9][..]));
    assert_eq!(
        plate.schema().category_labels.keys().copied().collect::<Vec<_>>(),
        vec![2, 9]
    );
}

#[test]
fn adapter_rejects_mismatched_generation_outputs_atomically() {
    let (bounds, voronoi) = four_cell_legacy_geometry();
    assert!(matches!(
        LegacyTerrainDisplayAdapter::build(bounds, &voronoi, &[1, 2], &[0]),
        Err(LegacyDisplayError::OutputLengthMismatch { .. })
    ));
}
```

`four_cell_legacy_geometry` must construct an actual `IndexedVoronoiDiagram` with four `VoronoiCell` records and shared vertex coordinates; it must not mock the final display packet.

- [ ] **Step 2: Run adapter tests and confirm missing adapter**

Run:

```powershell
cargo test --lib app::legacy_display
```

Expected: compilation fails because the submodule does not exist.

- [ ] **Step 3: Implement validated legacy schemas and geometry**

Exact IDs:

```rust
FieldId::new("sekai.legacy", "elevation", 1)
FieldId::new("sekai.legacy", "plate_id", 1)
```

Elevation:

- `FieldDomain::Cells`;
- `FieldValueType::ScalarF32`;
- unitless because the old `u8` values are display values, not authoritative meters;
- range `0..=255`;
- sequential palette;
- label key `field.legacy.elevation`;
- zero decimals.

Plate IDs:

- `FieldDomain::Cells`;
- `FieldValueType::CategoryU32`;
- sorted unique category label map `plate.<raw>`;
- categorical palette;
- label key `field.legacy.plate_id`.

Convert legacy Voronoi vertices to finite `WorldPoint` values and build with `MeshCompleteness::AllowMissing`. Record one warning per omitted malformed cell, capped at 64 plus one summary warning.

Use a private
`LegacyCellGeometry { bounds, polygons: Vec<Option<Vec<WorldPoint>>> }` that
implements `CellGeometrySource`. Treat each `VoronoiCell.site_idx` as its stable
field index, reject duplicates and out-of-range indices, and leave an absent
slot as `None`. This adapter is the only layer that knows the legacy Voronoi
layout.

- [ ] **Step 4: Integrate generation results into TemplateApp**

Add serde-skipped fields for:

- optional legacy display document;
- `FieldViewerStateResource`;
- `FieldDisplayResource`;
- `FieldRendererResource`;
- last generated plate IDs.

Create and register `FieldRendererResource` with egui's callback resources in
the same composition location that currently registers
`HeightmapRendererResource`. The left panel and canvas receive clones of the
same `FieldViewerStateResource`; do not keep a second UI-only state.

`LegacyTerrainDisplay` owns its registry, extension payloads, prepared mesh, and
owned diagnostics. Build the borrowed `FieldCatalog` only within each panel or
packet-update call; never store a self-referential catalog.

When terrain generation succeeds:

1. retain heights in existing `CellsData` for compatibility;
2. pass heights and plate IDs to the adapter;
3. build a catalog and reconcile selection;
4. prepare the selected field and diagnostic mask;
5. construct a complete packet with new revisions;
6. replace the field display resource;
7. on failure, retain the previous packet and show the error.

Add the field controls and inspector to the left panel. Keep the map generation and legacy overlay toggles.

After `show_field_controls`, process returned actions in the app composition
layer:

- field selection prepares a new `PreparedCellField`, advances only the field
  revision, and reuses the mesh;
- display-range changes replace only the packet's uniform range and do not
  advance any buffer revision;
- palette changes advance only the palette revision;
- the diagnostics-enabled toggle changes only a uniform flag;
- diagnostic-scope changes rebuild the mask and advance only the diagnostics
  revision.

All action handling constructs a complete candidate packet before atomically
replacing the current one.

- [ ] **Step 5: Remove the old fill renderer only after new integration passes**

Remove:

- all `HeightmapRendererResource` fields and registrations;
- `HeightmapCallback`;
- `HeightmapRenderer`;
- `gpu::heightmap`;
- `heightmap.wgsl`.

Rename `LayerVisibility.heightmap` to `cell_fill` and the UI label from `高度图` to `字段填色`, preserving default-on behavior. `MapSystemResource` is serde-skipped in `TemplateApp`, so no persisted field alias is needed.

- [ ] **Step 6: Verify Task 8**

Run:

```powershell
cargo test --lib app::legacy_display
cargo test --test field_display_integration
cargo test --workspace --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::all
```

Expected: all commands exit 0; legacy terrain, Delaunay, Voronoi, and template tests remain green.

- [ ] **Step 7: Commit Task 8**

Stage exact paths and deletions:

```powershell
git add src/app.rs src/app/legacy_display.rs src/resource/mod.rs src/gpu/mod.rs src/gpu/heightmap assets/shaders/heightmap.wgsl tests/field_display_integration.rs
git commit -m "feat: display legacy terrain through field views"
```

---

### Task 9: Add CPU Golden Images and Mandatory GPU Sampling

**Files:**

- Create: `src/view/reference.rs`
- Modify: `src/view/mod.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `tests/field_display_golden.rs`
- Create: `tests/golden/field-display/scalar.png`
- Create: `tests/golden/field-display/category.png`
- Create: `tests/golden/field-display/diagnostic.png`
- Add test module in: `src/gpu/field/renderer.rs`
- Modify: `.github/workflows/rust.yml`

**Interfaces:**

- Consumes: prepared mesh/field/diagnostics, CPU palette sampler, GPU renderer.
- Produces:

```rust
pub struct ReferenceImage {
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
}

pub fn rasterize_reference(
    packet: &PreparedFieldDisplay,
    width: u32,
    height: u32,
) -> Result<ReferenceImage, DisplayPrepareError>;
```

- [ ] **Step 1: Add explicit test-only image dependencies**

In `Cargo.toml`:

```toml
[dev-dependencies]
image = { version = "0.25", default-features = false, features = ["png"] }
pollster = "0.4"
```

Run:

```powershell
cargo check --tests
```

Expected: dependencies resolve without adding another image or executor version beyond Cargo's compatible resolution.

- [ ] **Step 2: Write failing CPU golden tests**

Create exact 128×64 fixtures:

```rust
#[test]
fn scalar_golden_matches() {
    assert_golden("scalar.png", &scalar_packet(), 128, 64);
}

#[test]
fn category_golden_matches() {
    assert_golden("category.png", &category_packet(), 128, 64);
}

#[test]
fn diagnostic_golden_matches() {
    assert_golden("diagnostic.png", &diagnostic_packet(), 128, 64);
}

#[test]
#[ignore = "writes reviewed field-display golden PNGs"]
fn regenerate_field_goldens() {
    assert_eq!(
        std::env::var("SEKAI_UPDATE_FIELD_GOLDENS").as_deref(),
        Ok("1")
    );
    write_golden("scalar.png", &scalar_packet(), 128, 64);
    write_golden("category.png", &category_packet(), 128, 64);
    write_golden("diagnostic.png", &diagnostic_packet(), 128, 64);
}
```

`assert_golden`:

- rasterizes into RGBA8;
- decodes the checked-in PNG without any write path;
- compares dimensions, every pixel, and a BLAKE3 hash included in the assertion message;
- never overwrites on an ordinary failure.

Only the ignored `regenerate_field_goldens` test calls `write_golden`, and it
requires the explicit update environment variable.

- [ ] **Step 3: Run golden tests and confirm missing reference rasterizer**

Run:

```powershell
cargo test --test field_display_golden
```

Expected: compilation fails because `rasterize_reference` does not exist.

- [ ] **Step 4: Implement deterministic triangle rasterization**

For each pixel center:

1. convert to normalized coordinates;
2. walk mesh triangles in index order;
3. use one exact edge-function convention with top-left inclusion;
4. on the first containing triangle, read its cell ID;
5. use the same CPU scalar/category palette and diagnostic precedence;
6. convert linear RGB to sRGB and round to nearest RGBA8;
7. leave uncovered pixels transparent black.

Reject zero width/height and checked `width * height * 4` overflow.

- [ ] **Step 5: Generate and inspect the three baselines**

Run only the ignored regeneration test:

```powershell
$env:SEKAI_UPDATE_FIELD_GOLDENS='1'
cargo test --test field_display_golden regenerate_field_goldens -- --ignored
$goldenExitCode = $LASTEXITCODE
Remove-Item Env:SEKAI_UPDATE_FIELD_GOLDENS
if ($goldenExitCode -ne 0) { exit $goldenExitCode }
```

Inspect each PNG's size and independent file hash:

```powershell
Get-ChildItem tests/golden/field-display/*.png | ForEach-Object {
    [PSCustomObject]@{
        Name = $_.Name
        Length = $_.Length
        Hash = (Get-FileHash -Algorithm SHA256 $_.FullName).Hash
    }
}
```

Then run the ordinary comparison:

```powershell
cargo test --test field_display_golden
```

Expected: all three comparisons pass without the update environment variable.

- [ ] **Step 6: Add offscreen GPU sampling**

In the renderer unit-test module:

- request a fallback adapter with `force_fallback_adapter: true`;
- when unavailable and `SEKAI_REQUIRE_FIELD_GPU=1`, fail;
- when unavailable without that variable, print one skip message and return;
- render the four-cell scalar and category packets to `Rgba8UnormSrgb`;
- copy to a mapped readback buffer with aligned rows;
- sample the center of each cell;
- compare each RGB channel to `rasterize_reference` with absolute error `<= 1`;
- assert alpha equals 255.

The test name is:

```rust
#[test]
fn offscreen_scalar_and_category_match_cpu_reference()
```

- [ ] **Step 7: Make one CI GPU sampling job mandatory**

Add a Linux job named `field_display_gpu`:

```yaml
field_display_gpu:
  name: Field display GPU reference
  runs-on: ubuntu-latest
  env:
    SEKAI_REQUIRE_FIELD_GPU: "1"
    WGPU_BACKEND: vulkan
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - run: sudo apt-get update && sudo apt-get install -y mesa-vulkan-drivers libvulkan1
    - run: cargo test --lib gpu::field::renderer::tests::offscreen_scalar_and_category_match_cpu_reference -- --exact
```

Confirm the exact path with `cargo test --lib -- --list`; the workflow must retain the full module-qualified path shown above.

- [ ] **Step 8: Verify Task 9**

Run:

```powershell
cargo test --test field_display_golden
cargo test --lib gpu::field::renderer::tests::offscreen_scalar_and_category_match_cpu_reference -- --exact
cargo test --workspace --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::all
```

Expected: all local commands exit 0; if local fallback GPU is unavailable, the dedicated CI job remains the mandatory evidence.

- [ ] **Step 9: Commit Task 9**

```powershell
git add Cargo.toml Cargo.lock src/view/reference.rs src/view/mod.rs src/gpu/field/renderer.rs tests/field_display_golden.rs tests/golden/field-display .github/workflows/rust.yml
git commit -m "test: verify field display output"
```

---

### Task 10: Run the Field Display Acceptance Gate

**Files:**

- Modify only if a gate exposes a display-system defect.

**Interfaces:**

- Consumes: all V1 display components and existing repository gates.
- Produces: clean native, release, wasm, Trunk, dependency-boundary, static-upload, and scope evidence.

- [ ] **Step 1: Scan architecture boundaries**

Run:

```powershell
rg -n 'egui|eframe|wgpu|crate::app|crate::gpu|crate::ui|crate::engine|crate::generators|crate::terrain|crate::models' src/view
rg -n 'crate::view|egui|eframe|wgpu' src/world src/engine src/generators
rg -n 'crate::engine|crate::generators|crate::terrain|crate::models' src/gpu/field src/ui/field
```

Expected: no matches. Imports inside comments or documentation must also be removed or reworded so the boundary scan remains mechanical.

- [ ] **Step 2: Prove there is one cell-fill path**

Run:

```powershell
rg -n 'HeightmapRenderer|HeightmapCallback|heightmap\.wgsl|height_to_color' src assets
rg -n 'CellFieldRenderer|FieldFillCallback|field_fill\.wgsl' src assets
```

Expected:

- first command has no matches;
- second command finds the new renderer, callback, and shader only in the intended GPU/UI/app composition paths.

- [ ] **Step 3: Run focused display tests**

```powershell
cargo test --test field_view_contracts
cargo test --test field_display_palette
cargo test --test field_display_diagnostics
cargo test --test field_display_mesh
cargo test --test field_display_integration
cargo test --test field_display_golden
cargo test --lib gpu::field
cargo test --lib ui::field
cargo test --lib app::legacy_display
```

Expected: every command passes with zero failures.

- [ ] **Step 4: Run exact repository gates**

```powershell
cargo check --workspace --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::all
cargo test --workspace --all-targets --all-features
cargo test --workspace --doc
cargo test --all-targets --release
```

Expected: all commands exit 0; the existing extreme Voronoi test remains the only expected ignored legacy test unless the new local-optional GPU test also reports a documented skip.

- [ ] **Step 5: Run the WASM library gate**

```powershell
$env:RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
$env:RUSTDOCFLAGS='--cfg getrandom_backend="wasm_js"'
cargo check --workspace --all-features --lib --target wasm32-unknown-unknown
$wasmExitCode = $LASTEXITCODE
Remove-Item Env:RUSTFLAGS
Remove-Item Env:RUSTDOCFLAGS
if ($wasmExitCode -ne 0) { exit $wasmExitCode }
```

Expected: exit 0 and no environment variables remain set.

- [ ] **Step 6: Run pinned Trunk**

```powershell
trunk --version
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ((trunk --version) -notmatch '^trunk 0\.21\.14$') { exit 1 }
$env:RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
$env:RUSTDOCFLAGS='--cfg getrandom_backend="wasm_js"'
trunk build
$trunkExitCode = $LASTEXITCODE
Remove-Item Env:RUSTFLAGS
Remove-Item Env:RUSTDOCFLAGS
if ($trunkExitCode -ne 0) { exit $trunkExitCode }
```

Expected: Trunk 0.21.14 exits 0 and `dist/` remains ignored.

- [ ] **Step 7: Verify static upload behavior directly**

Run the exact stats test:

```powershell
cargo test --lib gpu::field::renderer::tests::static_second_frame_uploads_only_uniforms -- --exact
```

The test must prepare the same packet twice and assert:

```rust
assert_eq!(stats.geometry_uploads, 1);
assert_eq!(stats.field_uploads, 1);
assert_eq!(stats.diagnostic_uploads, 1);
assert_eq!(stats.palette_uploads, 1);
assert_eq!(stats.uniform_updates, 2);
```

Before the final run, confirm this full path appears in `cargo test --lib -- --list`.

- [ ] **Step 8: Inspect final scope**

```powershell
git diff --check
git status --short --branch
git log --oneline --decorate -15
$displayBase = git merge-base HEAD origin/main
git diff --stat "$displayBase..HEAD"
git ls-files target dist screenshots .vscode
```

Expected:

- no uncommitted source changes;
- no `target/` or `dist/` files tracked;
- legacy screenshot/editor files are unchanged from the merge base;
- commits are limited to the design, plan, display subsystem, intended app adapter, tests, shader, and CI GPU test.

- [ ] **Step 9: Create a correction commit only when gates required a fix**

If a correction was necessary, stage only its reviewed files:

```powershell
git diff --check
git status --short
git commit -m "fix: satisfy field display acceptance gate"
```

Do not create an empty commit.

## Completion Evidence

The implementation is complete only when:

- `main` contains the merged foundation engine;
- the design and this plan are committed;
- `view` has no UI, GPU, engine, generator, terrain, model, or app dependencies;
- scalar and category cell fields share one GPU renderer;
- all other field types remain inspectable and explicitly unsupported for V1 fill;
- schema metadata drives compatible ranges, labels, units, and palette families;
- field and diagnostic preparation preserves cell cardinality and stable IDs;
- spatial mesh preparation is normalized, bounded, deterministic, and pickable;
- range changes do not upload mesh or field values;
- static second frames upload only small uniforms;
- failed preparation retains the last complete packet;
- current height and plate IDs appear through the legacy app adapter;
- the old heightmap fill callback, renderer, and shader are removed;
- CPU goldens are stable and GPU samples agree within one 8-bit channel value;
- focused, native, release, wasm, and Trunk gates pass;
- no generated output is tracked.
