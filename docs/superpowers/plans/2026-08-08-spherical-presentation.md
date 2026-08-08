# Spherical Natural Presentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the formally generated spherical natural world the default product canvas, with switchable Equal Earth/equirectangular 2D maps, a strictly undeformed 3D unit globe, all 36 natural fields shown through fills/edge annotations/dynamic vector arrows, stable shared picking, and an explicit legacy-planar migration boundary.

**Architecture:** Keep `SphericalSurfaceSnapshot` and the spherical natural Artifacts authoritative and projection-free. Prepare one immutable, source-tagged `Arc<PreparedFieldLayers>` shared by two independent presenters: a seam-aware projected-map presenter and an undeformed unit-globe presenter. Both presenters use one `SphericalEntityLocator`, while geometry, cameras, GPU buffers, and picking transforms remain view-specific. Build the complete science document and all initial presentation derivatives as a candidate, cross-validate source identity, then publish atomically. Retain the current planar renderer only for persisted `LegacyPlanarV1` state.

**Tech Stack:** Rust 1.85 / edition 2021, serde, thiserror, BLAKE3 build identity, eframe/egui 0.31, wgpu through `egui_wgpu`, WGSL, Cargo unit/integration/offscreen tests; no new dependencies.

## Global Constraints

- Never deform the globe. Every `PreparedGlobeVertex.position` is copied from a validated `UnitVector3` cell centroid or boundary vertex and must stay at radius `1.0`; elevation and every other field affect colors/annotations only.
- `SphericalSurfaceSnapshot` is the sole geometry/topology authority. Display duplication may carry an existing `CellId`/`EdgeId`, but may not create semantic entities or write back to the snapshot.
- Use exactly one fill slot plus zero or one overlay slot. Fill accepts `Cells + ScalarF32/CategoryU32`; overlay accepts `Edges + ScalarF32/CategoryU32` or `Cells + Vector2F32`.
- Dynamic arrows encode authoritative direction, magnitude through length/color, and display-only flow phase. Animation speed is explicitly non-physical; a frame advances only a fixed-size uniform and never rebuilds or uploads instances.
- The 2D and 3D presenters must hold the exact same `Arc<PreparedFieldLayers>` allocation. They may not each copy all 36 payloads, diagnostics, or palettes.
- Equal Earth is the default map projection. Equirectangular is a diagnostic alternative. Both support central-meridian changes, inverse picking, seams, and poles without clamping invalid hits.
- 2D and 3D picking both end in the same `SphericalEntityLocator`; ties use the lowest stable ID. Edge selection is limited to incident edges and an 8-logical-pixel tolerance converted to angle.
- Candidate failures preserve the previously published complete document, map, globe, locator, layers, GPU packet, state, and revision clock. Never fall back silently to planar science.
- New app state defaults to `SphericalV1`. Missing persisted origin tags deserialize to `LegacyPlanarV1`; legacy becomes spherical only through an explicit regenerate action.
- The app inserts the exact eight external Artifacts into `spherical_natural_foundation_graph()`. Preview and full build use that same graph and input builder.
- Static frames perform no O(cell-count) work and no large-buffer uploads. Camera, pan/zoom, view switch, and vector phase update uniforms only.
- At 20k cells: CPU presentation derivatives add at most 128 MiB and prepare in at most 1 second in Release on the recorded reference machine. Native target is 60 FPS; wasm target is at least 30 FPS for the frozen 1280×720 interaction scenario.
- Preserve the existing legacy planar graph, wire schemas, tests, and display goldens. Do not generalize `PreparedCellMesh` or `CellFieldRenderer` into a universal 2D/3D abstraction.
- Every production behavior follows strict TDD: write and run the focused failing test, implement the minimum behavior, rerun focused and adjacent tests, then commit.

## File Structure

- `src/view/spherical_source.rs`: immutable presentation source identity and exact-source validation.
- `src/view/field_layers.rs`: spherical fill/overlay state, prepared payloads, palettes, diagnostics, vector ranges, LOD, and revisions.
- `src/view/spherical_projection.rs`: Equal Earth/equirectangular forward/inverse math and local-vector mapping.
- `src/view/spherical_picking.rs`: source-bound cell/incident-edge locator and unit-sphere ray intersection.
- `src/view/spherical_mesh.rs`: seam-aware projected map mesh, undeformed globe mesh, edge segments, and geometry budgets.
- `src/view/spherical_camera.rs`: independent 2D camera state and orthographic trackball globe camera.
- `src/view/mod.rs`: public renderer-neutral exports.
- `src/gpu/spherical/mod.rs`: spherical renderer exports and upload counters.
- `src/gpu/spherical/callback.rs`: one egui paint callback selecting map/globe passes.
- `src/gpu/spherical/renderer.rs`: independent map/globe pipelines, edge/vector passes, atomic buffer updates.
- `src/gpu/mod.rs`: register the spherical module without changing legacy `gpu::field`.
- `assets/shaders/spherical_field.wgsl`: shared field coloring plus map/globe/edge/vector entry points.
- `src/app/spherical_presentation.rs`: formal spherical external inputs, complete candidate composition, source checks, and atomic publication.
- `src/app/spherical_natural_display.rs`: expose validated identity values only to the presentation composer.
- `src/app.rs`: persisted origin, default sphere spec, presenter resources, build/publish orchestration, and explicit migration.
- `src/resource/mod.rs`: spherical renderer/presentation/state resources beside legacy resources.
- `src/ui/spherical.rs`: single-canvas view/projection/animation controls and spherical inspector.
- `src/ui/mod.rs`: spherical UI exports.
- `tests/spherical_projection.rs`: projection references, round trips, bounds, poles, seams, and vector mapping.
- `tests/spherical_picking.rs`: shared cell/edge/ray picking and stable ties.
- `tests/spherical_presentation_mesh.rs`: seam semantics and undeformed globe invariants.
- `tests/spherical_field_layers.rs`: exact 36-field channel matrix, shared allocation, revisions, and glyph LOD.
- `tests/spherical_presentation_integration.rs`: graph inputs, source identity, atomic publication, UI-state invalidation, and legacy migration.
- `tests/spherical_presentation_gpu.rs`: CPU/GPU fill, edge, vector, visibility, and upload-count fixtures.
- `tests/spherical_presentation_performance.rs`: ignored Release 20k time/memory/upload gate.
- `docs/superpowers/plans/2026-08-08-spherical-presentation.md`: checklist, RED/GREEN evidence, commits, and final measurements.

---

### Task 1: Add source-bound spherical display state and channel classification

**Files:**
- Create: `src/view/spherical_source.rs`
- Create: `src/view/field_layers.rs`
- Modify: `src/view/mod.rs`
- Modify: `src/app/spherical_natural_display.rs`
- Create: `tests/spherical_field_layers.rs`

**Interfaces:**
- Produces `SphericalPresentationSource`, `SphericalFieldDisplayState`, `SelectedSurfaceEntity`, `SphericalFieldChannel`, `VectorGlyphLod`, and `classify_spherical_channel`.
- `SphericalPresentationSource` contains `RootSeed`, `SurfaceRef`, `BuildResultHash`, and graph contract version; its constructor is `pub(crate)` and app composition creates it only from `SphericalNaturalBuildIdentity` getters.
- Existing `FieldDisplayState` remains unchanged for legacy V1.

- [x] **Step 1: Write the failing public contract tests**

Add these tests to `tests/spherical_field_layers.rs` using schemas from `spherical_natural_field_registry`:

```rust
use sekai::view::{
    classify_spherical_channel, SelectedSurfaceEntity, SphericalFieldChannel,
    SphericalFieldDisplayState, VectorGlyphLod,
};
use sekai::world::fields::{FieldDomain, FieldValueType};
use sekai::world::natural::{
    boundary_kind_field_id, plate_velocity_field_id, surface_elevation_m_field_id,
};
use sekai::world::{CellId, EdgeId};

#[test]
fn exact_supported_domain_type_pairs_map_to_display_channels() {
    assert_eq!(
        classify_spherical_channel(FieldDomain::Cells, FieldValueType::ScalarF32),
        Some(SphericalFieldChannel::CellFill)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Cells, FieldValueType::CategoryU32),
        Some(SphericalFieldChannel::CellFill)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Edges, FieldValueType::ScalarF32),
        Some(SphericalFieldChannel::EdgeOverlay)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Edges, FieldValueType::CategoryU32),
        Some(SphericalFieldChannel::EdgeOverlay)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Cells, FieldValueType::Vector2F32),
        Some(SphericalFieldChannel::VectorOverlay)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Edges, FieldValueType::Vector2F32),
        None
    );
}

#[test]
fn spherical_state_preserves_fill_overlay_and_stable_entity_independently() {
    let mut state = SphericalFieldDisplayState::default();
    state.select_fill(surface_elevation_m_field_id());
    state.select_overlay(Some(plate_velocity_field_id()));
    state.select_entity(Some(SelectedSurfaceEntity::Cell(CellId::from_raw(7))));
    state.set_vector_lod(VectorGlyphLod::Medium);
    state.set_vector_paused(false);
    state.set_vector_display_speed(1.5).unwrap();

    assert_eq!(state.fill_field(), Some(&surface_elevation_m_field_id()));
    assert_eq!(state.overlay_field(), Some(&plate_velocity_field_id()));
    assert_eq!(
        state.selected_entity(),
        Some(SelectedSurfaceEntity::Cell(CellId::from_raw(7)))
    );
    assert_eq!(state.vector_lod(), VectorGlyphLod::Medium);
    assert!(!state.vector_paused());
    assert_eq!(state.vector_display_speed(), 1.5);

    state.select_overlay(Some(boundary_kind_field_id()));
    state.select_entity(Some(SelectedSurfaceEntity::Edge(EdgeId::from_raw(3))));
    assert_eq!(state.overlay_field(), Some(&boundary_kind_field_id()));
    assert_eq!(
        state.selected_entity(),
        Some(SelectedSurfaceEntity::Edge(EdgeId::from_raw(3)))
    );
}
```

Add an app-module test that builds the existing 162-cell spherical outcome, constructs `SphericalNaturalFieldDocument`, derives `SphericalPresentationSource`, and asserts all four identity fields equal the document identity. The test must not call a public free-form source constructor.

- [x] **Step 2: Run RED**

Run: `cargo test --test spherical_field_layers -- --nocapture`

Expected: compilation fails because the spherical state/channel types do not exist.

- [x] **Step 3: Implement the narrow contracts**

Use these exact enum shapes and defaults:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SphericalFieldChannel { CellFill, EdgeOverlay, VectorOverlay }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedSurfaceEntity { Cell(CellId), Edge(EdgeId) }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VectorGlyphLod { Low, #[default] Medium, High }

pub fn classify_spherical_channel(
    domain: FieldDomain,
    value_type: FieldValueType,
) -> Option<SphericalFieldChannel> {
    match (domain, value_type) {
        (FieldDomain::Cells, FieldValueType::ScalarF32 | FieldValueType::CategoryU32) =>
            Some(SphericalFieldChannel::CellFill),
        (FieldDomain::Edges, FieldValueType::ScalarF32 | FieldValueType::CategoryU32) =>
            Some(SphericalFieldChannel::EdgeOverlay),
        (FieldDomain::Cells, FieldValueType::Vector2F32) =>
            Some(SphericalFieldChannel::VectorOverlay),
        _ => None,
    }
}
```

`SphericalFieldDisplayState::default()` uses no explicit field until reconciliation, no overlay, data ranges, schema palettes, diagnostics enabled/selected-field scope, no selected entity, medium glyph LOD, animation playing, and display speed `1.0`. Validate display speed as finite and in `0.0..=4.0` with a typed `FieldLayerError::InvalidVectorDisplaySpeed`.

- [x] **Step 4: Verify focused and legacy state tests**

Run:

```powershell
cargo test --test spherical_field_layers -- --nocapture
cargo test --lib view::state -- --nocapture
cargo test --lib app::spherical_natural_display -- --nocapture
```

- [x] **Step 5: Commit**

```powershell
git add src/view/spherical_source.rs src/view/field_layers.rs src/view/mod.rs src/app/spherical_natural_display.rs tests/spherical_field_layers.rs
git commit -m "feat: add spherical presentation state"
```

---

### Task 2: Prepare all spherical fill, edge, and vector field layers once

**Files:**
- Modify: `src/view/field_layers.rs`
- Modify: `src/view/palette.rs`
- Modify: `src/view/mod.rs`
- Modify: `src/app/field_document.rs`
- Modify: `tests/spherical_field_layers.rs`

**Interfaces:**
- Produces `PreparedFieldLayers`, `PreparedSphericalOverlay`, `PreparedEdgeField`, `PreparedVectorField`, `FieldLayerRevisions`, `prepare_spherical_field_layers`, and `update_spherical_field_layers`.
- Consumes a borrowed `FieldCatalog`, cell/edge cardinalities, diagnostics, preferred ranges, `SphericalFieldDisplayState`, source, and `DisplayRevisionClock`.
- The returned packet owns only the selected fill/overlay payloads, two palettes, diagnostic mask, and vector magnitudes; the document remains owner of all 36 authoritative arrays.

- [x] **Step 1: Extend the test with the exact 36-field matrix**

Build the existing full spherical document fixture inside `src/app/spherical_natural_display.rs` tests and pass its catalog through a public pure helper. Assert:

```rust
let counts = catalog.entries().iter().fold([0usize; 3], |mut counts, entry| {
    let schema = entry.schema();
    match classify_spherical_channel(schema.domain, schema.value_type).unwrap() {
        SphericalFieldChannel::CellFill => counts[0] += 1,
        SphericalFieldChannel::EdgeOverlay => counts[1] += 1,
        SphericalFieldChannel::VectorOverlay => counts[2] += 1,
    }
    counts
});
assert_eq!(counts, [32, 2, 2]);
assert_eq!(catalog.entries().len(), 36);
```

Prepare elevation fill plus wind vector overlay and assert the vector packet contains the original east/north pairs, finite magnitudes, a resolved magnitude range, and a sequential palette. Prepare boundary category and boundary scalar overlays and assert edge cardinality, compact keys/range, and kind. Reject wrong cardinality and unsupported channels with field IDs in the error.

Add pointer tests:

```rust
let shared = Arc::new(layers);
let map_layers = shared.clone();
let globe_layers = shared.clone();
assert!(Arc::ptr_eq(&map_layers, &globe_layers));
assert!(Arc::ptr_eq(map_layers.fill_arc(), globe_layers.fill_arc()));
assert!(Arc::ptr_eq(map_layers.fill_palette_arc(), globe_layers.fill_palette_arc()));
assert!(Arc::ptr_eq(map_layers.diagnostics_arc(), globe_layers.diagnostics_arc()));
```

- [x] **Step 2: Run RED**

Run: `cargo test --test spherical_field_layers -- --nocapture`

Expected: missing preparation types/functions.

- [x] **Step 3: Implement typed preparation without renderer geometry**

Use these packet shapes:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedOverlayKind { EdgeScalar, EdgeCategory, CellVector }

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedVectorField {
    field_id: FieldId,
    components: Vec<[f32; 2]>,
    magnitudes: Vec<f32>,
    display_range: ResolvedDisplayRange,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEdgeField {
    field_id: FieldId,
    kind: PreparedFieldKind,
    raw_values: Vec<u32>,
    display_range: Option<ResolvedDisplayRange>,
    category_keys: Vec<u32>,
}

#[derive(Debug, Clone)]
pub enum PreparedSphericalOverlay {
    Edge(Arc<PreparedEdgeField>),
    Vector(Arc<PreparedVectorField>),
}

#[derive(Debug, Clone)]
pub struct PreparedFieldLayers {
    source: SphericalPresentationSource,
    fill: Arc<PreparedCellField>,
    overlay: Option<PreparedSphericalOverlay>,
    diagnostics: Arc<PreparedDiagnosticMask>,
    fill_palette: Arc<[LinearRgba]>,
    overlay_palette: Option<Arc<[LinearRgba]>>,
    revisions: FieldLayerRevisions,
    diagnostics_enabled: bool,
}
```

Extract the scalar/category packing internals behind a private domain-aware helper while retaining the existing public legacy `prepare_cell_field` signature and `PreparedCellField` unchanged. `prepare_edge_field` calls that helper with `FieldDomain::Edges` and returns `PreparedEdgeField`; it must not mislabel edge cardinality as cells. Vector preparation computes `hypot(x, y)` once, rejects non-finite components/magnitudes, resolves schema/data/manual range over magnitudes, and uses `PaletteId::Sequential` for `FieldPaletteHint::Vector`.

Reconciliation must retain still-compatible choices; otherwise choose document-preferred elevation (or first fill), clear an invalid overlay, and clear out-of-range cell/edge selections. Update only affected Arcs/revisions: changing fill does not rebuild overlay, changing overlay does not rebuild fill, changing range only changes its resolved range/revision, and toggling diagnostics/animation changes no large Arc.

- [x] **Step 4: Verify field preparation and frozen legacy behavior**

Run:

```powershell
cargo test --test spherical_field_layers -- --nocapture
cargo test --lib app::field_document -- --nocapture
cargo test --test natural_display_golden -- --nocapture
```

- [x] **Step 5: Commit**

```powershell
git add src/view/field_layers.rs src/view/palette.rs src/view/mod.rs src/app/field_document.rs tests/spherical_field_layers.rs
git commit -m "feat: prepare spherical field layers"
```

---

### Task 3: Implement Equal Earth and equirectangular projection math

**Files:**
- Create: `src/view/spherical_projection.rs`
- Modify: `src/view/mod.rs`
- Create: `tests/spherical_projection.rs`

**Interfaces:**
- Produces `SphericalProjectionKind`, `SphericalProjection`, `ProjectionPoint`, `ProjectionBounds`, `ProjectedDirection`, and `SphericalProjectionError`.
- `forward(UnitVector3) -> Result<ProjectionPoint, _>`, `inverse(ProjectionPoint) -> Result<UnitVector3, _>`, `bounds()`, `outline_contains()`, and `map_local_vector(radial, [east,north])`.
- Central meridian is normalized into `[-π, π)` and stored in radians.

- [x] **Step 1: Write projection reference and failure tests**

Add tests that derive a unit vector from longitude/latitude and assert:

```rust
const EPS: f64 = 2.0e-12;

#[test]
fn equal_earth_matches_published_spherical_reference_values() {
    let projection = SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0).unwrap();
    let equator = projection.forward(direction(0.0, 0.0)).unwrap();
    assert!((equator.x() - 0.0).abs() < EPS);
    assert!((equator.y() - 0.0).abs() < EPS);

    let sample = projection.forward(direction(45.0, 30.0)).unwrap();
    assert!((sample.x() - 0.6329254189568163).abs() < 2.0e-12);
    assert!((sample.y() - 0.5929351198480170).abs() < 2.0e-12);
}
```

Before committing the constants, independently calculate them from the author formula in the test fixture and record the citation comment; do not copy values produced by the implementation under test. Add a grid over longitude `-180..=180` by 5° and latitude `-90..=90` by 5°, seam offsets `±1e-10`, poles, and central meridians `[-170, -30, 0, 75, 179]`; forward/inverse angular error must be below `2e-10` radians except longitude is ignored exactly at a pole. Assert non-finite inputs, out-of-outline coordinates, and forced Newton non-convergence return distinct variants.

Test equirectangular exact normalized coordinates: x is relative longitude divided by π and y is latitude divided by π/2. Test zero local vector returns `None`; a finite east/north vector returns a finite normalized 2D direction or `ProjectionJacobianDegenerate` at a true singularity.

- [x] **Step 2: Run RED**

Run: `cargo test --test spherical_projection -- --nocapture`

Expected: module and projection types are missing.

- [x] **Step 3: Implement the published Equal Earth formula**

Use the author coefficients exactly:

```rust
const A1: f64 = 1.340_264;
const A2: f64 = -0.081_106;
const A3: f64 = 0.000_893;
const A4: f64 = 0.003_796;
const M: f64 = 3.0_f64.sqrt() / 2.0;
const MAX_NEWTON_ITERATIONS: usize = 12;
const NEWTON_TOLERANCE: f64 = 1.0e-13;
```

For relative longitude `lambda`, latitude `phi`, and `theta = asin(M * sin(phi))`:

```rust
let theta2 = theta * theta;
let theta6 = theta2 * theta2 * theta2;
let denominator = M * (A1 + 3.0 * A2 * theta2 + theta6 * (7.0 * A3 + 9.0 * A4 * theta2));
let x = lambda * theta.cos() / denominator;
let y = theta * (A1 + A2 * theta2 + theta6 * (A3 + A4 * theta2));
```

Inverse solves the y polynomial for theta with a bounded Newton iteration, validates theta and recovered latitude, then recovers longitude. Reject coordinates outside the analytical outline before iteration. Convert `UnitVector3` using `atan2(y, x)` and `asin(z)` and reconstruct through finite `UnitVector3::new`.

Map vectors with a centered finite-difference Jacobian in tangent east/north directions (`1e-7` radians), unwrap seam x deltas before differencing, and reject mapped length below `1e-12`.

- [x] **Step 4: Verify math and format/lint the module**

Run:

```powershell
cargo test --test spherical_projection -- --nocapture
cargo test --lib view::spherical_projection -- --nocapture
cargo fmt --all -- --check
cargo clippy --test spherical_projection -- -D warnings
```

- [x] **Step 5: Commit**

```powershell
git add src/view/spherical_projection.rs src/view/mod.rs tests/spherical_projection.rs
git commit -m "feat: add spherical map projections"
```

---

### Task 4: Add one stable cell/edge locator and unit-sphere ray picking

**Files:**
- Create: `src/view/spherical_picking.rs`
- Modify: `src/view/mod.rs`
- Create: `tests/spherical_picking.rs`

**Interfaces:**
- Produces source-bound `SphericalEntityLocator`, `UnitRay`, `RaySphereHit`, `locate_cell`, `locate_incident_edge`, and `intersect_unit_sphere`.
- Locator caches unit sites and each cell's incident `EdgeId`s/midpoints; it does not own polygon geometry or field payloads.

- [x] **Step 1: Write shared locator tests against a generated 162-cell surface**

Generate a public spherical surface fixture through `spherical_foundation_graph`. For every authoritative site, assert `locate_cell(site) == cell.id`. For every edge midpoint, assert a lookup started from either owner and a generous angular tolerance returns that edge. Add a synthetic equal-dot tie and require lowest `CellId`; add equal-distance incident edge tie and require lowest `EdgeId`.

Test the ray primitive exactly:

```rust
#[test]
fn ray_sphere_returns_nearest_positive_hit_and_rejects_misses() {
    let hit = intersect_unit_sphere(UnitRay::new([0.0, 0.0, 3.0], [0.0, 0.0, -1.0]).unwrap())
        .unwrap();
    assert_eq!(hit.direction().components(), [0.0, 0.0, 1.0]);
    assert_eq!(hit.distance(), 2.0);
    assert!(intersect_unit_sphere(
        UnitRay::new([0.0, 0.0, 3.0], [1.0, 0.0, 0.0]).unwrap()
    ).is_none());
}
```

Project a fixed direction through both projections, inverse it, and compare its cell ID to a camera ray hitting the same direction. Test outside-map, ray miss, edge farther than tolerance, and non-incident edge return `None`.

- [x] **Step 2: Run RED**

Run: `cargo test --test spherical_picking -- --nocapture`

- [x] **Step 3: Implement deterministic bounded lookup**

Start with a source-bound contiguous site array and a deterministic maximum-dot scan; ties within exact `f64::total_cmp` ordering resolve by lower ID. This O(n) lookup is allowed only on click, not hover/frame. Record a follow-up benchmark before adding any spatial index. `locate_incident_edge` iterates only the hit cell's stored boundary edges and evaluates minor-arc distance to each great-circle segment; enforce finite `0..=π` tolerance and stable `EdgeId` tie-breaking.

Normalize ray direction at construction. Solve `|origin + t direction|² = 1` with a stable quadratic, choose the nearest `t >= 0`, and return a normalized intersection direction. Non-finite/zero directions are structured errors.

- [x] **Step 4: Verify picking and surface contracts**

Run:

```powershell
cargo test --test spherical_picking -- --nocapture
cargo test --test spherical_foundation_build -- --nocapture
cargo clippy --test spherical_picking -- -D warnings
```

- [x] **Step 5: Commit**

```powershell
git add src/view/spherical_picking.rs src/view/mod.rs tests/spherical_picking.rs
git commit -m "feat: add shared spherical picking"
```

---

### Task 5: Build seam-safe projected map geometry with stable IDs

**Files:**
- Create: `src/view/spherical_mesh.rs`
- Modify: `src/view/mod.rs`
- Create: `tests/spherical_presentation_mesh.rs`

**Interfaces:**
- Produces `PreparedProjectedMap`, `ProjectedMapVertex`, `ProjectedEdgeSegment`, `SphericalMeshBudgets`, and `SphericalMeshError`.
- `PreparedProjectedMap::build(source, surface, projection, budgets)` derives cell triangle fans, seam fragments, edge fragments, bounds, indices, and source identity.

- [x] **Step 1: Write seam, pole, and semantic-ID tests**

For generated 42- and 162-cell surfaces, both projections, and central meridians `0`, `π/2`, and `π - 1e-9`, assert all positions finite; every triangle has nonzero signed area; all indices are in range; and the set of vertex/triangle `CellId`s equals the authoritative cell set exactly. Assert every authoritative cell has at least one triangle.

Classify authoritative cell fans as seam/non-seam before projection. Assert non-seam fan triangles remain one triangle and seam fan triangles may split but all fragments retain the same `CellId`. Assert every projected edge fragment retains an existing `EdgeId`, and split fragments do not enlarge the semantic edge-ID set. Assert no triangle edge spans more than the projection's bounded half-width plus a numeric epsilon.

Pick directions one micro-radian to each side of the anti-meridian, inverse through the projection, and assert the shared locator yields the corresponding authoritative IDs. Add budget tests for cell, vertex, index, edge-segment, and checked-integer overflow errors.

- [x] **Step 2: Run RED**

Run: `cargo test --test spherical_presentation_mesh -- --nocapture`

- [x] **Step 3: Implement spherical seam clipping before projection**

For each cell, form triangles `(centroid, boundary[i], boundary[i+1])`. Convert vertices to latitude/relative-longitude; unwrap each triangle around its first longitude. If its unwrapped range stays inside one `[-π, π]` copy, project directly. Otherwise clip against `-π` or `π` in longitude space, compute intersections on the original minor great-circle arc by bounded bisection on wrapped longitude, emit one polygon per side, and triangulate each convex fragment as a fan. Duplicate display vertices only; retain the source `CellId`.

Apply the same arc split to each authoritative edge, emitting one or two `ProjectedEdgeSegment`s with the same `EdgeId`. Reject degenerate/non-finite output instead of dropping an authoritative cell. Enforce checked `usize -> u32` conversions and explicit default budgets derived from `MAX_SPHERICAL_*` constants.

- [x] **Step 4: Verify mesh and projection adjacency**

Run:

```powershell
cargo test --test spherical_presentation_mesh -- --nocapture
cargo test --test spherical_projection -- --nocapture
cargo test --test spherical_picking -- --nocapture
```

- [x] **Step 5: Commit**

```powershell
git add src/view/spherical_mesh.rs src/view/mod.rs tests/spherical_presentation_mesh.rs
git commit -m "feat: build seam-safe spherical maps"
```

---

### Task 6: Build the undeformed globe mesh and orthographic trackball camera

**Files:**
- Modify: `src/view/spherical_mesh.rs`
- Create: `src/view/spherical_camera.rs`
- Modify: `src/view/mod.rs`
- Modify: `tests/spherical_presentation_mesh.rs`
- Modify: `tests/spherical_picking.rs`

**Interfaces:**
- Produces `PreparedGlobeMesh`, `GlobeVertex`, `GlobeCamera`, `MapCamera`, `SphericalViewMode`, and screen-to-ray transforms.
- Globe geometry depends only on presentation source plus spherical surface; no field/range/palette argument is accepted.

- [ ] **Step 1: Write the no-deformation and camera-only invalidation tests**

Build a globe, serialize raw position/index/CellId bytes into a BLAKE3 hash, then prepare two radically different elevation arrays/ranges and assert the globe hash and every vertex component are unchanged. Assert every vertex radius differs from `1.0` by at most `2e-6`, every triangle normal points outward, and the semantic `CellId` set equals the surface cell set.

Add compile-time/API regression by constructing the globe only as:

```rust
let globe = PreparedGlobeMesh::build(source.clone(), surface, budgets).unwrap();
```

No test or production call may pass elevation. Test camera reset, deterministic trackball drag, bounded orthographic zoom, front/back visibility, screen-center ray, outside-disc miss, and that camera changes leave globe bytes and geometry revision unchanged.

- [ ] **Step 2: Run RED**

Run: `cargo test --test spherical_presentation_mesh --test spherical_picking -- --nocapture`

- [ ] **Step 3: Implement unit geometry and view transforms**

Emit a per-cell triangle fan. Convert every `f64` unit component to finite `f32`, verify radius before storing, and reverse only a triangle whose computed normal points inward. Store `[f32; 3]` plus raw `CellId` in vertices and checked `u32` indices.

Represent globe orientation as a normalized quaternion owned by `GlobeCamera`; fixed trackball mapping turns screen coordinates inside the canvas into unit trackball vectors, applies the shortest rotation, and renormalizes. Clamp orthographic scale to `0.55..=8.0`. Screen-to-ray transforms camera-space x/y through inverse orientation; a click outside the visible unit disc returns `None` before the shared ray-sphere helper.

`MapCamera` stores pan/zoom independently per `SphericalProjectionKind`; switching modes does not overwrite either map camera or globe camera.

- [ ] **Step 4: Verify no-deformation and camera behavior**

Run:

```powershell
cargo test --test spherical_presentation_mesh -- --nocapture
cargo test --test spherical_picking -- --nocapture
cargo fmt --all -- --check
```

- [ ] **Step 5: Commit**

```powershell
git add src/view/spherical_mesh.rs src/view/spherical_camera.rs src/view/mod.rs tests/spherical_presentation_mesh.rs tests/spherical_picking.rs
git commit -m "feat: add undeformed globe presentation"
```

---

### Task 7: Render shared fills on independent 2D and 3D GPU pipelines

**Files:**
- Create: `src/gpu/spherical/mod.rs`
- Create: `src/gpu/spherical/callback.rs`
- Create: `src/gpu/spherical/renderer.rs`
- Modify: `src/gpu/mod.rs`
- Create: `assets/shaders/spherical_field.wgsl`
- Create: `tests/spherical_presentation_gpu.rs`

**Interfaces:**
- Produces `SphericalFieldRenderer`, `SphericalPaintCallback`, `SphericalRenderMode`, `SphericalGpuPacket`, `SphericalUploadCounters`, and atomic `prepare_packet`/`paint` behavior.
- Map and globe have separate geometry buffers/pipelines/camera uniforms; both consume one field/palette/diagnostic packet and the same value-color WGSL functions.

- [ ] **Step 1: Write offscreen RED tests and upload-count expectations**

Adapt the existing `gpu::field::renderer` offscreen fixture. Render a four-color scalar and category fixture through map and front-facing globe modes into RGBA8. Compare sampled pixels against `scalar_color`/`category_color` within two 8-bit quantization steps. Assert unlit globe front pixels equal map value colors rather than being multiplied by a lighting term. Assert back-facing primitives are culled.

Upload once, render two static frames, then rotate/zoom/switch mode and assert:

```rust
let after_upload = renderer.upload_counters();
renderer.paint_for_test(SphericalRenderMode::Map, &map_uniform);
renderer.paint_for_test(SphericalRenderMode::Globe, &globe_uniform);
renderer.paint_for_test(SphericalRenderMode::Globe, &rotated_uniform);
assert_eq!(renderer.upload_counters().map_geometry, after_upload.map_geometry);
assert_eq!(renderer.upload_counters().globe_geometry, after_upload.globe_geometry);
assert_eq!(renderer.upload_counters().fill_field, after_upload.fill_field);
assert_eq!(renderer.upload_counters().diagnostics, after_upload.diagnostics);
assert_eq!(renderer.upload_counters().palettes, after_upload.palettes);
assert!(renderer.upload_counters().uniforms > after_upload.uniforms);
```

Force a candidate with mismatched source/cardinality and assert the last complete GPU packet and counters remain installed.

- [ ] **Step 2: Run RED**

Run: `cargo test --test spherical_presentation_gpu -- --nocapture`

- [ ] **Step 3: Implement checked atomic GPU resources**

Use one storage-buffer layout for packed fill values and diagnostic severity, one palette buffer, and mode-specific vertex layouts. WGSL helper functions must decode scalar/category and diagnostic overlay exactly once, with separate `vs_map`/`vs_globe` entry points and one `fs_fill`. Globe uses front-face CCW and back-face culling; neither shader reads elevation as geometry.

Build all replacement buffers/bind groups in local candidates after source, revision, byte-count, `u32/u64`, and `wgpu::Limits` checks. Swap renderer state only after all allocations succeed. Cache revisions independently for map geometry, globe geometry, fill, diagnostics, and palettes. Every paint updates only the fixed-size camera/mode uniform.

- [ ] **Step 4: Verify offscreen output and legacy renderer**

Run:

```powershell
cargo test --test spherical_presentation_gpu -- --nocapture
cargo test --lib gpu::field::renderer::tests::offscreen_scalar_and_category_match_cpu_reference -- --nocapture
cargo clippy --test spherical_presentation_gpu -- -D warnings
```

- [ ] **Step 5: Commit**

```powershell
git add src/gpu/spherical src/gpu/mod.rs assets/shaders/spherical_field.wgsl tests/spherical_presentation_gpu.rs
git commit -m "feat: render spherical field fills"
```

---

### Task 8: Add edge annotations and dynamic vector arrows with nested LOD

**Files:**
- Modify: `src/view/field_layers.rs`
- Modify: `src/view/spherical_projection.rs`
- Modify: `src/view/spherical_mesh.rs`
- Modify: `src/gpu/spherical/renderer.rs`
- Modify: `src/gpu/spherical/callback.rs`
- Modify: `assets/shaders/spherical_field.wgsl`
- Modify: `tests/spherical_field_layers.rs`
- Modify: `tests/spherical_presentation_gpu.rs`

**Interfaces:**
- Produces `PreparedVectorGlyphs`, `MapVectorGlyph`, `GlobeVectorGlyph`, `VectorAnimationUniform`, and discrete `GlyphLodKey` rebuilds.
- Edge lines and arrows use triangle/quad instances, not platform-dependent wide lines.

- [ ] **Step 1: Write field semantics, LOD, and animation RED tests**

For both vector fields, assert glyph direction reconstructs from canonical east/north components, length and color share the same magnitude range, zero vectors produce no direction instance, and any degenerate 2D Jacobian omits only the map glyph while retaining the globe glyph and inspector value.

For every source and selected cell, generate Low/Medium/High sets and assert `Low ⊂ Medium ⊂ High`, deterministic repeated bytes, no duplicates, and selected cell inclusion at every level. Use a source-keyed stable BLAKE3 score over `CellId` and fixed denominators `16`, `8`, and `4`; do not use runtime randomness.

Render scalar/category edge fixtures and static/animated arrow fixtures. Assert back-hemisphere globe edges/arrows do not appear. Advance only animation phase and assert instance upload counters do not change while uniform count does. Pause and render twice with the same uniform; images must be byte-identical.

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test --test spherical_field_layers -- --nocapture
cargo test --test spherical_presentation_gpu -- --nocapture
```

- [ ] **Step 3: Implement bounded edge and vector instances**

Filter only display primitives for zero/no-event edges; keep the complete prepared edge payload. Scalar edge width maps magnitude into `1.0..=4.0` logical pixels and category width stays `2.0`; color comes from the overlay palette. Expand segments into camera-facing quads in the shader with viewport size.

Build globe glyph direction as `east * x + north * y` tangent to its cell radial. Build map glyph direction through `map_local_vector`. Instance length maps normalized magnitude into `0.35..=1.0` of the LOD cell spacing; color uses the same normalized magnitude. Animate a bright segment using `fract(along_arrow - phase)` and keep arrowhead direction static. Clamp display speed and advance phase modulo one using frame delta; label it display speed in UI later.

Only a world/field/projection/selected-cell/discrete-LOD-key change rebuilds instances. Camera and ordinary zoom changes update uniforms; crossing a predefined LOD threshold changes `GlyphLodKey` once and rebuilds the relevant instance Arc.

- [ ] **Step 4: Verify overlays, frozen instances, and platform primitives**

Run:

```powershell
cargo test --test spherical_field_layers --test spherical_presentation_gpu -- --nocapture
cargo test --test spherical_presentation_mesh -- --nocapture
cargo fmt --all -- --check
```

- [ ] **Step 5: Commit**

```powershell
git add src/view/field_layers.rs src/view/spherical_projection.rs src/view/spherical_mesh.rs src/gpu/spherical/renderer.rs src/gpu/spherical/callback.rs assets/shaders/spherical_field.wgsl tests/spherical_field_layers.rs tests/spherical_presentation_gpu.rs
git commit -m "feat: animate spherical field annotations"
```

---

### Task 9: Compose and atomically publish the formal spherical product

**Files:**
- Create: `src/app/spherical_presentation.rs`
- Modify: `src/app/spherical_natural_display.rs`
- Modify: `src/app.rs`
- Modify: `src/resource/mod.rs`
- Create: `tests/spherical_presentation_integration.rs`

**Interfaces:**
- Produces `PublishedSphericalPresentation`, `SphericalPresentationCandidate`, `SphericalPresentationError`, `build_spherical_external_artifacts`, `build_spherical_presentation_candidate`, and atomic publication.
- The candidate owns `Arc<SphericalNaturalFieldDocument>`, source, `Arc<SphericalEntityLocator>`, `Arc<PreparedProjectedMap>`, `Arc<PreparedGlobeMesh>`, and one `Arc<PreparedFieldLayers>`.

- [ ] **Step 1: Write exact graph-input, source, sharing, and failure tests**

Build external inputs for radius `6_371_000` and 162 cells, assert length 8 and exact Artifact keys/types including `SphericalSpaceArtifact` and excluding `PlanarSpaceArtifact`. Build a candidate and assert its report contains all spherical stage IDs and no planar spatial/natural stage ID.

Assert every candidate component source equals the document-derived source; map and globe presenter handles share one layers Arc by `Arc::ptr_eq`; map and globe source mismatch constructors return `SphericalPresentationError::SourceMismatch` even when counts match.

Publish a valid candidate, record all Arcs, state, clock, and report. Inject failures at document, locator, map, globe, layers, and GPU preparation boundaries through test-only failure points; each attempt must preserve every recorded handle and revision. A successful different root seed must atomically replace all source-bound handles.

- [ ] **Step 2: Run RED**

Run: `cargo test --test spherical_presentation_integration -- --nocapture`

- [ ] **Step 3: Implement the exact candidate pipeline**

Build external Artifacts in this order-independent set: `SphericalSpaceArtifact`, tectonic/geologic/climate/hydro specs, formation spec, rule-pack set, and author constraints. Call only `spherical_natural_foundation_graph()`.

Construct in strict order: verified document/source, locator, default Equal Earth map, globe, reconciled field layers, cross-validation, GPU-neutral packet. `PublishedSphericalPresentation::try_replace` accepts a complete candidate by value and swaps once. Projection updates and field updates use smaller candidates and retain the previous valid sub-cache on failure.

Add spherical resource aliases without disturbing legacy aliases:

```rust
pub type SphericalRendererResource = resource_impl::Resource<SphericalFieldRenderer>;
pub type SphericalPresentationResource =
    resource_impl::Resource<Option<Arc<PublishedSphericalPresentation>>>;
pub type SphericalViewerStateResource = resource_impl::Resource<SphericalFieldDisplayState>;
```

- [ ] **Step 4: Verify formal graph and atomic app boundary**

Run:

```powershell
cargo test --test spherical_presentation_integration -- --nocapture
cargo test --test spherical_natural_stage_graph -- --nocapture
cargo test --lib app::spherical_natural_display -- --nocapture
```

- [ ] **Step 5: Commit**

```powershell
git add src/app/spherical_presentation.rs src/app/spherical_natural_display.rs src/app.rs src/resource/mod.rs tests/spherical_presentation_integration.rs
git commit -m "feat: publish spherical natural presentation"
```

---

### Task 10: Cut over the single canvas, inspector, and legacy-origin migration

**Files:**
- Create: `src/ui/spherical.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/ui/field/localization.rs`
- Modify: `src/app.rs`
- Modify: `src/resource/mod.rs`
- Modify: `tests/spherical_presentation_integration.rs`

**Interfaces:**
- Produces `PersistedWorldOrigin::{LegacyPlanarV1,SphericalV1}`, `SphericalCanvasAction`, a single spherical canvas widget, view/projection/camera controls, fill/overlay controls, vector animation controls, and entity inspector.
- Missing persisted `world_origin` uses a custom serde default returning `LegacyPlanarV1`; Rust `Default` for a new app explicitly sets `SphericalV1`.

- [ ] **Step 1: Write migration and declarative UI RED tests**

Assert `TemplateApp::default()` serializes `"world_origin":"SphericalV1"`, includes explicit spherical radius/cell-count spec, and has no ordinary planar/spherical mode selector. Remove `world_origin` and spherical spec from JSON, deserialize, and assert legacy origin. Instantiate app runtime from each origin through test helpers: new state builds only the spherical graph; old state builds only the explicit legacy graph and exposes a compatibility notice/action.

Feed declarative UI actions and assert 2D/3D switching preserves fill, overlay, diagnostics, selected entity, separate map cameras, and globe camera. Projection/central-meridian action changes only map geometry; trackball/pan/zoom changes only uniforms; animation tick changes only phase uniform. Explicit `RegenerateAsSpherical` changes origin only after a successful spherical candidate publication.

Inspector tests must read from catalog/surface, not GPU: cell shows fill, vector east/north, magnitude, direction angle, unit, and cell diagnostics; edge shows edge value, owners, unit, and only global/field diagnostics. The same entity reports byte-equal formatted values in map and globe modes.

- [ ] **Step 2: Run RED**

Run: `cargo test --test spherical_presentation_integration -- --nocapture`

- [ ] **Step 3: Implement the single-canvas product UI**

Add top-level `SphericalViewMode::{Map,Globe}` segmented controls. Map mode shows projection, central meridian, reset map; globe mode shows reset globe. Fill list contains exactly the 32 fill-capable fields; overlay list contains none plus 2 edge and 2 vector fields. When a vector overlay is active, show play/pause, `显示速度（非物理时间）`, and Low/Medium/High glyph density.

The canvas issues one egui wgpu callback for the active presenter. Screen clicks convert through the active camera/projection to a unit direction and then use the shared locator. With edge overlay active, convert the fixed 8-logical-pixel radius through local map/globe scale and attempt incident-edge selection; otherwise select the cell.

`TemplateApp::new` dispatches by persisted origin. New/default `SphericalV1` uses default `SphericalSpaceSpec { radius: 6_371_000m, target_cell_count: 20_000 }`. Legacy state continues the current renderer and never changes origin implicitly. Save only author/UI state and source tag, not Artifacts or GPU caches.

- [ ] **Step 4: Verify UI, migration, and legacy compatibility**

Run:

```powershell
cargo test --test spherical_presentation_integration -- --nocapture
cargo test --test legacy_planar_boundary -- --nocapture
cargo test --lib natural_app_tests -- --nocapture
cargo test --lib ui -- --nocapture
```

- [ ] **Step 5: Commit**

```powershell
git add src/ui/spherical.rs src/ui/mod.rs src/ui/field/localization.rs src/app.rs src/resource/mod.rs tests/spherical_presentation_integration.rs
git commit -m "feat: make spherical world the default canvas"
```

---

### Task 11: Lock performance, wasm, GPU goldens, and end-to-end acceptance

**Files:**
- Modify: `tests/spherical_presentation_gpu.rs`
- Create: `tests/spherical_presentation_performance.rs`
- Modify: `tests/spherical_presentation_integration.rs`
- Modify: `docs/superpowers/plans/2026-08-08-spherical-presentation.md`
- Modify only if a test reveals a defect: files introduced in Tasks 1-10

**Interfaces:**
- Produces reproducible CPU/GPU measurement evidence, upload counters, stable offscreen fixture hashes, wasm compile evidence, and a complete acceptance audit.

- [ ] **Step 1: Add ignored Release 20k performance and memory test**

The ignored test must separately time/map byte-account these stages: Equal Earth geometry, globe geometry, locator, field layers, and medium wind glyph instances. Use public `resident_bytes()` methods that sum capacities with checked arithmetic. Assert combined bytes `<= 128 * 1024 * 1024` and combined CPU preparation `<= 1 second`; print each component and reference-machine metadata. It must also assert static second-frame large-upload deltas are zero.

- [ ] **Step 2: Freeze complete offscreen and invalidation goldens**

Cover map/globe scalar/category fills, edge scalar/category, vector paused/animated, seam fragments, poles, front/back visibility, and diagnostics. Hash RGBA8 fixtures with BLAKE3, store expected hashes in the test with width/height/pixel format, and keep CPU semantic assertions so a reviewed visual update cannot mask a wrong ID/value.

- [ ] **Step 3: Run automated acceptance gates from a clean status**

Run exactly:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --target-dir C:\Users\DYZ\Documents\dev\hemifuture\sekai\target
cargo clippy --workspace --all-targets --all-features --target-dir C:\Users\DYZ\Documents\dev\hemifuture\sekai\target -- -D warnings
cargo test --workspace --all-targets --all-features --target-dir C:\Users\DYZ\Documents\dev\hemifuture\sekai\target
cargo test --workspace --doc --target-dir C:\Users\DYZ\Documents\dev\hemifuture\sekai\target
cargo test --release --test spherical_presentation_performance -- --ignored --nocapture
cargo check --target wasm32-unknown-unknown --all-features --lib --target-dir C:\Users\DYZ\Documents\dev\hemifuture\sekai\target
git diff --check
git status --short
```

- [ ] **Step 4: Perform native and browser interaction smoke tests**

Use the same 1280×720, 20k-cell, elevation-fill, medium-wind-overlay scenario for 10 seconds. On native, verify map pan/zoom, Equal Earth/equirectangular switch, central-meridian seam, globe rotate/zoom, same-entity pick, edge pick, arrow pause/play, and no geometry deformation. On wasm in a real browser, repeat and record browser/version, hardware, average FPS, 1% low, and any GPU validation messages. Require native target 60 FPS and browser at least 30 FPS; if manual measurement infrastructure cannot calculate 1% low, do not claim this gate complete.

- [ ] **Step 5: Audit boundaries and update plan evidence**

Use `rg` to prove spherical presenters do not import legacy mesh/renderer or planar graph; globe mesh code does not mention elevation/relief/height; animation tick does not invoke geometry/glyph builders; and there is one spherical graph call path. Record RED/GREEN commands, commit IDs, byte/time measurements, smoke-test environment, golden hashes, and any intentional deviations under each task.

- [ ] **Step 6: Commit final gates**

```powershell
git add tests/spherical_presentation_gpu.rs tests/spherical_presentation_performance.rs tests/spherical_presentation_integration.rs docs/superpowers/plans/2026-08-08-spherical-presentation.md
git commit -m "test: lock spherical presentation acceptance"
```

---

## Plan Self-Review Checklist

- [x] Every design section 4-13 maps to at least one task and focused test.
- [x] All 36 fields are accounted for as exactly 32 fills, 2 edge overlays, and 2 vector overlays.
- [x] No task passes elevation or any field payload into globe geometry construction.
- [x] Source identity and atomic replacement cover document, locator, both geometries, field layers, and GPU state.
- [x] 2D seam/pole semantics and 3D unit-radius/backface semantics have automated tests.
- [x] Dynamic arrows distinguish scientific magnitude/direction from display-only phase/speed.
- [x] Persisted missing-tag migration is legacy-safe; new default is spherical and has no casual product toggle back to planar.
- [x] Every task contains a RED command, minimum implementation instruction, focused verification, and commit boundary.
- [x] Placeholder-marker scan reports no unfinished implementation instructions.
- [x] Native, wasm, fmt, Clippy, docs, full tests, offscreen GPU, performance, and manual smoke gates are explicit.
