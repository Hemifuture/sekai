# Natural Quality Infrastructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the versioned quality-report artifact, deterministic metric builder, current spherical baseline evaluator, and reproducible 17-seed report command required by P0.

**Architecture:** Quality facts live in `world::natural` as immutable validated values. A pure generator module evaluates already-published natural artifacts without changing them, and a final engine stage publishes the report after the existing spherical natural graph. A test-only ignored corpus driver writes evidence beneath `target/` and never changes production inputs or golden hashes.

**Tech Stack:** Rust 2021, serde/serde_json, thiserror, existing stage engine, existing spherical natural snapshots, BTreeMap/BTreeSet.

## Global Constraints

- Follow `docs/superpowers/specs/2026-08-17-complete-natural-world-pipeline-design.md`, especially sections 4, 7, 18, and 19.
- Add no dependency and change no legacy-planar artifact, output, or golden hash.
- A report containing failed or unavailable metrics is valid evidence; malformed metrics are invalid artifacts.
- Metric identifiers are stable lowercase ASCII components and are serialized in sorted order.
- Empty samples become `Unavailable` with a non-empty reason; they never become a passing zero.
- Metric evaluation is deterministic, read-only, area-weighted where area is relevant, and independent of renderer/UI state.
- Corpus files are generated only beneath `target/natural-quality/` and are never tracked.
- Every task follows RED -> verify RED -> minimal GREEN -> focused verification -> commit.

---

## File Structure

```text
src/world/natural/quality.rs
    Stable metric IDs, bounds, status, values, report validation and serde.
src/generators/natural/quality/mod.rs
    Pure report builder and shared finite/area-weighted helpers.
src/generators/natural/quality/spherical.rs
    Current spherical foundation metrics only.
src/generators/natural/spherical_quality_stage.rs
    Final typed artifact/stage adapter; no scientific formulas.
tests/natural_quality_contracts.rs
    Construction, validation, serde and allocation rejection.
tests/natural_quality_stage.rs
    Graph dependencies, deterministic report and current known baseline.
tests/natural_quality_baseline.rs
    Ignored 17-seed JSON/CSV evidence writer under target/.
```

### Task 1: Versioned quality report contract

**Files:**
- Create: `src/world/natural/quality.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/natural_quality_contracts.rs`

**Interfaces:**
- Consumes: `SurfaceRef`, serde bounded-wire conventions.
- Produces: `QualityMetricId`, `QualityMetricStatus`, `QualityBounds`, `QualityMetric`, `NaturalQualityReport`, `NaturalQualityValidationError`, `NATURAL_QUALITY_REPORT_SCHEMA_V1`.

- [x] **Step 1: Write contract tests that fail to compile**

Construct this exact public API:

```rust
let metric = QualityMetric::new(
    QualityMetricId::new("tectonics", "continental-area-fraction", 1)?,
    QualityMetricStatus::Fail,
    Some(0.199),
    20_252,
    QualityBounds::between(0.30, 0.45)?,
    None,
)?;
let report = NaturalQualityReport::new(
    NATURAL_QUALITY_REPORT_SCHEMA_V1,
    surface_ref,
    vec![metric],
)?;
```

Assert exact JSON round-trip, sorted unique IDs, finite values/bounds, `min <= max`, nonzero samples for available values, no value for `Unavailable`, non-empty reason for `Unavailable`, no reason for `Pass`, bound/status agreement, surface validation, wrong-schema rejection, duplicate rejection, invalid identifier rejection, and a maximum of 4,096 metrics during deserialization.

- [x] **Step 2: Run the contract test and verify RED**

Run: `cargo test --test natural_quality_contracts -- --nocapture`

Expected: compile failure because `world::natural::QualityMetric` and related types do not exist.

- [x] **Step 3: Implement the minimal validated contract**

Use these representations:

```rust
pub const NATURAL_QUALITY_REPORT_SCHEMA_V1: u16 = 1;
const MAX_QUALITY_METRICS: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct QualityMetricId {
    namespace: String,
    name: String,
    version: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityMetricStatus { Pass, Fail, Unavailable }

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct QualityBounds {
    min: Option<f64>,
    max: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QualityMetric {
    id: QualityMetricId,
    status: QualityMetricStatus,
    value: Option<f64>,
    sample_count: u32,
    bounds: QualityBounds,
    reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NaturalQualityReport {
    schema_version: u16,
    surface_ref: SurfaceRef,
    metrics: Vec<QualityMetric>,
}
```

Deserialize each validated struct through a private wire and constructor. Identifier syntax matches field-component syntax: 1..=128 lowercase ASCII bytes, `a-z0-9-_.`, alphanumeric endpoints. Provide `at_least`, `at_most`, and `between` constructors plus zero-copy getters. Sort metrics in `NaturalQualityReport::new`, reject duplicates, and never infer status during deserialization.

- [x] **Step 4: Run contract and adjacent world tests**

```powershell
cargo test --test natural_quality_contracts -- --nocapture
cargo test --test surface_ref_contracts -- --nocapture
cargo test --test world_primitives -- --nocapture
```

Expected: all pass.

- [x] **Step 5: Commit**

```powershell
git add src/world/natural/quality.rs src/world/natural/mod.rs tests/natural_quality_contracts.rs
git commit -m "feat: define natural quality report"
```

### Task 2: Deterministic metric builder

**Files:**
- Create: `src/generators/natural/quality/mod.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: module tests in `src/generators/natural/quality/mod.rs`

**Interfaces:**
- Consumes: public quality contract and deterministic metric samples.
- Produces: crate-private `NaturalQualityReportBuilder`, `MetricAccumulator`, `area_weighted_fraction`, `jaccard_fraction`, `QualityBuildError`.

- [x] **Step 1: Write builder RED tests**

Test sorted output regardless of insertion order, duplicate rejection, stable Neumaier area summation, unavailable empty accumulator, finite rejection, inclusive threshold behavior, and exact Jaccard behavior for empty/non-empty masks.

```rust
let mut builder = NaturalQualityReportBuilder::new(surface_ref);
builder.record_at_most(id, 0.25, 32, 0.35)?;
builder.record_between(other_id, 0.38, 32, 0.30, 0.45)?;
let report = builder.finish()?;
```

- [x] **Step 2: Run module tests and verify RED**

Run: `cargo test --lib generators::natural::quality -- --nocapture`

Expected: missing module and builder.

- [x] **Step 3: Implement builder and numeric helpers**

`MetricAccumulator` stores `(value, nonnegative_weight)` samples in stable caller order and uses an `f64` Neumaier sum for numerator and denominator. It returns `Unavailable("no positive finite sample weight")` if no positive weight exists. `jaccard_fraction` returns unavailable when both masks have zero weighted union; it never returns 1.0 for an empty comparison.

The builder creates `QualityMetric` through its constructor so generator code cannot bypass report validation. `finish` calls `NaturalQualityReport::new` and maps validation errors into `QualityBuildError`.

- [x] **Step 4: Run focused tests**

```powershell
cargo test --lib generators::natural::quality -- --nocapture
cargo test --test natural_quality_contracts -- --nocapture
```

Expected: all pass.

- [x] **Step 5: Commit**

```powershell
git add src/generators/natural/quality src/generators/natural/mod.rs
git commit -m "feat: build deterministic quality metrics"
```

### Task 3: Current spherical foundation evaluator

**Files:**
- Create: `src/generators/natural/quality/spherical.rs`
- Modify: `src/generators/natural/quality/mod.rs`
- Create: `tests/natural_quality_stage.rs`

**Interfaces:**
- Consumes: `SphericalSurfaceSnapshot`, `ResolvedWorldFormation`, `ReliefSpec`, `SphericalTectonicSnapshot`, `SphericalReliefSnapshot`, `SphericalHydroErosionSnapshot`.
- Produces: `evaluate_spherical_foundation_quality(...) -> Result<NaturalQualityReport, QualityBuildError>` and stable P0 metric IDs.

- [x] **Step 1: Write evaluator RED tests**

Build a 162-cell fixed spherical fixture through the formal graph, then call:

```rust
let report = evaluate_spherical_foundation_quality(
    surface,
    formation,
    relief_spec,
    tectonic,
    relief,
    hydro_erosion,
)?;
```

Assert the report contains these exact IDs and no renderer-derived values:

```text
tectonics.continental-area-fraction.v1
tectonics.continental-retention.v1
relief.requested-land-area-fraction.v1
relief.actual-land-area-fraction.v1
relief.land-crust-jaccard.v1
relief.oceanic-emergent-area-fraction.v1
hydrology.outlet-area-coverage.v1
hydrology.river-segment-count.v1
quality.non-finite-value-count.v1
```

Repeat evaluation twice and assert byte-identical JSON. Mutate display palette state in a separate app fixture and assert the report hash is unchanged.

- [x] **Step 2: Run and verify RED**

Run: `cargo test --test natural_quality_stage evaluator -- --nocapture`

Expected: missing evaluator.

- [x] **Step 3: Implement area-weighted P0 metrics**

Use each authoritative cell's spherical area. Continental retention divides evolved continental area by `formation.recommended_continental_crust_fraction()` times total area. Requested land comes from `relief_spec.target_land_fraction()`. Land/crust Jaccard compares final land with evolved continental crust. Oceanic emergent fraction divides oceanic-crust land area by all oceanic-crust area. Outlet coverage uses hydrology basin/outlet membership; river count is an exact count converted to `f64`. Scan every dense numeric field used by the evaluator and count non-finite values.

P0 thresholds are evidence, not a production veto:

```text
continental area             0.30..=0.45
continental retention        0.75..=1.15
requested/actual land error  <= one maximum cell area fraction
land/crust Jaccard            >= 0.75
oceanic emergent fraction     <= 0.10
outlet area coverage          >= 0.999999
non-finite count              <= 0
```

`river-segment-count` has unbounded evidence bounds in P0 and reports Pass for a finite available count; morphology bounds arrive in P5.

- [x] **Step 4: Run evaluator and existing causal tests**

```powershell
cargo test --test natural_quality_stage evaluator -- --nocapture
cargo test --test spherical_tectonic_causality -- --nocapture
cargo test --test spherical_hydro_erosion_contracts -- --nocapture
```

Expected: all pass; the current product may contain valid `Fail` metrics.

- [x] **Step 5: Commit**

```powershell
git add src/generators/natural/quality tests/natural_quality_stage.rs
git commit -m "feat: measure spherical natural quality"
```

### Task 4: Final spherical quality stage

**Files:**
- Create: `src/generators/natural/spherical_quality_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/spherical_stage.rs`
- Modify: `tests/natural_quality_stage.rs`
- Modify: `tests/spherical_natural_stage_graph.rs`
- Modify: `tests/spherical_natural_graph_performance.rs`

**Interfaces:**
- Consumes: formal surface, resolved formation, relief spec, tectonic, relief, and hydro-erosion artifacts.
- Produces: `NaturalQualityArtifact`, `SphericalNaturalQualityStage`, artifact key `world.natural-quality`, stage ID `natural.spherical-quality`, version 1.

- [x] **Step 1: Write stage graph RED tests**

Assert exact dependency keys, artifact serde/validation, stage identity/version, inclusion after `SphericalHydroErosionStage`, invalidation when an upstream scientific artifact changes, and no invalidation for palette/view state. Update the expected formal graph artifact count by exactly one.

- [x] **Step 2: Run and verify RED**

```powershell
cargo test --test natural_quality_stage stage -- --nocapture
cargo test --test spherical_natural_stage_graph -- --nocapture
```

Expected: missing stage/artifact and old graph count.

- [x] **Step 3: Implement the thin stage adapter**

Define inputs with only these artifact dependencies:

```rust
pub struct SphericalNaturalQualityStageInputs {
    formation: Arc<ResolvedWorldFormationArtifact>,
    hydro_erosion: Arc<SphericalHydroErosionArtifact>,
    relief: Arc<SphericalReliefArtifact>,
    relief_spec: Arc<ReliefSpecArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
    tectonic: Arc<SphericalTectonicArtifact>,
}
```

The stage validates identity compatibility, calls the pure evaluator once, validates the report, and wraps it. It does not read UI state, renderer resources, or an entire `WorldSpec`.

- [x] **Step 4: Run graph, performance, and serialization tests**

```powershell
cargo test --test natural_quality_stage -- --nocapture
cargo test --test spherical_natural_stage_graph -- --nocapture
cargo test --release --test spherical_natural_graph_performance -- --ignored --nocapture
```

Expected: functional tests pass. Performance output records the quality stage separately; a timeout or budget regression is fixed before commit.

- [x] **Step 5: Commit**

```powershell
git add src/generators/natural/spherical_quality_stage.rs src/generators/natural/mod.rs src/generators/natural/spherical_stage.rs tests/natural_quality_stage.rs tests/spherical_natural_stage_graph.rs tests/spherical_natural_graph_performance.rs
git commit -m "feat: publish spherical quality report"
```

### Task 5: Reproducible 17-seed baseline evidence

**Files:**
- Create: `tests/natural_quality_baseline.rs`
- Create: `tests/support/natural_quality.rs`
- Modify: `tests/support/mod.rs`
- Modify: `.gitignore` only if `target/` is not already ignored.

**Interfaces:**
- Consumes: formal spherical graph and `NaturalQualityArtifact`.
- Produces: ignored test `write_v4_natural_quality_baseline`, `target/natural-quality/v4-baseline.json`, `target/natural-quality/v4-metrics.csv`.

- [x] **Step 1: Write the ignored corpus driver**

Use the fixed seeds:

```rust
const QUALITY_SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];
```

For every seed build `Continents`, Draft 20,000 target cells, 12 initial plates, initial crust 0.38, and target land 0.38. Serialize a stable top-level object containing schema, git-independent scenario metadata, ordered per-seed reports, and aggregate median/min/max for every metric ID. CSV rows are ordered by `(metric_id, seed)`.

- [x] **Step 2: Run the ignored test and verify output isolation**

Run: `cargo test --release --test natural_quality_baseline -- --ignored --nocapture`

Expected: two files under `target/natural-quality/`, no files elsewhere, and output explicitly shows the known V4 continental/land mismatch rather than hiding failed metrics.

- [x] **Step 3: Add deterministic output assertions**

Run the writer twice in one test process, hash both byte buffers before replacement, and assert equality. Assert all 17 seeds exist, all reports expose the same metric ID set, no metric is omitted, and aggregate sample counts equal the sum of per-seed counts.

- [x] **Step 4: Run focused and repository hygiene checks**

```powershell
cargo test --test natural_quality_baseline -- --nocapture
cargo test --release --test natural_quality_baseline -- --ignored --nocapture
git status --short
```

Expected: non-ignored isolation/determinism tests pass and generated evidence remains ignored.

- [x] **Step 5: Commit**

```powershell
git add tests/natural_quality_baseline.rs tests/support/mod.rs tests/support/natural_quality.rs .gitignore
git commit -m "test: record spherical v4 quality baseline"
```

### Task 6: P0 phase verification and completion record

**Files:**
- Modify: `docs/superpowers/plans/2026-08-17-natural-quality-infrastructure.md` checkboxes only.
- Create: `docs/superpowers/specs/2026-08-17-natural-quality-infrastructure-completion.md`

**Interfaces:**
- Consumes: Tasks 1-5 and their fresh outputs.
- Produces: auditable P0 completion record and the exact P1 input contract.

- [x] **Step 1: Run all P0 engineering gates fresh**

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --test natural_quality_contracts -- --nocapture
cargo test --test natural_quality_stage -- --nocapture
cargo test --test natural_quality_baseline -- --nocapture
cargo check --target wasm32-unknown-unknown --workspace --all-features
git diff --check
```

Expected: every command exits 0; no warning is waived.

- [x] **Step 2: Generate and inspect the V4 baseline**

Run: `cargo test --release --test natural_quality_baseline -- --ignored --nocapture`

Inspect `target/natural-quality/v4-baseline.json`. Record exact aggregate values, failed/unavailable counts, runtime, machine/protocol information, and the fact that failures are expected evidence rather than P0 failures.

- [x] **Step 3: Write the completion record**

The completion record contains the exact commands, exit codes, metric inventory, aggregate V4 values, generated paths, known limitations, commit IDs, and P1 handoff: `NaturalQualityReport` V1 plus the fixed seed corpus. It contains no generated images or JSON blobs.

- [x] **Step 4: Mark checkboxes and commit**

```powershell
git add docs/superpowers/plans/2026-08-17-natural-quality-infrastructure.md docs/superpowers/specs/2026-08-17-natural-quality-infrastructure-completion.md
git commit -m "docs: record natural quality foundation"
```

P0 is complete only when the worktree is clean and the latest fresh commands support the completion record.
