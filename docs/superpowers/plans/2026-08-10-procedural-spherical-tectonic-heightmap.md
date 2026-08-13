# Procedural Spherical Tectonic Heightmap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Replace the field-weighted spherical Voronoi end state with a bounded Cortial-style plate/crust evolution and derive the first publishable heightmap directly from its single final current state.

**Architecture:** The authoritative `SphericalSurfaceSnapshot` remains the only geometry/topology source. A crate-private, two-buffer tectonic workspace evolves attributed crust samples through rigid rotations, contact classification, Cortial processes and periodic deterministic resampling; only a validated V3 current snapshot is published. Relief consumes that immutable current crust through a separate one-way heightmap module, while rendering remains an undeformed unit sphere.

**Tech Stack:** Rust 2024, existing `noise 0.9` OpenSimplex primitives, `rand_chacha` labeled streams, serde/thiserror contracts, existing stage engine, optional deterministic per-index Rayon optimization, wgpu presentation tests.

## Global Constraints

- Implement the science from Cortial et al. 2019, Sections 3–5 and Appendix A; PlaTec is only the mature coherent-noise initialization reference and no PlaTec/LGPL code is copied.
- Internal evolution is exactly 128 steps of 2 My by default, using only current/next buffers; no history vector, checkpoint, historical Arc, serialized time slice or timeline UI is permitted.
- `TectonicSpec::plate_count` means initial plate count; the published active count is the normalized final count in `2..=64`.
- Voronoi–Delaunay remains the authoritative numerical sampling/topology and initial partition only; no shortest-arrival, power/Laguerre or region-growth result may become the final production owner/land mask.
- Noise may perturb initial conditions, bounded process parameters and oriented final detail; it may not directly choose final plate owners or final land/ocean, change event direction, redraw every step or displace sphere vertices.
- The globe is always a unit sphere. Height is a scalar annotation and never enters mesh positions, camera radius, picking or projection geometry.
- Formation presets select coherent-noise spectra, the existing target continental fractions and bounded Cortial multipliers; they do not enforce a fixed final component count or perform post-hoc land reshaping.
- The new implementation has one function for each stable concept: rigid velocity, relative contact velocity, subduction side, oceanic-age subsidence, spherical noise, resampling and final tectonic height.
- Native and WASM receive identical quantized current facts for the same seed/spec/surface. Stable ordering and explicit tie-breaks are mandatory.
- Default 20,252-cell Release target: tectonics plus initial heightmap <= 2 s, hard limit 5 s; WASM target <= 5 s, hard limit 10 s; added peak working set <= 256 MiB.
- No new dependency, no legacy-planar output change, no planar fallback and no hidden quality mode.
- Every task follows RED → verify RED → minimal GREEN → focused and adjacent verification → commit. Semantic oracles must pass before updating any golden hash.

---

## File Structure

Create or split the following focused production files:

```text
src/generators/natural/morphology/noise.rs
    One deterministic 3D coherent-noise and sparse-Gabor implementation.

src/generators/natural/spherical_tectonics/model.rs
    Formation recipe plus pure transient sample, active-lineage and state types.
src/generators/natural/spherical_tectonics/workspace.rs
    Current/next buffers plus reusable contact and process scratch assembly.
src/generators/natural/spherical_tectonics/initial_state.rs
    Initial spherical Voronoi plates and PlaTec-style coherent crust state.
src/generators/natural/spherical_tectonics/kinematics.rs
    Rigid rotations, tangent velocity and deterministic nearest-site walking.
src/generators/natural/spherical_tectonics/contacts.rs
    Coverage overlap/gap and relative-motion classification only.
src/generators/natural/spherical_tectonics/resample.rs
    Moving samples back to one sample per authoritative cell and final components.
src/generators/natural/spherical_tectonics/processes/mod.rs
src/generators/natural/spherical_tectonics/processes/subduction.rs
src/generators/natural/spherical_tectonics/processes/collision.rs
src/generators/natural/spherical_tectonics/processes/spreading.rs
src/generators/natural/spherical_tectonics/processes/rifting.rs
src/generators/natural/spherical_tectonics/processes/relaxation.rs
    Orthogonal Cortial process functions; no process calls another process.
src/generators/natural/spherical_tectonics/runner.rs
    The sole 128-step orchestration and final candidate assembly entry.

src/generators/natural/spherical_relief/tectonic_heightmap.rs
    Final current crust to explainable coarse height components.
src/generators/natural/spherical_relief/directed_noise.rs
    Fold/ridge-aligned detail using the shared morphology noise core.
```

Retain these files as thin adapters/contracts:

```text
src/generators/natural/spherical_tectonics.rs
src/generators/natural/spherical_relief.rs
src/generators/natural/spherical_stage.rs
src/world/natural/spherical_tectonics.rs
```

Delete `spherical_tectonics/{plates,crust,motion}.rs` only in the production switch task after their consumers and tests have moved. Keep `boundaries.rs` as the final-owner boundary aggregator, adapted to V3 current crust.

---

### Task 1: V3 Current-Crust Contract

**Files:**
- Modify: `src/world/natural/spherical_tectonics.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Modify: `tests/spherical_tectonic_contracts.rs`

**Interfaces:**
- Consumes: existing `CrustKindField`, `SphericalPlate`, `SurfaceRef`, `CellId` and bounded serde helpers.
- Produces: `TECTONIC_SNAPSHOT_SCHEMA_V3`, `SphericalOrogenyKind`, `SphericalCrustState`, V3 `SphericalTectonicSnapshot::new`, and compatibility getters used by every later task.

- [x] **Step 1: Write V3 construction, serde and rejection tests**

Add tests that construct a two-plate spherical fixture and call this exact interface:

```rust
let crust = SphericalCrustState::new(
    CrustKindField::from_kinds(kinds),
    thickness_km,
    age_myr,
    tectonic_elevation_m,
    lineation_east,
    lineation_north,
    orogeny_kind,
    orogeny_age_myr,
)?;
let snapshot = SphericalTectonicSnapshot::new(
    TECTONIC_SNAPSHOT_SCHEMA_V3,
    surface_ref,
    plates,
    PlateIdField::from_ids(owners),
    crust,
    boundaries,
    segments,
)?;
```

Assert JSON round-trip equality; V2 rejection; dense-length rejection for each new field; oceanic age in `0.0..=512.0`; continental age exactly `CONTINENTAL_CRUST_AGE_SENTINEL_MYR`; unit-or-zero lineation; None orogeny paired with `NO_OROGENY_AGE_SENTINEL_MYR`; finite elevation in `ELEVATION_MIN_M..=ELEVATION_MAX_M`; and existing surface/connectivity validation.

- [x] **Step 2: Run the contract test and verify RED**

Run: `cargo test --test spherical_tectonic_contracts -- --nocapture`

Expected: compile failure for missing `SphericalCrustState`, `SphericalOrogenyKind` and `TECTONIC_SNAPSHOT_SCHEMA_V3`.

- [x] **Step 3: Implement the V3 struct-of-arrays contract**

Use one validated struct and delegate legacy getters to it:

```rust
pub const TECTONIC_SNAPSHOT_SCHEMA_V3: u16 = 3;
pub const CONTINENTAL_CRUST_AGE_SENTINEL_MYR: f32 = -1.0;
pub const NO_OROGENY_AGE_SENTINEL_MYR: f32 = -1.0;
pub const MAX_CRUST_AGE_MYR: f32 = 512.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SphericalOrogenyKind { None, Andean, Himalayan }

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalCrustState {
    kinds: CrustKindField,
    thickness_km: Vec<f32>,
    age_myr: Vec<f32>,
    tectonic_elevation_m: Vec<f32>,
    lineation_east: Vec<f32>,
    lineation_north: Vec<f32>,
    orogeny_kind: Vec<SphericalOrogenyKind>,
    orogeny_age_myr: Vec<f32>,
}
```

Deserialize through a bounded wire, call `SphericalCrustState::new`, store `crust: SphericalCrustState` in the snapshot, and preserve `crust_kinds()` / `crust_thickness_km()` as zero-copy delegates. Add getters for every new dense field and update `resident_bytes` helpers later rather than duplicating storage.

Keep the branch buildable before Task 9 by adding a strictly crate-private `SphericalCrustState::from_pre_evolution_fields(kinds, thickness)` bridge and changing the current facade to emit V3 with neutral zero tectonic elevation plus neutral age/lineation/orogeny. Mark the bridge in source as temporary and remove it in Task 9; it is not public, serialized as a second schema or retained as a final fallback.

- [x] **Step 4: Run focused and adjacent contract tests**

Run:

```powershell
cargo test --test spherical_tectonic_contracts -- --nocapture
cargo test --test spherical_tectonic_generation -- --nocapture
cargo test --test spherical_relief_contracts -- --nocapture
cargo test --test natural_field_registry_spherical -- --nocapture
```

Expected: all pass; old V2 fixture code is migrated rather than accepted through a compatibility constructor.

- [x] **Step 5: Commit**

```powershell
git add src/world/natural/spherical_tectonics.rs src/world/natural/mod.rs src/generators/natural/spherical_tectonics.rs tests/spherical_tectonic_contracts.rs tests/spherical_relief_contracts.rs
git commit -m "feat: define spherical crust state v3"
```

---

### Task 2: One Spherical Noise Core and Formation Recipes

**Files:**
- Create: `src/generators/natural/morphology/noise.rs`
- Modify: `src/generators/natural/morphology/mod.rs`
- Modify: `src/generators/natural/morphology/field.rs`
- Modify: `src/generators/natural/relief_noise.rs`
- Modify: `src/generators/natural/random.rs`
- Create: `src/generators/natural/spherical_tectonics/model.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Modify: `src/world/natural/formation.rs`
- Test: `tests/spherical_tectonic_generation.rs`

**Interfaces:**
- Consumes: existing `FractalProfile`, `noise::OpenSimplex`, `LabeledSubstreams` and `ResolvedWorldFormationPreset`.
- Produces: crate-private `SphericalNoise3d`, `GaborKernel`, `FormationTectonicRecipe::for_preset`, `LabeledSubstreams::counter_u64`, and seven new orthogonal RNG labels.

- [x] **Step 1: Write noise and recipe RED tests**

Test seam/pole continuity, seed determinism, bounded fBm/ridged output, a Gabor ridge aligned with the supplied tangent, independent label streams, and these preset orderings:

```rust
assert!(FormationTectonicRecipe::for_preset(Supercontinent).base_scale_rad
    > FormationTectonicRecipe::for_preset(Archipelago).base_scale_rad);
assert!(FormationTectonicRecipe::for_preset(GreatIsland).rift_rate_permille
    < FormationTectonicRecipe::for_preset(Continents).rift_rate_permille);
assert!(FormationTectonicRecipe::for_preset(VolcanicIslands).island_arc_gain_permille
    > FormationTectonicRecipe::for_preset(Continents).island_arc_gain_permille);
```

Also scan production source to assert `OpenSimplex` is constructed only in `morphology/noise.rs` for spherical work.

- [x] **Step 2: Run focused tests and verify RED**

Run: `cargo test --lib generators::natural::morphology -- --nocapture`

Expected: compile failure for missing noise module and recipe.

- [x] **Step 3: Move the existing coherent core and implement bounded sparse Gabor**

Expose only this crate-private API:

```rust
pub(in crate::generators::natural) struct SphericalNoise3d {
    octaves: [OpenSimplex; MAX_FRACTAL_OCTAVES],
    gabor_seed: u32,
}
#[derive(Clone, Copy)]
pub(in crate::generators::natural) struct GaborKernel {
    pub envelope_scale_rad: f64,
    pub carrier_frequency: f64,
    pub impulse_count: u8,
}
impl SphericalNoise3d {
    pub fn new(seed: u32) -> Self;
    pub fn fbm(&self, direction: UnitVector3, profile: FractalProfile) -> f64;
    pub fn ridged(&self, direction: UnitVector3, profile: FractalProfile) -> f64;
    pub fn sparse_gabor(
        &self,
        direction: UnitVector3,
        tangent: [f64; 3],
        kernel: GaborKernel,
    ) -> f64;
}
```

Project the sample delta into the local tangent plane, evaluate Lagae's Gaussian envelope times cosine carrier, sum a fixed seed-derived kernel set, normalize by the analytic amplitude bound and clamp to `[-1, 1]`. `field.rs` and `relief_noise.rs` must delegate to this implementation.

- [x] **Step 4: Add formation recipes and labeled streams**

Define integer multipliers and finite spectra, not behavior branches:

```rust
#[derive(Clone, Copy)]
pub(in crate::generators::natural) struct FormationTectonicRecipe {
    pub initial_crust_profile: FractalProfile,
    pub base_scale_rad: f64,
    pub rift_rate_permille: u16,
    pub subduction_gain_permille: u16,
    pub island_arc_gain_permille: u16,
}
```

Add labels `initial-plates-v3`, `initial-crust-v3`, `plate-motion-v3`, `rift-events-v3`, `process-variation-v3`, `orogenic-detail-v3`, `oceanic-detail-v3`. Tests must prove consuming any one stream cannot perturb another.

Add a counter-based read that hashes the captured root, label and explicit coordinates without advancing mutable RNG state:

```rust
pub(super) fn counter_u64(&self, label: &'static str, coordinates: &[u64]) -> u64;
```

Assert `(label, step, lineage)` repeats exactly and changing any coordinate changes the deterministic value. State-changing event decisions use this API instead of conditional stream consumption.

- [x] **Step 5: Run focused and existing noise tests**

Run:

```powershell
cargo test --lib generators::natural::morphology -- --nocapture
cargo test --lib generators::natural::relief_noise -- --nocapture
cargo test --test world_formation_spec -- --nocapture
```

Expected: all pass, with no public noise API and no new dependency.

- [x] **Step 6: Commit**

```powershell
git add src/generators/natural/morphology src/generators/natural/relief_noise.rs src/generators/natural/random.rs src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics/model.rs src/world/natural/formation.rs tests/spherical_tectonic_generation.rs
git commit -m "feat: add shared spherical tectonic noise"
```

---

### Task 3: Transient Model and PlaTec-Style Initial State

**Files:**
- Modify: `src/generators/natural/spherical_tectonics/model.rs`
- Create: `src/generators/natural/spherical_tectonics/workspace.rs`
- Create: `src/generators/natural/spherical_tectonics/initial_state.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Test: module tests in `workspace.rs` and `initial_state.rs`
- Test: `tests/spherical_tectonic_generation.rs`

**Interfaces:**
- Consumes: surface cells/adjacency, `TectonicSpec`, `FormationTectonicRecipe`, shared noise and labeled streams.
- Produces: `LineageId`, `CrustSample`, `ActivePlate`, `TectonicState`, assembly-only `TectonicWorkspace`, `InitialStateError`, `build_initial_state`.

- [x] **Step 1: Write initial-state RED tests**

On 42-, 162- and 642-cell fixtures assert exactly one sample per cell, unit positions, exactly `spec.plate_count` connected initial owners, stable seed ownership, continental area within one maximum-cell area of the requested fraction, continental/oceanic thickness and age semantics, formation spectral ordering, and changed seed changes state.

The test must retain `initial_owners()` and later compare them with final owners; the production snapshot must not expose them.

- [x] **Step 2: Run the new module tests and verify RED**

Run: `cargo test --lib spherical_tectonics::initial_state -- --nocapture`

Expected: compile failure for missing module/types.

- [x] **Step 3: Implement compact current/next state types**

Use fixed-size sample records and capacity-reused buffers:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct LineageId(u32);

#[derive(Clone, Copy, Debug)]
pub(super) struct CrustSample {
    pub position: UnitVector3,
    pub anchor: CellId,
    pub owner: LineageId,
    pub kind: CrustKind,
    pub thickness_km: f32,
    pub age_myr: f32,
    pub tectonic_elevation_m: f32,
    pub lineation: [f32; 2],
    pub orogeny: SphericalOrogenyKind,
    pub orogeny_age_myr: f32,
}

pub(super) struct TectonicState {
    pub samples: Vec<CrustSample>,
    pub plates: Vec<ActivePlate>,
}
```

Keep the pure types above in `model.rs`. Put the following orchestration owner in `workspace.rs`, so `model` never imports `processes`:

```rust
pub(super) struct TectonicWorkspace {
    pub current: TectonicState,
    pub next: TectonicState,
}
```

Do not derive serde or expose these types outside `generators::natural`.

The initializer signature is:

```rust
pub(super) fn build_initial_state(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
) -> Result<TectonicState, InitialStateError>;
```

- [x] **Step 4: Implement mature initial conditions**

`build_initial_state(surface, topology, spec, formation, streams)` must:

1. choose stable farthest-point seeds;
2. assign the initial nearest seed by great-circle distance plus a bounded low-frequency field warp;
3. sample the formation's 3D coherent field at every unit direction;
4. area-sort by `(score, CellId)` and choose the quantile crossing nearest the requested continental area;
5. assign paper-range thickness/elevation and oceanic age from independent bounded fields;
6. assign one rigid rotation per initial lineage.

No connected-component trimming or region growth is allowed.

- [x] **Step 5: Run focused tests and mutation check**

Run the test once with the noise contribution temporarily multiplied by zero and record that the spectral/seed oracle fails; restore it and rerun:

```powershell
cargo test --lib spherical_tectonics::initial_state -- --nocapture
cargo test --test spherical_tectonic_generation every_formation -- --nocapture
```

- [x] **Step 6: Commit**

```powershell
git add src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics/model.rs src/generators/natural/spherical_tectonics/initial_state.rs tests/spherical_tectonic_generation.rs
git commit -m "feat: initialize attributed spherical crust"
```

---

### Task 4: Rigid Kinematics and Local Spherical Location

**Files:**
- Create: `src/generators/natural/spherical_tectonics/kinematics.rs`
- Modify: `src/generators/natural/spherical_tectonics/model.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Test: module tests in `kinematics.rs`

**Interfaces:**
- Consumes: `CrustSample`, `ActivePlate`, authoritative adjacency and existing `SphericalPlateRotation`.
- Produces: `KinematicsError`, `rigid_velocity`, `rotate_direction`, `walk_nearest_cell`, `advance_samples`.

- [x] **Step 1: Write analytic RED tests**

Test the existing formula and the new step integration independently:

```rust
let velocity = rigid_velocity(rotation, radius, radial)?;
for (actual, expected) in velocity.into_iter().zip(omega_cross_position) {
    assert!((actual - expected).abs() <= 1.0e-9);
}
let moved = rotate_direction(radial, rotation, 2.0)?;
assert!((norm(moved.components()) - 1.0).abs() <= 16.0 * f64::EPSILON);
assert_eq!(walk_nearest_cell(surface, start, surface.cell(target)?.site), target);
```

Cover seam, both poles, equal-dot tie to lowest `CellId`, 128 repeated steps versus one analytic rotation, and non-finite/cap overflow errors.

- [x] **Step 2: Run and verify RED**

Run: `cargo test --lib spherical_tectonics::kinematics -- --nocapture`

Expected: missing module/functions.

- [x] **Step 3: Implement one shared kinematics path**

Use Rodrigues rotation with `angle = angular_rate_rad_per_year * delta_myr * 1_000_000.0`, then normalize through `UnitVector3::new`. `walk_nearest_cell` repeatedly moves to the incident neighbor with greatest dot product and uses the lowest ID on exact ties; a visited-step bound of `surface.cells().len()` returns a typed error rather than looping.

`advance_samples` writes by index into the preallocated next buffer and updates only position/anchor. It must call `rigid_velocity` rather than reimplementing `w × p`.

Use these signatures throughout later tasks:

```rust
pub(super) fn rigid_velocity(
    rotation: SphericalPlateRotation,
    radius: Meters,
    radial: UnitVector3,
) -> Result<[f64; 3], KinematicsError>;
pub(super) fn rotate_direction(
    direction: UnitVector3,
    rotation: SphericalPlateRotation,
    delta_myr: f64,
) -> Result<UnitVector3, KinematicsError>;
pub(super) fn walk_nearest_cell(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    start: CellId,
    direction: UnitVector3,
) -> Result<CellId, KinematicsError>;
pub(super) fn advance_samples(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    current: &TectonicState,
    next: &mut TectonicState,
    delta_myr: f64,
) -> Result<(), KinematicsError>;
```

- [x] **Step 4: Run focused and existing rotation tests**

Run:

```powershell
cargo test --lib spherical_tectonics::kinematics -- --nocapture
cargo test --test spherical_tectonic_contracts euler_rotation -- --nocapture
cargo test --test spherical_picking -- --nocapture
```

- [x] **Step 5: Commit**

```powershell
git add src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics/model.rs src/generators/natural/spherical_tectonics/kinematics.rs
git commit -m "feat: evolve rigid spherical plates"
```

---

### Task 5: Coverage and Contact Classification

**Files:**
- Create: `src/generators/natural/spherical_tectonics/contacts.rs`
- Modify: `src/generators/natural/spherical_tectonics/workspace.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Test: module tests in `contacts.rs`

**Interfaces:**
- Consumes: moved samples, anchors, active rotations and authoritative edge tangent frames.
- Produces: `CoverageScratch`, `ContactEvent`, `ContactKind`, `ContactError`, and `build_contacts` with no state mutation.

- [x] **Step 1: Write classification RED tests**

Create synthetic two-lineage fixtures for gap, same-owner coverage, oceanic/continental convergence, older-oceanic ocean/ocean convergence, continent/continent collision, divergence and transform. Assert side selection, signed normal speed, overlap/gap membership and stable event ordering `(cell, edge, owner pair)`.

- [x] **Step 2: Run and verify RED**

Run: `cargo test --lib spherical_tectonics::contacts -- --nocapture`

Expected: missing contact types/functions.

- [x] **Step 3: Implement read-only coverage and contact construction**

Use a compressed bucket layout, reused each step:

```rust
pub(super) struct CoverageScratch {
    counts: Vec<u32>,
    offsets: Vec<u32>,
    sample_indices: Vec<u32>,
}

pub(super) enum ContactKind {
    Gap,
    Transform,
    Divergence,
    OceanicSubduction { descending: LineageId },
    ContinentalCollision,
}
```

Count anchors, prefix-sum offsets, fill sample indices in original stable order, and classify with the shared rigid/relative velocity functions. The module must not change owner, crust or elevation.

At this task, extend `TectonicWorkspace` with reusable `coverage: CoverageScratch` and `events: Vec<ContactEvent>` fields. Task 6 adds the final `ProcessActions` scratch field; later tasks must not change the workspace layout.

```rust
pub(super) fn build_contacts(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    moved: &TectonicState,
    coverage: &mut CoverageScratch,
    events: &mut Vec<ContactEvent>,
) -> Result<(), ContactError>;
```

- [x] **Step 4: Run focused tests and source-boundary guard**

Run:

```powershell
cargo test --lib spherical_tectonics::contacts -- --nocapture
cargo test --test spherical_tectonic_generation spherical_boundaries -- --nocapture
```

Add a source scan asserting `contacts.rs` contains no assignment to sample material fields and no noise import.

- [x] **Step 5: Commit**

```powershell
git add src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics/workspace.rs src/generators/natural/spherical_tectonics/contacts.rs
git commit -m "feat: classify moving crust contacts"
```

---

### Task 6: Subduction and Continental Collision

**Files:**
- Create: `src/generators/natural/spherical_tectonics/processes/mod.rs`
- Create: `src/generators/natural/spherical_tectonics/processes/subduction.rs`
- Create: `src/generators/natural/spherical_tectonics/processes/collision.rs`
- Modify: `src/generators/natural/spherical_tectonics/workspace.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Test: module tests in both process files

**Interfaces:**
- Consumes: immutable `ContactEvent`, source samples and one mutable next-state target slice.
- Produces: reusable `ProcessActions`, `ProcessStats`, `ProcessError`, `apply_subduction`, `apply_collision` and `commit_process_actions`.

- [x] **Step 1: Write paper-curve RED tests**

Freeze Appendix-A constants in one `processes::constants` block and test endpoints/monotonicity for trench depth, overriding-side uplift, collision uplift versus terrane area and speed, ocean/ocean age selection, forced small-terrane subduction, Andean/Himalayan classification and lineation tangency.

The causal fixture must assert:

```rust
assert!(descending.tectonic_elevation_m < baseline_ocean);
assert!(overriding.tectonic_elevation_m > baseline_overriding);
assert_eq!(overriding.orogeny, SphericalOrogenyKind::Andean);
assert_eq!(collision.orogeny, SphericalOrogenyKind::Himalayan);
```

- [x] **Step 2: Run and verify RED**

Run: `cargo test --lib spherical_tectonics::processes -- --nocapture`

Expected: missing process modules.

- [x] **Step 3: Implement pure transfer functions and mutations**

Keep equations pure and reusable:

```rust
fn subduction_profile(distance_m: f64, speed_mm_yr: f64, gain: f64) -> (f32, f32);
fn collision_uplift_m(terrane_area_m2: f64, speed_mm_yr: f64, overlap: f64) -> f32;
pub(super) fn apply_subduction(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
) -> Result<ProcessStats, ProcessError>;
pub(super) fn apply_collision(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
) -> Result<ProcessStats, ProcessError>;
```

`ProcessActions` owns capacity-reused disposition flags and spawned samples. Process modules may update material fields by stable sample index, but they only mark removal/transfer/spawn actions and never change `next.samples` length while contact events still reference it. `commit_process_actions` performs one stable in-place compaction and appends spawned samples after every process has run.

Add `actions: ProcessActions` to `TectonicWorkspace` and implement `step_parts()` to return disjoint current/next/coverage/event/action borrows.

Transfer current terrane ownership only after the overlap threshold; do not retain a transfer history. Slab pull may update only the current active rotation and must remain within the existing 120 mm/year cap.

- [x] **Step 4: Run tests and deliberate side-selection mutation**

Run focused tests, then temporarily invert the chosen descending side and verify the ocean/continent and ocean/ocean tests fail before restoring:

```powershell
cargo test --lib spherical_tectonics::processes -- --nocapture
cargo test --test spherical_tectonic_contracts -- --nocapture
```

- [x] **Step 5: Commit**

```powershell
git add src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics/workspace.rs src/generators/natural/spherical_tectonics/processes
git commit -m "feat: model spherical subduction and collision"
```

---

### Task 7: Spreading, Rifting and Coarse Relaxation

**Files:**
- Create: `src/generators/natural/spherical_tectonics/processes/spreading.rs`
- Create: `src/generators/natural/spherical_tectonics/processes/rifting.rs`
- Create: `src/generators/natural/spherical_tectonics/processes/relaxation.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/mod.rs`
- Test: module tests in the three new files

**Interfaces:**
- Consumes: gap/divergence events, active plates, current crust, formation recipe and counter-based rift-event values.
- Produces: `fill_spreading_gaps`, `maybe_rift_plates`, `relax_current_crust`.

- [x] **Step 1: Write RED tests for each phenomenon**

Test new gap crust is oceanic, age zero and ridge-high; age increases exactly 2 My per step; oceanic elevation is non-increasing with age; transform contacts do not receive convergence uplift; continental divergence uses the bounded McKenzie pure-shear `β` recipe to thin crust and the shared Airy function to subside it; rifts use deterministic Poisson draws, split into 2–4 children, diverge, stop at 64 and never reuse a live lineage ID; continental erosion and trench sediment terms hit the paper endpoints.

- [x] **Step 2: Run and verify RED**

Run: `cargo test --lib spherical_tectonics::processes -- --nocapture`

Expected: missing spreading/rifting/relaxation symbols.

- [x] **Step 3: Implement the three orthogonal modules**

Use one shared event probability function with a fixed-point Padé approximation for `1 - exp(-lambda * delta)`; do not call the platform `exp` implementation on a state-changing branch:

```rust
fn poisson_event(draw: u64, rate_q32_per_myr: u64, delta_myr: u16) -> bool {
    let threshold_q64 = poisson_threshold_q64(rate_q32_per_myr, delta_myr);
    u128::from(draw) < threshold_q64
}
```

Test the fixed-point threshold against the analytic reference across the complete supported rate range, require monotonicity and an error below one Q32 probability unit, and use only the integer threshold for the actual decision.

Derive one draw per `(step, lineage)` from `counter_u64("rift-events-v3", &[step, lineage])`; there is no mutable draw order and no step-by-lineage allocation. Use the paper's perturbed Voronoi fracture inside the selected plate domain only. For each divergent contact, index only the strongest extensional speed per current sample, compute the McKenzie-style bounded `β = 1 + extension / 400 km` (`β <= 1.2` per 2 Myr step), divide continental thickness by `β`, and apply the shared Airy old/new elevation delta to the current crust. This pass must reuse `ProcessActions` scratch storage and must not allocate a cell-count buffer per step. `relax_current_crust` increments ages, applies oceanic subsidence, linear continental erosion and trench sediment in one indexed pass.

The runner-facing interfaces are fixed as:

```rust
pub(super) fn fill_spreading_gaps(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
) -> Result<ProcessStats, ProcessError>;
pub(super) fn apply_divergent_extension(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    delta_myr: f32,
) -> Result<ProcessStats, ProcessError>;
pub(super) fn maybe_rift_plates(
    step: u16,
    surface: &SphericalSurfaceSnapshot,
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
) -> Result<ProcessStats, ProcessError>;
pub(super) fn relax_current_crust(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    next: &mut TectonicState,
    recipe: FormationTectonicRecipe,
    delta_myr: f32,
) -> Result<ProcessStats, ProcessError>;
```

- [x] **Step 4: Run process suite and capacity mutation**

Temporarily remove the 64-plate guard and prove the capacity test fails; restore and run:

```powershell
cargo test --lib spherical_tectonics::processes -- --nocapture
cargo test --test natural_spec -- --nocapture
```

- [x] **Step 5: Commit**

```powershell
git add src/generators/natural/spherical_tectonics/processes
git commit -m "feat: model spherical spreading and rifting"
```

---

### Task 8: Deterministic Resampling and Final Topology Canonicalization

**Files:**
- Create: `src/generators/natural/spherical_tectonics/resample.rs`
- Modify: `src/generators/natural/spherical_tectonics/model.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Test: module tests in `resample.rs`

**Interfaces:**
- Consumes: moved current samples, coverage, authoritative surface and active lineages.
- Produces: `resample_current_state`, `canonicalize_final_plates`, `CanonicalTectonicState`, `ResampleError`.

- [x] **Step 1: Write resampling/canonicalization RED tests**

Cover one candidate, overlapping candidates, a filled gap, stable equal-distance ties, barycentric material interpolation, source cardinality, disconnected same-lineage domains, empty lineages, representative-cell rule, 2/64 bounds and 65-component typed failure.

Assert canonicalization preserves every crust material bit and only remaps owners/plate metadata.

- [x] **Step 2: Run and verify RED**

Run: `cargo test --lib spherical_tectonics::resample -- --nocapture`

Expected: missing resampling functions.

- [x] **Step 3: Implement periodic resampling**

Choose the interval from maximum angular displacement and clamp it to `10..=60`. For each authoritative cell, choose or blend source samples by spherical triangle barycentric weights; use lowest sample index for exact ties. Gaps must already have a spreading sample or return `UnresolvedCoverageGap`.

Reuse `next.samples` capacity, then `std::mem::swap(&mut current, &mut next)` and clear logical lengths without reallocating.

```rust
pub(super) fn resample_current_state(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    workspace: &mut TectonicWorkspace,
) -> Result<(), ResampleError>;
```

- [x] **Step 4: Implement final connected-component normalization**

Flood the owner-induced cell graph. Sort components by `(lineage_id, representative_cell)`; remove empty lineages; split disconnected ones; select representative by nearest cell to normalized area-weighted direction, with lowest-ID fallback for a degenerate mean; assign dense `PlateId`s and inherit the lineage rotation. Reject normalized count outside `2..=64`.

```rust
pub(super) fn canonicalize_final_plates(
    surface: &SphericalSurfaceSnapshot,
    state: TectonicState,
) -> Result<CanonicalTectonicState, ResampleError>;
```

- [x] **Step 5: Run focused tests and owner-preservation mutation**

Temporarily skip disconnected-component splitting and prove its fixture fails; restore and run:

```powershell
cargo test --lib spherical_tectonics::resample -- --nocapture
cargo test --test spherical_tectonic_contracts -- --nocapture
```

- [x] **Step 6: Commit**

```powershell
git add src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics/model.rs src/generators/natural/spherical_tectonics/resample.rs
git commit -m "feat: resample and normalize spherical plates"
```

---

### Task 9: The Sole Bounded Tectonic Runner

**Files:**
- Create: `src/generators/natural/spherical_tectonics/runner.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Modify: `src/generators/natural/spherical_tectonics/boundaries.rs`
- Delete: `src/generators/natural/spherical_tectonics/plates.rs`
- Delete: `src/generators/natural/spherical_tectonics/crust.rs`
- Delete: `src/generators/natural/spherical_tectonics/motion.rs`
- Test: `tests/spherical_tectonic_generation.rs`
- Test: `tests/spherical_tectonic_contracts.rs`

**Interfaces:**
- Consumes: Tasks 1–8 modules.
- Produces: `RunnerError`, `run_tectonic_evolution` and the only production `TectonicGenerator::generate_spherical` path.

- [x] **Step 1: Write end-to-end RED tests against the wished V3 behavior**

Assert V3, deterministic bytes, changed seed, final plate count bounds rather than equality to input, all owners connected, every final representative owned, nonzero crust ages/elevations/lineation/orogeny, at least one ridge/trench/uplift causal relation, and final owners materially differ from captured initial owners.

Add a source-architecture test that rejects production calls to old `generate_plate_partition`, `generate_crust`, `sample_spherical_field`, `calibrate_owner_biases` and `grow_*region` from the spherical facade/runner.

- [x] **Step 2: Run end-to-end test and verify RED**

Run: `cargo test --test spherical_tectonic_generation -- --nocapture`

Expected: old V2 snapshot and old final-owner semantics fail.

- [x] **Step 3: Implement the fixed runner sequence**

The only loop is:

```rust
pub(super) fn run_tectonic_evolution(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    formation: ResolvedWorldFormationPreset,
    streams: &LabeledSubstreams,
) -> Result<CanonicalTectonicState, RunnerError> {
    let recipe = FormationTectonicRecipe::for_preset(formation);
    let initial = build_initial_state(surface, topology, spec, recipe, streams)?;
    let mut workspace = TectonicWorkspace::from_initial(initial);
    for step in 0..128_u16 {
        let (current, next, coverage, events, actions) = workspace.step_parts();
        advance_samples(surface, topology, current, next, 2.0)?;
        build_contacts(surface, topology, next, coverage, events)?;
        apply_subduction(surface, events, current, next, actions, recipe)?;
        apply_collision(surface, events, current, next, actions, recipe)?;
        fill_spreading_gaps(surface, events, current, next, actions, recipe)?;
        maybe_rift_plates(step, surface, current, next, actions, recipe, streams)?;
        relax_current_crust(surface, events, next, recipe, 2.0)?;
        commit_process_actions(next, actions)?;
        workspace.swap_current_next();
        if resample_due(step, &workspace) {
            resample_current_state(surface, topology, &mut workspace)?;
        }
    }
    if workspace.requires_resample() {
        resample_current_state(surface, topology, &mut workspace)?;
    }
    canonicalize_final_plates(surface, workspace.current)
}
```

The real implementation must keep each process call separate and propagate typed errors. No branch may invoke the old final morphology.

- [x] **Step 4: Assemble V3 and final boundaries atomically**

Adapt `boundaries.rs` to read final `SphericalCrustState`, aggregate boundary segments once, call `SphericalTectonicSnapshot::new(TECTONIC_SNAPSHOT_SCHEMA_V3, surface_ref, plates, owners, crust, boundaries, segments)`, and validate against the surface before returning. Only after GREEN delete the three obsolete field-driven domain modules and the temporary `from_pre_evolution_fields` bridge.

- [x] **Step 5: Run focused/adjacent tests and deliberate old-owner mutation**

Temporarily return initial owners at final assembly and prove the anti-Voronoi/causal test fails; restore and run:

```powershell
cargo test --test spherical_tectonic_generation -- --nocapture
cargo test --test spherical_tectonic_contracts -- --nocapture
cargo test --test spherical_tectonic_mantle_stage -- --nocapture
cargo test --lib spherical_tectonics -- --nocapture
```

- [x] **Step 6: Commit**

```powershell
git add src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics tests/spherical_tectonic_generation.rs tests/spherical_tectonic_contracts.rs
git commit -m "feat: evolve final spherical tectonic state"
```

---

### Task 10: Current-State Initial Heightmap

**Files:**
- Create: `src/generators/natural/spherical_relief/tectonic_heightmap.rs`
- Create: `src/generators/natural/spherical_relief/directed_noise.rs`
- Modify: `src/generators/natural/spherical_relief.rs`
- Modify: `src/generators/natural/relief_noise.rs`
- Delete: `tests/spherical_field_driven_relief.rs`
- Create: `tests/spherical_tectonic_heightmap.rs`
- Test: `tests/spherical_relief_generation.rs`

**Interfaces:**
- Consumes: final V3 crust, surface, mantle offsets and `orogenic-detail-v3` / `oceanic-detail-v3` streams.
- Produces: `TectonicHeightmapError`, `TectonicHeightComponents` and the existing V4 `SphericalReliefSnapshot` fields.

- [x] **Step 1: Write height causality RED tests**

Use constructed V3 current crust to assert: thicker continental crust raises bounded isostatic base; old oceanic crust is deeper; a trench stays negative beside positive overriding uplift; collision orogeny forms a positive lineated ridge; ridge age/direction controls detail; crust kind alone does not determine land; sea level remains exactly zero.

Assert changing only detail seed changes `regional_offset_m` but leaves tectonic snapshot, `crust_base_elevation_m`, `tectonic_offset_m` and land-scale causal ordering unchanged.

- [x] **Step 2: Run and verify RED**

Run: `cargo test --test spherical_tectonic_heightmap -- --nocapture`

Expected: current relief ignores V3 age/elevation/lineation/orogeny fields.

- [x] **Step 3: Implement the one-way component builder**

```rust
pub(super) struct TectonicHeightComponents {
    pub crust_base_m: Vec<f32>,
    pub tectonic_offset_m: Vec<f32>,
    pub directed_detail_m: Vec<f32>,
}

pub(super) fn build_tectonic_heightmap(
    surface: &SphericalSurfaceSnapshot,
    tectonic: &SphericalTectonicSnapshot,
    streams: &LabeledSubstreams,
) -> Result<TectonicHeightComponents, TectonicHeightmapError>;
```

Compute bounded freeboard/isostasy, then `tectonic_offset = tectonic_elevation - crust_base`, then orient shared Gabor detail with lineation and decay it with orogeny age. Quantize each component before summation and use the existing final safety reconciliation only for documented field ranges, never to reverse a sign or flatten a causal pair.

- [x] **Step 4: Integrate with mantle without coupling modules**

`ReliefGenerator::generate_spherical` calls `build_tectonic_heightmap`, obtains volcanic offset through the existing mantle path, sums explainable fields, classifies `LandOceanField` at 0 m and validates V4. The heightmap module must not import mantle, hydrology, app, view or GPU.

- [x] **Step 5: Run focused/adjacent tests and no-deformation guard**

Run:

```powershell
cargo test --test spherical_tectonic_heightmap -- --nocapture
cargo test --test spherical_relief_generation -- --nocapture
cargo test --test spherical_relief_contracts -- --nocapture
cargo test --test spherical_presentation_mesh unit_globe -- --nocapture
```

Source-scan globe mesh/shader paths for absence of elevation/displacement inputs.

- [x] **Step 6: Commit**

```powershell
git add src/generators/natural/spherical_relief.rs src/generators/natural/spherical_relief src/generators/natural/relief_noise.rs tests/spherical_field_driven_relief.rs tests/spherical_tectonic_heightmap.rs tests/spherical_relief_generation.rs
git commit -m "feat: derive height from current spherical crust"
```

---

### Task 11: Stage, Field and Authoring Integration

**Files:**
- Modify: `src/generators/natural/spherical_stage.rs`
- Modify: `src/world/natural/spec.rs`
- Modify: `src/world/natural/formation.rs`
- Modify: `src/app.rs`
- Modify: `src/app/spherical_natural_display.rs`
- Modify: `tests/spherical_tectonic_mantle_stage.rs`
- Modify: `tests/spherical_natural_stage_graph.rs`
- Modify: `tests/natural_field_registry_spherical.rs`
- Modify: `tests/spherical_natural_matrix.rs`
- Modify: `tests/spherical_relief_geology_matrix.rs`

**Interfaces:**
- Consumes: V3 tectonic and unchanged V4 relief public getters.
- Produces: stage version 3, relief stage version 2, actual-final-count registry binding and “初始板块数” authoring text.

- [x] **Step 1: Write integration RED tests**

Assert `SphericalTectonicStage.version() == 3`, `SphericalReliefStage.version() == 2`, a changed stage invalidates all downstream spherical artifacts, the field registry category range equals `snapshot.plates().len()`, and a seed/spec whose final count differs from initial still builds the full 16-stage graph.

Add an app source/UI test for the exact label `初始板块数` and no label claiming final count.

- [x] **Step 2: Run and verify RED**

Run:

```powershell
cargo test --test spherical_tectonic_mantle_stage -- --nocapture
cargo test --test spherical_natural_stage_graph -- --nocapture
cargo test --test natural_field_registry_spherical -- --nocapture
```

Expected: old versions and tests that equate final count to input fail.

- [x] **Step 3: Update adapters and downstream expectations**

Change only stage versions and data reads. Preserve artifact keys and graph ordering. Replace assertions of `plates.len() == spec.plate_count` with bounds/connectivity assertions; leave author constraints on initial `TectonicSpec::plate_count` unchanged. Update persistent-byte accounting for all V3 crust arrays.

- [x] **Step 4: Run complete spherical natural integration**

Run:

```powershell
cargo test --test spherical_natural_matrix -- --nocapture
cargo test --test spherical_relief_geology_matrix -- --nocapture
cargo test --test spherical_natural_stage_graph -- --nocapture
cargo test --test spherical_presentation_integration -- --nocapture
```

Expected: all pass with one current publication and exact source/Arc binding.

- [x] **Step 5: Commit**

```powershell
git add src/generators/natural/spherical_stage.rs src/world/natural/spec.rs src/world/natural/formation.rs src/app.rs src/app/spherical_natural_display.rs tests/spherical_tectonic_mantle_stage.rs tests/spherical_natural_stage_graph.rs tests/natural_field_registry_spherical.rs tests/spherical_natural_matrix.rs tests/spherical_relief_geology_matrix.rs
git commit -m "feat: publish evolved spherical terrain"
```

---

### Task 12: Multi-Seed Scientific and Anti-Voronoi Acceptance

**Files:**
- Rewrite: `tests/spherical_morphology_quality.rs`
- Modify: `tests/spherical_tectonic_generation.rs`
- Modify: `tests/spherical_tectonic_heightmap.rs`
- Create: `tests/spherical_tectonic_causality.rs`

**Interfaces:**
- Consumes: public V3/current relief snapshots only.
- Produces: frozen semantic and statistical acceptance thresholds established before hashes.

- [x] **Step 1: Replace obsolete shape contracts with RED oracles**

For seed 42 plus 16 fixed seeds, compute physical-scale simplified boundaries, spherical compactness, convexity, turn-angle distribution, triple-junction angles, continental component concavity, land/crust Jaccard and coast/plate-boundary overlap. First run these against commit `11c72b6` or a locally reconstructed old facade and record that the honeycomb/circular baseline fails.

The new tests must not require exact final continent counts. They must verify formation ordering across the complete seed matrix and initial continental area within one cell.

- [x] **Step 2: Add direct process causality oracles**

`spherical_tectonic_causality.rs` finds actual final events and checks trench/uplift side, ridge/age depth, collision/orogeny uplift and transform neutrality. If a seed lacks one event type, aggregate across the fixed matrix; fail if the matrix still lacks coverage.

- [x] **Step 3: Run semantic acceptance and tune only documented constants**

Run:

```powershell
cargo test --release --test spherical_morphology_quality -- --nocapture
cargo test --release --test spherical_tectonic_causality -- --nocapture
cargo test --release --test spherical_tectonic_generation -- --nocapture
cargo test --release --test spherical_tectonic_heightmap -- --nocapture
```

If a threshold fails, inspect the multi-seed distribution and change only a named Appendix-A/formation/noise constant with a report entry. Do not add post-processing or per-seed exceptions.

- [x] **Step 4: Run deliberate causality and anti-Voronoi mutations**

Individually disable contact-driven owner transfer, reverse subduction side, zero spreading age, return initial owners and replace lineated detail with isotropic noise. Each mutation must make its dedicated oracle fail; restore each change and rerun GREEN.

- [x] **Step 5: Commit**

```powershell
git add tests/spherical_morphology_quality.rs tests/spherical_tectonic_generation.rs tests/spherical_tectonic_heightmap.rs tests/spherical_tectonic_causality.rs
git commit -m "test: lock spherical tectonic causality"
```

---

### Task 13: Dual-View Visual Evidence and Golden Refresh

**Files:**
- Modify: `tests/spherical_presentation_gpu.rs`
- Modify: `src/gpu/spherical/renderer.rs` only if a contract fixture must read new fields; do not change shaders or geometry for height.
- Create: `tests/spherical_tectonic_atlas.rs`
- Modify: expected hashes in existing golden tests only after all semantic tests pass.

**Interfaces:**
- Consumes: formal stage graph, field catalog, map/globe renderers and the semantic gates from Task 12.
- Produces: test-only `AtlasConfig`, `AtlasError`, a reviewable seed/field/view atlas and audited RGBA8 hashes.

- [x] **Step 1: Add an ignored atlas generator**

Generate seed 42 and the 16-seed matrix for plate owner, crust kind, crust age, tectonic elevation, final elevation, boundary kind/strength and lineation in Equal Earth and globe views. Write only under `target/spherical-tectonic-atlas/`; never commit generated images.

```rust
struct AtlasConfig {
    seeds: [u64; 17],
    target_cell_count: u32,
    render_map: bool,
    render_globe: bool,
}

impl AtlasConfig {
    fn formal_seed_matrix() -> Self {
        Self {
            seeds: [42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97],
            target_cell_count: 20_000,
            render_map: true,
            render_globe: true,
        }
    }
}

fn render_atlas(config: AtlasConfig, output: &std::path::Path) -> Result<(), AtlasError>;

#[test]
#[ignore = "manual dual-view tectonic atlas"]
fn render_spherical_tectonic_atlas() {
    let output = std::path::Path::new("target/spherical-tectonic-atlas");
    render_atlas(AtlasConfig::formal_seed_matrix(), output).unwrap();
}
```

- [x] **Step 2: Run semantic tests before any hash update**

Run Task 12's four Release commands. Expected: all GREEN. Record their output in the ignored implementation report.

- [x] **Step 3: Generate and inspect the atlas**

Run: `cargo test --release --test spherical_tectonic_atlas -- --ignored --nocapture`

Inspect all images for honeycombs, straight macro edges, circular land, plate=land, uniform rings, seam/pole concentration, cell checkerboarding and any globe displacement. Reject and fix the causal model if any is visible.

- [x] **Step 4: Refresh exact GPU hashes only after semantic and visual GREEN**

Run required-GPU Vulkan and GL suites, capture changed hashes, verify every difference comes from field data rather than mesh/shader behavior, update constants, then rerun twice per audited backend:

```powershell
$env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test --test spherical_presentation_gpu -- --nocapture
$env:WGPU_BACKEND='gl'; $env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test --test spherical_presentation_gpu -- --nocapture
```

Unknown adapters remain semantic-only and must not be labeled exact.

- [x] **Step 5: Exercise the shipped native and browser canvases**

Build and open the Release app in both native and Chrome. With seed 42/default 20,252 cells inspect plate ownership, crust kind, elevation and boundary strength on Equal Earth and globe; rotate/pan/zoom, switch fields and confirm the same selected cell facts remain. Capture screenshots proving the globe remains round and the new heightmap is color/annotation only. Inspect native stderr and the browser console for validation errors or panics.

Acceptance record: the final native Release inspection was completed at logical 1280×720 with seed 42/default 20,252 cells across plate owner, crust kind and tectonic elevation in both views, including pan/zoom/rotate and persistent edge selection; stderr was empty and the globe remained round. A fresh browser launch was blocked by local desktop policy in the final session, so no policy bypass was attempted. The browser path is instead closed by the fresh WASM all-features build, audited Vulkan and GL RGBA8 suites, and the complete 17-seed × 7-field × dual-view atlas; this task changes generated field data, not the already-accepted browser renderer or interaction path. The ignored implementation report records this environment substitution explicitly.

- [x] **Step 6: Commit**

```powershell
git add tests/spherical_tectonic_atlas.rs tests/spherical_presentation_gpu.rs src/gpu/spherical/renderer.rs
git commit -m "test: refresh evolved terrain visuals"
```

---

### Task 14: Performance, Memory and Final Verification

**Files:**
- Modify: `tests/spherical_natural_graph_performance.rs`
- Modify: `tests/spherical_natural_performance.rs`
- Modify: `docs/superpowers/plans/2026-08-10-procedural-spherical-tectonic-heightmap.md` checkboxes only
- Create ignored report: `.superpowers/sdd/2026-08-10-procedural-spherical-tectonic-heightmap/implementation-report.md`

**Interfaces:**
- Consumes: the complete production graph.
- Produces: measured native/WASM budgets, memory proof, static-frame proof and final auditable gate evidence.

- [x] **Step 1: Write the Release budget RED gate**

Measure seed 42, product defaults and exactly 20,252 cells in a fresh child process. Record tectonic, heightmap and total graph duration separately. Sample current working set before construction and during the target phase; assert target <=2 s, hard <=5 s, added peak <=256 MiB. Assert the published current snapshot contains no history collection and dropping the build retires workspace allocations.

- [x] **Step 2: Run Release performance gate**

Run:

```powershell
cargo test --release --test spherical_natural_graph_performance release_spherical_natural_full_graph_budget -- --ignored --nocapture
```

If over target, optimize in this order: reuse buffers, compact sample layout, per-index parallelism with stable writes, sparse active-boundary tracking, then a fixed lower-resolution Cortial simulation conservatively resampled to the authoritative surface. Do not remove phenomena or restore final Voronoi ownership.

The accepted approximation keeps the authoritative surface as the sole published surface: for worlds above 5,000 cells, run all 128 steps on a transient target-5,000 geodesic control surface and project only the final current state once. Preserve categorical identity by stable nearest control material; interpolate continuous crust attributes with the shared spherical-triangle-area barycentric primitive inside compatible owner/kind/orogeny domains. Apply the existing volume-preserving graph MBO after intermediate resampling, and index spreading events/current anchors so the common gap path is O(cells + events). The control topology must never enter an artifact, cache, history collection, UI or GPU resource.

Report direct tectonic construction separately from the formal stage's validation and deterministic semantic hash. Lock the direct Release path to 300 ms and formal publication path to 1 s on the acceptance machine, while retaining the complete-graph 2 s target, 5 s hard cap and frozen-baseline relative gate. A faster helper measurement never substitutes for the formal graph run.

- [x] **Step 3: Prove no presentation-time recomputation or deformation**

Run existing static-frame/camera/view/phase counters and unit-sphere tests. Assert a second static frame has zero large upload, zero tectonic/heightmap construction calls and only fixed-size uniform writes.

- [x] **Step 4: Run all engineering gates fresh**

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --target wasm32-unknown-unknown --workspace --all-features
cargo test --workspace --all-targets --all-features -- --nocapture
$env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test --workspace --all-targets --all-features -- --nocapture
cargo test --doc --workspace --all-features -- --nocapture
git diff --check
```

Read every exit code and failure count. A command timeout is not a pass; rerun it with a larger timeout.

- [x] **Step 5: Final source and requirement audit**

Use `rg` to prove: one production spherical graph call; no old spherical partition calls; no production history vector; no tectonic imports from app/view/gpu; no elevation/displacement in globe mesh/shader; no planar fallback; no public transient constructor; no tracked `.superpowers/sdd` file. Re-read the design sections 1–17 and map every requirement to a passing task/test in the ignored report.

- [x] **Step 6: Commit acceptance evidence**

```powershell
git add tests/spherical_natural_graph_performance.rs tests/spherical_natural_performance.rs docs/superpowers/plans/2026-08-10-procedural-spherical-tectonic-heightmap.md
git commit -m "test: lock evolved spherical terrain acceptance"
```

The ignored report remains local and must not be staged. Finish only when `git status --short` is clean and the latest fresh gates, not earlier cached runs, support every completion claim.
