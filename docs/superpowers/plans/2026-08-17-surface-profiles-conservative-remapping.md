# Surface Profiles and Conservative Remapping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> `superpowers:executing-plans` task-by-task. Every production task follows
> RED -> verify RED -> minimal GREEN -> focused verification -> commit.

**Goal:** Complete P1 by adding fixed natural quality profiles, a validated
conservative spherical remap, field remapping, cooperative build cancellation,
atomic profile-surface construction, and recorded Draft/Standard/High evidence.

**Architecture:** Stable profile and map values live under `world`; spherical
polygon intersection and field application live under `generators::spatial`;
engine cancellation remains independent of scientific hashes. A
`ProfileSurfaceBundle` atomically owns the authoritative surface, transient
tectonic control surface, control-to-authoritative map, resolution plan, and P1
quality report.

**Design:**
`docs/superpowers/specs/2026-08-17-surface-profiles-conservative-remapping-design.md`

**Tech stack:** Rust 2021, serde, thiserror, BLAKE3, existing geodesic Voronoi
surface, existing stage engine, no new crate dependency.

## Global constraints

- Preserve the 20,252-cell Draft surface fingerprint
  `0d09df7aa131d120490202741b0fd3184919ea9681f16537a14f81f0e5806f2e`.
- Preserve all pre-P1 legacy-planar hashes and existing V4 tectonic/relief hashes.
- Use real spherical convex-polygon overlaps. Never substitute nearest-neighbour
  weights when overlap closure fails.
- Keep the tectonic control surface transient and surface-bound.
- Bound every public/deserialized collection before allocation.
- Cancellation never affects a successful deterministic hash and never caches or
  publishes partial output.
- Existing `BuildEngine::build` remains source compatible and delegates to a
  never-cancelled execution.
- P1 failures do not alter P0 thresholds or hide the known V4 negative baseline.

---

### Task 1: Stable natural quality-profile contract

**Files:**

- Create: `src/world/natural/profile.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/world/spec.rs`
- Create: `tests/natural_quality_profiles.rs`
- Modify: `tests/spherical_foundation_build.rs`
- Modify: `tests/spherical_surface_generation.rs`

**Interfaces:**

```rust
pub const NATURAL_RESOLUTION_PLAN_SCHEMA_V1: u16 = 1;
pub enum NaturalQualityProfile { Draft, Standard, High }
pub struct NaturalResolutionPlan { /* private validated fields */ }

impl NaturalQualityProfile {
    pub const fn authoritative_target_cell_count(self) -> u32;
    pub const fn tectonic_control_target_cell_count(self) -> u32;
    pub const fn climate_face_resolution(self) -> u16;
    pub fn resolve(self, authoritative: &SphericalSpaceSpec)
        -> Result<NaturalResolutionPlan, NaturalProfileError>;
}
```

- [x] **Step 1: Write RED profile tests**

Assert exact settings:

```text
Draft     20,000 -> 20,252; control 4,842 -> 4,842; climate 24
Standard  80,000 -> 79,212; control 20,000 -> 20,252; climate 32
High     200,000 -> 198,812; control 20,000 -> 20,252; climate 48
```

Assert strict rejection when the authoritative target does not equal the
selected profile target, exact radius propagation into the control spec,
unknown-field/wrong-schema rejection, byte-identical serde repetition, and
zero-copy getters for all requested/resolved values. Also assert that a 200,000
High request is valid and resolves to 198,812, while 200,001 is rejected; keep
198,812 as the maximum actual spherical cell allocation.

- [x] **Step 2: Verify RED**

```powershell
cargo test --test natural_quality_profiles -- --nocapture
```

Expected: compile failure because the profile contract does not exist.

- [x] **Step 3: Implement the minimal validated profile values**

Use private serde wires with `deny_unknown_fields`. `NaturalResolutionPlan::new`
validates schema, exact profile targets, geodesic resolved counts, supported
climate resolution, and finite valid radius. Do not add a `Custom` or test-only
serialized profile. Split `MAX_SPHERICAL_TARGET_CELL_COUNT = 200_000` from the
existing maximum resolved allocation `MAX_SPHERICAL_CELL_COUNT = 198_812` and
update target-bound tests without weakening snapshot allocation bounds.

- [x] **Step 4: Run focused and compatibility tests**

```powershell
cargo test --test natural_quality_profiles -- --nocapture
cargo test --test world_spec --test spherical_surface_generation -- --nocapture
cargo check --target wasm32-unknown-unknown --workspace --all-features
```

- [x] **Step 5: Commit**

```powershell
git add src/world/natural/profile.rs src/world/natural/mod.rs src/world/spec.rs tests/natural_quality_profiles.rs tests/spherical_foundation_build.rs tests/spherical_surface_generation.rs
git commit -m "feat: define natural quality profiles"
```

---

### Task 2: Validated conservative-map wire contract

**Files:**

- Create: `src/world/spatial/remap.rs`
- Modify: `src/world/spatial/mod.rs`
- Create: `tests/conservative_surface_map_contracts.rs`

**Interfaces:**

```rust
pub const CONSERVATIVE_SURFACE_MAP_SCHEMA_V1: u16 = 1;
pub struct TangentTransform { /* four finite f64 coefficients */ }
pub struct SurfaceOverlapWeight { /* source ID, area, transform */ }
pub struct RemapSolveStats { /* iteration and closure evidence */ }
pub struct ConservativeSurfaceMap { /* canonical target CSR */ }
```

Getters expose source/target references, per-cell areas, target row ranges,
weights, solve stats, and maximum row/column errors without cloning.

- [ ] **Step 1: Write RED construction and serde tests**

Construct a small synthetic identity map and assert canonical target rows/source
IDs, finite positive areas/transforms, cardinality, monotone offsets, complete
row/column coverage, maximum closure error `<= 1e-10`, and exact JSON round-trip.
Reject duplicate/unsorted sources, wrong schema, unknown fields, invalid refs,
NaN/infinity, zero/negative area, bad offsets, excessive allocation, and
contradictory stored stats.

- [ ] **Step 2: Verify RED**

```powershell
cargo test --test conservative_surface_map_contracts -- --nocapture
```

- [ ] **Step 3: Implement bounded validated storage**

Bound areas by `MAX_SPHERICAL_CELL_COUNT`, offsets by that maximum plus one, and
overlaps by a documented sparse maximum. Use compensated sums in validation.
The constructor calculates closure and rejects contradictory solve evidence.

- [ ] **Step 4: Verify contracts and adjacent spatial values**

```powershell
cargo test --test conservative_surface_map_contracts -- --nocapture
cargo test --test surface_ref_contracts --test spherical_surface_contracts -- --nocapture
```

- [ ] **Step 5: Commit**

```powershell
git add src/world/spatial/remap.rs src/world/spatial/mod.rs tests/conservative_surface_map_contracts.rs
git commit -m "feat: define conservative surface maps"
```

---

### Task 3: Spherical overlap kernel and balanced sparse map

**Files:**

- Create: `src/generators/spatial/conservative_remap.rs`
- Modify: `src/generators/spatial/mod.rs`
- Create: `tests/conservative_surface_map_generation.rs`

**Interfaces:**

```rust
pub struct ConservativeSurfaceMapBuilder;
impl ConservativeSurfaceMapBuilder {
    pub fn build(source: &SphericalSurfaceSnapshot, target: &SphericalSurfaceSnapshot)
        -> Result<ConservativeSurfaceMap, ConservativeRemapError>;
    pub fn build_cancellable(
        source: &SphericalSurfaceSnapshot,
        target: &SphericalSurfaceSnapshot,
        is_cancelled: impl FnMut() -> bool,
    ) -> Result<ConservativeSurfaceMap, ConservativeRemapError>;
}
```

- [ ] **Step 1: Write RED analytic map tests**

Cover 42 -> 42 identity, 42 -> 162 and the transposed 162 -> 42 map, row/column
closure `<= 1e-10`, positive finite intersections, sphere-area closure,
byte-identical repetition, unequal-radius rejection, and cancellation without a
returned map.

- [ ] **Step 2: Verify RED**

```powershell
cargo test --test conservative_surface_map_generation -- --nocapture
```

- [ ] **Step 3: Implement geometric primitives**

Implement deterministic 3-D k-d nearest lookup, sorted adjacency rings,
oriented great-circle half-space clipping, minor-arc plane intersection,
duplicate-vertex removal, compensated spherical polygon area, and canonical
sparse rows. Expand candidate rings if raw fine-cell closure exceeds `1e-9`;
never fall back to nearest-neighbour weights.

- [ ] **Step 4: Implement deterministic margin balancing**

Alternate target-row and source-column scaling in canonical order. Require both
relative residuals `<= 1e-12` within 96 iterations. Reject a correction large
enough to indicate missing geometry. Derive tangent transforms from canonical
source/target bases.

- [ ] **Step 5: Run focused and spatial tests**

```powershell
cargo test conservative_remap --lib -- --nocapture
cargo test --test conservative_surface_map_generation -- --nocapture
cargo test --test spherical_surface_generation --test spherical_primitives -- --nocapture
```

- [ ] **Step 6: Commit**

```powershell
git add src/generators/spatial/conservative_remap.rs src/generators/spatial/mod.rs tests/conservative_surface_map_generation.rs
git commit -m "feat: construct conservative spherical maps"
```

---

### Task 4: Scalar, extensive, tangent-vector, and category remapping

**Files:**

- Create: `src/generators/spatial/remap_fields.rs`
- Modify: `src/generators/spatial/mod.rs`
- Create: `tests/conservative_surface_field_remap.rs`

**Interfaces:**

```rust
pub fn remap_intensive_f64(map: &ConservativeSurfaceMap, source: &[f64])
    -> Result<Vec<f64>, ConservativeRemapError>;
pub fn remap_intensive_f32(map: &ConservativeSurfaceMap, source: &[f32])
    -> Result<Vec<f32>, ConservativeRemapError>;
pub fn remap_extensive_f64(map: &ConservativeSurfaceMap, source: &[f64])
    -> Result<ExtensiveRemap, ConservativeRemapError>;
pub fn remap_tangent_components_f64(
    map: &ConservativeSurfaceMap,
    source_east_north: &[[f64; 2]],
) -> Result<Vec<[f64; 2]>, ConservativeRemapError>;
pub fn remap_categories_u16(map: &ConservativeSurfaceMap, source: &[u16])
    -> Result<CategoricalRemap, ConservativeRemapError>;
```

- [ ] **Step 1: Write RED field tests**

Use 42 -> 162, 162 -> 42, and Draft control -> Draft authoritative fixtures.
Assert exact `f64`/post-quantization `f32` constants, bounded latitude
interpolation, positive/signed extensive conservation `<= 1e-6`, solid-body
direction agreement `>= 0.999`, target tangency, category majority/tie/ambiguity,
and atomic rejection of length mismatch/non-finite input.

- [ ] **Step 2: Verify RED**

```powershell
cargo test --test conservative_surface_field_remap -- --nocapture
```

- [ ] **Step 3: Implement the locked field semantics**

Use compensated row/global accumulation. Constant scalars take an exact fast
path. Extensive results publish source total, target total, absolute error, and
relative error. Vectors apply stored transforms before area weighting.
Categories aggregate in a `BTreeMap`; lower value wins an equal overlap.

- [ ] **Step 4: Run field and WASM gates**

```powershell
cargo test --test conservative_surface_field_remap -- --nocapture
cargo check --target wasm32-unknown-unknown --workspace --all-features
```

- [ ] **Step 5: Commit**

```powershell
git add src/generators/spatial/remap_fields.rs src/generators/spatial/mod.rs tests/conservative_surface_field_remap.rs
git commit -m "feat: remap spherical scientific fields"
```

---

### Task 5: Cooperative engine and spherical-surface cancellation

**Files:**

- Create: `src/engine/cancellation.rs`
- Modify: `src/engine/mod.rs`
- Modify: `src/engine/random.rs`
- Modify: `src/engine/scheduler.rs`
- Modify: `src/generators/spatial/geodesic_voronoi.rs`
- Modify: `src/generators/spatial/spherical_stage.rs`
- Create: `tests/build_cancellation.rs`

**Interfaces:**

```rust
pub struct BuildCancellation;
impl BuildCancellation {
    pub fn new() -> Self;
    pub fn cancel(&self);
    pub fn is_cancelled(&self) -> bool;
}

impl BuildEngine {
    pub fn build_with_cancellation(
        &self,
        root_seed: RootSeed,
        external: ExternalArtifacts,
        cache: &mut MemoryStageCache,
        cancellation: &BuildCancellation,
    ) -> Result<BuildOutcome, BuildFailure>;
}
```

- [ ] **Step 1: Write RED engine cancellation tests**

Assert cancellation before build, between stages, during a cooperative stage,
and before cache restore/final publication. No cancelled output enters cache or
escapes as `BuildOutcome`. `BuildEngine::build` and a never-cancelled token must
produce exact matching artifacts, RNG values, diagnostics, and hashes. A prior
published outcome remains valid after a later cancelled attempt.

- [ ] **Step 2: Verify RED**

```powershell
cargo test --test build_cancellation -- --nocapture
```

- [ ] **Step 3: Implement the engine token without changing random streams**

`StageRng::from_seed` installs a never-cancelled handle; the scheduler-only
constructor attaches the build token. Check cancellation at every scheduler
boundary and immediately before cache insertion/final publication. Emit stable
`engine.cancelled` diagnostics with a stage ID when known.

- [ ] **Step 4: Make the geodesic builder cooperative**

Add `GeodesicVoronoiBuilder::build_cancellable`; check bounded intervals during
site, triangle, vertex, edge, and cell work. The original `build` delegates to a
never-cancelled closure. Add `SphericalSurfaceBuildError::Cancelled` and a stable
stage mapping.

- [ ] **Step 5: Verify compatibility**

```powershell
cargo test --test build_cancellation -- --nocapture
cargo test --test engine_execution --test stage_random -- --nocapture
cargo test --test spherical_surface_generation --test spherical_foundation_build -- --nocapture
```

- [ ] **Step 6: Commit**

```powershell
git add src/engine src/generators/spatial/geodesic_voronoi.rs src/generators/spatial/spherical_stage.rs tests/build_cancellation.rs
git commit -m "feat: cancel spherical builds atomically"
```

---

### Task 6: Atomic profile-surface bundle and P1 quality report

**Files:**

- Create: `src/generators/spatial/profile_surface.rs`
- Modify: `src/generators/spatial/mod.rs`
- Modify: `src/generators/natural/quality/mod.rs`
- Create: `src/generators/natural/quality/spatial.rs`
- Create: `tests/profile_surface_bundle.rs`

**Interfaces:**

```rust
pub struct ProfileSurfaceBundle { /* validated private fields */ }
pub struct ProfileSurfaceBuilder;
impl ProfileSurfaceBuilder {
    pub fn build(
        profile: NaturalQualityProfile,
        radius: Meters,
        cancellation: &BuildCancellation,
    ) -> Result<ProfileSurfaceBundle, ProfileSurfaceBuildError>;
}
```

- [ ] **Step 1: Write RED bundle tests**

For Draft, assert exact counts, the frozen authoritative fingerprint, map
identity binding, the exact eight P1 metric IDs, passing hard P1 metrics,
byte-identical repetition, and cancellation with no bundle.

- [ ] **Step 2: Verify RED**

```powershell
cargo test --test profile_surface_bundle -- --nocapture
```

- [ ] **Step 3: Implement atomic orchestration and P1 metrics**

Build the authoritative surface, control surface, then control-to-authoritative
map. Cross-validate all outputs before constructing the bundle. Evaluate closed
area, paired-edge cancellation, map margins, constant/extensive fixtures,
solid-body rotation, and deterministic category ambiguity into a P1
`NaturalQualityReport` containing the design's eight versioned IDs.

- [ ] **Step 4: Verify Draft and unchanged P0/V4 outputs**

```powershell
cargo test --test profile_surface_bundle -- --nocapture
cargo test --test natural_quality_contracts --test natural_quality_stage --test natural_quality_baseline -- --nocapture
cargo test --test spherical_natural_stage_graph -- --nocapture
```

- [ ] **Step 5: Commit**

```powershell
git add src/generators/spatial/profile_surface.rs src/generators/spatial/mod.rs src/generators/natural/quality tests/profile_surface_bundle.rs
git commit -m "feat: build atomic profile surfaces"
```

---

### Task 7: Standard/High release evidence and P1 completion

**Files:**

- Create: `tests/profile_surface_performance.rs`
- Create: `tests/profile_surface_evidence.rs`
- Create: `docs/superpowers/specs/2026-08-17-surface-profiles-conservative-remapping-completion.md`
- Modify: this plan

- [ ] **Step 1: Add ignored release product tests**

Build Draft, Standard, and High sequentially. Record component duration, overlap
count, serialized/persistent bytes, closure errors, direction agreement, and
cancellation latency. Assert exact counts and every P1 limit. Generated evidence
lives only under `target/natural-quality/p1/`.

- [ ] **Step 2: Run all P1 engineering gates fresh**

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --test natural_quality_profiles -- --nocapture
cargo test --test conservative_surface_map_contracts -- --nocapture
cargo test --test conservative_surface_map_generation -- --nocapture
cargo test --test conservative_surface_field_remap -- --nocapture
cargo test --test build_cancellation -- --nocapture
cargo test --test profile_surface_bundle -- --nocapture
cargo check --target wasm32-unknown-unknown --workspace --all-features
cargo test --release --test profile_surface_performance -- --ignored --nocapture
cargo test --release --test profile_surface_evidence -- --ignored --nocapture
cargo test --release --test natural_quality_baseline -- --ignored --nocapture
git diff --check
```

- [ ] **Step 3: Inspect evidence and review the phase**

Record exact fingerprints, overlap counts, closure/conservation/vector metrics,
runtimes, memory, cancellation evidence, limitations, and the unchanged
five-failure V4 terrain baseline. Fix every Critical/Important review issue and
rerun affected gates.

- [ ] **Step 4: Write completion record, check all boxes, and commit**

```powershell
git add tests/profile_surface_performance.rs tests/profile_surface_evidence.rs docs/superpowers/plans/2026-08-17-surface-profiles-conservative-remapping.md docs/superpowers/specs/2026-08-17-surface-profiles-conservative-remapping-completion.md
git commit -m "docs: record conservative profile surfaces"
```

The completion record freezes the exact P2 handoff: profile plan, authoritative
surface, transient control surface, conservative map, P1 quality report, and
cancellation contract.
