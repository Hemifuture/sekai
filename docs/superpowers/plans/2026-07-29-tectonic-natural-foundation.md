# Tectonic Natural Foundation V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task by task. This repository session does not authorize sub-agent delegation, so execute and review each task in the active worktree.

**Goal:** Replace Sekai's default ellipse-driven terrain path with a deterministic, typed, finite-planar tectonic foundation that generates independent plates and crust, classifies relative-motion boundary segments, synthesizes explainable floating-point relief, and displays the formal fields in the real application.

**Architecture:** `world::natural` owns immutable specifications and validated snapshots. `generators::natural` owns integer-stable graph algorithms plus two typed stages: tectonics and relief. `app` is the only composition root that builds candidates and adapts core fields into the existing read-only field viewer. Legacy terrain stays compiled but no longer enters the default application chain.

**Tech Stack:** Rust 1.85, serde/serde_json, thiserror, BLAKE3, rand/rand_chacha, delaunator through the existing spatial stage, egui/eframe/wgpu through the existing display system, image/pollster for golden and GPU tests, native and `wasm32-unknown-unknown`.

**Design Source:** `docs/superpowers/specs/2026-07-29-tectonic-natural-foundation-design.md`

## Global Constraints

- Keep the existing single-package layout.
- Follow TDD for every behavioral change: add the focused test, observe the intended failure, implement the smallest coherent behavior, then refactor.
- `src/world/natural/**` must not import engine, generators, terrain, models, view, app, egui, eframe, wgpu, image, or noise.
- `src/generators/natural/**` must not import app, ui, gpu, view, models, egui, eframe, wgpu, or the legacy `terrain` module.
- New natural algorithms consume only `SpatialSnapshot` semantic geometry/topology and typed specs.
- Do not use `ContinentEllipse`, radial continent masks, old templates, `CellsData.height`, global RNG, wall-clock time, or `HashMap` iteration to define results.
- Plate ownership, crust category, boundary category, boundary segment identity, and land/ocean classification must use deterministic integer ordering and explicit tie breaks.
- Dense authoritative elevation and thickness values must be finite `f32` values with declared units and validated ranges. Do not introduce authoritative `u8` height.
- Plate and crust generation must use independent labeled substreams.
- Every core field has one writer: tectonics owns plate/crust/boundaries; relief owns elevation components and land/ocean.
- No history dates, event timelines, time integration, climate, hydrology, erosion, magic, society, or placeholder `WorldSnapshot`.
- Candidate builds and display packets publish atomically; failure retains the previous complete map.
- Do not mix legacy and formal spatial geometry in one visible application view.
- Do not delete legacy terrain modules in this plan; delete them only after a separate consumer audit.
- Public types and errors require concise rustdoc.
- Serialized collections and externally observable iteration use stable vectors or ordered collections.
- Each task ends with focused tests, `cargo fmt --all -- --check`, relevant Clippy, and an intentional commit.

## Target File Map

### Existing files modified

- `src/lib.rs` — continues to expose public module roots.
- `src/world/mod.rs` — exports natural contracts.
- `src/world/ids.rs` — adds `PlateId` and `BoundarySegmentId`.
- `src/view/field.rs` — supports borrowed core payload references.
- `src/view/mod.rs` — exports the borrowed payload type.
- `src/generators/mod.rs` — exports the natural generator.
- `src/app.rs` — switches the default composition to the formal natural graph.
- `src/app/legacy_display.rs` — implements the common private display-document boundary until later deletion.
- `src/ui/canvas/canvas.rs` — removes the legacy map dependency and tracks fit-to-view revision.
- `src/ui/canvas/widget_impl.rs` — renders the formal field packet and performs automatic fitting.
- `src/ui/canvas/input/state_manager.rs` — supports geological-scale zoom factors.
- `src/resource/mod.rs` — drops default-app dependence on legacy map/overlay resources where no longer needed.
- `.github/workflows/rust.yml` — adds the natural golden test to the test gate if integration tests are not already covered by the selected command.

### New world contracts

- `src/world/natural/mod.rs`
- `src/world/natural/spec.rs`
- `src/world/natural/tectonics.rs`
- `src/world/natural/relief.rs`
- `src/world/natural/fields.rs`

### New generator

- `src/generators/natural/mod.rs`
- `src/generators/natural/random.rs`
- `src/generators/natural/topology.rs`
- `src/generators/natural/tectonics.rs`
- `src/generators/natural/relief.rs`
- `src/generators/natural/stage.rs`

### New application adapter

- `src/app/field_document.rs`
- `src/app/natural_display.rs`

### New tests and reviewed artifacts

- `tests/natural_spec.rs`
- `tests/tectonic_contracts.rs`
- `tests/tectonic_generation.rs`
- `tests/tectonic_boundaries.rs`
- `tests/relief_contracts.rs`
- `tests/relief_generation.rs`
- `tests/natural_stage_graph.rs`
- `tests/natural_field_views.rs`
- `tests/natural_display_golden.rs`
- `tests/golden/natural-foundation/plate.png`
- `tests/golden/natural-foundation/crust.png`
- `tests/golden/natural-foundation/elevation.png`

## Task 1: Establish the Isolated Implementation Worktree and Baseline

**Files:**

- No source changes.

**Interfaces:**

- Consumes: clean `main` at the plan commit.
- Produces: isolated `feature/tectonic-natural-foundation` worktree with recorded green baseline.

- [ ] **Step 1: Read the required worktree and execution skills**

Read fully:

```text
superpowers:using-git-worktrees
superpowers:executing-plans
superpowers:test-driven-development
```

- [ ] **Step 2: Confirm the main worktree is clean and synchronized**

Run:

```powershell
git status --short --branch
git rev-parse HEAD
git rev-parse origin/main
```

Expected: clean `main`; local and remote revisions match the plan commit.

- [ ] **Step 3: Create the isolated feature worktree exactly as directed by the worktree skill**

Use branch:

```text
feature/tectonic-natural-foundation
```

Do not reuse or overwrite the existing field-display worktree.

- [ ] **Step 4: Run the baseline from the new worktree**

Run:

```powershell
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS with only the already documented ignored tests.

- [ ] **Step 5: Record the baseline revision in the execution notes**

No commit is required for a no-change task.

## Task 2: Let Field Views Borrow Core Payloads

**Files:**

- Modify: `src/view/field.rs`
- Modify: `src/view/mod.rs`
- Modify: `tests/field_view_contracts.rs`

**Interfaces:**

- Consumes: validated `FieldSchema` plus either existing `FieldData` or borrowed typed slices.
- Produces: `FieldPayloadRef<'a>`, `FieldView::from_payload`, and `FieldCatalog::from_payloads`.

- [ ] **Step 1: Add failing borrowed-payload contract tests**

Add tests proving:

```rust
let scalar = vec![0.0_f32, 0.5, 1.0];
let catalog = FieldCatalog::from_payloads(
    &registry,
    [(scalar_id(), FieldPayloadRef::ScalarF32(&scalar))],
)?;
assert!(std::ptr::eq(
    catalog.get(&scalar_id()).unwrap().view().unwrap().scalar_values().unwrap().as_ptr(),
    scalar.as_ptr(),
));
```

Also cover:

- category and vector slice borrowing;
- stable-ID target matching;
- bit-packed `Vec<bool>` borrowing;
- unknown payload ID rejection;
- duplicate payload ID rejection;
- schema/payload type mismatch;
- registered-but-absent payload behavior;
- existing `from_extension_fields` behavior and serialized `ExtensionFieldSet` immutability.

Run:

```powershell
cargo test --test field_view_contracts borrowed -- --nocapture
```

Expected: FAIL because `FieldPayloadRef` and `from_payloads` do not exist.

- [ ] **Step 2: Add the borrowed payload enum**

Implement a copyable reference enum:

```rust
pub enum FieldPayloadRef<'a> {
    ScalarF32(&'a [f32]),
    CategoryU32(&'a [u32]),
    Boolean(&'a Vec<bool>),
    Vector2F32(&'a [[f32; 2]]),
    StableIdU32 {
        target: StableIdKind,
        values: &'a [u32],
    },
}
```

Provide a private conversion from `&FieldData` so existing extension fields use the same `FieldView` implementation.

- [ ] **Step 3: Refactor `FieldView` to store `FieldPayloadRef`**

Requirements:

- `FieldView::new(schema, data)` remains source compatible;
- `FieldView::from_payload(schema, payload)` checks exact type and stable-ID target;
- value, length, scalar/category/vector/stable-ID accessors preserve semantics;
- boolean reads index through `Vec<bool>` without pretending it is a `&[bool]`;
- no source array copy.

- [ ] **Step 4: Add deterministic catalog construction**

`FieldCatalog::from_payloads` must:

- reject unknown IDs;
- reject duplicate payload IDs;
- iterate entries in registry order;
- retain missing schemas;
- never borrow the temporary map or iterator container after construction.

Use `BTreeMap`, not `HashMap`.

- [ ] **Step 5: Run focused and display regression tests**

Run:

```powershell
cargo test --test field_view_contracts
cargo test --test field_display_integration
cargo test --test field_display_golden
cargo test --lib app::legacy_display
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add -- src/view/field.rs src/view/mod.rs tests/field_view_contracts.rs
git commit -m "feat: borrow core field payloads"
```

## Task 3: Add Natural IDs, Units, and Semantic Specification

**Files:**

- Modify: `src/world/ids.rs`
- Modify: `src/world/mod.rs`
- Add: `src/world/natural/mod.rs`
- Add: `src/world/natural/spec.rs`
- Add: `tests/natural_spec.rs`

**Interfaces:**

- Consumes: user/world-law semantic tectonic settings.
- Produces: `PlateId`, `BoundarySegmentId`, `TectonicActivity`, and validated `TectonicSpec`.

- [ ] **Step 1: Add failing public contract tests**

Cover:

- typed ID raw round trips and serde;
- `TECTONIC_SPEC_SCHEMA_V1`;
- default earth-like values `12`, `0.38`, `Moderate`;
- plate count `2..=64`;
- continental fraction `0.10..=0.75`;
- finite fraction validation;
- unsupported schema rejection;
- deterministic JSON round trip.

Run:

```powershell
cargo test --test natural_spec
```

Expected: FAIL because the natural module does not exist.

- [ ] **Step 2: Add typed IDs**

Extend the existing ID macro invocations:

```rust
define_id!(PlateId, u32);
define_id!(BoundarySegmentId, u32);
```

Export them from `world::mod`.

- [ ] **Step 3: Implement the semantic spec and errors**

Do not expose algorithm tuning constants. Keep all fields serialized and documented.

Use a dedicated `NaturalSpecError` with exact variants for:

- unsupported schema;
- plate count;
- non-finite continental fraction;
- out-of-range continental fraction.

- [ ] **Step 4: Wire module exports and run tests**

Run:

```powershell
cargo test --test natural_spec
cargo test --test world_primitives
cargo test --test world_spec
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add -- src/world/ids.rs src/world/mod.rs src/world/natural tests/natural_spec.rs
git commit -m "feat: define tectonic world specification"
```

## Task 4: Define and Validate the Tectonic Snapshot

**Files:**

- Modify: `src/world/natural/mod.rs`
- Add: `src/world/natural/tectonics.rs`
- Add: `tests/tectonic_contracts.rs`

**Interfaces:**

- Consumes: complete ordered tectonic parts plus optional `SpatialSnapshot` for cross-validation.
- Produces: immutable `TectonicSnapshot`, plate/crust/boundary value accessors, and structured errors.

- [ ] **Step 1: Add failing snapshot contract tests**

Create a small valid spatial fixture and a matching tectonic fixture. Test:

- `PlateVelocity` integer mm/year bounds;
- contiguous `PlateId` table;
- cell plate values reference valid plates;
- raw plate/category slices are stable and borrowable;
- crust type decode/encode;
- crust thickness range by type;
- edge-aligned boundary record length;
- same-plate edge must be `None`;
- cross-plate edge must be non-`None`;
- strength finite and `0..=1`;
- segment IDs contiguous;
- segment member edges sorted, unique, and non-empty;
- every cross-plate edge belongs to exactly one segment;
- serde round trip;
- deserialized invalid data is rejected by `validate`;
- connectivity and edge ownership are rejected by `validate_against`.

Run:

```powershell
cargo test --test tectonic_contracts
```

Expected: FAIL because the tectonic types do not exist.

- [ ] **Step 2: Implement value types**

Add:

```text
PlateVelocity
Plate
CrustKind
BoundaryKind
BoundaryRecord
BoundarySegment
PlateIdField
CrustKindField
TectonicSnapshot
TectonicValidationError
```

Dense category wrappers own validated raw `Vec<u32>` so generic field display can borrow slices while ordinary domain access returns typed values.

- [ ] **Step 3: Implement self-validation**

Self-validation must not require an external artifact. It checks:

- schema and cardinalities stored in the snapshot;
- dense lengths;
- ID/range/ordering rules;
- segment partition rules that can be checked using stored edge IDs.

- [ ] **Step 4: Implement topology-aware validation**

`validate_against(&SpatialSnapshot)` additionally checks:

- exact cell/edge counts;
- every plate contains its seed;
- each plate's cells are connected;
- boundary records agree with spatial internal/outer edges and cell plate assignments;
- segment member adjacency and plate pairs agree with their edges.

- [ ] **Step 5: Run focused and serialization tests**

Run:

```powershell
cargo test --test tectonic_contracts
cargo test --test spatial_contracts
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add -- src/world/natural tests/tectonic_contracts.rs
git commit -m "feat: add validated tectonic snapshot"
```

## Task 5: Build Deterministic Natural Graph Primitives

**Files:**

- Modify: `src/generators/mod.rs`
- Add: `src/generators/natural/mod.rs`
- Add: `src/generators/natural/random.rs`
- Add: `src/generators/natural/topology.rs`

**Interfaces:**

- Consumes: `SpatialSnapshot` and one stage RNG.
- Produces: labeled private RNG substreams, a quantized topology index, stable seed selection, and integer multi-source graph distance/ownership.

- [ ] **Step 1: Add failing private unit tests**

Inside the new modules, build a small regular spatial fixture and prove:

- labeled substreams repeat exactly;
- consuming one label does not alter another label;
- topology index maps every neighbor pair to the correct edge;
- length, center, and area quantization are positive and stable;
- outer boundary cells are identified;
- farthest-point seeds are unique and exact for the fixture;
- multi-source ownership has stable tie breaks;
- distance propagation uses no negative or zero traversal cost;
- input edge order normalization cannot change results.

Run:

```powershell
cargo test --lib generators::natural
```

Expected: FAIL because the module does not exist.

- [ ] **Step 2: Implement fixed labeled substreams**

Read exactly 32 bytes once from `StageRng`, then derive each label through length-framed BLAKE3 into `ChaCha8Rng`.

Reject invalid internal labels in debug assertions; all labels are compile-time constants.

- [ ] **Step 3: Implement `NaturalTopologyIndex`**

Build ordered structures from semantic spatial records:

- per-cell neighbor arcs with `EdgeId` and quantized traversal length;
- edge owner lookup;
- fixed-point cell centers;
- quantized area weights;
- boundary cell set.

Do not mutate or duplicate polygon geometry.

- [ ] **Step 4: Implement stable graph helpers**

Add private helpers for:

- farthest-point seed selection with RNG-rotated stable ties;
- multi-source owner assignment;
- multi-source distance with optional maximum distance;
- stable priority queue entries implementing total `Ord`.

- [ ] **Step 5: Run focused tests and dependency scan**

Run:

```powershell
cargo test --lib generators::natural
rg -n "HashMap|thread_rng|rand::rng|egui|eframe|wgpu|crate::terrain|ContinentEllipse" src/generators/natural
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: tests pass; forbidden import scan has no result.

- [ ] **Step 6: Commit**

```powershell
git add -- src/generators/mod.rs src/generators/natural
git commit -m "feat: add deterministic natural graph primitives"
```

## Task 6: Generate Independent Plate and Crust Fields

**Files:**

- Modify: `src/generators/natural/mod.rs`
- Add: `src/generators/natural/tectonics.rs`
- Add: `tests/tectonic_generation.rs`

**Interfaces:**

- Consumes: spatial snapshot, validated tectonic spec, and stage RNG.
- Produces: an internal tectonic draft with connected plates, independent crust, and thickness.

- [ ] **Step 1: Add failing unit and integration tests**

Test the public generator entry point or a deliberately narrow crate-visible draft boundary:

- exact repeatability for one seed;
- a different seed changes plate or crust values;
- exact configured plate count with no empty plate;
- every plate region is connected;
- continental crust area is within one cell-area tolerance of the target;
- both crust kinds exist for valid non-extreme defaults;
- crust thickness falls in type-specific ranges;
- changing only `plate_count` leaves the crust field identical for the same spatial snapshot and root RNG seed;
- fixed quality fixture has at least one mixed-crust plate and one crust component spanning plates.

Run:

```powershell
cargo test --test tectonic_generation
```

Expected: FAIL because generation is absent.

- [ ] **Step 2: Implement plate seeds and partition**

Use:

- labeled `plate-seeds` stream;
- farthest-point seeds;
- common plate edge weights;
- one integer multi-source ownership pass;
- stable `(cost, PlateId, CellId)` ties.

Do not inspect crust or elevation.

- [ ] **Step 3: Implement independent crust nuclei and score**

Use distinct `crust-seeds` and `crust-shape` streams:

- choose interior-biased continental nuclei;
- choose boundary-soft-biased oceanic nuclei;
- compute continent and ocean graph distances;
- rank the signed distance contrast;
- select by quantized physical area;
- never read plate IDs while assigning crust.

- [ ] **Step 4: Derive crust thickness**

Use graph distance to the nearest crust transition plus a bounded, graph-smoothed regional variation.

Keep category and thickness ownership in the tectonic draft.

- [ ] **Step 5: Run tests and inspect a textual fixture summary**

Run:

```powershell
cargo test --test tectonic_generation -- --nocapture
cargo test --lib generators::natural::tectonics
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS with deterministic printed counts only under `--nocapture`.

- [ ] **Step 6: Commit**

```powershell
git add -- src/generators/natural tests/tectonic_generation.rs
git commit -m "feat: generate independent plates and crust"
```

## Task 7: Assign Motion, Classify Boundaries, and Aggregate Segments

**Files:**

- Modify: `src/generators/natural/tectonics.rs`
- Add: `tests/tectonic_boundaries.rs`
- Modify: `tests/tectonic_generation.rs`

**Interfaces:**

- Consumes: plate/crust draft and spatial edge semantics.
- Produces: complete validated `TectonicSnapshot`.

- [ ] **Step 1: Add failing motion and boundary tests**

Use handcrafted and generated fixtures to cover:

- all plate velocities remain inside `PlateVelocity` bounds;
- every adjacent plate pair satisfies the activity-specific relative-speed minimum;
- a deterministic candidate tie chooses the stable velocity;
- approaching continental sides classify as collision;
- approaching ocean/continent sides classify as subduction with oceanic polarity;
- receding continental sides classify as rift;
- receding oceanic sides classify as ridge;
- tangential motion classifies as transform;
- low relative motion classifies as weak;
- same-plate and outer edges are `None`;
- segment aggregation is invariant to input edge order;
- segments have continuous members, exact type/pair/polarity, and stable IDs.

Run:

```powershell
cargo test --test tectonic_boundaries
```

Expected: FAIL because motion and boundaries are absent.

- [ ] **Step 2: Build the plate adjacency graph**

Derive ordered adjacency from cross-plate spatial edges after partitioning.

- [ ] **Step 3: Assign fixed-point velocities**

Use the fixed integer candidate lattice and `plate-motion` substream:

- process plates in ID order;
- rotate candidate enumeration deterministically;
- maximize minimum squared relative velocity to assigned adjacent plates;
- choose stable candidate on ties;
- validate all final adjacent pairs.

- [ ] **Step 4: Classify each cross-plate edge**

Use quantized cell-center direction and integer relative velocity projections. Compute the normalized strength only after classification.

Subduction polarity uses crust kind, thickness, then stable plate ID as a final tie break.

- [ ] **Step 5: Aggregate deterministic segments**

Union only compatible edges sharing quantized endpoints and meeting the direction threshold. Sort components by minimum member edge before assigning IDs.

- [ ] **Step 6: Construct and validate the snapshot**

Call both self-validation and `validate_against` before returning.

- [ ] **Step 7: Run the complete tectonic suite**

Run:

```powershell
cargo test --test tectonic_contracts
cargo test --test tectonic_generation
cargo test --test tectonic_boundaries
cargo test --lib generators::natural
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 8: Commit**

```powershell
git add -- src/generators/natural/tectonics.rs tests/tectonic_generation.rs tests/tectonic_boundaries.rs
git commit -m "feat: classify tectonic boundary segments"
```

## Task 8: Integrate the Tectonic Stage and Artifact

**Files:**

- Modify: `src/generators/natural/mod.rs`
- Add: `src/generators/natural/stage.rs`
- Add: `tests/natural_stage_graph.rs`

**Interfaces:**

- Consumes: `SpatialArtifact` and external `TectonicSpecArtifact`.
- Produces: validated `TectonicArtifact` through `TectonicStage`.

- [ ] **Step 1: Add failing stage tests**

Cover:

- artifact serde and validation;
- exact keys:
  - `natural.tectonic-spec`
  - `world.tectonics`
- stage ID `natural.tectonics`;
- namespace `sekai.core`;
- version `1`;
- exact sorted dependencies;
- invalid spec fails before stage execution;
- successful build publishes only complete validated output;
- repeated build cache hit;
- changing only tectonic spec reuses cached spatial output but reruns tectonics;
- root seed change reruns both random stages.

Run:

```powershell
cargo test --test natural_stage_graph tectonic
```

Expected: FAIL because stage adapters do not exist.

- [ ] **Step 2: Add artifact wrappers**

`TectonicSpecArtifact::validate` delegates to spec validation.  
`TectonicArtifact::validate` delegates to snapshot self-validation.

- [ ] **Step 3: Add typed stage inputs and `TectonicStage`**

The stage:

- reads exactly spatial and tectonic spec;
- validates spec against available cell count;
- runs the generator with its isolated RNG;
- validates the result against spatial;
- maps failures to stable `natural.*` codes.

- [ ] **Step 4: Add a temporary test graph builder**

Tests may construct:

```rust
StageGraphBuilder::new()
    .external::<PlanarSpaceArtifact>()
    .external::<TectonicSpecArtifact>()
    .stage(SpatialStage)
    .stage(TectonicStage)
    .build()
```

Do not expose a temporary production graph that conflicts with the final natural graph.

- [ ] **Step 5: Run stage and engine regressions**

Run:

```powershell
cargo test --test natural_stage_graph tectonic
cargo test --test foundation_build
cargo test --test engine_execution
cargo test --test stage_graph
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add -- src/generators/natural tests/natural_stage_graph.rs
git commit -m "feat: publish tectonic stage artifacts"
```

## Task 9: Define and Validate the Relief Snapshot

**Files:**

- Modify: `src/world/natural/mod.rs`
- Add: `src/world/natural/relief.rs`
- Add: `tests/relief_contracts.rs`

**Interfaces:**

- Consumes: aligned elevation component arrays and sea-level classification.
- Produces: immutable `ReliefSnapshot` and explicit validation errors.

- [ ] **Step 1: Add failing relief contract tests**

Cover:

- finite component arrays;
- exact cell lengths;
- `-11_000..=9_000 m` elevation range;
- bounded component ranges;
- component identity within fixed tolerance;
- sea level finite;
- centimeter-quantized `Land` / `Ocean` consistency;
- both typed and raw category access;
- serde round trip;
- invalid deserialized values rejected;
- `validate_against` cell-count mismatch.

Run:

```powershell
cargo test --test relief_contracts
```

Expected: FAIL because relief contracts do not exist.

- [ ] **Step 2: Implement relief value types**

Add:

```text
LandOceanKind
LandOceanField
ElevationField
ReliefSnapshot
ReliefValidationError
```

Keep raw dense vectors private behind typed and display-oriented read-only accessors.

- [ ] **Step 3: Implement self and spatial validation**

Classification uses:

```text
round(elevation_m * 100) compared with round(sea_level_m * 100)
```

Document the exact shoreline convention.

- [ ] **Step 4: Run focused tests**

Run:

```powershell
cargo test --test relief_contracts
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add -- src/world/natural tests/relief_contracts.rs
git commit -m "feat: add validated relief snapshot"
```

## Task 10: Synthesize Crust Base and Tectonic Relief

**Files:**

- Modify: `src/generators/natural/mod.rs`
- Add: `src/generators/natural/relief.rs`
- Add: `tests/relief_generation.rs`

**Interfaces:**

- Consumes: spatial and tectonic snapshots plus relief-stage RNG.
- Produces: validated explainable `ReliefSnapshot`.

- [ ] **Step 1: Add failing causal relief tests**

Use deterministic generated and targeted fixtures to prove:

- continental interior base is higher than oceanic interior base;
- continental and oceanic margins transition toward the common sea-level neighborhood;
- continental collision yields positive tectonic offset near the boundary;
- subduction yields a negative trench on the subducting side and positive offset on the overriding side;
- oceanic divergence yields a positive ridge;
- continental divergence yields a negative rift center;
- transform amplitude remains below collision amplitude;
- cells outside an event's maximum support receive no contribution from it;
- regional field is deterministic, finite, bounded, and approximately zero mean;
- final elevation equals the three stored components;
- fixed default produces both land and ocean;
- changing only display state cannot affect relief bytes.

Run:

```powershell
cargo test --test relief_generation
```

Expected: FAIL because relief generation is absent.

- [ ] **Step 2: Implement crust base and signed margin distance**

Compute graph distance from crust-transition cells with bounded multi-source propagation. Combine:

- crust kind;
- thickness;
- signed transition distance;
- one fixed sea-level baseline.

- [ ] **Step 3: Convert boundary segments to side-specific effect sources**

Create fixed effect classes:

```text
collision uplift
trench
overriding arc/uplift
rift
ridge
transform
```

Each class receives one bounded multi-source propagation, not one full-map pass per segment.

- [ ] **Step 4: Apply compact smooth kernels**

Use polynomial compact support instead of platform-sensitive transcendental functions where practical. Preserve segment strength and side role.

- [ ] **Step 5: Generate regional graph-diffusion relief**

Use a separately labeled relief substream and fixed smoothing passes. Keep it independent of plate/crust category generation and below tectonic macro amplitudes.

- [ ] **Step 6: Sum, diagnose, quantize, and validate**

- Sum the three exact stored components.
- Clamp only at the final safety range.
- Emit bounded structured diagnostics for clamped cells.
- Classify land/ocean after centimeter quantization.
- Validate before return.

- [ ] **Step 7: Run focused and multi-seed property tests**

Run:

```powershell
cargo test --test relief_generation -- --nocapture
cargo test --test tectonic_generation
cargo test --test tectonic_boundaries
cargo test --lib generators::natural::relief
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 8: Commit**

```powershell
git add -- src/generators/natural/relief.rs src/generators/natural/mod.rs tests/relief_generation.rs
git commit -m "feat: synthesize tectonic relief fields"
```

## Task 11: Publish Relief and the Complete Natural Foundation Graph

**Files:**

- Modify: `src/generators/natural/stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `tests/natural_stage_graph.rs`

**Interfaces:**

- Consumes: spatial and tectonic artifacts.
- Produces: `ReliefArtifact`, `ReliefStage`, and `natural_foundation_graph`.

- [ ] **Step 1: Add failing full-graph tests**

Assert:

```text
spatial.planar-voronoi
natural.tectonics
natural.relief
```

in exact execution order, with:

- relief key `world.relief`;
- relief stage version/namespace/dependencies;
- complete external artifact set;
- deterministic artifact hashes and result hash;
- second build hits all three stages;
- tectonic spec change hits spatial and misses tectonic/relief;
- malformed relief output cannot publish;
- successful artifacts validate against one another.

Run:

```powershell
cargo test --test natural_stage_graph
```

Expected: FAIL until relief stage and graph exist.

- [ ] **Step 2: Implement `ReliefArtifact` and `ReliefStage`**

Map generator and validation failures to stable codes:

```text
natural.invalid-tectonics
natural.relief-failed
natural.invalid-relief
```

- [ ] **Step 3: Implement `natural_foundation_graph`**

Register exactly:

- external `PlanarSpaceArtifact`;
- external `TectonicSpecArtifact`;
- `SpatialStage`;
- `TectonicStage`;
- `ReliefStage`.

- [ ] **Step 4: Run full graph and foundation regressions**

Run:

```powershell
cargo test --test natural_stage_graph
cargo test --test foundation_build
cargo test --test engine_execution
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add -- src/generators/natural tests/natural_stage_graph.rs
git commit -m "feat: build the natural foundation stage graph"
```

## Task 12: Register Formal Natural Fields

**Files:**

- Modify: `src/world/natural/mod.rs`
- Add: `src/world/natural/fields.rs`
- Add: `tests/natural_field_views.rs`

**Interfaces:**

- Consumes: plate cardinality and stable natural field semantics.
- Produces: validated `FieldRegistry`, stable field ID constructors, units, dependencies, ranges, labels, and borrowed-view fixtures.

- [ ] **Step 1: Add failing schema tests**

Assert exact schemas for:

```text
plate_id
crust_kind
crust_thickness_km
plate_velocity
boundary_kind
boundary_strength
crust_base_elevation_m
tectonic_offset_m
regional_offset_m
elevation_m
land_ocean
```

Check:

- namespace/name/version;
- domain and payload type;
- custom unit symbols `km`, `cm/year`, and `m`;
- valid scalar ranges;
- missing policy;
- category labels;
- dependency closure and acyclicity;
- palette hints;
- stable registry serialization.

Run:

```powershell
cargo test --test natural_field_views schema
```

Expected: FAIL because field registration is absent.

- [ ] **Step 2: Implement stable field IDs and registry builder**

Use pure world contracts only. Dynamic plate category labels must be sorted and bounded by the validated maximum plate count.

- [ ] **Step 3: Add borrowed payload integration tests**

Build real small natural artifacts and a payload list. Prove:

- every registered produced field has matching domain cardinality;
- scalar/category arrays are borrowed;
- vector cell velocity is explicitly a derived display cache;
- edge fields remain inspectable but not cell-fill renderable;
- default elevation can prepare a complete cell field;
- plate/crust/elevation switching cannot mutate artifacts.

- [ ] **Step 4: Run field and display regressions**

Run:

```powershell
cargo test --test natural_field_views
cargo test --test field_view_contracts
cargo test --test field_display_integration
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add -- src/world/natural tests/natural_field_views.rs
git commit -m "feat: register natural foundation fields"
```

## Task 13: Add a Common App Field Document and Natural Adapter

**Files:**

- Add: `src/app/field_document.rs`
- Add: `src/app/natural_display.rs`
- Modify: `src/app/legacy_display.rs`
- Modify: `src/app.rs`

**Interfaces:**

- Consumes: immutable build artifacts and report.
- Produces: one private document interface used by common packet preparation and controls.

- [ ] **Step 1: Add failing private app tests**

Test:

- natural document borrows formal scalar/category arrays;
- derived cell velocities match plate records;
- complete `SpatialSnapshot` builds a required-completeness mesh;
- build diagnostics are converted without borrowing temporary report strings;
- default selected field is formal elevation;
- the default display range is symmetric around sea level;
- switching fields reuses mesh and untouched buffers;
- a failed candidate leaves the current document and packet unchanged;
- the legacy adapter still passes its existing tests through the common interface.

Run:

```powershell
cargo test --lib app::natural_display
cargo test --lib app::field_document
```

Expected: FAIL because modules do not exist.

- [ ] **Step 2: Extract the common private interface**

Move legacy-specific packet helpers behind:

```rust
trait AppFieldDocument {
    fn mesh(&self) -> &Arc<PreparedCellMesh>;
    fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError>;
    fn diagnostics(&self) -> &[OwnedViewDiagnostic];
    fn preferred_field(&self) -> Option<FieldId>;
    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode>;
}
```

Generic preparation functions must preserve the existing revision-sharing behavior.

- [ ] **Step 3: Implement `NaturalFieldDocument`**

Store `Arc` artifacts, registry, mesh, owned diagnostics, and only the explicitly derived cell-velocity cache.

Build payload refs on demand without duplicating authoritative arrays.

- [ ] **Step 4: Keep legacy compatibility tests green**

Do not alter legacy generation behavior yet. This task only moves common display preparation.

- [ ] **Step 5: Run app and display tests**

Run:

```powershell
cargo test --lib app::
cargo test --test field_display_integration
cargo test --test natural_field_views
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add -- src/app.rs src/app/field_document.rs src/app/natural_display.rs src/app/legacy_display.rs
git commit -m "feat: adapt natural artifacts for field display"
```

## Task 14: Switch the Default Application to Formal Natural Generation

**Files:**

- Modify: `src/app.rs`
- Modify: `src/ui/canvas/canvas.rs`
- Modify: `src/ui/canvas/widget_impl.rs`
- Modify: `src/ui/canvas/input/state_manager.rs`
- Modify: `src/resource/mod.rs`
- Add or modify private app/canvas unit tests.

**Interfaces:**

- Consumes: root seed and semantic tectonic settings from persisted app state.
- Produces: atomically replaced formal natural document and display packet.

- [ ] **Step 1: Add failing application-state tests**

Test pure/private helpers for:

- default planar extent `20_000 km × 10_000 km`;
- default target about `20_000` cells;
- default tectonic spec;
- exact external artifact set;
- build success extracts all three artifacts;
- build failure preserves the last complete display;
- no default-app call path references `TerrainGenerator`;
- canvas fit transform maps the world rectangle inside the allocated panel with margin;
- geological-scale zoom does not jump to `0.1`;
- a new mesh revision refits once, while field-only revisions preserve user pan/zoom.

Run:

```powershell
cargo test --lib app::
cargo test --lib ui::canvas
```

Expected: FAIL until the app is migrated.

- [ ] **Step 2: Replace template controls with semantic natural controls**

Expose only:

- root seed;
- generate-new-seed action;
- plate count;
- continental crust percentage;
- activity enum;
- rebuild action.

These are inputs to `WorldSpec`/`TectonicSpec`, not direct field mutations.

- [ ] **Step 3: Persist and reuse the bounded stage cache**

Add a skipped/defaulted `MemoryStageCache` field to the app so rebuilds can reuse unchanged spatial and upstream results.

- [ ] **Step 4: Build candidates atomically**

`generate_natural_world` must:

1. validate specs;
2. run `natural_foundation_graph`;
3. extract and cross-validate artifacts;
4. prepare `NaturalFieldDocument`;
5. prepare the complete display packet;
6. replace app document/state/resources only after every step succeeds.

On any error, publish status and keep the previous document/packet.

- [ ] **Step 5: Remove legacy geometry from the default canvas composition**

- Canvas no longer reads `MapSystemResource`.
- Field fill is the only active default map geometry.
- Do not register points/Delaunay/Voronoi renderer resources in `TemplateApp::new`.
- Remove the corresponding app fields and creation methods.
- Keep legacy modules compiled for later deletion.

- [ ] **Step 6: Implement fit-to-view**

On a changed mesh revision:

- read `local_extent`;
- compute a finite uniform scale with a fixed screen margin;
- center the map in the allocated canvas rectangle;
- record the fitted mesh revision.

Change zoom limits to safely include the geological default scale.

- [ ] **Step 7: Run app, canvas, and bundle tests**

Run:

```powershell
cargo test --lib app::
cargo test --lib ui::canvas
cargo test --test natural_stage_graph
cargo test --test natural_field_views
cargo check --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 8: Run the app once and inspect logs**

Run:

```powershell
cargo run
```

Expected:

- one formal spatial/tectonic/relief build;
- no legacy template generation log;
- no panic, validation failure, or missing GPU resource.

Stop the process after confirming startup; visual review is Task 16.

- [ ] **Step 9: Commit**

```powershell
git add -- src/app.rs src/ui/canvas src/resource/mod.rs
git commit -m "feat: run formal natural generation by default"
```

## Task 15: Add Natural Golden and Multi-Seed Quality Gates

**Files:**

- Add: `tests/natural_display_golden.rs`
- Add: `tests/golden/natural-foundation/plate.png`
- Add: `tests/golden/natural-foundation/crust.png`
- Add: `tests/golden/natural-foundation/elevation.png`
- Modify: `.github/workflows/rust.yml` only if required to execute the integration golden gate.

**Interfaces:**

- Consumes: fixed small natural foundation build and generic CPU reference renderer.
- Produces: reviewed deterministic images and objective quality metrics.

- [ ] **Step 1: Add failing quality tests before creating goldens**

For a fixed set of at least eight seeds, assert:

- plates are connected and non-empty;
- boundary partition and non-comoving invariants;
- continental fraction tolerance;
- both land and ocean;
- at least one mixed-crust plate in the quality set;
- at least one crust component crossing plate boundaries;
- no non-finite values;
- elevation component identity;
- no single plate or continental component consumes an unreasonable default share;
- generated field dimensions and display meshes match.

Run:

```powershell
cargo test --test natural_display_golden quality -- --nocapture
```

Expected: RED until helper/build assertions are complete, then GREEN before golden files are accepted.

- [ ] **Step 2: Add ignored golden regeneration**

Use:

```text
SEKAI_UPDATE_NATURAL_GOLDENS=1
```

and an ignored `regenerate_natural_goldens` test, following the established field-display pattern.

Render:

- plate category;
- crust category;
- elevation with symmetric sea-level range.

Use a reviewed fixed seed and a test-scale cell count large enough to show macro shapes without making CI slow.

- [ ] **Step 3: Generate candidate PNGs**

Run:

```powershell
$env:SEKAI_UPDATE_NATURAL_GOLDENS='1'
cargo test --test natural_display_golden regenerate_natural_goldens -- --ignored --exact
Remove-Item Env:SEKAI_UPDATE_NATURAL_GOLDENS
```

- [ ] **Step 4: Inspect every image**

Use local image inspection on all three PNGs. Reject and adjust the generator if:

- continents are visibly composed from a few ellipses;
- plate and crust boundaries coincide;
- elevation is flat noise with no boundary-aligned structures;
- holes, seams, invalid colors, or clipping appear.

Do not approve a golden solely because the test can reproduce it.

- [ ] **Step 5: Make golden comparisons pass**

Run:

```powershell
cargo test --test natural_display_golden
cargo test --test field_display_golden
```

Expected: PASS; only regeneration tests ignored.

- [ ] **Step 6: Ensure CI runs the test**

The current CI `cargo test --lib` does not execute integration tests. Add a focused natural golden step or expand the test job without dropping existing gates.

- [ ] **Step 7: Commit**

```powershell
git add -- tests/natural_display_golden.rs tests/golden/natural-foundation .github/workflows/rust.yml
git commit -m "test: verify natural foundation output"
```

## Task 16: Profile and Visually Validate the Real Application

**Files:**

- Modify only if evidence reveals a defect.

**Interfaces:**

- Consumes: release natural build and actual wgpu application.
- Produces: measured baseline, screenshots, and verified interaction behavior.

- [ ] **Step 1: Read the required systematic-debugging and computer-use skills if a defect appears or desktop interaction is needed**

Use `computer-use` for local desktop inspection. If any unexpected result appears, switch to `superpowers:systematic-debugging` before changing code.

- [ ] **Step 2: Record release generation timing**

Use a focused ignored performance test or a release diagnostic invocation around the default 20,000-cell build. Record:

- spatial time;
- tectonic time;
- relief time;
- total time;
- cell/edge/plate/segment counts;
- approximate dense output bytes;
- land and continental crust fractions.

Do not add a brittle machine-specific millisecond assertion.

- [ ] **Step 3: Run the actual application**

Run the release app in a hidden/background-safe way supported by the environment, then inspect the visible window.

Verify:

- elevation is selected by default;
- map fits the canvas;
- no ellipse-composed continents;
- no white holes;
- meaningful ocean/land/mountain structure;
- switching to plate and crust fields works without rebuild;
- plate/crust boundaries visibly differ;
- field inspector values and units are correct;
- pan, zoom, and cell selection work;
- generating the same seed reproduces the map;
- a new seed changes it;
- a field-only control does not regenerate the world.

- [ ] **Step 4: Capture and inspect screenshots**

Capture at minimum:

- elevation;
- plate;
- crust.

Inspect at original detail. If quality fails, add the smallest failing automated regression and return to the responsible task.

- [ ] **Step 5: Run a static architecture scan**

Run:

```powershell
rg -n "ContinentEllipse|TerrainGenerator|CellsData|MapSystem|egui|eframe|wgpu|HashMap|thread_rng|rand::rng" src/world/natural src/generators/natural
rg -n "TerrainGenerator|generate_terrain_with_template|LegacyTerrainDisplayAdapter" src/app.rs
```

Expected:

- no forbidden natural-module hits;
- no legacy generator call in default app composition.

- [ ] **Step 6: Commit only evidence-driven fixes**

Use focused commit messages matching the defect. Do not create a no-op “visual review” commit.

## Task 17: Run Final Verification, Review the Diff, and Publish

**Files:**

- All files changed by the plan.

**Interfaces:**

- Consumes: completed feature branch.
- Produces: verified commit series merged to `main` and pushed.

- [ ] **Step 1: Read the verification and branch-finishing skills**

Read fully:

```text
superpowers:verification-before-completion
superpowers:finishing-a-development-branch
```

- [ ] **Step 2: Run focused suites**

Run:

```powershell
cargo test --test natural_spec
cargo test --test tectonic_contracts
cargo test --test tectonic_generation
cargo test --test tectonic_boundaries
cargo test --test relief_contracts
cargo test --test relief_generation
cargo test --test natural_stage_graph
cargo test --test natural_field_views
cargo test --test natural_display_golden
```

Expected: PASS.

- [ ] **Step 3: Run the complete native gates**

Run:

```powershell
cargo test --workspace --all-targets
cargo test --workspace --all-targets --release
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets --all-features
```

Expected: PASS with only explicitly documented ignored regeneration/performance tests.

- [ ] **Step 4: Run wasm and Trunk gates**

Run:

```powershell
$oldRustflags = $env:RUSTFLAGS
$oldRustdocflags = $env:RUSTDOCFLAGS
$env:RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
$env:RUSTDOCFLAGS='--cfg getrandom_backend="wasm_js"'
cargo check --workspace --all-features --lib --target wasm32-unknown-unknown
$wasmExit = $LASTEXITCODE
trunk build
$trunkExit = $LASTEXITCODE
$env:RUSTFLAGS = $oldRustflags
$env:RUSTDOCFLAGS = $oldRustdocflags
if ($wasmExit -ne 0) { exit $wasmExit }
if ($trunkExit -ne 0) { exit $trunkExit }
```

Expected: PASS.

- [ ] **Step 5: Review architecture and scope**

Run:

```powershell
git diff main...HEAD --check
git diff main...HEAD --stat
git log --oneline main..HEAD
git status --short
```

Manually verify:

- no unrelated user files changed;
- no hidden history model;
- no legacy generator enters the default path;
- no core/display dependency inversion;
- no duplicate authoritative field writer;
- design and implementation agree.

- [ ] **Step 6: Review the implementation against every completion criterion**

Use a fresh self-review pass. Since sub-agent review is not authorized in this session, do not invoke delegated review tools.

Any found issue returns to a failing test and focused fix commit.

- [ ] **Step 7: Merge according to the branch-finishing skill**

The user previously selected merge-to-main and authorized push for this project. Confirm `main` has not moved unexpectedly, merge the verified feature branch without destructive reset, then rerun the smoke gate on merged `main`:

```powershell
cargo test --test natural_stage_graph
cargo test --test natural_display_golden
cargo check --all-targets
```

- [ ] **Step 8: Push and verify the remote**

```powershell
git push origin main
git status --short --branch
git rev-parse HEAD
git rev-parse origin/main
```

Expected: clean `main`; local and remote revisions identical.

- [ ] **Step 9: Run the merged application for the user**

Start the actual merged app and leave it available long enough for user inspection, unless the environment requires the process to be stopped before handoff.

## Completion Gate

This plan is complete only when:

- formal spatial → tectonic → relief artifacts build through the engine;
- plate and crust are independently generated and validated;
- every cross-plate edge is classified and segmented;
- adjacent plates satisfy non-comoving constraints;
- explainable float elevation components and sea-level classification are valid;
- default app no longer calls the ellipse-driven generator;
- formal plate, crust, boundary, component, and elevation fields are inspectable;
- borrowed core fields avoid whole-array adapter copies;
- fixed multi-seed properties and reviewed natural goldens pass;
- the actual application has been visually inspected at elevation, plate, and crust views;
- native, release, Clippy, fmt, wasm, Trunk, and integration gates pass;
- commits are merged and pushed to `main`;
- no unresolved major direction decision remains inside this slice.
