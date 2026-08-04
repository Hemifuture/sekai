# Spherical Natural Product Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish the completed spherical natural-process stack through one authoritative typed engine graph and one projection-free field document while freezing the planar V1 path as isolated legacy compatibility.

**Architecture:** Add six surface-bound spherical Stage/Artifact adapters around the already-tested scientific generators, then compose them with the existing geometry-neutral rule and formation stages. Keep stage caching granular and assemble a zero-copy `SphericalNaturalFieldDocument` only after the complete `BuildOutcome` cross-validates; split data-only field access from presentation meshes so S0C can derive 2D and 3D views without regenerating science. The existing planar graph and app document remain byte-compatible but are explicitly named legacy and are never registered by the spherical graph.

**Tech Stack:** Rust 1.85 / edition 2021, serde + serde_json float-roundtrip, thiserror, BLAKE3 engine hashes, deterministic StageRng, eframe/egui presentation boundary, Cargo integration tests.

## Global Constraints

- `SphericalSurfaceSnapshot` is the only geometry/topology authority for new worlds; no natural Artifact may own copied cells, edges, vertices, adjacency, projection coordinates, or GPU data.
- The active spherical graph has six new stages with namespace `sekai.core`, version `1`, and the exact Stage IDs and Artifact keys approved in the S0B.6 design.
- Preserve the existing planar graph, Artifact keys, stage identities, wire schemas, deterministic hashes, serialized registry, and reviewed display goldens without drift.
- Every spherical natural stage directly depends on `SphericalSurfaceArtifact` and validates the exact `SurfaceRef`; equal counts never imply compatibility.
- Reuse the existing rule-resolution, resolved-input, formation, scientific generator, and geometry-independent core implementations; do not copy a second scientific algorithm into stage adapters.
- Build publication is atomic. Failure keeps the previous complete document and never falls back to planar physics or publishes partial sphere fields.
- The field document contains no `PreparedCellMesh`; local east/north vectors are disposable view caches derived from authoritative 3D tangent vectors.
- No 2D projection, 3D globe, final circulation/ocean model, ENSO, cyclone, history timeline, project archive, or planar/spherical product selector is added in S0B.6.
- At roughly 20,000 cells, Release full-graph time must remain below `2.5×` the frozen planar baseline and below the existing `5 s` hard ceiling; additional working memory must remain below `256 MiB`.
- Do not add dependencies or use language/library features newer than Rust 1.85; native and `wasm32-unknown-unknown` builds must both pass.
- Use strict TDD for every production change: write a focused test, observe the expected failure, implement the minimum behavior, rerun focused and adjacent tests, then commit.

## File Structure

- `src/generators/natural/spherical_stage.rs`: spherical tectonic/relief Artifact adapters plus the complete spherical natural graph.
- `src/generators/natural/spherical_geologic_stage.rs`: spherical mantle and geologic Artifact adapters.
- `src/generators/natural/spherical_climate_stage.rs`: spherical preliminary-climate Artifact adapter.
- `src/generators/natural/spherical_hydro_erosion_stage.rs`: spherical atomic hydro-erosion Artifact adapter.
- `src/generators/natural/stage.rs`: unchanged planar algorithms; add an explicit legacy graph entry while retaining the old public alias.
- `src/generators/natural/mod.rs`: module declarations and public exports for the new graph and artifacts.
- `src/world/natural/fields.rs`: one schema builder with frozen planar limits and exact sphere-area limits.
- `src/world/spatial/sphere_geometry.rs`: canonical projection-independent east/north tangent basis.
- `src/world/spatial/mod.rs`: export the tangent-basis helper.
- `src/app/field_document.rs`: split data-only and presented document traits.
- `src/app/natural_field_payloads.rs`: sole application mapping from stable natural Field IDs to borrowed payload arrays.
- `src/app/natural_display.rs`: legacy planar document, refactored to the common payload bundle without output drift.
- `src/app/spherical_natural_display.rs`: projection-free spherical document, build identity, cross-validation, and tangent-vector display cache.
- `src/app.rs`: explicitly call the legacy planar builder until the S0C presenter cutover; register the spherical document modules without a geometry toggle.
- `tests/legacy_planar_boundary.rs`: frozen legacy graph/application boundary.
- `tests/spherical_tectonic_mantle_stage.rs`: Stage/Artifact contracts for the two independent sphere foundations.
- `tests/spherical_relief_geologic_stage.rs`: downstream relief/geology Stage/Artifact contracts.
- `tests/spherical_climate_hydro_stage.rs`: preliminary climate and atomic hydro-erosion contracts.
- `tests/spherical_natural_stage_graph.rs`: exact whole-graph dependencies, cache invalidation, determinism, and SurfaceRef identity.
- `tests/natural_field_registry_spherical.rs`: sphere-safe field ranges and frozen planar registry bytes.
- `tests/spherical_natural_graph_performance.rs`: ignored Release full-graph time/memory gate.
- `docs/superpowers/plans/2026-08-04-spherical-natural-product-integration.md`: checklist and execution evidence.

---

### Task 1: Isolate and freeze the planar V1 compatibility boundary

**Files:**
- Modify: `src/generators/natural/stage.rs:287-320`
- Modify: `src/generators/natural/mod.rs:87-94`
- Modify: `src/app.rs:14-45, 666-706`
- Modify: `src/app/natural_display.rs:39-54, 393-444`
- Create: `tests/legacy_planar_boundary.rs`

**Interfaces:**
- Consumes: existing `natural_foundation_graph()`, planar Artifact types, and `TemplateApp` serialization.
- Produces: `legacy_planar_natural_foundation_graph() -> Result<StageGraph, GraphError>` while retaining `natural_foundation_graph()` as an exact compatibility alias; `LegacyPlanarNaturalFieldDocument` as the explicit internal presenter name.

- [ ] **Step 1: Write the failing legacy-boundary test**

Add a test that demands the new explicit alias and proves both entry points retain the exact graph contract:

```rust
use sekai::generators::natural::{
    legacy_planar_natural_foundation_graph, natural_foundation_graph,
};

#[test]
fn legacy_alias_preserves_the_exact_planar_graph() {
    let legacy = legacy_planar_natural_foundation_graph().unwrap();
    let compatibility = natural_foundation_graph().unwrap();
    assert_eq!(legacy.stage_ids(), compatibility.stage_ids());
    assert_eq!(legacy.descriptors(), compatibility.descriptors());
    assert!(legacy
        .descriptors()
        .iter()
        .all(|descriptor| !descriptor.output().as_str().contains("spherical")));
}

#[test]
fn old_application_state_defaults_into_the_legacy_presenter_contract() {
    let restored: sekai::TemplateApp = serde_json::from_value(serde_json::json!({
        "world_seed": 7
    }))
    .unwrap();
    let encoded = serde_json::to_value(restored).unwrap();
    assert_eq!(encoded["world_seed"], 7);
    assert!(encoded.get("spherical_mode").is_none());
    assert!(encoded.get("geometry_mode").is_none());
}
```

In `src/app.rs` tests, replace the existing source-grep test with a real candidate build named `active_canvas_build_executes_the_legacy_planar_graph`. Build 128 cells through `build_legacy_planar_natural_candidate`, assert the report includes `spatial.planar-voronoi`, contains no stage ID prefixed by `natural.spherical-`, and assert the resulting document contains the 128-cell `SpatialArtifact`, all seven planar natural snapshots, and a prepared display packet. This test fails if the active presenter is accidentally wired to sphere inputs or bypasses the typed legacy graph.

- [ ] **Step 2: Run the test and observe RED**

Run: `cargo test --test legacy_planar_boundary -- --nocapture`

Expected: compilation fails because `legacy_planar_natural_foundation_graph` is not exported.

- [ ] **Step 3: Add the compatibility alias and explicit app naming**

Move the existing graph body without changing any stage registration:

```rust
pub fn legacy_planar_natural_foundation_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<PlanarSpaceArtifact>()
        .external::<TectonicSpecArtifact>()
        .external::<GeologicSpecArtifact>()
        .external::<ClimateSpecArtifact>()
        .external::<HydroErosionSpecArtifact>()
        .external::<WorldFormationSpecArtifact>()
        .external::<RulePackSetArtifact>()
        .external::<AuthorConstraintsArtifact>()
        .stage(SpatialStage)
        .stage(RuleTectonicResolutionStage)
        .stage(RuleGeologicResolutionStage)
        .stage(RuleClimateResolutionStage)
        .stage(RuleHydroErosionResolutionStage)
        .stage(ResolvedTectonicInputStage)
        .stage(ResolvedGeologicInputStage)
        .stage(ResolvedClimateInputStage)
        .stage(ResolvedHydroErosionInputStage)
        .stage(WorldFormationStage)
        .stage(TectonicStage)
        .stage(MantleStage)
        .stage(ReliefStage)
        .stage(GeologicStage)
        .stage(PreliminaryClimateStage)
        .stage(HydroErosionStage)
        .build()
}

pub fn natural_foundation_graph() -> Result<StageGraph, GraphError> {
    legacy_planar_natural_foundation_graph()
}
```

Export both functions. Rename only private application symbols (`NaturalFieldDocument` to `LegacyPlanarNaturalFieldDocument`, `natural_document` to `legacy_planar_document`, `build_natural_candidate` to `build_legacy_planar_natural_candidate`, `build_natural_candidate_from_external` to `build_legacy_planar_natural_candidate_from_external`, and the remaining planar candidate/external helpers to the same `legacy_planar_*` convention) and make the app call the explicit legacy graph. Replace `default_application_source_has_no_legacy_generator_call_path` with the behavior test specified in Step 1. Do not change persisted fields or runtime behavior.

- [ ] **Step 4: Run focused legacy compatibility gates**

Run:

```powershell
cargo test --test legacy_planar_boundary -- --nocapture
cargo test --test natural_stage_graph --test foundation_build --test natural_display_golden -- --nocapture
cargo test --lib natural_app_tests -- --nocapture
```

Expected: all pass with the pre-existing planar stage IDs and golden images.

- [ ] **Step 5: Commit**

```powershell
git add src/generators/natural/stage.rs src/generators/natural/mod.rs src/app.rs src/app/natural_display.rs tests/legacy_planar_boundary.rs
git commit -m "refactor: isolate legacy planar natural path"
```

---

### Task 2: Publish spherical tectonics and mantle through typed stages

**Files:**
- Create: `src/generators/natural/spherical_stage.rs`
- Create: `src/generators/natural/spherical_geologic_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/spherical_tectonic_mantle_stage.rs`

**Interfaces:**
- Consumes: `SphericalSurfaceArtifact`, `ResolvedTectonicInputArtifact`, `ResolvedGeologicInputArtifact`, `ResolvedWorldFormationArtifact`, `TectonicGenerator::generate_spherical`, and `MantleGenerator::generate_spherical`.
- Produces: `SphericalTectonicArtifact`, `SphericalTectonicStage`, `SphericalTectonicStageInputs`, `SphericalMantleArtifact`, `SphericalMantleStage`, and `SphericalMantleStageInputs`.

- [ ] **Step 1: Write RED tests for exact keys, dependencies, strict wires, and stage results**

The tests must assert:

```rust
assert_eq!(SphericalTectonicArtifact::KEY.as_str(), "world.spherical-tectonics");
assert_eq!(SphericalMantleArtifact::KEY.as_str(), "world.spherical-mantle");
assert_eq!(SphericalTectonicStage.id().as_str(), "natural.spherical-tectonics");
assert_eq!(SphericalMantleStage.id().as_str(), "natural.spherical-mantle");
assert_eq!(SphericalTectonicStage.version(), 1);
assert_eq!(SphericalMantleStage.version(), 1);
assert_eq!(SphericalTectonicStage.namespace(), "sekai.core");
assert_eq!(SphericalMantleStage.namespace(), "sekai.core");
```

Build each stage in a minimal `StageGraphBuilder` with its exact external dependencies, compare the output to a direct generator call using `derive_stage_seed` with the stage identity, validate against the same sphere, round-trip the Artifact through JSON, and reject an unknown wrapper field.

- [ ] **Step 2: Run the focused test and observe RED**

Run: `cargo test --test spherical_tectonic_mantle_stage -- --nocapture`

Expected: compilation fails because the spherical Artifact and Stage types do not exist.

- [ ] **Step 3: Implement strict Artifact wrappers and input bundles**

Use this exact wrapper shape for each output:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalTectonicArtifact {
    snapshot: SphericalTectonicSnapshot,
}

impl SphericalTectonicArtifact {
    pub const fn new(snapshot: SphericalTectonicSnapshot) -> Self { Self { snapshot } }
    pub const fn snapshot(&self) -> &SphericalTectonicSnapshot { &self.snapshot }
}

impl Artifact for SphericalTectonicArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.spherical-tectonics");
    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new("spherical-natural.invalid-tectonics", error.to_string())
        })
    }
}
```

Use this complete mantle wrapper rather than sharing a wire type with tectonics:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalMantleArtifact {
    snapshot: SphericalMantleSnapshot,
}

impl SphericalMantleArtifact {
    pub const fn new(snapshot: SphericalMantleSnapshot) -> Self { Self { snapshot } }
    pub const fn snapshot(&self) -> &SphericalMantleSnapshot { &self.snapshot }
}

impl Artifact for SphericalMantleArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.spherical-mantle");
    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new("spherical-natural.invalid-mantle", error.to_string())
        })
    }
}
```

`SphericalTectonicStageInputs::dependencies()` returns `[ResolvedTectonicInputArtifact::KEY, ResolvedWorldFormationArtifact::KEY, SphericalSurfaceArtifact::KEY]`. `SphericalMantleStageInputs::dependencies()` returns `[ResolvedGeologicInputArtifact::KEY, ResolvedWorldFormationArtifact::KEY, SphericalSurfaceArtifact::KEY]`. The graph normalizer owns final sorting.

- [ ] **Step 4: Implement the minimal stage adapters**

Match the resolved model exactly, call the existing generator, validate against the supplied sphere, and map errors to stable sphere-specific stage codes:

```rust
let snapshot = match inputs.resolved_input.input().model() {
    TectonicModel::CurrentSliceV1 => TectonicGenerator::generate_spherical(
        inputs.surface.snapshot(),
        inputs.resolved_input.input().spec(),
        inputs.formation.formation(),
        rng,
    ),
}
.map_err(tectonic_generation_failure)?;
snapshot.validate_against(inputs.surface.snapshot()).map_err(invalid_tectonics)?;
Ok(SphericalTectonicArtifact::new(snapshot))
```

The mantle stage matches `GeologicModel::CurrentSliceV1` and calls:

```rust
let snapshot = MantleGenerator::generate_spherical(
    inputs.surface.snapshot(),
    inputs.resolved_input.input().spec(),
    inputs.formation.formation().mantle_bias(),
    rng,
)
.map_err(mantle_generation_failure)?;
snapshot.validate_against(inputs.surface.snapshot()).map_err(invalid_mantle)?;
Ok(SphericalMantleArtifact::new(snapshot))
```

Do not construct a topology index or draw RNG in either adapter; the tested generators own those operations.

- [ ] **Step 5: Run RED-to-GREEN and adjacent scientific tests**

Run:

```powershell
cargo test --test spherical_tectonic_mantle_stage -- --nocapture
cargo test --test spherical_tectonic_generation --test spherical_mantle_generation --test spherical_natural_matrix -- --nocapture
```

Expected: all pass and the existing frozen scientific hashes remain unchanged.

- [ ] **Step 6: Commit**

```powershell
git add src/generators/natural/spherical_stage.rs src/generators/natural/spherical_geologic_stage.rs src/generators/natural/mod.rs tests/spherical_tectonic_mantle_stage.rs
git commit -m "feat: publish spherical tectonic mantle stages"
```

---

### Task 3: Publish spherical relief and geology through typed stages

**Files:**
- Modify: `src/generators/natural/spherical_stage.rs`
- Modify: `src/generators/natural/spherical_geologic_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/spherical_relief_geologic_stage.rs`

**Interfaces:**
- Consumes: the Task 2 Artifact types, `SphericalSurfaceArtifact`, `ResolvedGeologicInputArtifact`, `ReliefGenerator::generate_spherical`, and `GeologicGenerator::generate_spherical`.
- Produces: `SphericalReliefArtifact`, `SphericalReliefStage`, `SphericalReliefStageInputs`, `SphericalGeologicArtifact`, `SphericalGeologicStage`, and `SphericalGeologicStageInputs`.

- [ ] **Step 1: Write failing Stage/Artifact contract tests**

Assert keys `world.spherical-relief` and `world.spherical-geology`, Stage IDs `natural.spherical-relief` and `natural.spherical-geology`, namespace `sekai.core`, version `1`, exact dependency arrays, strict wrapper serde, cross-surface rejection, and equality with direct generator results under the same stage identities.

The relief test must also verify that a generator diagnostic emitted through the stage appears in `BuildReport`; the geology stage must emit no synthetic diagnostic.

- [ ] **Step 2: Run and observe RED**

Run: `cargo test --test spherical_relief_geologic_stage -- --nocapture`

Expected: compilation fails on the missing relief/geology Artifact types.

- [ ] **Step 3: Implement the relief adapter**

Use exact dependencies `[SphericalMantleArtifact::KEY, SphericalSurfaceArtifact::KEY, SphericalTectonicArtifact::KEY]`. Call:

```rust
ReliefGenerator::generate_spherical(
    inputs.surface.snapshot(),
    inputs.tectonic.snapshot(),
    inputs.mantle.snapshot(),
    rng,
    diagnostics,
)
```

Then call the public three-upstream `validate_against` method and publish the strict Artifact.

- [ ] **Step 4: Implement the geology adapter**

Load the resolved geologic input plus surface, tectonic, mantle, and relief. Match `GeologicModel::CurrentSliceV1`, call the existing spherical generator, then call:

```rust
snapshot.validate_against(
    inputs.surface.snapshot(),
    inputs.tectonic.snapshot(),
    inputs.mantle.snapshot(),
    inputs.relief.snapshot(),
)
```

Use the stable geology codes `spherical-natural.invalid-geologic-input`, `spherical-natural.geologic-build-failed`, and `spherical-natural.invalid-geology`; use the relief codes `spherical-natural.invalid-relief-input`, `spherical-natural.relief-build-failed`, and `spherical-natural.invalid-relief`. Preserve the source error text so a `SurfaceRef` mismatch remains identifiable.

- [ ] **Step 5: Run focused and scientific regression tests**

Run:

```powershell
cargo test --test spherical_relief_geologic_stage -- --nocapture
cargo test --test spherical_relief_generation --test spherical_geologic_generation --test spherical_relief_geology_matrix -- --nocapture
```

Expected: all pass with existing direct-generator hashes unchanged.

- [ ] **Step 6: Commit**

```powershell
git add src/generators/natural/spherical_stage.rs src/generators/natural/spherical_geologic_stage.rs src/generators/natural/mod.rs tests/spherical_relief_geologic_stage.rs
git commit -m "feat: publish spherical relief geology stages"
```

---

### Task 4: Publish spherical preliminary climate and atomic hydro-erosion stages

**Files:**
- Create: `src/generators/natural/spherical_climate_stage.rs`
- Create: `src/generators/natural/spherical_hydro_erosion_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/spherical_climate_hydro_stage.rs`

**Interfaces:**
- Consumes: `SphericalSurfaceArtifact`, `SphericalReliefArtifact`, `SphericalGeologicArtifact`, `ResolvedClimateInputArtifact`, `ResolvedHydroErosionInputArtifact`, `ClimateGenerator::generate_spherical`, and `HydroErosionGenerator::generate_spherical`.
- Produces: `SphericalPreliminaryClimateArtifact`, `SphericalPreliminaryClimateStage`, `SphericalPreliminaryClimateStageInputs`, `SphericalHydroErosionArtifact`, `SphericalHydroErosionStage`, and `SphericalHydroErosionStageInputs`.

- [ ] **Step 1: Write RED tests for both downstream stages**

Assert the approved keys/IDs/version/namespace, exact typed dependencies, strict serde, same-count/different-surface rejection, and equality with direct generator calls. The climate and hydro adapters consume no RNG draws; prove that their outputs equal direct calls regardless of an unused `StageRng` value.

- [ ] **Step 2: Run and observe RED**

Run: `cargo test --test spherical_climate_hydro_stage -- --nocapture`

Expected: compilation fails on the missing stage modules and types.

- [ ] **Step 3: Implement preliminary-climate transport**

Match only `ClimateModel::SeasonalEnergyMoistureV1`, call:

```rust
ClimateGenerator::generate_spherical(
    inputs.surface.snapshot(),
    inputs.relief.snapshot(),
    inputs.resolved_input.input().spec(),
)
```

Validate with `snapshot.validate_against(surface, relief)` and publish key `world.spherical-preliminary-climate`.

- [ ] **Step 4: Implement atomic hydro-erosion transport**

Match only `HydroErosionModel::PriorityFloodStreamPowerV1`, call the one-index, two-hydrology-pass generator, validate with the public four-upstream `validate_against`, and publish key `world.spherical-hydro-erosion`. The stage must not publish initial hydrology separately.

- [ ] **Step 5: Run focused and direct-generator regressions**

Run:

```powershell
cargo test --test spherical_climate_hydro_stage -- --nocapture
cargo test --test spherical_climate_generation --test spherical_climate_matrix --test spherical_hydro_erosion_generation --test spherical_hydro_erosion_matrix -- --nocapture
```

Expected: all pass and no existing sphere scientific fixture changes.

- [ ] **Step 6: Commit**

```powershell
git add src/generators/natural/spherical_climate_stage.rs src/generators/natural/spherical_hydro_erosion_stage.rs src/generators/natural/mod.rs tests/spherical_climate_hydro_stage.rs
git commit -m "feat: publish spherical climate hydro stages"
```

---

### Task 5: Compose the authoritative spherical graph and prove cache identity

**Files:**
- Modify: `src/generators/natural/spherical_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/spherical_natural_stage_graph.rs`

**Interfaces:**
- Consumes: all Task 2–4 stages, `SphericalSurfaceStage`, and the existing rule/formation stages.
- Produces: `spherical_natural_foundation_graph() -> Result<StageGraph, GraphError>` with exactly eight external inputs and all required resolved stages.

- [ ] **Step 1: Write the failing whole-graph metadata test**

Construct the graph and assert:

```rust
let graph = spherical_natural_foundation_graph().unwrap();
let ids = graph.stage_ids();
for required in [
    "spatial.spherical-voronoi",
    "natural.spherical-tectonics",
    "natural.spherical-mantle",
    "natural.spherical-relief",
    "natural.spherical-geology",
    "natural.spherical-preliminary-climate",
    "natural.spherical-hydro-erosion",
] {
    assert!(ids.contains(&required), "missing {required}");
}
assert!(graph.descriptors().iter().all(|descriptor| {
    !descriptor.dependencies().iter().any(|key| {
        matches!(key.as_str(), "spatial.planar-spec" | "world.spatial")
    }) || !descriptor.id().as_str().starts_with("natural.spherical-")
}));
```

Also assert every sphere-natural descriptor directly includes `SphericalSurfaceArtifact::KEY`.

- [ ] **Step 2: Run the metadata test and observe RED**

Run: `cargo test --test spherical_natural_stage_graph graph_declares_the_authoritative_sphere_path -- --nocapture`

Expected: compilation fails because `spherical_natural_foundation_graph` is missing.

- [ ] **Step 3: Implement the graph with exact external inputs**

Use this registration shape:

```rust
StageGraphBuilder::new()
    .external::<SphericalSpaceArtifact>()
    .external::<TectonicSpecArtifact>()
    .external::<GeologicSpecArtifact>()
    .external::<ClimateSpecArtifact>()
    .external::<HydroErosionSpecArtifact>()
    .external::<WorldFormationSpecArtifact>()
    .external::<RulePackSetArtifact>()
    .external::<AuthorConstraintsArtifact>()
    .stage(SphericalSurfaceStage)
    .stage(RuleTectonicResolutionStage)
    .stage(RuleGeologicResolutionStage)
    .stage(RuleClimateResolutionStage)
    .stage(RuleHydroErosionResolutionStage)
    .stage(ResolvedTectonicInputStage)
    .stage(ResolvedGeologicInputStage)
    .stage(ResolvedClimateInputStage)
    .stage(ResolvedHydroErosionInputStage)
    .stage(WorldFormationStage)
    .stage(SphericalTectonicStage)
    .stage(SphericalMantleStage)
    .stage(SphericalReliefStage)
    .stage(SphericalGeologicStage)
    .stage(SphericalPreliminaryClimateStage)
    .stage(SphericalHydroErosionStage)
    .build()
```

- [ ] **Step 4: Add end-to-end cross-validation and deterministic hash tests**

Build a 162-cell Earth-radius fixture through `BuildEngine`. Extract all eight sphere surface/natural Artifacts and call each public `validate_against` chain. Build twice with identical inputs and assert Artifact content hashes and `BuildResultHash` equality.

For fixed graph goldens, temporarily make the test print all seven produced sphere hashes and the result hash under `--nocapture`, run it once, then replace the print-only assertions with exact byte/hex constants in the same test before committing. This is the approved golden-capture step; do not regenerate those constants after review without a scientific version change.

- [ ] **Step 5: Add the exact cache invalidation matrix**

For every row below, create a fresh `MemoryStageCache::with_max_entries(128)`, warm it with the same 162-cell Earth baseline, change only the named input, then assert the exact miss set; every stage not named in that row must be a hit:

1. Root seed: all 16 stages miss because the root-derived stage seed participates in every cache key.
2. Formation spec: `natural.resolve-world-formation` and all six `natural.spherical-*` stages miss.
3. Tectonic spec: `natural.resolve-tectonic-rules`, `natural.project-tectonic-input`, `natural.spherical-tectonics`, `natural.spherical-relief`, `natural.spherical-geology`, `natural.spherical-preliminary-climate`, and `natural.spherical-hydro-erosion` miss; spherical mantle remains a hit.
4. Geologic spec: `natural.resolve-geologic-rules`, `natural.project-geologic-input`, `natural.spherical-mantle`, `natural.spherical-relief`, `natural.spherical-geology`, `natural.spherical-preliminary-climate`, and `natural.spherical-hydro-erosion` miss; spherical tectonics remains a hit.
5. Climate spec: `natural.resolve-climate-rules`, `natural.project-climate-input`, `natural.spherical-preliminary-climate`, and `natural.spherical-hydro-erosion` miss.
6. Hydro-erosion spec: `natural.resolve-hydro-erosion-rules`, `natural.project-hydro-erosion-input`, and `natural.spherical-hydro-erosion` miss.

Use one additional fresh cache and the same root seed for spatial identity:

1. First Earth-radius build: all stages are cold.
2. Identical build: all stages report `cache_hit() == true`.
3. Same resolved cell count with a different radius: rule/resolved/formation stages remain hits, while `spatial.spherical-voronoi` and every `natural.spherical-*` stage are misses.

Assert both surfaces have equal cell/edge counts and unequal `SurfaceRef` values so the test cannot pass by changing cardinality.

- [ ] **Step 6: Run graph, engine, and legacy regressions**

Run:

```powershell
cargo test --test spherical_natural_stage_graph -- --nocapture
cargo test --test engine_execution --test natural_stage_graph --test legacy_planar_boundary -- --nocapture
```

Expected: all pass; the sphere and legacy graphs remain disjoint.

- [ ] **Step 7: Commit**

```powershell
git add src/generators/natural/spherical_stage.rs src/generators/natural/mod.rs tests/spherical_natural_stage_graph.rs
git commit -m "feat: compose spherical natural stage graph"
```

---

### Task 6: Make the field registry sphere-area safe without changing planar V1

**Files:**
- Modify: `src/world/natural/fields.rs:21-29, 211-230, 229-704, 854-868`
- Modify: `src/world/natural/mod.rs:51-67`
- Create: `tests/natural_field_registry_spherical.rs`

**Interfaces:**
- Consumes: existing `natural_field_registry(plate_count)` and natural field schema builder.
- Produces: `spherical_natural_field_registry(plate_count: u16, total_surface_area_m2: f64) -> Result<FieldRegistry, NaturalFieldRegistryError>` using the same Field IDs and schema builder.

- [ ] **Step 1: Capture and freeze current planar registry bytes with a deliberate RED assertion**

Create a test that serializes `natural_field_registry(12)` and compares its BLAKE3 hash to a deliberately invalid 64-zero string. Run it once; the expected assertion failure prints the actual hash. Replace only the expected string with that observed value and rerun to GREEN before modifying production code. This establishes a pre-refactor planar compatibility constant.

- [ ] **Step 2: Write failing sphere-area range tests**

For radius `100_000_000 m`, compute `4πR²`, build the proposed spherical registry, and assert that the valid maxima for `drainage_area_km2` and `mean_annual_discharge_m3_s` cover the full-surface physical maxima. Also assert zero, negative, non-finite, and f32-overflowing areas return explicit registry errors.

- [ ] **Step 3: Run and observe RED**

Run: `cargo test --test natural_field_registry_spherical -- --nocapture`

Expected: compilation fails because `spherical_natural_field_registry` does not exist.

- [ ] **Step 4: Parameterize the single schema builder**

Introduce a private limits record:

```rust
#[derive(Debug, Clone, Copy)]
struct NaturalFieldRegistryLimits {
    max_drainage_area_km2: f32,
    max_mean_annual_discharge_m3_s: f32,
}
```

Keep `natural_field_registry` passing the exact existing constants. The spherical wrapper validates `total_surface_area_m2`, converts area to km², derives maximum discharge from `ANNUAL_PRECIPITATION_MAX_MM / CLIMATOLOGICAL_YEAR_SECONDS`, and calls the same `schemas(plate_count, limits)` function. Add named error variants for invalid area and representational overflow.

- [ ] **Step 5: Run focused, field, and planar golden tests**

Run:

```powershell
cargo test --test natural_field_registry_spherical -- --nocapture
cargo test --test field_contracts --test natural_field_views --test natural_display_golden -- --nocapture
```

Expected: sphere ranges pass and the frozen planar registry hash remains exact.

- [ ] **Step 6: Commit**

```powershell
git add src/world/natural/fields.rs src/world/natural/mod.rs tests/natural_field_registry_spherical.rs
git commit -m "feat: bound natural fields by spherical area"
```

---

### Task 7: Separate data-only field documents from presentation and centralize payload mapping

**Files:**
- Modify: `src/app/field_document.rs:1-267`
- Create: `src/app/natural_field_payloads.rs`
- Modify: `src/app/natural_display.rs:1-444`
- Modify: `src/app.rs:5-13`
- Test: `src/app/field_document.rs` unit tests
- Test: `src/app/natural_display.rs` unit tests

**Interfaces:**
- Consumes: current `AppFieldDocument`, planar natural snapshots, and the stable registry.
- Produces: `FieldDocument`, `PresentedFieldDocument: FieldDocument`, and `NaturalFieldPayloadBundle<'a>` with a single `payloads()` ID mapping.

- [ ] **Step 1: Write RED compile/behavior tests for the split traits**

Add a `DataOnlyDocument` fixture that implements `FieldDocument` but has no mesh. Assert its catalog, diagnostics, preferred field, and preferred range are usable. Keep `prepare_new_document_display` constrained to `PresentedFieldDocument` so attempting to pass `DataOnlyDocument` is impossible at the type boundary; use the unit test module's real presented fixture to prove existing preparation still works.

- [ ] **Step 2: Run and observe RED**

Run: `cargo test --lib app::field_document::tests -- --nocapture`

Expected: compilation fails because the two traits do not exist.

- [ ] **Step 3: Split the trait without relying on trait-object upcasting**

Use generic functions compatible with Rust 1.85:

```rust
pub(super) trait FieldDocument {
    fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError>;
    fn diagnostics(&self) -> &[OwnedViewDiagnostic];
    fn preferred_field(&self) -> Option<FieldId>;
    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode>;
}

pub(super) trait PresentedFieldDocument: FieldDocument {
    fn mesh(&self) -> &Arc<PreparedCellMesh>;
}

pub(super) fn prepare_new_document_display<D: PresentedFieldDocument + ?Sized>(
    document: &D,
    current_state: &FieldDisplayState,
    clock: &mut DisplayRevisionClock,
) -> Result<(FieldDisplayState, Arc<PreparedFieldDisplay>), DisplayPrepareError> {
    let catalog = document.catalog()?;
    let mut state = current_state.clone();
    let retained_selection = state
        .selected_field()
        .and_then(|field| catalog.get(field))
        .and_then(|entry| entry.view())
        .is_some_and(|view| view.cell_fill_kind().is_ok());
    if !retained_selection {
        if let Some(preferred) = document.preferred_field() {
            state.select_field(preferred);
        }
    }
    state.reconcile(&catalog, document.mesh().cell_count());
    if !retained_selection {
        if let Some(mode) = state
            .selected_field()
            .and_then(|field| document.preferred_range(field))
        {
            state.set_range_mode(mode);
        }
    }
    let parts = prepare_display_parts(document, &catalog, &state)?;
    let revisions = issue_all_revisions(clock)?;
    let packet = Arc::new(PreparedFieldDisplay::new(
        document.mesh().clone(),
        parts.field,
        parts.diagnostics,
        parts.palette,
        revisions,
        state.diagnostics_enabled(),
    )?);
    Ok((state, packet))
}
```

Use these exact generic boundaries for the remaining helpers; `selected_field_view`, `prepare_palette`, `rebuild_changed_packet`, and `issue_all_revisions` remain non-document helpers:

```rust
pub(super) fn prepare_control_action<D: PresentedFieldDocument + ?Sized>(
    document: &D,
    current: &PreparedFieldDisplay,
    state: &mut FieldDisplayState,
    clock: &mut DisplayRevisionClock,
    action: FieldControlAction,
) -> Result<Arc<PreparedFieldDisplay>, DisplayPrepareError>;

fn prepare_display_parts<D: PresentedFieldDocument + ?Sized>(
    document: &D,
    catalog: &FieldCatalog<'_>,
    state: &FieldDisplayState,
) -> Result<PreparedDisplayParts, DisplayPrepareError>;

fn prepare_diagnostics<D: PresentedFieldDocument + ?Sized>(
    document: &D,
    state: &FieldDisplayState,
) -> Result<Arc<PreparedDiagnosticMask>, DisplayPrepareError>;
```

- [ ] **Step 4: Write a failing single-mapping test for natural payloads**

The planar document test must construct `NaturalFieldPayloadBundle::from_legacy_planar(...)`, call `payloads()`, and assert every registry ID appears exactly once, every cell payload has the planar cell count, every edge payload has the edge count, and the borrowed elevation pointer still equals the authoritative snapshot slice pointer.

- [ ] **Step 5: Implement the common borrowed bundle and refactor planar document**

`NaturalFieldPayloadBundle<'a>` stores only borrowed slices for the existing 36 natural fields. The one mapping covers these exact IDs, in registry order: `plate_id`, `crust_kind`, `crust_thickness_km`, `plate_velocity`, `boundary_kind`, `boundary_strength`, `crust_base_elevation_m`, `tectonic_offset_m`, `regional_offset_m`, `elevation_m`, `land_ocean`, `mantle_heat_flow_mw_m2`, `volcanic_influence`, `volcanic_offset_m`, `bedrock_kind`, `fracture_intensity`, `erosion_resistance`, `relative_permeability`, `metallic_mineral_potential`, `geothermal_potential`, `sedimentary_basin_potential`, `latitude_degrees`, `maritime_influence`, `preliminary_prevailing_wind_m_s`, `preliminary_mean_air_temperature_c`, `preliminary_temperature_seasonality_c`, `preliminary_annual_precipitation_mm`, `surface_elevation_m`, `fluvial_erosion_depth_m`, `sediment_deposition_thickness_m`, `surface_water_kind`, `lake_depth_m`, `annual_local_runoff_mm`, `mean_annual_discharge_m3_s`, `drainage_area_km2`, and `strahler_stream_order`. It has two constructors—`from_legacy_planar` now and `from_spherical` in Task 8—but exactly one `payloads()` method owns this Field ID mapping. Move the current `NaturalFieldDocument::payloads` mapping into this file and have `LegacyPlanarNaturalFieldDocument` delegate to it.

Also extract exactly two shared helpers: `owned_view_diagnostics(&BuildReport) -> Vec<OwnedViewDiagnostic>` in `field_document.rs`, and `natural_preferred_range(&FieldRegistry, sea_level_m: f32, surface_elevation_m: &[f32], &FieldId) -> Option<DisplayRangeMode>` in `natural_field_payloads.rs`. Both planar and spherical documents use them. Keep `PreparedCellMesh` and the existing `NaturalFieldDisplayCache` solely in the legacy document.

- [ ] **Step 6: Run all field/display regressions**

Run:

```powershell
cargo test --lib app::field_document::tests -- --nocapture
cargo test --lib app::natural_display::tests -- --nocapture
cargo test --test natural_field_views --test natural_display_golden --test field_display_golden -- --nocapture
```

Expected: all pass with identical planar field pointers, ranges, and images.

- [ ] **Step 7: Commit**

```powershell
git add src/app/field_document.rs src/app/natural_field_payloads.rs src/app/natural_display.rs src/app.rs
git commit -m "refactor: separate natural data presentation"
```

---

### Task 8: Build the projection-free spherical field document and provenance identity

**Files:**
- Modify: `src/world/spatial/sphere_geometry.rs`
- Modify: `src/world/spatial/mod.rs`
- Modify: `src/app/natural_field_payloads.rs`
- Create: `src/app/spherical_natural_display.rs`
- Modify: `src/app.rs:5-13`
- Test: `src/world/spatial/sphere_geometry.rs` unit tests
- Test: `src/app/spherical_natural_display.rs` unit tests

**Interfaces:**
- Consumes: a successful spherical `BuildOutcome`, root seed, all sphere Artifacts, the Task 6 registry, and Task 7 data-only trait/bundle.
- Produces: `canonical_east_north_basis(UnitVector3) -> ([f64; 3], [f64; 3])`, `SphericalNaturalBuildIdentity`, `SphericalNaturalDisplayCache`, and `SphericalNaturalFieldDocument: FieldDocument`.

- [ ] **Step 1: Write RED geometry tests for the canonical tangent basis**

Test a generic radial vector and both poles. Assert east/north are finite, unit length, mutually orthogonal, tangent to radial, right-handed under the documented convention, and byte-deterministic on repeat. At the poles assert the exact canonical axes rather than accepting any basis.

- [ ] **Step 2: Run basis tests and observe RED**

Run: `cargo test --lib world::spatial::sphere_geometry::tests -- --nocapture`

Expected: compilation fails because `canonical_east_north_basis` is missing.

- [ ] **Step 3: Implement the projection-independent basis**

Use the +Z spin axis away from the poles and a fixed +Y east axis at both poles:

```rust
pub fn canonical_east_north_basis(radial: UnitVector3) -> ([f64; 3], [f64; 3]) {
    let [x, y, z] = radial.components();
    let horizontal = x.hypot(y);
    if horizontal > f64::EPSILON {
        let east = [-y / horizontal, x / horizontal, 0.0];
        let north = [-z * east[1], z * east[0], horizontal];
        (east, north)
    } else {
        let east = [0.0, 1.0, 0.0];
        let north = if z >= 0.0 {
            [-1.0, 0.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        (east, north)
    }
}
```

- [ ] **Step 4: Write RED spherical-document tests**

Build the 162-cell whole graph and demand this exact API:

```rust
let document = SphericalNaturalFieldDocument::from_build_outcome(
    RootSeed::new(42),
    &outcome,
).unwrap();
assert_eq!(
    document.identity().surface_ref(),
    SurfaceRef::for_spherical(document.surface.snapshot()),
);
assert_eq!(document.identity().build_result_hash(), outcome.report.result_hash().unwrap());
assert_eq!(document.catalog().unwrap().entries().len(), 36);
```

`SphericalNaturalBuildIdentity::surface_ref()` returns `SurfaceRef`; `build_result_hash()` returns `&BuildResultHash`; `root_seed()` returns `RootSeed`; `graph_contract_version()` returns `u16` with initial value `1`.

Also test:

- every payload appears once with the correct cell/edge cardinality;
- scalar/category slices borrow authoritative snapshot memory;
- a registry-order field hash over each Field ID, domain, payload discriminant, and every little-endian scalar/category/vector payload value is identical across two documents built from the same outcome, and is frozen to one exact BLAKE3 hex constant captured once under `--nocapture` before commit;
- local east/north plate velocity and wind reconstruct the authoritative 3D tangent vectors within `1e-5` in their source units;
- a self-valid equal-count Artifact from a different surface is rejected;
- a `BuildReport` without `BuildResultHash` is rejected;
- constructing the document twice from the same successful outcome preserves `Arc::ptr_eq` for every Artifact while recreating byte-identical disposable vector/boundary caches, proving S0C can discard and rebuild presentation derivations without regenerating science;
- a failed document candidate leaves a previously published `Arc<SphericalNaturalFieldDocument>` pointer unchanged in the composition-boundary test;
- the type owns no `PreparedCellMesh` and implements only `FieldDocument`.

- [ ] **Step 5: Run and observe RED**

Run: `cargo test --lib app::spherical_natural_display::tests -- --nocapture`

Expected: compilation fails because the spherical document module is missing.

- [ ] **Step 6: Implement build identity, display cache, and complete validation**

The document stores `Arc` handles to surface, formation, tectonic, mantle, relief, geology, climate, and hydro-erosion Artifacts plus registry, diagnostics, vector cache, and identity. `from_build_outcome` extracts typed handles without cloning snapshots, obtains the report hash, then calls a narrow `build` constructor.

Validation order is:

```rust
surface.snapshot().validate()?;
formation.formation().validate()?;
tectonic.snapshot().validate_against(surface.snapshot())?;
mantle.snapshot().validate_against(surface.snapshot())?;
relief.snapshot().validate_against(surface.snapshot(), tectonic.snapshot(), mantle.snapshot())?;
geology.snapshot().validate_against(
    surface.snapshot(), tectonic.snapshot(), mantle.snapshot(), relief.snapshot(),
)?;
climate.snapshot().validate_against(surface.snapshot(), relief.snapshot())?;
hydro_erosion.snapshot().validate_against(
    surface.snapshot(), relief.snapshot(), geology.snapshot(), climate.snapshot(),
)?;
```

Build the sphere registry from the authoritative sum of cell areas. Derive plate velocity in cm/year and wind in m/s by dotting each source 3D vector against the canonical basis. Boundary kind/strength arrays remain disposable derivations from the authoritative edge records.

- [ ] **Step 7: Complete the shared spherical payload constructor and run GREEN**

Implement `NaturalFieldPayloadBundle::from_spherical` using the sphere snapshots and display cache, then run:

```powershell
cargo test --lib world::spatial::sphere_geometry::tests -- --nocapture
cargo test --lib app::spherical_natural_display::tests -- --nocapture
cargo test --lib app::field_document::tests -- --nocapture
cargo test --lib app::natural_display::tests -- --nocapture
```

Expected: all pass and the sphere document has no presentation mesh.

- [ ] **Step 8: Commit**

```powershell
git add src/world/spatial/sphere_geometry.rs src/world/spatial/mod.rs src/app/natural_field_payloads.rs src/app/spherical_natural_display.rs src/app.rs
git commit -m "feat: publish spherical natural field document"
```

---

### Task 9: Run final compatibility, performance, and ownership acceptance

**Files:**
- Create: `tests/spherical_natural_graph_performance.rs`
- Modify: `docs/superpowers/plans/2026-08-04-spherical-natural-product-integration.md`
- Potential corrective files, only when named by a failing acceptance test: `src/generators/natural/spherical_stage.rs`, `src/generators/natural/spherical_geologic_stage.rs`, `src/generators/natural/spherical_climate_stage.rs`, `src/generators/natural/spherical_hydro_erosion_stage.rs`, `src/world/natural/fields.rs`, `src/app/natural_field_payloads.rs`, `src/app/spherical_natural_display.rs`

**Interfaces:**
- Consumes: the completed spherical graph/document and frozen planar compatibility path.
- Produces: Release performance/memory evidence, final golden hashes, ownership audit, and a completed S0B.6 checklist.

- [ ] **Step 1: Write the ignored Release full-graph budget test**

Build an Earth-radius `target_cell_count = 20_000` sphere through `spherical_natural_foundation_graph`. Measure the full `BuildEngine::build`, validate the final document, calculate persistent semantic bytes per Artifact, serialized Artifact bytes separately, and process working-set delta with the existing Windows/Linux helpers.

Assert:

```rust
assert!(sphere_elapsed <= Duration::from_secs(5));
assert!(sphere_elapsed.as_secs_f64() <= planar_baseline.as_secs_f64() * 2.5);
if let Some(additional_working_set_bytes) = additional_working_set_bytes {
    assert!(additional_working_set_bytes <= 256 * 1024 * 1024);
}
```

Run the planar baseline first in the same Release test binary by reusing the exact `tests/natural_performance.rs` fixture: root seed `42`, `PlanarSpaceSpec { width: 40_000_000 m, height: 20_000_000 m, target_cell_count: 20_000, boundary: WrapBoth }`, default tectonic/geologic/climate/hydro/formation inputs, and empty rules/constraints. Time only `BuildEngine::build(legacy_planar_natural_foundation_graph(), ...)`. Drop that outcome and cache before taking the sphere working-set baseline. Then build the Earth-radius sphere with root seed `42`, the same non-spatial external inputs, and `target_cell_count: 20_000`; print all counts/timings/bytes with one stable evidence line.

- [ ] **Step 2: Run focused Release and scientific acceptance**

Run:

```powershell
cargo test --release --test spherical_natural_graph_performance -- --ignored --nocapture
cargo test --release --test spherical_natural_stage_graph -- --nocapture
cargo test --test spherical_natural_matrix --test spherical_relief_geology_matrix --test spherical_climate_matrix --test spherical_hydro_erosion_matrix -- --nocapture
```

Expected: hard time/memory gates pass and every frozen scientific matrix remains exact.

- [ ] **Step 3: Audit ownership and forbidden dependencies**

Run:

```powershell
rg -n -g 'spherical_*stage.rs' 'PlanarSpaceArtifact|SpatialArtifact|PreparedCellMesh|projection|gpu|wgpu|Canvas' src/generators/natural
rg -n 'Vec<.*(Cell|Edge|Vertex)|PreparedCellMesh|projection|gpu|wgpu' src/app/spherical_natural_display.rs
rg -n 'spherical_natural_foundation_graph|legacy_planar_natural_foundation_graph' src/app.rs src/generators/natural
```

Expected: sphere stage files contain no planar/presentation types; the spherical document contains Artifact handles and derived value arrays but no copied geometry or mesh; the visible S0B.6 canvas calls only the explicit legacy presenter while the new graph is the sole sphere creation entry.

- [ ] **Step 4: Run the complete verification suite**

Run fresh, in this order:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo test --workspace --doc
cargo check --target wasm32-unknown-unknown --all-features --lib
```

Expected: every command exits `0`, with only the repository's explicitly ignored stress/doc examples reported as ignored.

- [ ] **Step 5: Record exact execution evidence and self-audit the spec**

Append an `Execution Evidence` section containing:

- compiler/OS/CPU;
- RED and GREEN commands for each task;
- exact frozen graph hashes and cache-hit matrix;
- Release planar/sphere time ratio, persistent bytes, serialized bytes, and working-set delta;
- full verification counts;
- ownership grep results;
- a line-by-line mapping of all eight S0B.6 completion criteria to tests/evidence.

Then search the plan for unchecked implementation steps and forbidden incomplete-work markers; resolve every actual implementation checkbox before final review.

- [ ] **Step 6: Commit final acceptance evidence**

```powershell
git add tests/spherical_natural_graph_performance.rs docs/superpowers/plans/2026-08-04-spherical-natural-product-integration.md
git commit -m "test: gate spherical natural product integration"
```
