# Spherical Authoring Compliance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make spherical layer visibility explicit and make authored initial crust and target land-area parameters independently correct, measurable, persistent, and fully tested.

**Architecture:** Add a relief-owned author spec and a pure area-weighted hypsometric selector, feed it only into the spherical relief stage, and cache resulting compliance metrics in the immutable document. Add renderer-neutral layer visibility to the existing canvas state and carry it through the fixed frame uniform without changing scientific packets or immutable GPU buffers.

**Tech Stack:** Rust 2024, serde, egui/eframe, wgpu/WGSL, existing StageGraph/Artifact system, cargo test/Clippy/wasm32.

## Global Constraints

- One authoritative current world only; no history slices, checkpoints, or published transient tectonic state.
- Height remains scalar annotation data; unit-globe vertices never move with elevation.
- Voronoi/Delaunay remains the closed-sphere numerical skeleton, never a second visible/debug geometry stack in the formal UI.
- Target land area changes only sea level and land classification, never height values, plate ownership, crust kind, coast components, or random streams.
- Initial continental crust and target land area are orthogonal author parameters with independently versioned validation.
- All area compliance uses authoritative spherical cell areas, never cell counts or framebuffer pixels.
- Static/camera/view/phase/layer-visibility updates perform no O(cell-count) scans or immutable uploads.
- LegacyPlanarV1 source, graph, hashes, RNG streams, and UI migration boundary remain frozen.
- Use TDD for every production behavior; capture the expected RED before implementation.
- No new external dependency.

---

### Task 1: Relief Spec and Area-Weighted Sea Level

**Files:**
- Create: `src/world/natural/relief_spec.rs`
- Create: `src/generators/natural/relief_spec.rs`
- Create: `src/generators/natural/land_fraction.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/world/natural/relief.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/natural_spec.rs`
- Test: `src/generators/natural/land_fraction.rs` module tests

**Interfaces:**
- Produces: `ReliefSpec { schema_version: u16, target_land_fraction: f32 }`.
- Produces: `RELIEF_SPEC_SCHEMA_V1`, `MIN_TARGET_LAND_FRACTION`, `MAX_TARGET_LAND_FRACTION`.
- Produces: `ReliefSpecArtifact::new(ReliefSpec)` and `ReliefSpecArtifact::spec()`.
- Produces crate-private `select_area_weighted_sea_level(cell_areas, elevation, target) -> Result<LandFractionSelection, LandFractionError>` where selection exposes `sea_level_m`, `actual_land_fraction`, and `target_land_fraction`.

- [x] **Step 1: Write failing spec tests**

Add tests that require V1 default `0.38`, valid inclusive bounds `0.05..=0.75`, rejection of NaN/out-of-range/schema, strict serde roundtrip, and artifact validation.

- [x] **Step 2: Run the spec RED**

Run: `cargo test --test natural_spec relief_spec -- --nocapture`

Expected: compile failure because `ReliefSpec` and its constants do not exist.

- [x] **Step 3: Implement the minimal validated spec and artifact**

Use a dedicated `ReliefSpecError`; do not reuse tectonic `NaturalSpecError`. `Deserialize` must validate before returning the value.

- [x] **Step 4: Write failing weighted-quantile tests**

Use a small synthetic surface/elevation fixture with unequal cell areas. Assert that a cell-count median gives the wrong result while the wished production selector chooses the closest real-area prefix, uses stable `CellId` ties, returns finite sea level, and keeps the input elevation bits unchanged.

- [x] **Step 5: Run the quantile RED**

Run: `cargo test --lib land_fraction::tests -- --nocapture`

Expected: compile failure on the missing selector or test-only public facade.

- [x] **Step 6: Implement and verify GREEN**

Implement one stable `O(N log N)` sort over cell-index ties. Reuse the existing centimeter-quantized `LandOceanKind::classify` semantics and recompute actual area from the resulting mask rather than trusting prefix arithmetic.

Run:

```powershell
cargo test --test natural_spec -- --nocapture
cargo test --lib land_fraction::tests -- --nocapture
cargo fmt --all -- --check
```

Expected: all exit 0.

- [x] **Step 7: Commit**

```powershell
git add src/world/natural/relief_spec.rs src/world/natural/mod.rs src/generators/natural/relief_spec.rs src/generators/natural/land_fraction.rs src/generators/natural/mod.rs tests/natural_spec.rs
git commit -m "feat: define spherical land-area target"
```

### Task 2: Stage Graph, App Persistence, and Atomic Publication

**Files:**
- Modify: `src/generators/natural/spherical_stage.rs`
- Modify: `src/app/spherical_presentation.rs`
- Modify: `src/app.rs`
- Modify: `src/world/natural/formation.rs`
- Test: `tests/spherical_natural_stage_graph.rs`
- Test: `tests/spherical_presentation_integration.rs`
- Test: `src/app.rs` module tests

**Interfaces:**
- `SphericalReliefStageInputs` consumes `Arc<ReliefSpecArtifact>`.
- `ReliefGenerator::generate_spherical` consumes `&ReliefSpec`.
- Every whole-world candidate builder consumes `&ReliefSpec`; smaller projection/field candidates remain unchanged.
- `TemplateApp` persists `relief_spec: ReliefSpec` with field-level default migration.
- `ResolvedWorldFormationPreset::recommended_land_fraction() -> f32` is the only preset mapping used by app selection.

- [ ] **Step 1: Write graph and relief-stage RED**

Assert the spherical graph declares `ReliefSpecArtifact` as an external dependency, `SphericalReliefStage` reads it, invalid values fail before relief construction, and changing only target land invalidates relief and downstream artifacts but leaves surface/tectonic/mantle hashes unchanged.

- [ ] **Step 2: Run graph RED**

Run: `cargo test --test spherical_natural_stage_graph -- --nocapture`

Expected: missing artifact/dependency and unchanged relief output make the new assertions fail.

- [ ] **Step 3: Implement stage integration**

Advance only `SphericalReliefStage::version`. Replace fixed `SEA_LEVEL_M` in the spherical path with the selected finite sea level; leave planar `ReliefGenerator::generate` at 0 m.

- [ ] **Step 4: Write app persistence/publication RED**

Cover old serialized `TemplateApp` without `relief_spec`, roundtrip of a manual target, named preset updating both explicit fields, Random preserving both, initial startup/rebuild passing the exact target, and invalid candidate preserving publication address/source/revisions/renderer counters.

- [ ] **Step 5: Run app RED**

Run:

```powershell
cargo test --lib natural_app_tests -- --nocapture
cargo test --test spherical_presentation_integration -- --nocapture
```

Expected: failures on missing persisted field/signatures and unchanged output.

- [ ] **Step 6: Implement app and publication plumbing**

Add the relief spec to external-artifact composition and all whole-world builders. Do not add it to projection/field candidates. Keep the existing renderer preflight → graph build → GPU prepare → single CPU assignment order.

- [ ] **Step 7: Verify GREEN and commit**

Run the three affected suites plus `cargo test --test legacy_planar_boundary -- --nocapture`; all must exit 0 and legacy hashes must be unchanged.

Commit: `feat: bind land target to spherical relief`

### Task 3: Cached Area Compliance Summary and Author UI

**Files:**
- Modify: `src/app/spherical_natural_display.rs`
- Modify: `src/app.rs`
- Modify: `src/app/spherical_presentation.rs` only if a narrow getter is required
- Test: `src/app/spherical_natural_display.rs` module tests
- Test: `tests/spherical_presentation_integration.rs`

**Interfaces:**
- Produces immutable `SphericalNaturalAreaSummary` with requested initial crust, evolved crust, target land, actual land, and sea level getters.
- `SphericalNaturalFieldDocument::area_summary()` returns `&SphericalNaturalAreaSummary` in O(1).

- [ ] **Step 1: Write document-summary RED**

Build a real outcome and independently recompute both area fractions from authoritative cells. Assert exact agreement, finite values, matching sea level/target, different source rebuild replacing the summary, and repeated reads causing zero catalog/diagnostic/field scans.

- [ ] **Step 2: Run RED**

Run: `cargo test --lib spherical_natural_display::tests::document_caches_authoritative_area_compliance -- --nocapture`

Expected: missing summary API.

- [ ] **Step 3: Implement cached summary**

Read resolved tectonic input and relief spec from the verified BuildOutcome only while constructing the document; store five scalars, not extra large Artifact Arcs.

- [ ] **Step 4: Write UI-copy RED**

Exercise an egui frame and require “面积依从性”, requested/evolved crust, target/actual land, signed pp delta, and sea level. Ensure no value is derived from screen pixels or raw cell count.

- [ ] **Step 5: Implement and verify GREEN**

Show one-decimal percentages and signed one-decimal percentage-point error below the spherical controls. Run document, app, and integration suites.

- [ ] **Step 6: Commit**

Commit: `feat: report spherical area compliance`

### Task 4: Persistent Formal Layer Visibility State

**Files:**
- Modify: `src/view/field_layers.rs`
- Modify: `src/ui/spherical.rs`
- Test: `src/view/field_layers.rs` module tests
- Test: `tests/spherical_presentation_integration.rs`

**Interfaces:**
- `SphericalFieldDisplayState::{fill_visible, overlay_visible, set_fill_visible, set_overlay_visible}`.
- `SphericalLayerVisibility { fill: bool, overlay: bool }` returned by state.
- `SphericalCanvasAction::{SetFillVisible(bool), SetOverlayVisible(bool)}`.

- [ ] **Step 1: Write state/action/persistence RED**

Assert defaults are visible, old wire data defaults to visible, roundtrip preserves both flags, changed actions return only presenter-uniform invalidation, identical actions return `NONE`, and packet/layer source/revisions/Arcs remain exact.

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test --lib field_layers -- --nocapture
cargo test --test spherical_presentation_integration layer_visibility -- --nocapture
```

Expected: missing methods/actions and missing persisted fields.

- [ ] **Step 3: Implement state and declarative actions**

Visibility must not enter `PreparedLayerState`, layer matching, diagnostic fingerprints, revision clocks, or packet keys.

- [ ] **Step 4: Write UI RED**

Run a real egui controls frame and require the “显示图层” group with three checkboxes. Toggling each emits exactly one corresponding action; overlay visibility is disabled when no overlay is selected but its stored preference is retained.

- [ ] **Step 5: Implement controls and verify GREEN**

Keep fill/overlay field ComboBoxes; place the new group immediately after them. Run UI and presentation integration suites.

- [ ] **Step 6: Commit**

Commit: `feat: restore spherical layer visibility controls`

### Task 5: GPU-Independent Fill, Overlay, and Diagnostic Visibility

**Files:**
- Modify: `src/gpu/spherical/callback.rs`
- Modify: `src/gpu/spherical/renderer.rs`
- Modify: `assets/shaders/spherical_field.wgsl`
- Modify: `src/ui/spherical.rs`
- Test: `src/gpu/spherical/renderer.rs` module tests
- Test: `tests/spherical_presentation_gpu.rs`

**Interfaces:**
- `SphericalPaintCallback::with_layer_visibility(SphericalLayerVisibility)`.
- `SphericalFrameUniform` adds aligned `fill_visible` and `overlay_visible` u32 flags.

- [ ] **Step 1: Write real offscreen RED**

For map and globe, render one source-bound packet into RGBA8 with: all visible; fill hidden/diagnostics off; fill hidden/diagnostics on with a known diagnostic cell; overlay hidden. Assert semantic foreground masks and unchanged immutable upload counters/installed key.

- [ ] **Step 2: Run required-GPU RED**

Run: `$env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test --lib gpu::spherical::renderer::tests::layer_visibility -- --nocapture`

Expected: visible output is unchanged because the flags do not exist.

- [ ] **Step 3: Implement uniform/shader plumbing**

Transparent base fill uses alpha blending; diagnostics can still replace transparent base. Overlay fragment discards when hidden. Keep geometry, palette, diagnostics, and instance buffers installed.

- [ ] **Step 4: Add same-frame callback integration RED/GREEN**

Use the public two-phase interact → publish → queue API and real renderer readback. Verify visibility captures the current persisted state and cannot use a stale callback packet.

- [ ] **Step 5: Verify affected GPU suites and commit**

Run renderer, callback adjacency, public GPU integration, Vulkan/GL audited goldens, and nonzero viewport tests. Existing all-visible hashes must remain byte-identical.

Commit: `feat: render independent spherical layer visibility`

### Task 6: Multi-Seed Compliance, Performance, Visual Smoke, and Final Gates

**Files:**
- Modify: `tests/spherical_morphology_quality.rs`
- Modify: `tests/spherical_natural_graph_performance.rs`
- Modify: `tests/spherical_natural_matrix.rs`
- Modify: `tests/spherical_tectonic_atlas.rs` to record the selected sea level and target/actual land fractions in atlas metadata
- Modify: this plan to append exact execution evidence

**Interfaces:**
- No new production interface; this task freezes cross-module acceptance.

- [ ] **Step 1: Write multi-seed compliance RED**

Add 17-seed 642-cell formation coverage, direct initial-crust monotonicity at `0.20/0.38/0.55`, land-target monotonicity at the same values with identical elevation bits, and five 20,252-cell Release cases with absolute target error `<=0.01`.

- [ ] **Step 2: Run RED then GREEN**

First run each focused test before refreshing hashes. Any scientific assertion must pass before changing downstream snapshot/GPU hashes.

- [ ] **Step 3: Release performance and memory**

Run: `cargo test --release --test spherical_natural_graph_performance -- --ignored --nocapture`

Require the existing 20k full-graph, tectonic, memory, upload, and static-frame budgets. Record added relief quantile duration separately; do not weaken frozen limits.

- [ ] **Step 4: Native and browser UI smoke**

At exact logical 1280×720, confirm the left panel shows both author sliders, actual four percentages, sea level, and the three layer checkboxes. Rebuild at target 20%, 38%, and 55%; inspect Equal Earth and globe; confirm area changes without unit-sphere deformation and edge/vector overlays remain independent.

- [ ] **Step 5: Run full engineering gates**

Run serially:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --target wasm32-unknown-unknown --workspace --all-features
cargo test --workspace --all-targets --all-features
cargo test --workspace --doc
$env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test
git diff --check
```

Every command must exit 0; required GPU tests must not skip.

- [ ] **Step 6: Boundary audit**

Confirm no planar graph imports in spherical relief, no elevation in unit-globe vertex construction, no history state, no second land-mask implementation, no per-frame area scan, and no immutable upload on layer visibility.

- [ ] **Step 7: Commit acceptance and verify clean completion**

Commit: `test: lock spherical authoring compliance`

Run `git status --short` and `git log -12 --oneline`; tracked worktree must be clean and this plan must contain exact RED/GREEN, hash, performance, UI, GPU, and commit evidence.
