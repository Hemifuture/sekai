# Geologic Substrate V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add rule-selected, deterministic current-slice mantle hotspots, volcanic relief, bedrock properties, and geology-based resource potentials to the formal natural generation graph and field display.

**Architecture:** Add an independent `MantleStage` upstream of the sole relief writer, then derive a read-only `GeologicSnapshot` downstream from spatial, tectonic, mantle, and relief artifacts. Extend the closed rule capability system with one required world-law geology model and preserve audit-to-projection cache isolation. Keep world contracts, generators, engine adapters, app composition, and display adapters in one-way dependency order.

**Tech Stack:** Rust 2021, serde, thiserror, rand/ChaCha8, blake3, existing typed stage engine, egui/eframe, wgpu field display, cargo test/clippy/fmt, wasm32, Trunk.

## Global Constraints

- Generate only a current world slice: no geological dates, hotspot tracks, event timelines, eruption histories, or plate-time integration.
- `world::natural` must not import engine, rules, generators, app, view, UI, egui, eframe, or wgpu.
- `rules` may select compiled models and validate typed data only; it must not read natural snapshot arrays.
- The engine-owned required world-law capability ID is exactly `sekai.core.natural.geologic-model@1`.
- `generators::natural` must not import legacy `terrain`, app, view, UI, egui, eframe, or wgpu.
- `ReliefStage` remains the only authoritative writer of final elevation and land/ocean classification.
- `MantleGenerator` must not read plate IDs, crust, boundaries, relief, or display state.
- `GeologicGenerator` is read-only over all upstream artifacts.
- `hotspot_count` is `0..=16`; default is `4`, with `MantleActivity::Moderate`.
- `MantleSnapshot` heat flow is finite `20..=400 mW/m²`; volcanic influence is finite `0..=1`.
- Relief V2 adds finite `volcanic_offset_m` in `0..=4_000 m` while final elevation remains `-11_000..=9_000 m`.
- Every continuous geology field is finite and in `0..=1`.
- Potentials are relative geologic permissiveness, never probability, reserve, grade, value, or a discovered deposit.
- Dense authoritative arrays must be borrowed into `FieldView`; do not duplicate them into `ExtensionFieldSet`.
- Native and WASM classifications must use stable order and fixed quantization before thresholds.
- All changes follow red-green-refactor TDD and end in focused commits.

---

### Task 1: Define Stable Hotspot Identity and Geologic Specification

**Files:**

- Modify: `src/world/ids.rs`
- Modify: `src/world/mod.rs`
- Create: `src/world/natural/geologic_spec.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/geologic_spec.rs`

**Interfaces:**

- Produces: `HotspotId`, `GEOLOGIC_SPEC_SCHEMA_V1`, `MAX_HOTSPOT_COUNT`,
  `MantleActivity`, `GeologicSpec`, and `GeologicSpecError`.
- `GeologicSpec::validate(&self) -> Result<(), GeologicSpecError>`.
- `Default for GeologicSpec` returns schema `1`, four hotspots, and moderate activity.

- [ ] **Step 1: Write failing identity and specification tests**

Create `tests/geologic_spec.rs` with exact boundary, default, JSON, and private-invariant checks:

```rust
use sekai::world::natural::{
    GeologicSpec, GeologicSpecError, MantleActivity, GEOLOGIC_SPEC_SCHEMA_V1,
    MAX_HOTSPOT_COUNT,
};
use sekai::world::HotspotId;

#[test]
fn hotspot_ids_round_trip_raw_values() {
    let id = HotspotId::from_raw(7);
    assert_eq!(id.raw(), 7);
    assert_eq!(serde_json::from_str::<HotspotId>(&serde_json::to_string(&id).unwrap()).unwrap(), id);
}

#[test]
fn default_geologic_spec_is_earthlike_and_valid() {
    let spec = GeologicSpec::default();
    assert_eq!(spec.schema_version, GEOLOGIC_SPEC_SCHEMA_V1);
    assert_eq!(spec.hotspot_count, 4);
    assert_eq!(spec.mantle_activity, MantleActivity::Moderate);
    spec.validate().unwrap();
}

#[test]
fn hotspot_count_accepts_inclusive_boundaries() {
    for count in [0, MAX_HOTSPOT_COUNT] {
        GeologicSpec {
            hotspot_count: count,
            ..GeologicSpec::default()
        }
        .validate()
        .unwrap();
    }
}

#[test]
fn invalid_schema_and_hotspot_count_are_rejected() {
    assert!(matches!(
        GeologicSpec {
            schema_version: 2,
            ..GeologicSpec::default()
        }
        .validate(),
        Err(GeologicSpecError::UnsupportedSchema { .. })
    ));
    assert!(matches!(
        GeologicSpec {
            hotspot_count: MAX_HOTSPOT_COUNT + 1,
            ..GeologicSpec::default()
        }
        .validate(),
        Err(GeologicSpecError::HotspotCountOutOfRange { .. })
    ));
}
```

Add a JSON mutation test that serializes the default, sets `schema_version` to `2`, and asserts deserialization fails. This proves serde cannot bypass `validate`.

- [ ] **Step 2: Run the focused test and verify the missing-contract failure**

Run:

```powershell
cargo test --test geologic_spec
```

Expected: compilation fails because the new world types and module do not exist.

- [ ] **Step 3: Implement the minimal validated contracts**

Add `define_id!(HotspotId, u32)` and export it from `world::mod`.

In `geologic_spec.rs`, implement:

```rust
pub const GEOLOGIC_SPEC_SCHEMA_V1: u16 = 1;
pub const MAX_HOTSPOT_COUNT: u16 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MantleActivity {
    Quiet,
    Moderate,
    Active,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GeologicSpec {
    pub schema_version: u16,
    pub hotspot_count: u16,
    pub mantle_activity: MantleActivity,
}

impl Default for GeologicSpec {
    fn default() -> Self {
        Self {
            schema_version: GEOLOGIC_SPEC_SCHEMA_V1,
            hotspot_count: 4,
            mantle_activity: MantleActivity::Moderate,
        }
    }
}
```

Implement manual `Deserialize` through a private wire type, call `validate`, and map the error with `serde::de::Error::custom`. Define errors:

```rust
pub enum GeologicSpecError {
    UnsupportedSchema { found: u16, supported: u16 },
    HotspotCountOutOfRange { found: u16, max: u16 },
}
```

- [ ] **Step 4: Run contract and format checks**

Run:

```powershell
cargo test --test geologic_spec
cargo fmt --all -- --check
```

Expected: all geologic specification tests pass and formatting is clean.

- [ ] **Step 5: Commit the domain specification**

```powershell
git add src/world/ids.rs src/world/mod.rs src/world/natural/geologic_spec.rs src/world/natural/mod.rs tests/geologic_spec.rs
git commit -m "feat: define geologic world specs"
```

---

### Task 2: Register the Required Geologic World-Law Capability

**Files:**

- Modify: `src/rules/capability.rs`
- Modify: `src/rules/builtin.rs`
- Modify: `src/rules/mod.rs`
- Modify: `src/rules/tectonics.rs`
- Modify: `tests/rule_capabilities.rs`
- Modify: `tests/builtin_rules.rs`
- Modify: `tests/rule_manifests.rs`
- Modify: `tests/rule_pack_resolution.rs`
- Modify: `tests/rule_tectonic_resolution.rs`

**Interfaces:**

- Produces: `geologic_model_capability_id() -> CapabilityId`.
- Produces: closed `GeologicModel::CurrentSliceV1`.
- Extends `CapabilityContribution` with `GeologicModel(GeologicModel)`.
- `core_capability_registry()` contains three exact capabilities.
- `earthlike_rule_pack()` provides both required model capabilities.

- [ ] **Step 1: Extend capability and built-in tests first**

Add exact assertions:

```rust
assert_eq!(
    geologic_model_capability_id(),
    CapabilityId::new("sekai.core.natural", "geologic-model", 1).unwrap()
);
assert_eq!(core_capability_registry().unwrap().len(), 3);
```

Assert the new descriptor is:

```rust
CapabilityCardinality::UniqueRequired
RulePackKind::WorldLaw
author_allowed == false
```

Update the built-in contribution expectation to:

```rust
vec![
    CapabilityContribution::TectonicModel(TectonicModel::CurrentSliceV1),
    CapabilityContribution::GeologicModel(GeologicModel::CurrentSliceV1),
]
```

Add tests proving an ordinary pack cannot provide the geologic model and a second world-law geologic model fails unique cardinality.

Update test pack factories that resolve against `core_capability_registry()` so their world-law helper supplies both model contributions. Synthetic tests with custom registries stay unchanged.

- [ ] **Step 2: Run rule suites and verify the new symbols are missing**

Run:

```powershell
cargo test --test rule_capabilities --test builtin_rules --test rule_manifests --test rule_pack_resolution --test rule_tectonic_resolution
```

Expected: compilation fails on the missing geologic capability and model.

- [ ] **Step 3: Implement the closed capability**

Add:

```rust
const GEOLOGIC_MODEL_NAME: &str = "geologic-model";

pub fn geologic_model_capability_id() -> CapabilityId {
    CapabilityId::new(
        CORE_NATURAL_NAMESPACE,
        GEOLOGIC_MODEL_NAME,
        CAPABILITY_SCHEMA_V1,
    )
    .expect("the engine-owned geologic model capability ID is valid")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GeologicModel {
    CurrentSliceV1,
}
```

Extend contribution routing:

```rust
Self::GeologicModel(_) => geologic_model_capability_id()
```

Its `rule_item_id` is `None`. In `TectonicRuleResolver`, explicitly ignore
`CapabilityContribution::GeologicModel(_)`; do not add a wildcard arm.

Register the descriptor in `core_capability_registry` and add the contribution to the earthlike pack.

- [ ] **Step 4: Run all rule contract suites**

Run:

```powershell
cargo test --test rule_capabilities
cargo test --test builtin_rules
cargo test --test rule_manifests
cargo test --test rule_pack_resolution
cargo test --test rule_tectonic_resolution
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: all pass with no unhandled contribution variants.

- [ ] **Step 5: Commit the rule capability**

```powershell
git add src/rules/capability.rs src/rules/builtin.rs src/rules/mod.rs src/rules/tectonics.rs tests/rule_capabilities.rs tests/builtin_rules.rs tests/rule_manifests.rs tests/rule_pack_resolution.rs tests/rule_tectonic_resolution.rs
git commit -m "feat: register geologic world law"
```

---

### Task 3: Resolve and Project Geologic Rule Input

**Files:**

- Create: `src/rules/resolution.rs`
- Create: `src/rules/geology.rs`
- Modify: `src/rules/tectonics.rs`
- Modify: `src/rules/mod.rs`
- Create: `src/generators/natural/geologic_rule_input.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/rule_geologic_resolution.rs`
- Create: `tests/rule_geologic_stage.rs`

**Interfaces:**

- Moves the existing public `ResolvedRulePackRef` into `rules::resolution` without changing its public re-export.
- Produces `GEOLOGIC_RULE_RESOLUTION_SCHEMA_V1`.
- Produces `GeologicRuleResolution`, `GeologicRuleResolutionError`,
  `GeologicRuleResolver::resolve(&GeologicSpec, &ResolvedRulePackSet)`.
- Produces artifacts:
  - `GeologicSpecArtifact`, key `natural.geologic-spec`;
  - `GeologicRuleResolutionArtifact`, key `rules.geologic-resolution`;
  - `ResolvedGeologicInputArtifact`, key `natural.resolved-geologic-input`.
- Produces `RuleGeologicResolutionStage` and `ResolvedGeologicInputStage`.

- [ ] **Step 1: Write resolver and projection tests**

In `tests/rule_geologic_resolution.rs`, construct a pack set containing the earthlike pack and assert:

```rust
let resolved_packs = packs.resolve(&core_capability_registry().unwrap(), WORLD_SPEC_SCHEMA_V1).unwrap();
let resolution = GeologicRuleResolver::resolve(&GeologicSpec::default(), &resolved_packs).unwrap();
assert_eq!(resolution.model(), GeologicModel::CurrentSliceV1);
assert_eq!(resolution.spec(), &GeologicSpec::default());
assert_eq!(resolution.resolved_packs().len(), 1);
resolution.validate().unwrap();
```

Add:

- input pack order produces identical JSON;
- missing model fails `MissingGeologicModel`;
- duplicate model fails at capability resolution;
- invalid base spec fails before model resolution;
- private JSON mutations for schema, duplicate pack references, and invalid spec fail deserialization.

In `tests/rule_geologic_stage.rs`, build a focused graph:

```text
GeologicSpecArtifact + RulePackSetArtifact
  → RuleGeologicResolutionStage
  → ResolvedGeologicInputStage
```

Assert exact stage IDs, dependencies, artifact keys, no RNG effect, and:

```rust
assert_eq!(projection.input().model(), GeologicModel::CurrentSliceV1);
assert_eq!(projection.input().spec(), &GeologicSpec::default());
```

Create a second earthlike-equivalent pack with different audit identity but identical geologic model. Assert the resolution hash changes while the projection hash remains identical.

- [ ] **Step 2: Run the new tests and verify missing types**

Run:

```powershell
cargo test --test rule_geologic_resolution --test rule_geologic_stage
```

Expected: compilation fails because the resolver, artifacts, and stages are absent.

- [ ] **Step 3: Implement generic audit reference and pure resolver**

Move `ResolvedRulePackRef` and its accessors into `rules/resolution.rs`. Keep
`pub use` in `rules/mod.rs`, so existing users do not change import paths.

Implement:

```rust
pub struct GeologicRuleResolution {
    schema_version: u16,
    resolved_packs: Vec<ResolvedRulePackRef>,
    model: GeologicModel,
    spec: GeologicSpec,
}

impl GeologicRuleResolver {
    pub fn resolve(
        base: &GeologicSpec,
        packs: &ResolvedRulePackSet<'_>,
    ) -> Result<GeologicRuleResolution, GeologicRuleResolutionError>;
}
```

The resolver must:

1. validate the base spec;
2. record resolved pack refs in dependency order;
3. accept exactly one `CapabilityContribution::GeologicModel`;
4. ignore tectonic model and tectonic constraint contributions explicitly;
5. validate the final resolution before returning it.

Manual deserialization must re-run `validate`.

- [ ] **Step 4: Implement engine transport and audit projection**

In `geologic_rule_input.rs`, mirror the established tectonic boundary with geology-specific types:

```rust
pub struct ResolvedGeologicInput {
    model: GeologicModel,
    spec: GeologicSpec,
}

impl ResolvedGeologicInput {
    pub fn new(model: GeologicModel, spec: GeologicSpec) -> Result<Self, GeologicSpecError>;
    pub const fn model(&self) -> GeologicModel;
    pub const fn spec(&self) -> &GeologicSpec;
}
```

Use stage IDs:

```text
natural.resolve-geologic-rules
natural.project-geologic-input
```

Neither stage consumes RNG. Map pack dependency/capability errors to the existing stable rule diagnostic families and geologic base/spec errors to:

```text
rules.invalid-base-geologic-spec
rules.invalid-geologic-resolution
natural.invalid-resolved-geologic-input
```

Run:

```powershell
cargo test --test rule_geologic_resolution
cargo test --test rule_geologic_stage
cargo test --test rule_stage_graph
cargo test --test rule_tectonic_resolution
cargo fmt --all -- --check
```

Expected: all rule and projection tests pass.

- [ ] **Step 5: Commit the resolver boundary**

```powershell
git add src/rules/resolution.rs src/rules/geology.rs src/rules/tectonics.rs src/rules/mod.rs src/generators/natural/geologic_rule_input.rs src/generators/natural/mod.rs tests/rule_geologic_resolution.rs tests/rule_geologic_stage.rs
git commit -m "feat: resolve geologic rule input"
```

---

### Task 4: Define Validated Mantle and Hotspot Contracts

**Files:**

- Create: `src/world/natural/mantle.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/mantle_contracts.rs`

**Interfaces:**

- Produces `MANTLE_SNAPSHOT_SCHEMA_V1`.
- Produces heat-flow bounds `20.0..=400.0`.
- Produces `Hotspot`.
- Produces `MantleSnapshot::new(...) -> Result<Self, MantleValidationError>`.
- Produces `validate()` and `validate_against(&SpatialSnapshot)`.

- [ ] **Step 1: Write failing contract tests**

Build a four-cell spatial fixture and assert:

```rust
let snapshot = MantleSnapshot::new(
    MANTLE_SNAPSHOT_SCHEMA_V1,
    4,
    vec![
        Hotspot::new(
            HotspotId::from_raw(0),
            CellId::from_raw(1),
            800,
            Meters::new(250_000.0).unwrap(),
        )
        .unwrap(),
    ],
    vec![50.0, 190.0, 90.0, 55.0],
    vec![0.0, 1.0, 0.3, 0.0],
)
.unwrap();
snapshot.validate_against(&spatial).unwrap();
```

Add rejection tests for:

- unsupported schema;
- non-contiguous hotspot IDs;
- duplicate source cells;
- source cell outside `cell_count`;
- zero or over-1000 strength;
- zero, non-finite, or over-diagonal support radius;
- heat flow outside `20..=400` or non-finite;
- volcanic influence outside `0..=1` or non-finite;
- dense length mismatch;
- spatial cell count mismatch;
- malformed private JSON.

- [ ] **Step 2: Run the tests and verify the contract is absent**

```powershell
cargo test --test mantle_contracts
```

Expected: compilation fails on missing mantle types.

- [ ] **Step 3: Implement the immutable snapshot**

Use private fields for `MantleSnapshot`, public read-only accessors, and a checked `Hotspot::new`. Sort hotspots by ID in the constructor, then reject gaps and duplicate source cells. Do not silently deduplicate.

The snapshot getters are:

```rust
pub const fn schema_version(&self) -> u16;
pub const fn cell_count(&self) -> u32;
pub fn hotspots(&self) -> &[Hotspot];
pub fn heat_flow_mw_m2(&self) -> &[f32];
pub fn volcanic_influence(&self) -> &[f32];
```

Manual deserialization must invoke the constructor or `validate`.

- [ ] **Step 4: Run contract, serde, and lint checks**

```powershell
cargo test --test mantle_contracts
cargo test --test world_primitives
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: all pass.

- [ ] **Step 5: Commit the mantle contract**

```powershell
git add src/world/natural/mantle.rs src/world/natural/mod.rs tests/mantle_contracts.rs
git commit -m "feat: define mantle snapshot contracts"
```

---

### Task 5: Generate Deterministic Mantle Forcing

**Files:**

- Modify: `src/generators/natural/random.rs`
- Modify: `src/generators/natural/topology.rs`
- Create: `src/generators/natural/mantle.rs`
- Create: `src/generators/natural/geologic_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/mantle_generation.rs`
- Create: `tests/mantle_stage.rs`

**Interfaces:**

- Produces `MantleGenerator::generate(&SpatialSnapshot, &GeologicSpec, &mut StageRng)`.
- Produces `MantleGenerationError`.
- Produces `MantleArtifact`, key `world.mantle`.
- Produces `MantleStage`, ID `natural.mantle`, version `1`.
- Adds topology conversion from meters to the same quantized graph distance used by edge traversal.

- [ ] **Step 1: Write generation and isolation tests**

For several fixed seeds, assert:

- hotspot count equals the specification exactly;
- hotspot source cells are unique;
- repeated generation is byte-identical;
- different seeds change at least one source or strength;
- zero hotspots produces background heat flow and all-zero volcanic influence;
- every hotspot source has volcanic influence `1.0`;
- heat and influence fields stay within domain bounds;
- `Quiet`, `Moderate`, and `Active` produce strictly increasing background heat flow;
- a spec with more hotspots does not change the first hotspot's strength stream.

Add a structural independence test:

```rust
let mantle = MantleGenerator::generate(&spatial, &spec, &mut rng).unwrap();
assert_eq!(mantle.cell_count() as usize, spatial.cell_count());
assert_eq!(mantle.hotspots().len(), spec.hotspot_count as usize);
```

The test module must not construct a `TectonicSnapshot`; this proves the generator does not need it.

In `tests/mantle_stage.rs`, assert exact dependencies:

```rust
&[ResolvedGeologicInputArtifact::KEY, SpatialArtifact::KEY]
```

and exact output key `world.mantle`.

- [ ] **Step 2: Run tests and verify the generator/stage are missing**

```powershell
cargo test --test mantle_generation --test mantle_stage
```

Expected: compilation fails on missing generator and artifact.

- [ ] **Step 3: Add independent labeled streams and topology scaling**

Add constants:

```rust
pub(super) const HOTSPOT_SEEDS_LABEL: &str = "hotspot-seeds-v1";
pub(super) const HOTSPOT_STRENGTH_LABEL: &str = "hotspot-strength-v1";
```

Extend the RNG tests so consuming one hotspot stream cannot perturb the other.

Store the spatial maximum dimension in `NaturalTopologyIndex` and add:

```rust
pub(super) fn quantized_distance_for_meters(&self, distance_m: f64) -> u64;
```

It must use the existing `LENGTH_QUANTIZATION`, return at least `1` for positive distances, and have a focused unit test comparing one known spatial edge length with its traversal cost.

- [ ] **Step 4: Implement hotspot, heat-flow, and stage generation**

Algorithm constants:

```rust
const BACKGROUND_HEAT_FLOW: [f32; 3] = [45.0, 65.0, 85.0];
const HOTSPOT_ANOMALY_MAX: [f32; 3] = [160.0, 220.0, 280.0];
const HOTSPOT_RADIUS_SHORT_SIDE: [f64; 3] = [0.04, 0.055, 0.07];
```

Map the enum to index `0`, `1`, or `2`. Use `farthest_point_seeds` with the seed-stream first `u64` as tie rotation. Generate strength as `650 + (next_u32 % 351)`, so it is `650..=1000`.

For each hotspot:

1. derive support radius from short-side fraction and strength multiplier `0.8..=1.2`;
2. convert the radius to quantized graph distance;
3. run bounded multi-source or per-source distance;
4. apply compact smoothstep support;
5. combine overlaps as `1 - product(1 - influence)`;
6. set heat to `background + anomaly_max * combined_influence`, clamped to `400`.

Construct and validate `MantleSnapshot`.

`MantleStage` dispatches only:

```rust
GeologicModel::CurrentSliceV1 => MantleGenerator::generate(...)
```

Run:

```powershell
cargo test --test mantle_generation
cargo test --test mantle_stage
cargo test generators::natural::random
cargo test generators::natural::topology
```

Expected: all pass.

- [ ] **Step 5: Commit deterministic mantle generation**

```powershell
git add src/generators/natural/random.rs src/generators/natural/topology.rs src/generators/natural/mantle.rs src/generators/natural/geologic_stage.rs src/generators/natural/mod.rs tests/mantle_generation.rs tests/mantle_stage.rs
git commit -m "feat: generate mantle forcing"
```

---

### Task 6: Integrate Volcanic Relief and Schedule Mantle Upstream

**Files:**

- Modify: `src/world/natural/relief.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/generators/natural/relief.rs`
- Modify: `src/generators/natural/stage.rs`
- Modify: `src/app.rs`
- Modify: `tests/relief_contracts.rs`
- Modify: `tests/relief_generation.rs`
- Modify: `tests/natural_stage_graph.rs`
- Modify: `tests/natural_spec.rs`
- Modify: `tests/natural_display_golden.rs`
- Modify: `tests/natural_field_views.rs`
- Modify: `tests/natural_performance.rs`

**Interfaces:**

- Produces `RELIEF_SCHEMA_V2`.
- Adds `volcanic_offset_m()` to `ReliefSnapshot`.
- Changes `ReliefGenerator::generate` to consume `&MantleSnapshot`.
- Changes `ReliefStageInputs` to include `MantleArtifact`.
- Adds `GeologicSpecArtifact` and the geologic rule/mantle stages to
  `natural_foundation_graph`.
- The app supplies `GeologicSpecArtifact::new(GeologicSpec::default())`.

- [ ] **Step 1: Change relief tests to require the fourth component**

Update fixtures to construct Relief V2 with:

```rust
let volcanic = ElevationField::new(vec![0.0; cell_count]).unwrap();
```

Assert:

```rust
for index in 0..cell_count {
    let sum = snapshot.crust_base_elevation_m().values()[index]
        + snapshot.tectonic_offset_m().values()[index]
        + snapshot.volcanic_offset_m().values()[index]
        + snapshot.regional_offset_m().values()[index];
    assert!((sum - snapshot.elevation_m().values()[index]).abs() <= COMPONENT_IDENTITY_TOLERANCE_M);
}
```

Add rejection tests for negative, non-finite, and over-4000 volcanic offsets and for schema V1 input.

In generation tests, compare a zero-hotspot mantle with a one-hotspot mantle using otherwise identical inputs. Assert:

- zero-hotspot volcanic offsets are all zero;
- the hotspot source offset is positive;
- the source and nearby cells have greater elevation in the hotspot case;
- final elevation remains valid.

- [ ] **Step 2: Run relief tests and verify signature/schema failures**

```powershell
cargo test --test relief_contracts --test relief_generation
```

Expected: compilation fails until Relief V2 and the mantle input are implemented.

- [ ] **Step 3: Upgrade the relief contract and generator**

Add:

```rust
pub const RELIEF_SCHEMA_V1: u16 = 1;
pub const RELIEF_SCHEMA_V2: u16 = 2;
pub const VOLCANIC_OFFSET_MIN_M: f32 = 0.0;
pub const VOLCANIC_OFFSET_MAX_M: f32 = 4_000.0;
```

Validate only V2 as supported. Extend constructor, getters, serde, length/range checks, and component identity.

Add:

```rust
fn synthesize_volcanic_offset(
    tectonic: &TectonicSnapshot,
    mantle: &MantleSnapshot,
) -> Vec<f32>
```

Use per-cell amplitude:

```text
oceanic crust: 3_200 m × volcanic influence
continental crust: 2_200 m × volcanic influence
```

Apply a smooth bounded response so low influence does not create a flat pedestal. Clamp to `0..=4_000`.

Extend final-safety reconciliation so positive overflow reduces volcanic contribution first, then follows the existing deterministic component adjustment order. Negative overflow cannot reduce volcanic below zero. Preserve the identity exactly within the existing tolerance.

- [ ] **Step 4: Wire mantle into the graph and app**

Set `ReliefStage::version()` to `2`. Its dependencies become:

```rust
&[MantleArtifact::KEY, SpatialArtifact::KEY, TectonicArtifact::KEY]
```

Add to the graph:

```rust
.external::<GeologicSpecArtifact>()
.stage(RuleGeologicResolutionStage)
.stage(ResolvedGeologicInputStage)
.stage(MantleStage)
```

Place `MantleStage` before `ReliefStage` by artifact dependency, not manual ordering.

Update every external-artifact fixture to insert:

```rust
GeologicSpecArtifact::new(GeologicSpec::default())
```

Update app construction to own one default `GeologicSpec` and supply the artifact, without adding UI controls.

Run:

```powershell
cargo test --test relief_contracts
cargo test --test relief_generation
cargo test --test natural_stage_graph
cargo test --test natural_spec
cargo test --lib app::natural_app_tests
cargo test --test natural_display_golden --no-run
cargo test --test natural_field_views --no-run
cargo test --test natural_performance --no-run
```

Expected: all tests and compile-only targets pass. The reviewed natural golden is allowed to fail only in Task 12, where it is intentionally regenerated and visually reviewed.

- [ ] **Step 5: Commit volcanic relief integration**

```powershell
git add src/world/natural/relief.rs src/world/natural/mod.rs src/generators/natural/relief.rs src/generators/natural/stage.rs src/app.rs tests/relief_contracts.rs tests/relief_generation.rs tests/natural_stage_graph.rs tests/natural_spec.rs tests/natural_display_golden.rs tests/natural_field_views.rs tests/natural_performance.rs
git commit -m "feat: integrate volcanic relief"
```

---

### Task 7: Define Bedrock and Geologic Snapshot Contracts

**Files:**

- Create: `src/world/natural/geology.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/geologic_contracts.rs`

**Interfaces:**

- Produces `GEOLOGIC_SNAPSHOT_SCHEMA_V1`.
- Produces `BedrockKind`, `BedrockKindField`, `GeologicSnapshot`, and
  `GeologicValidationError`.
- `GeologicSnapshot::validate_against` consumes spatial, tectonic, mantle, and relief snapshots.

- [ ] **Step 1: Write failing category, dense-field, and snapshot tests**

Assert exact category codes:

```rust
assert_eq!(BedrockKind::OceanicMafic.raw(), 0);
assert_eq!(BedrockKind::ContinentalCrystalline.raw(), 1);
assert_eq!(BedrockKind::Sedimentary.raw(), 2);
assert_eq!(BedrockKind::Metamorphic.raw(), 3);
assert_eq!(BedrockKind::Volcanic.raw(), 4);
```

Construct a valid four-cell snapshot:

```rust
let snapshot = GeologicSnapshot::new(
    GEOLOGIC_SNAPSHOT_SCHEMA_V1,
    4,
    BedrockKindField::new(vec![0, 1, 2, 4]).unwrap(),
    vec![0.2, 0.4, 0.1, 0.8],
    vec![0.8, 0.7, 0.4, 0.5],
    vec![0.1, 0.2, 0.7, 0.6],
    vec![0.2, 0.3, 0.1, 0.9],
    vec![0.1, 0.2, 0.3, 0.8],
    vec![0.0, 0.1, 0.9, 0.2],
)
.unwrap();
snapshot.validate().unwrap();
```

Add tests for:

- invalid category code;
- every field length mismatch;
- NaN and each inclusive range boundary;
- unsupported schema;
- spatial/tectonic/mantle/relief cell-count mismatch;
- oceanic mafic assigned to continental crust;
- continental crystalline or metamorphic assigned to oceanic crust;
- sedimentary and volcanic categories allowed on either crust;
- malformed private JSON.

- [ ] **Step 2: Run tests and verify missing geology types**

```powershell
cargo test --test geologic_contracts
```

Expected: compilation fails.

- [ ] **Step 3: Implement validated dense contracts**

`BedrockKindField` stores raw `Vec<u32>`, exposes `len`, `is_empty`,
`raw_values`, and checked `get`. Manual deserialization rejects unknown codes.

`GeologicSnapshot` has read-only getters matching all constructor fields. Use one shared helper for finite `0..=1` validation, but preserve the field name and cell ID in errors.

`validate_against` must:

1. re-run local validation;
2. compare all four upstream cell counts;
3. enforce the two crust/category restrictions;
4. avoid re-deriving generator heuristics or potential formulas.

- [ ] **Step 4: Run contract and full world checks**

```powershell
cargo test --test geologic_contracts
cargo test --test tectonic_contracts
cargo test --test relief_contracts
cargo fmt --all -- --check
```

Expected: all pass.

- [ ] **Step 5: Commit the geologic contracts**

```powershell
git add src/world/natural/geology.rs src/world/natural/mod.rs tests/geologic_contracts.rs
git commit -m "feat: define geologic substrate contracts"
```

---

### Task 8: Generate Bedrock, Physical Properties, and Potentials

**Files:**

- Modify: `src/generators/natural/random.rs`
- Create: `src/generators/natural/geology.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/geologic_generation.rs`

**Interfaces:**

- Produces `GeologicGenerator::generate(...)`.
- Produces `GeologicGenerationError`.
- Consumes only stable domain snapshots plus its stage RNG.

- [ ] **Step 1: Write failing deterministic and causal tests**

For fixed spatial/tectonic/mantle/relief fixtures, assert:

- same seed and inputs yield identical JSON;
- different geology RNG changes only province tie-breaking, not upstream artifacts;
- all cells are classified;
- oceanic mafic and continental crystalline appear in a neutral fixture;
- a strong collision fixture creates metamorphic cells;
- a strong hotspot fixture creates volcanic cells;
- a broad negative tectonic/relative-low fixture creates sedimentary cells;
- strong boundary fracture lowers resistance and raises permeability relative to stable crystalline interior;
- geothermal potential is monotonic in heat flow when fracture is held constant;
- metallic potential near a strong magmatic boundary exceeds stable interior;
- sedimentary basin potential is not identical to land/ocean or bedrock category;
- output validates against all inputs.

Add a source scan test or CI scan asserting the generator file contains none of:

```text
crate::terrain
crate::app
crate::view
egui
wgpu
```

- [ ] **Step 2: Run the focused test and verify missing generator**

```powershell
cargo test --test geologic_generation
```

Expected: compilation fails.

- [ ] **Step 3: Implement stable influence fields and classification**

Add `BEDROCK_PROVINCE_LABEL = "bedrock-province-v1"` and its isolation test.

In the generator:

1. validate every upstream snapshot;
2. build `NaturalTopologyIndex`;
3. collect source cells for collision, subduction, rift, ridge, transform, and any strong boundary;
4. calculate bounded graph-distance influence fields with compact kernels;
5. read mantle volcanic influence and Relief V2 components;
6. generate a small, smoothed, zero-centered province field from the independent RNG label;
7. calculate basin tendency from negative tectonic offset, local relative-low relief, and distance from strong active boundaries;
8. quantize every classification score to fixed millionths;
9. apply the design priority exactly:
   volcanic, metamorphic, sedimentary, oceanic mafic, continental crystalline.

For subduction, use the existing `subducting_plate` and edge owners so volcanic-arc influence is assigned to the overriding side. Do not infer plate side from final elevation.

- [ ] **Step 4: Implement properties and potential formulas**

Use category base pairs:

| Bedrock | Resistance | Permeability |
|---|---:|---:|
| OceanicMafic | 0.78 | 0.18 |
| ContinentalCrystalline | 0.86 | 0.12 |
| Sedimentary | 0.42 | 0.58 |
| Metamorphic | 0.82 | 0.10 |
| Volcanic | 0.68 | 0.24 |

Then:

```text
resistance = clamp(base_resistance - 0.30 × fracture, 0, 1)
permeability = clamp(base_permeability + 0.55 × fracture × (1 - base_permeability), 0, 1)
geothermal = clamp(normalized_heat × (0.45 + 0.55 × fracture), 0, 1)
```

Metallic potential is the bounded maximum/combination of magmatic, orogenic, and fracture influences. Sedimentary potential combines sedimentary classification, basin tendency, and low active-boundary disturbance. Keep separate arrays; do not choose a winning potential.

Construct `GeologicSnapshot`, call `validate_against`, and map errors without panics.

Run:

```powershell
cargo test --test geologic_generation
cargo test generators::natural::random
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: all pass.

- [ ] **Step 5: Commit geologic generation**

```powershell
git add src/generators/natural/random.rs src/generators/natural/geology.rs src/generators/natural/mod.rs tests/geologic_generation.rs
git commit -m "feat: generate geologic substrate"
```

---

### Task 9: Publish Geologic Artifacts and Verify Graph Cache Boundaries

**Files:**

- Modify: `src/generators/natural/geologic_stage.rs`
- Modify: `src/generators/natural/stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/geologic_stage.rs`
- Modify: `tests/natural_stage_graph.rs`
- Modify: `tests/natural_performance.rs`

**Interfaces:**

- Produces `GeologicArtifact`, key `world.geology`.
- Produces `GeologicStage`, ID `natural.geology`, version `1`.
- Extends the production graph to publish nine stages and five external input types.

- [ ] **Step 1: Write failing stage identity and cache tests**

Assert exact `GeologicStageInputs::dependencies()`:

```rust
&[
    ResolvedGeologicInputArtifact::KEY,
    MantleArtifact::KEY,
    ReliefArtifact::KEY,
    SpatialArtifact::KEY,
    TectonicArtifact::KEY,
]
```

The production graph must expose:

```text
external:
  natural.planar-space-spec
  natural.tectonic-spec
  natural.geologic-spec
  rules.pack-set
  rules.author-constraints

stages:
  spatial.planar-voronoi
  natural.resolve-tectonic-rules
  natural.project-tectonic-input
  natural.resolve-geologic-rules
  natural.project-geologic-input
  natural.tectonics
  natural.mantle
  natural.relief
  natural.geology
```

Use descriptor order returned by the graph, not the textual order above, for exact assertions.

Add cache properties:

- identical second build: nine hits, zero misses;
- tectonic spec change: mantle is the only random natural artifact whose hash remains unchanged and cache entry hits;
- geologic spec change: spatial and tectonic artifacts hit; mantle, relief, and geology miss;
- audit-only geologic pack identity change with identical projection: projection hash and all physical artifacts stay equal;
- root-seed change: rule audits/projections equal while spatial, tectonic, mantle, relief, and geology differ;
- failed geology stage publishes no outcome and valid prior entries recover.

- [ ] **Step 2: Run graph tests and verify the missing artifact/stage**

```powershell
cargo test --test geologic_stage --test natural_stage_graph
```

Expected: compilation or exact-stage assertions fail.

- [ ] **Step 3: Implement and register `GeologicStage`**

Dispatch:

```rust
match inputs.resolved_input.input().model() {
    GeologicModel::CurrentSliceV1 => GeologicGenerator::generate(
        inputs.spatial.snapshot(),
        inputs.tectonic.snapshot(),
        inputs.mantle.snapshot(),
        inputs.relief.snapshot(),
        rng,
    ),
}
```

Before generation, validate all upstream cross-artifact contracts. After generation, call `validate_against`. Map errors to:

```text
natural.invalid-geologic-input
natural.geologic-build-failed
natural.invalid-geologic-snapshot
```

Register `GeologicStage` after Relief by dependencies.

- [ ] **Step 4: Run graph, cache, and performance compile tests**

```powershell
cargo test --test geologic_stage
cargo test --test natural_stage_graph
cargo test --test natural_performance --no-run
cargo test --test rule_geologic_stage
```

Expected: exact graph and cache behavior passes.

- [ ] **Step 5: Commit formal stage publication**

```powershell
git add src/generators/natural/geologic_stage.rs src/generators/natural/stage.rs src/generators/natural/mod.rs tests/geologic_stage.rs tests/natural_stage_graph.rs tests/natural_performance.rs
git commit -m "feat: publish geologic stage artifacts"
```

---

### Task 10: Register and Borrow New Natural Fields

**Files:**

- Modify: `src/world/natural/fields.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/app/natural_display.rs`
- Modify: `tests/field_contracts.rs`
- Modify: `tests/natural_field_views.rs`

**Interfaces:**

- Produces the ten field-ID factory functions named in the design.
- Extends `natural_field_registry`.
- `NaturalFieldDocument` owns `Arc<MantleArtifact>` and `Arc<GeologicArtifact>`.
- All new dense arrays are borrowed by `FieldPayloadRef`.

- [ ] **Step 1: Write exact schema and zero-copy tests**

Assert IDs, domains, types, units, ranges, palette hints, category labels, and dependencies for:

```text
mantle_heat_flow_mw_m2
volcanic_influence
volcanic_offset_m
bedrock_kind
fracture_intensity
erosion_resistance
relative_permeability
metallic_mineral_potential
geothermal_potential
sedimentary_basin_potential
```

Assert `elevation_m@1` now depends on `volcanic_offset_m@1`.

For every scalar payload, compare source and view pointers:

```rust
assert_eq!(
    catalog
        .get(&geothermal_potential_field_id())
        .unwrap()
        .view()
        .unwrap()
        .scalar_values()
        .unwrap()
        .as_ptr(),
    document
        .geologic
        .snapshot()
        .geothermal_potential()
        .as_ptr()
);
```

Add the equivalent category pointer assertion for bedrock and scalar pointer assertions for mantle heat and volcanic relief.

- [ ] **Step 2: Run field suites and verify missing registrations**

```powershell
cargo test --test field_contracts --test natural_field_views
```

Expected: compilation fails on missing IDs and document artifacts.

- [ ] **Step 3: Register schemas in dependency-safe order**

Use exact ranges:

- heat flow `20..=400`, custom `milliwatt-per-square-meter`, `mW/m²`;
- volcanic influence and all properties/potentials `0..=1`, unitless;
- volcanic offset `0..=4000 m`;
- bedrock categories `0..=4` with five stable localization keys.

Update `elevation` dependencies:

```rust
vec![crust_base, tectonic_offset, volcanic_offset, regional_offset]
```

Keep registry construction deterministic and cycle-free.

- [ ] **Step 4: Extend the immutable display document**

Change `NaturalFieldDocument::build` to accept and validate:

```rust
Arc<SpatialArtifact>
Arc<TectonicArtifact>
Arc<MantleArtifact>
Arc<ReliefArtifact>
Arc<GeologicArtifact>
&BuildReport
```

Add each authoritative payload directly. Do not add derived copies to
`NaturalFieldDisplayCache`; it remains responsible only for plate velocity and edge-aligned display arrays.

Run:

```powershell
cargo test --test field_contracts
cargo test --test natural_field_views
cargo test --lib app::natural_display::tests
cargo test --test field_display_integration
```

Expected: all pass and pointer identity proves zero-copy fields.

- [ ] **Step 5: Commit field exposure**

```powershell
git add src/world/natural/fields.rs src/world/natural/mod.rs src/app/natural_display.rs tests/field_contracts.rs tests/natural_field_views.rs
git commit -m "feat: expose geologic natural fields"
```

---

### Task 11: Compose Complete Geologic Candidates Atomically in the App

**Files:**

- Modify: `src/app.rs`
- Modify: `src/app/natural_display.rs`
- Modify: `tests/natural_stage_graph.rs`

**Interfaces:**

- The app extracts all five physical artifacts:
  `SpatialArtifact`, `TectonicArtifact`, `MantleArtifact`, `ReliefArtifact`,
  `GeologicArtifact`.
- Candidate validation and display packet preparation remain atomic.
- The existing rule summary remains derived from the tectonic full audit; geology adds no author adoptions in V1.

- [ ] **Step 1: Strengthen app atomicity tests**

Add tests that:

- a successful candidate stores all five artifact Arcs;
- missing/corrupt mantle or geology prevents publication;
- a geology-stage failure preserves the prior document Arc, GPU packet revision, field selection, and rule summary;
- default source constructs `GeologicSpec::default()` only at the app composition boundary;
- app source does not call `MantleGenerator`, `GeologicGenerator`, or construct `ResolvedGeologicInputArtifact` directly.

Use the existing source-scan pattern from `natural_app_tests`.

- [ ] **Step 2: Run app tests and verify candidate extraction failure**

```powershell
cargo test --lib app::natural_app_tests
```

Expected: tests fail because app publication does not yet extract mantle/geology.

- [ ] **Step 3: Implement all-or-nothing extraction**

Extract all artifacts before building the display document:

```rust
let spatial = outcome.artifacts.get::<SpatialArtifact>()?;
let tectonic = outcome.artifacts.get::<TectonicArtifact>()?;
let mantle = outcome.artifacts.get::<MantleArtifact>()?;
let relief = outcome.artifacts.get::<ReliefArtifact>()?;
let geologic = outcome.artifacts.get::<GeologicArtifact>()?;
let document = NaturalFieldDocument::build(
    spatial,
    tectonic,
    mantle,
    relief,
    geologic,
    &outcome.report,
)?;
```

Only after document validation and GPU packet preparation succeed may the app replace current state. Preserve the existing failure path exactly.

- [ ] **Step 4: Run app and integration suites**

```powershell
cargo test --lib app::natural_app_tests
cargo test --lib app::natural_display::tests
cargo test --test natural_stage_graph
cargo test --test natural_field_views
cargo test --test field_display_integration
```

Expected: all pass.

- [ ] **Step 5: Commit app composition**

```powershell
git add src/app.rs src/app/natural_display.rs tests/natural_stage_graph.rs
git commit -m "feat: compose geologic artifacts in app"
```

---

### Task 12: Review Visual Quality, Goldens, and Performance

**Files:**

- Modify: `tests/natural_display_golden.rs`
- Modify: `tests/natural_performance.rs`
- Modify: `.github/workflows/rust.yml`
- Modify: `tests/golden/natural-foundation/elevation.png`
- Create: `tests/golden/natural-foundation/heat-flow.png`
- Create: `tests/golden/natural-foundation/volcanic-influence.png`
- Create: `tests/golden/natural-foundation/bedrock.png`
- Create: `tests/golden/natural-foundation/metallic-potential.png`
- Create: `tests/golden/natural-foundation/geothermal-potential.png`
- Create: `tests/golden/natural-foundation/sedimentary-basin-potential.png`

**Interfaces:**

- Golden generator emits high elevation, heat flow, volcanic influence, bedrock, metallic potential, geothermal potential, and sedimentary potential references for fixed seeds.
- Performance test records all nine formal stages and validates their outputs.

- [ ] **Step 1: Add quality assertions before recording images**

For the reviewed fixed seed set, assert:

- at least one hotspot has positive relief support;
- heat-flow maximum exceeds background by a meaningful margin;
- bedrock includes oceanic mafic, continental crystalline, and at least one of volcanic/metamorphic/sedimentary;
- each potential field has non-zero spread;
- no two potential arrays are byte-identical;
- high heat plus high fracture ranks in the upper geothermal quartile;
- geology does not alter plate/crust independence.

Run:

```powershell
cargo test --test natural_display_golden
```

Expected: quality assertions pass; reviewed PNG comparison fails because approved baselines still represent Relief V1.

- [ ] **Step 2: Extend the screenshot generator and regenerate candidates**

Use the existing ignored regeneration test or screenshot binary. Generate the exact fixed dimensions and palettes declared by field schemas. Do not edit PNGs manually.

Run the repository's regeneration command discovered in the existing test:

```powershell
cargo test --test natural_display_golden regenerate_natural_goldens -- --ignored --exact
```

Expected: reviewed natural PNG fixtures are rewritten from formal field views.

- [ ] **Step 3: Inspect every generated image**

Open the generated PNGs and verify:

- high elevation still shows irregular continents and coherent tectonic ranges;
- hotspot relief is local and compact, not a stripe or global pedestal;
- no hotspot track or age sequence is implied;
- bedrock provinces follow crust and active structures rather than raw cell noise;
- the three potential fields are distinct and can overlap;
- there are no blank holes, invalid colors, clipping seams, or geometry shifts.

If any image fails, adjust only the owning generator formula, rerun its focused tests, regenerate, and inspect again.

- [ ] **Step 4: Record performance and CI gates**

Update `natural_performance` to require `MantleArtifact` and `GeologicArtifact`, validate all artifacts, and print per-stage timing. Keep timing informational; enforce only complexity/safety and successful completion.

Add focused CI:

```yaml
- run: cargo test --test mantle_generation --test geologic_generation --test natural_stage_graph
```

Run:

```powershell
cargo test --test natural_display_golden
cargo test --test natural_performance --release -- --ignored
cargo test --test natural_field_views
cargo fmt --all -- --check
```

Expected: goldens and quality gates pass; performance output is recorded.

- [ ] **Step 5: Commit reviewed quality baselines**

```powershell
git status --short
git add .github/workflows/rust.yml tests/natural_display_golden.rs tests/natural_performance.rs
git add tests/golden/natural-foundation/elevation.png tests/golden/natural-foundation/heat-flow.png tests/golden/natural-foundation/volcanic-influence.png tests/golden/natural-foundation/bedrock.png tests/golden/natural-foundation/metallic-potential.png tests/golden/natural-foundation/geothermal-potential.png tests/golden/natural-foundation/sedimentary-basin-potential.png
git commit -m "test: verify geologic visual quality"
```

---

### Task 13: Run Release Gates, Inspect the Real App, Merge, and Publish

**Files:**

- Verify all changed files
- No new source file is expected unless a gate exposes a defect

**Interfaces:**

- Produces a clean feature branch whose tip is merged into `main`.
- Leaves the merged main release application running for user inspection.

- [ ] **Step 1: Run focused boundary scans**

Run:

```powershell
$scans = @(
  @{ Name = 'world_to_business'; Pattern = 'crate::(engine|rules|generators|app|view|ui|gpu)|egui|eframe|wgpu'; Paths = @('src/world/natural') },
  @{ Name = 'rules_to_engine'; Pattern = 'crate::engine|crate::generators|crate::app|crate::view|crate::ui|crate::gpu'; Paths = @('src/rules') },
  @{ Name = 'natural_to_legacy_or_ui'; Pattern = 'crate::terrain|crate::app|crate::view|crate::ui|crate::gpu|egui|eframe|wgpu'; Paths = @('src/generators/natural') },
  @{ Name = 'app_bypass'; Pattern = 'MantleGenerator|GeologicGenerator|ResolvedGeologicInputArtifact\\s*\\{'; Paths = @('src/app.rs','src/app') },
  @{ Name = 'history_leak'; Pattern = 'year|age_|timeline|chronology|eruption_event|hotspot_track'; Paths = @('src/world/natural/mantle.rs','src/world/natural/geology.rs','src/generators/natural/mantle.rs','src/generators/natural/geology.rs') }
)
foreach ($scan in $scans) {
  $hits = & rg -n --glob '*.rs' $scan.Pattern $scan.Paths 2>$null
  if ($LASTEXITCODE -eq 0) {
    Write-Output ("BOUNDARY_SCAN_FAILED " + $scan.Name)
    Write-Output $hits
    exit 1
  }
  if ($LASTEXITCODE -gt 1) { exit $LASTEXITCODE }
  Write-Output ("BOUNDARY_SCAN_OK " + $scan.Name)
}
```

Expected: every scan prints `BOUNDARY_SCAN_OK`. If legitimate unit names contain `year`, narrow only that scan to the new geology files and keep any false-positive exclusion explicit.

- [ ] **Step 2: Run complete native and cross-target gates**

Run:

```powershell
cargo test --workspace --all-targets
cargo test --workspace --all-targets --release
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets --all-features
```

Then:

```powershell
$previousRustflags = $env:RUSTFLAGS
$previousRustdocflags = $env:RUSTDOCFLAGS
try {
  $env:RUSTFLAGS = '--cfg getrandom_backend="wasm_js"'
  $env:RUSTDOCFLAGS = '--cfg getrandom_backend="wasm_js"'
  cargo check --workspace --all-features --lib --target wasm32-unknown-unknown
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  trunk build
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
  $env:RUSTFLAGS = $previousRustflags
  $env:RUSTDOCFLAGS = $previousRustdocflags
}
```

Expected: every gate passes with only explicitly documented ignored regeneration/extreme/performance tests.

- [ ] **Step 3: Build release and perform desktop visual smoke**

Build and launch the feature release. With the computer-use workflow:

1. verify the header still says pre-industrial medieval fantasy/current slice;
2. verify the rule pack summary is present;
3. switch to heat flow, volcanic influence, bedrock, and all three potential fields;
4. confirm each selected field is displayed without rebuilding the world;
5. switch back to elevation and inspect hotspot topography;
6. click new seed/rebuild and confirm seed plus natural statistics change;
7. confirm the rule summary remains valid and no error replaces the prior map.

Stop only the exact verified feature executable before merging.

- [ ] **Step 4: Merge, reverify main, push, and clean owned worktree**

From the main checkout:

```powershell
git fetch origin main
git status --short --branch
git merge --no-ff feature/geologic-substrate -m "merge: geologic natural substrate"
cargo test --test geologic_stage
cargo test --test natural_display_golden
cargo check --workspace --all-targets --all-features
git push origin main
git fetch origin main
if ((git rev-parse main) -ne (git rev-parse origin/main)) { exit 1 }
```

After verifying the feature worktree is clean and its resolved path exactly matches
`.worktrees/geologic-substrate`, remove only that worktree, prune, and delete only
`feature/geologic-substrate`. Do not touch `.worktrees/field-display-system`.

- [ ] **Step 5: Launch the merged main release and update the roadmap**

Build `main` release, launch it visibly, and inspect the default elevation and rule summary once more. Leave the main app running.

Update the working plan:

- mark geologic substrate complete;
- set preliminary climate design as the next in-progress slice;
- stop only if climate requires an unapproved celestial/season model that changes the product direction.

Report the merge commit, remote identity, full gate results, visual result, running PID/path, and the next slice selected.
