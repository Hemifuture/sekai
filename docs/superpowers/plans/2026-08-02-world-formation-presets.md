# World Formation Presets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add deterministic Azgaar-inspired world-formation presets that create distinct macro landmass layouts, remain compile-time orthogonal to plate generation, and guarantee ocean around the finite closed map.

**Architecture:** A versioned `WorldFormationSpec` resolves through its own engine stage into one concrete preset. Crust and mantle consume narrow projections of that resolved artifact, while plate generation never receives it. Closed-boundary ocean framing is enforced in crust eligibility and formal relief components, never by rendering.

**Tech Stack:** Rust 1.85, serde, deterministic `StageRng`, typed stage artifacts, polygonal spatial topology, egui/eframe, PNG golden tests.

## Global Constraints

- Generate only a present-day slice; add no history, events, dates, or timeline.
- Preserve pre-industrial medieval fantasy framing.
- Presets must not alter plate seeds, ownership, velocities, or plate random substreams.
- Plates and crust meet only in explicit boundary classification.
- Relief remains the sole constructional-elevation and formal land/ocean writer.
- Rendering remains read-only and must not mask or repair world data.
- `BoundaryCondition::Closed` must yield ocean on all outer cells and a visible east/west ocean band.
- Existing `continental_crust_fraction` remains the explicit author/rule target; presets own spatial shape.
- Public serialized inputs and resolved outputs revalidate on deserialization.
- Do not call legacy `src/terrain` from the production natural path.
- Use labeled deterministic random streams and integer/quantized tie-breaking.
- Run each named test red before production implementation, then commit only after focused green tests and `git diff --check`.

## File Responsibilities

- `src/world/natural/formation.rs`: serialized requested/resolved types and validation.
- `src/generators/natural/formation.rs`: deterministic Random resolution and narrow projections.
- `src/generators/natural/formation_stage.rs`: external/resolved artifacts and resolver stage.
- `src/generators/natural/tectonics.rs`: preset-shaped crust; plate functions stay preset-blind.
- `src/generators/natural/mantle.rs`: neutral or volcanic-island mantle bias.
- `src/generators/natural/relief.rs`: formal closed-boundary ocean envelope.
- `src/app.rs`: persisted selection, recommended visible defaults, build wiring and labels.
- `src/app/natural_display.rs`: atomically published resolved-preset provenance.

---

### Task 1: Define the world-formation domain contract

**Files:**
- Create: `src/world/natural/formation.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/world_formation_spec.rs`

**Interfaces:**
- Produces `WorldFormationPreset::{Random, Continents, Archipelago, Supercontinent, GreatIsland, VolcanicIslands}`.
- Produces the same resolved enum without `Random`, `MantleFormationBias`, `WorldFormationSpec`, and `ResolvedWorldFormation`.
- Consumes no engine, generator, app, UI, GPU, or legacy terrain type.

- [ ] **Step 1: Write failing tests for exact defaults, enum JSON, schema rejection, round trips, and recommendations.**

```rust
#[test]
fn defaults_to_named_multi_continents() {
    assert_eq!(WorldFormationSpec::default().preset, WorldFormationPreset::Continents);
}

#[test]
fn resolved_wire_cannot_contain_random() {
    let wire = serde_json::json!({"schema_version": 1, "requested": "Random", "resolved": "Random"});
    assert!(serde_json::from_value::<ResolvedWorldFormation>(wire).is_err());
}
```

- [ ] **Step 2: Run `cargo test --test world_formation_spec`; verify compile failure because the types do not exist.**
- [ ] **Step 3: Implement custom-deserializing V1 structs and these accessors.**

```rust
pub const fn requested(&self) -> WorldFormationPreset;
pub const fn resolved(&self) -> ResolvedWorldFormationPreset;
pub const fn mantle_bias(&self) -> MantleFormationBias;
pub const fn recommended_continental_crust_fraction(&self) -> f32;
```

- [ ] **Step 4: Run `cargo test --test world_formation_spec`, `cargo fmt --all -- --check`, and `git diff --check`.**
- [ ] **Step 5: Commit `feat: define world formation presets`.**

---

### Task 2: Resolve presets through a typed engine stage

**Files:**
- Create: `src/generators/natural/formation.rs`
- Create: `src/generators/natural/formation_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/stage.rs`
- Create: `tests/world_formation_stage.rs`
- Modify: `tests/natural_stage_graph.rs` and external-artifact fixtures.

**Interfaces:**
- `WorldFormationSpecArtifact`: `natural.world-formation-spec`.
- `ResolvedWorldFormationArtifact`: `natural.resolved-world-formation`.
- `WorldFormationStage`: `natural.resolve-world-formation`, version 1, namespace `sekai.core`.
- `WorldFormationGenerator::resolve(&WorldFormationSpec, &mut StageRng) -> Result<ResolvedWorldFormation, WorldFormationGenerationError>`.

- [ ] **Step 1: Write failing tests for exact keys/dependencies/identity, named pass-through, same-seed repeatability, concrete Random results, validation, and fixed bucket boundaries.**
- [ ] **Step 2: Run `cargo test --test world_formation_stage`; verify missing-artifact compile failure.**
- [ ] **Step 3: Implement the immutable weighted mapping.**

```text
0..=39   Continents
40..=64  Archipelago
65..=74  Supercontinent
75..=89  GreatIsland
90..=99  VolcanicIslands
```

- [ ] **Step 4: Register the eighth external and sixteenth stage; update exact graph/cache fixtures. Repeated builds must report 16 cache hits.**
- [ ] **Step 5: Run `cargo test --test world_formation_stage --test natural_stage_graph` and `git diff --check`.**
- [ ] **Step 6: Inspect the staged stat and commit `feat: resolve world formation presets`.**

---

### Task 3: Generate preset-shaped crust without perturbing plates

**Files:**
- Modify: `src/generators/natural/topology.rs`
- Modify: `src/generators/natural/tectonics.rs`
- Modify: `src/generators/natural/stage.rs`
- Modify: `tests/tectonic_generation.rs`, `tests/tectonic_boundaries.rs`, and direct tectonic fixtures.

**Interfaces:**
- `TectonicStageInputs` additionally consumes `ResolvedWorldFormationArtifact`.
- `TectonicGenerator::generate` receives `&ResolvedWorldFormation`.
- `generate_plates` retains its current signature and cannot receive formation data.
- `generate_crust` receives only resolved preset, target fraction, topology, and crust substreams.

- [ ] **Step 1: Write the failing orthogonality contract.**

```rust
#[test]
fn changing_formation_preset_cannot_perturb_plate_state() {
    let baseline = generate(91, ResolvedWorldFormationPreset::Continents);
    for preset in ALL_RESOLVED_PRESETS {
        let candidate = generate(91, preset);
        assert_eq!(candidate.plates(), baseline.plates());
        assert_eq!(candidate.cell_plates().raw_values(), baseline.cell_plates().raw_values());
    }
}
```

- [ ] **Step 2: Run that exact test and verify failure because formation input is absent.**
- [ ] **Step 3: Write red 20,000-cell morphology tests:** Continents has 3–6 major components and largest continental share ≤55%; Archipelago has ≥8 components and largest share ≤30%; Supercontinent has one major component holding ≥85%; GreatIsland has one 60–90% main component plus a satellite; every boundary cell is oceanic crust; every target fraction is within one maximum-cell area.
- [ ] **Step 4: Run `cargo test --release --test tectonic_generation preset_crust_profiles_have_distinct_macro_topology -- --exact --nocapture` and observe the old clustered algorithm fail.**
- [ ] **Step 5: Implement an internal profile and bounded seed spreading.**

```rust
struct CrustFormationProfile {
    nucleus_count: usize,
    hard_corridor: bool,
    satellite_scale_permille: &'static [u16],
    shape_noise_permille: i64,
}
```

Use O(cells × nuclei) farthest-point updates over quantized centers, one boundary-distance field, one ownership field, ownership-divider ocean corridors, adaptive hard edge exclusion, smoothed shape noise, deterministic owner scales, and stable `CellId` ties. Nucleus cells get the lowest stable score. Return `InsufficientCrustFormationArea` instead of violating explicit area targets. Increment `TectonicStage` version.

- [ ] **Step 6: Run `cargo test --release --test tectonic_generation --test tectonic_boundaries`, topology unit tests, and `git diff --check`.**
- [ ] **Step 7: Commit `feat: generate distinct crust formation profiles`.**

---

### Task 4: Compose volcanic-island mantle forcing independently

**Files:**
- Modify: `src/generators/natural/mantle.rs`
- Modify: `src/generators/natural/geologic_stage.rs`
- Modify: `tests/mantle_generation.rs`, `tests/mantle_stage.rs`, and direct mantle fixtures.

**Interfaces:**
- `MantleStageInputs` consumes resolved formation, geologic input, and spatial only.
- `MantleGenerator::generate` consumes `MantleFormationBias`, never plates or crust.
- Neutral is byte-identical to the prior same-seed/spec behavior.
- Volcanic islands use `max(base_hotspots, 9)` and effective `Active`, bounded by `MAX_HOTSPOT_COUNT`.

- [ ] **Step 1: Write a failing test asserting ≥9 hotspots and higher mean heat flow for volcanic bias, plus neutral byte stability.**
- [ ] **Step 2: Run the exact new test and verify missing bias input causes failure.**
- [ ] **Step 3: Implement a local effective input without mutating `GeologicSpec`; increment `MantleStage` version.**
- [ ] **Step 4: Run `cargo test --test mantle_generation --test mantle_stage --test geologic_generation --test geologic_stage` and `git diff --check`.**
- [ ] **Step 5: Commit `feat: bias volcanic island mantle forcing`.**

---

### Task 5: Guarantee the formal closed-map ocean frame

**Files:**
- Modify: `src/generators/natural/relief.rs`
- Modify: `src/generators/natural/stage.rs`
- Modify: `tests/relief_generation.rs`, `tests/hydro_erosion_generation.rs`, and `tests/natural_display_golden.rs`.

**Interfaces:**
- Consumes spatial boundary condition/distance and the four formal relief components.
- Produces the unchanged `ReliefSnapshot` schema and exact sum identity.
- Creates no display mask or derived fake world field.

- [ ] **Step 1: Write a red test over every preset asserting all spatial boundary cells have elevation below sea level, formal Ocean classification, and exact component identity.**
- [ ] **Step 2: Run `cargo test --test relief_generation closed_world_boundary_is_ocean_after_every_relief_component -- --exact`; observe exposed boundary land.**
- [ ] **Step 3: Before final safety reconciliation, apply an 8%-short-side envelope.**

```rust
crust_base[i] = -5_200.0 + (crust_base[i] + 5_200.0) * weight;
tectonic_offset[i] = attenuate_positive(tectonic_offset[i], weight);
volcanic_offset[i] = attenuate_positive(volcanic_offset[i], weight);
regional_offset[i] = attenuate_positive(regional_offset[i], weight);
```

Keep negative effects. Return a generation error if an outer cell still reaches sea level. Increment `ReliefStage` version.

- [ ] **Step 4: Add fixed-seed assertions that the outer 2% west/east centroid bands contain no land and ocean current-surface cells equal constructional relief.**
- [ ] **Step 5: Run release relief, hydro-erosion, and quality tests plus `git diff --check`.**
- [ ] **Step 6: Commit `fix: keep closed world boundaries oceanic`.**

---

### Task 6: Publish preset selection and provenance in the app

**Files:**
- Modify: `src/app.rs`
- Modify: `src/app/natural_display.rs`
- Modify their in-module tests and necessary external-artifact fixtures.

**Interfaces:**
- `TemplateApp` persists `formation_spec: WorldFormationSpec` with serde default compatibility.
- Every natural build entry point validates and supplies it.
- `NaturalFieldDocument` owns `Arc<ResolvedWorldFormationArtifact>`.
- UI uses `formation_preset_label` and displays requested → resolved for Random.

- [ ] **Step 1: Write red tests for default Continents provenance, eight exact externals, atomic failure preservation including formation artifact, Random publication, and visible recommended fraction changes.**
- [ ] **Step 2: Run the exact default-provenance test and verify missing app/document state failure.**
- [ ] **Step 3: Retrieve resolved formation before candidate construction and include it in the existing atomic publish transaction; add the narrow spec error variant.**
- [ ] **Step 4: Add the egui preset ComboBox before plate controls. Named selection visibly assigns 0.38/0.26/0.42/0.28/0.16 to the existing fraction field; Random leaves the current fraction unchanged. Rebuild remains explicit.**
- [ ] **Step 5: Run `cargo test --lib app::`, `cargo test --test natural_field_views --test natural_stage_graph`, and `git diff --check`.**
- [ ] **Step 6: Commit `feat: expose world formation presets`.**

---

### Task 7: Add quality matrices, reviewed goldens, performance, and CI

**Files:**
- Modify: `tests/natural_display_golden.rs`
- Modify: `tests/natural_performance.rs`
- Modify reviewed PNGs under `tests/golden/natural-foundation/`
- Modify: `.github/workflows/rust.yml`

- [ ] **Step 1: Expand `quality_across_fixed_seed_set` to five concrete presets × eight seeds. Print preset, seed, component counts, largest share, land share, and edge-land count; enforce the design ranges. Run it red before tuning.**
- [ ] **Step 2: Tune only nucleus counts, corridor/edge penalties, owner weights, noise amplitude, and frame support. Do not delete components post hoc, retry hidden seeds, paint final heights, or change renderer behavior.**
- [ ] **Step 3: Regenerate reviewed images.**

```powershell
$env:SEKAI_UPDATE_NATURAL_GOLDENS='1'
cargo test --release --test natural_display_golden regenerate_natural_goldens -- --exact --ignored
Remove-Item Env:SEKAI_UPDATE_NATURAL_GOLDENS
```

- [ ] **Step 4: Inspect `crust.png`, `elevation.png`, `current-surface.png`, `surface-water.png`, and `plate.png` at original resolution. Plate geometry must remain the same for the same seed while crust/surface show separated land and ocean edges.**
- [ ] **Step 5: Update performance expectation to 16 stages, preserve existing 20,000-cell budgets, and ensure CI names the release quality target.**
- [ ] **Step 6: Run release golden and ignored performance gates plus `git diff --check`.**
- [ ] **Step 7: Commit `test: verify world formation quality`.**

---

### Task 8: Verify, inspect desktop output, merge, and push

- [ ] **Step 1: Run architecture scans.**

```powershell
rg -n "crate::(app|ui|gpu|terrain|engine|rules|generators)" src/world
rg -n "PlateIdField|plates\(|plate_for_cell" src/generators/natural/formation.rs src/generators/natural/formation_stage.rs
rg -n "history|timeline|event_year|formation_year" src/world/natural src/generators/natural src/app.rs
```

- [ ] **Step 2: Run fresh full gates.**

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-targets
cargo test --workspace --all-targets --release
cargo clippy --workspace --all-targets --all-features -- -D warnings
git diff --check
```

- [ ] **Step 3: If installed, run WASM and Trunk gates; then `cargo build --release --bin sekai`.**
- [ ] **Step 4: Stop only the previously recorded exact Sekai executable after verifying its path. Start this worktree release executable and inspect every named preset plus Random in current surface, crust, land/ocean, and plate views. Require no edge land, clipped continents, ellipse-like macro blobs, or display errors.**
- [ ] **Step 5: Any discovered defect first gets a failing automated test, then the minimal fix and focused/full re-verification.**
- [ ] **Step 6: Fetch and rebase on `origin/main`, repeat focused gates if changed, merge `--no-ff` into main, push, fetch, and verify `main` and `origin/main` hashes match. Never force-push.**
- [ ] **Step 7: Build and launch the merged main release, inspect default current surface once, then remove only the clean merged feature worktree/branch.**

## Self-Review Results

- Spec coverage: Tasks 1–8 cover provenance, orthogonality, macro morphology, ocean framing, volcanic bias, UI, goldens, performance, desktop verification, and integration.
- Placeholder scan: no placeholder implementation or unnamed error/test remains.
- Type consistency: requested/resolved enums are distinct; only the resolved artifact crosses stages; `generate_plates` never receives formation.
- Scope: periodic/spherical topology remains a separate future project.

## Completion Criteria

- Default release visibly shows several separated continents surrounded by ocean.
- Every concrete preset has a distinct tested topology across eight fixed seeds.
- Every boundary cell and tested east/west band remain ocean in relief and current surface.
- Same seed/spec is deterministic; Random displays its concrete choice.
- Preset changes leave all plate records and ownership byte-identical.
- Full available debug/release/clippy/fmt/web gates pass and reviewed images are inspected.
- `main` and `origin/main` share the merge hash, and the verified main release is left running.
