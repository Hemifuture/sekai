# Causal Island Relief Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add deterministic hotspot island groups and oceanic subduction island arcs whose natural-looking relief comes from bounded multiscale noise without allowing global noise to invent land.

**Architecture:** Add a pure Relief-internal continuous-noise sampler and a separate causal-island morphology module. Mantle remains the sole owner of hotspot centers/supports, Tectonics remains the sole owner of boundary classification and plate velocity, and Relief alone converts those inputs into `volcanic_offset_m`, `tectonic_offset_m`, final elevation, and land/ocean. No public field is added and no old `terrain`/UI type enters the natural generator.

**Tech Stack:** Rust 2021, `noise 0.9` Perlin primitives, deterministic `StageRng`/ChaCha labeled substreams, quantized Voronoi world coordinates, graph-distance compact kernels, Cargo integration tests, reviewed PNG goldens, native/WASM/Trunk verification.

## Global Constraints

- Generate a present-day slice only; do not add ages, event records, historical simulation, hotspot chronology, or a time axis.
- World-formation presets continue to own macro crust morphology only; this feature must not change plate seeds, plate ownership, plate velocities, crust kinds, or boundary classifications.
- Mantle must not read tectonics or relief; Tectonics must not read mantle or final elevation; Display must remain read-only.
- Noise may modulate morphology only inside hotspot or oceanic-subduction causal support. Global noise alone must never create land.
- `mantle.volcanic_influence == 0` implies `relief.volcanic_offset_m == 0`.
- Hotspot contributions stay in `volcanic_offset_m`; oceanic subduction-arc contributions stay in `tectonic_offset_m`.
- Keep `ReliefSnapshot` V2 fields and current component ranges, including `volcanic_offset_m: 0..=4_000 m`.
- Preserve `elevation = crust_base + tectonic_offset + volcanic_offset + regional_offset` within the existing tolerance.
- Preserve the formal closed-boundary ocean guarantee and the visible east/west ocean band.
- Sample continuous noise from quantized world coordinates or radius-normalized local coordinates, never UI pixels, cell IDs, or iteration order.
- Every stochastic mechanism has an independent named substream and stable ID tie-breaks.
- Use strict TDD: each new behavior test must be observed failing for the intended missing behavior before production implementation.
- Do not blindly accept golden changes; inspect all changed images at original resolution.

---

### Task 1: Add a pure multiscale Relief noise sampler

**Files:**
- Create: `src/generators/natural/relief_noise.rs`
- Modify: `src/generators/natural/mod.rs`

**Interfaces:**
- Produces: `FractalProfile { octaves, frequency, lacunarity, persistence }`.
- Produces: `ReliefNoise2d::new(seed: u32)`, `fbm(point, profile) -> f64`, `ridged(point, profile) -> f64`, and `warp(point, frequency, strength) -> [f64; 2]`.
- Consumes: only numeric coordinates and `noise::Perlin`; no world, engine, UI, GPU, or legacy terrain types.
- Guarantees: fBm in `[-1, 1]`, ridged in `[0, 1]`, finite bounded warp, independent octave sources, deterministic output.

- [ ] **Step 1: Declare the private module and write failing sampler tests**

Add `mod relief_noise;` beside the other private natural-generator modules. Create `relief_noise.rs` with tests written against the desired API before defining it:

```rust
#[cfg(test)]
mod tests {
    use super::{FractalProfile, ReliefNoise2d};

    const PROFILE: FractalProfile = FractalProfile {
        octaves: 5,
        frequency: 1.25,
        lacunarity: 2.03,
        persistence: 0.5,
    };

    #[test]
    fn multiscale_noise_is_seeded_bounded_and_nonconstant() {
        let first = ReliefNoise2d::new(41);
        let repeated = ReliefNoise2d::new(41);
        let changed = ReliefNoise2d::new(42);
        let points = [[0.125, 0.25], [0.5, 0.75], [1.25, -0.5], [3.0, 2.0]];
        let actual = points.map(|point| first.fbm(point, PROFILE));

        assert_eq!(actual, points.map(|point| repeated.fbm(point, PROFILE)));
        assert_ne!(actual, points.map(|point| changed.fbm(point, PROFILE)));
        assert!(actual.iter().all(|value| (-1.0..=1.0).contains(value)));
        assert!(actual.windows(2).any(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn ridged_and_domain_warp_keep_their_bounded_contracts() {
        let noise = ReliefNoise2d::new(99);
        for point in [[0.0, 0.0], [0.25, 0.75], [2.0, -1.0]] {
            let ridge = noise.ridged(point, PROFILE);
            let warped = noise.warp(point, 0.8, 0.12);
            assert!((0.0..=1.0).contains(&ridge));
            assert!((warped[0] - point[0]).abs() <= 0.12);
            assert!((warped[1] - point[1]).abs() <= 0.12);
        }
    }
}
```

- [ ] **Step 2: Run the unit tests and verify the expected red result**

Run:

```powershell
cargo test --lib generators::natural::relief_noise::tests -- --nocapture
```

Expected: compile failure because `FractalProfile` and `ReliefNoise2d` do not yet exist.

- [ ] **Step 3: Implement the bounded sampler minimally**

Implement six independently seeded Perlin octave sources plus two warp sources. Rotate the sampling coordinate by a fixed non-axis-aligned matrix between octaves, multiply frequency by `lacunarity`, multiply amplitude by `persistence`, and normalize by the accumulated amplitude:

```rust
#[derive(Debug, Clone, Copy)]
pub(super) struct FractalProfile {
    pub(super) octaves: usize,
    pub(super) frequency: f64,
    pub(super) lacunarity: f64,
    pub(super) persistence: f64,
}

pub(super) struct ReliefNoise2d {
    octaves: [noise::Perlin; 6],
    warp_x: noise::Perlin,
    warp_y: noise::Perlin,
}
```

Use `debug_assert!` for compile-time-owned valid profiles, clamp returned fBm/ridged values to their documented ranges, and do not add a general public noise framework.

- [ ] **Step 4: Run focused green tests and formatting**

```powershell
cargo test --lib generators::natural::relief_noise::tests -- --nocapture
cargo fmt --all -- --check
git diff --check
```

- [ ] **Step 5: Commit the independently testable sampler**

```powershell
git add src/generators/natural/mod.rs src/generators/natural/relief_noise.rs
git commit -m "feat: add bounded relief noise sampler"
```

---

### Task 2: Shape hotspot support into current-slice volcanic island groups

**Files:**
- Create: `src/generators/natural/island_relief.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/random.rs`
- Modify: `src/generators/natural/relief.rs`
- Modify: `tests/relief_generation.rs`

**Interfaces:**
- Consumes: `SpatialSnapshot`, `NaturalTopologyIndex`, `TectonicSnapshot`, `MantleSnapshot`, and one Relief-owned `u32` morphology seed.
- Produces: `synthesize_hotspot_offset(...) -> Vec<f32>` aligned to spatial cells.
- Uses: `Hotspot::{source_cell, strength_permille, support_radius_m}`, current source-cell plate velocity, quantized world/local coordinates, and `ReliefNoise2d`.
- Preserves: `MantleSnapshot`, `TectonicSnapshot`, `ReliefGenerator::generate`, and every public field ID.

- [ ] **Step 1: Add the independent hotspot substream test**

Add this constant to `random.rs`:

```rust
pub(super) const RELIEF_HOTSPOT_MORPHOLOGY_LABEL: &str =
    "relief-hotspot-morphology-v1";
```

Before using it in production, add a test that consumes 100 values from this stream and proves the first eight values from `RELIEF_REGIONAL_LABEL` and `RELIEF_TECTONIC_DETAIL_LABEL` remain identical to pristine captures.

- [ ] **Step 2: Write failing integration tests for support, seed, and velocity semantics**

In `tests/relief_generation.rs`, add a validated rectangular oceanic fixture with one plate, oceanic thickness `14.0 km`, and a centered hotspot whose support does not reach the closed frame. Add:

```rust
#[test]
fn hotspot_morphology_is_seeded_support_bounded_and_kinematically_oriented() {
    let spatial = large_regular_grid();
    let mantle = centered_hotspot_mantle(&spatial);
    let east = uniform_oceanic_tectonics(&spatial, PlateVelocity::new(80, 0).unwrap());
    let north = uniform_oceanic_tectonics(&spatial, PlateVelocity::new(0, 80).unwrap());

    let first = generate_relief_with_mantle(&spatial, &east, &mantle, 71);
    let repeated = generate_relief_with_mantle(&spatial, &east, &mantle, 71);
    let changed_seed = generate_relief_with_mantle(&spatial, &east, &mantle, 72);
    let changed_velocity = generate_relief_with_mantle(&spatial, &north, &mantle, 71);

    assert_eq!(first.volcanic_offset_m(), repeated.volcanic_offset_m());
    assert_ne!(first.volcanic_offset_m(), changed_seed.volcanic_offset_m());
    assert_ne!(first.volcanic_offset_m(), changed_velocity.volcanic_offset_m());
    assert!(mantle
        .volcanic_influence()
        .iter()
        .zip(first.volcanic_offset_m().values())
        .all(|(&influence, &offset)| influence > 0.0 || offset == 0.0));
}
```

The production change that makes this pass is Relief-owned stochastic/kinematic hotspot morphology; the current purely radial `synthesize_volcanic_offset` must fail the changed-seed and changed-velocity assertions.

- [ ] **Step 3: Run the hotspot test and verify the intended red result**

```powershell
cargo test --test relief_generation hotspot_morphology_is_seeded_support_bounded_and_kinematically_oriented -- --exact --nocapture
```

Expected: FAIL because the current volcanic offset ignores both Relief seed and plate velocity.

- [ ] **Step 4: Write the failing emergent-peak behavior test**

Using the same fixture, assert the hotspot source is formal land, at least one supported non-source cell remains ocean with positive volcanic relief, and every formal land cell on oceanic crust is separated from the closed boundary:

```rust
#[test]
fn strong_oceanic_hotspot_creates_an_island_among_submerged_seamounts() {
    let spatial = large_regular_grid();
    let tectonic = uniform_oceanic_tectonics(
        &spatial,
        PlateVelocity::new(80, 20).unwrap(),
    );
    let mantle = centered_hotspot_mantle(&spatial);
    let relief = generate_relief_with_mantle(&spatial, &tectonic, &mantle, 71);
    let source = mantle.hotspots()[0].source_cell();

    assert_eq!(relief.land_ocean_kind(source), Some(LandOceanKind::Land));
    assert!((0..spatial.cell_count()).any(|index| {
        mantle.volcanic_influence()[index] > 0.0
            && relief.volcanic_offset_m().values()[index] > 0.0
            && relief.land_ocean().raw_values()[index] == 0
    }));
}
```

- [ ] **Step 5: Run the emergent-peak test and verify red**

```powershell
cargo test --test relief_generation strong_oceanic_hotspot_creates_an_island_among_submerged_seamounts -- --exact --nocapture
```

Expected: FAIL because the existing oceanic hotspot amplitude tops out at `3_200 m` and the radial response does not create a distinct exposed peak in the chosen deep-ocean fixture.

- [ ] **Step 6: Implement causal hotspot morphology**

Create `island_relief.rs` and implement:

```rust
pub(super) fn synthesize_hotspot_offset(
    spatial: &SpatialSnapshot,
    topology: &NaturalTopologyIndex,
    tectonic: &TectonicSnapshot,
    mantle: &MantleSnapshot,
    seed: u32,
) -> Vec<f32>;
```

For each hotspot in stable ID order:

- normalize local cell-centroid deltas by `support_radius_m`;
- derive an entity seed from the mechanism seed and `HotspotId` without consuming other streams;
- domain-warp local coordinates by at most `0.12` support radii;
- build a compact current-center edifice;
- when plate speed is nonzero, add a compact trail envelope along normalized current plate velocity, no farther than `0.8` support radii and with bounded cross-track width;
- combine a 4–5 octave fBm term, a 3–4 octave ridged term, and a `gamma > 1` peak curve;
- use strength only as a bounded amplitude/radius response, never as an unbounded multiplier;
- use a lower, broader response on continental crust and a sharper response up to the existing `4_000 m` maximum on oceanic crust;
- combine overlapping hotspot contributions with `max`, then multiply/clip by formal mantle support so zero influence stays exactly zero.

Replace the old radial `synthesize_volcanic_offset(tectonic, mantle)` call with this function, drawing exactly one seed from `RELIEF_HOTSPOT_MORPHOLOGY_LABEL`.

- [ ] **Step 7: Run hotspot green tests and existing Relief regressions**

```powershell
cargo test --test relief_generation hotspot_morphology_is_seeded_support_bounded_and_kinematically_oriented -- --exact --nocapture
cargo test --test relief_generation strong_oceanic_hotspot_creates_an_island_among_submerged_seamounts -- --exact --nocapture
cargo test --test relief_generation mantle_influence_adds_local_explainable_volcanic_relief -- --exact
cargo test --test relief_generation closed_world_boundary_is_ocean_after_every_relief_component -- --exact
git diff --check
```

- [ ] **Step 8: Commit the hotspot morphology slice**

```powershell
git add src/generators/natural/island_relief.rs src/generators/natural/mod.rs src/generators/natural/random.rs src/generators/natural/relief.rs tests/relief_generation.rs
git commit -m "feat: synthesize causal hotspot island groups"
```

---

### Task 3: Add discrete oceanic subduction island-arc peaks

**Files:**
- Modify: `src/generators/natural/island_relief.rs`
- Modify: `src/generators/natural/random.rs`
- Modify: `src/generators/natural/relief.rs`
- Modify: `tests/relief_generation.rs`

**Interfaces:**
- Consumes: validated `Subduction` boundary segments, member edges, subducting polarity, crust kinds, and one Relief-owned arc seed.
- Produces: `synthesize_oceanic_arc_peaks(...) -> Vec<f32>` aligned to spatial cells.
- Adds: only a nonnegative extra contribution to `tectonic_offset_m`; broad existing arc/trench effects remain unchanged.

- [ ] **Step 1: Add and isolate the island-arc substream**

Add:

```rust
pub(super) const RELIEF_ISLAND_ARC_LABEL: &str = "relief-island-arc-v1";
```

Extend the random-substream isolation test so consuming hotspot or arc morphology cannot perturb regional, tectonic-detail, or each other.

- [ ] **Step 2: Write a failing oceanic-arc ownership test**

Add a fixture pair with identical plates, velocities, `Subduction` records, strengths, segments, and Relief seed. One fixture has oceanic crust on both sides; the other has oceanic descending crust and continental overriding crust. Assert:

```rust
#[test]
fn ocean_ocean_subduction_adds_discrete_arc_peaks_without_changing_polarity() {
    let spatial = large_regular_grid();
    let oceanic = ocean_ocean_subduction(&spatial);
    let continental = ocean_continent_subduction(&spatial);
    let mantle = zero_hotspot_mantle(&spatial);
    let oceanic_relief = generate_relief_with_mantle(&spatial, &oceanic, &mantle, 91);
    let continental_relief = generate_relief_with_mantle(
        &spatial,
        &continental,
        &mantle,
        91,
    );

    let oceanic_max = oceanic_relief
        .tectonic_offset_m()
        .values()
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let continental_max = continental_relief
        .tectonic_offset_m()
        .values()
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(oceanic_max > continental_max + 500.0);
    assert!(oceanic_relief.tectonic_offset_m().values().iter().any(|&v| v < 0.0));
}
```

The current implementation gives both overriding sides the same arc amplitude, so the first assertion must fail while the negative trench assertion continues to pass.

- [ ] **Step 3: Run the arc test and verify red**

```powershell
cargo test --test relief_generation ocean_ocean_subduction_adds_discrete_arc_peaks_without_changing_polarity -- --exact --nocapture
```

Expected: FAIL because no oceanic-island-arc specialization exists.

- [ ] **Step 4: Implement sparse segment-owned peak selection**

Implement:

```rust
pub(super) fn synthesize_oceanic_arc_peaks(
    spatial: &SpatialSnapshot,
    topology: &NaturalTopologyIndex,
    tectonic: &TectonicSnapshot,
    seed: u32,
) -> Vec<f32>;
```

For each stable `BoundarySegmentId` whose kind is `Subduction`:

- orient each member edge with the existing subducting polarity and collect unique overriding-side cells;
- discard the segment unless both sides of a candidate edge are oceanic;
- score candidates from warped fBm plus ridged noise sampled at quantized world coordinates;
- apply a power curve so only the upper tail becomes a full peak;
- select candidates over the fixed score threshold and always retain the stable highest-scoring candidate per valid segment;
- stamp selected cells with a narrow compact kernel of one to two typical traversal steps;
- scale by the existing boundary strength and cap the combined tectonic field at `TECTONIC_OFFSET_MAX_M`.

In `synthesize_tectonic_offset`, preserve the old broad field and old detail substream, then add the independently generated arc-peak field before the final clamp. Do not modify `BoundaryRecord`, `BoundarySegment`, polarity, or sign.

- [ ] **Step 5: Run arc and signed-relief green tests**

```powershell
cargo test --test relief_generation ocean_ocean_subduction_adds_discrete_arc_peaks_without_changing_polarity -- --exact --nocapture
cargo test --test relief_generation targeted_boundary_events_have_the_expected_signed_relief -- --exact
cargo test --test relief_generation tectonic_relief_detail_is_seeded_repeatable_and_sign_preserving -- --exact
cargo test --test tectonic_boundaries
git diff --check
```

- [ ] **Step 6: Commit the island-arc slice**

```powershell
git add src/generators/natural/island_relief.rs src/generators/natural/random.rs src/generators/natural/relief.rs tests/relief_generation.rs
git commit -m "feat: add oceanic subduction island arcs"
```

---

### Task 4: Add morphology quality gates and invalidate old Relief cache entries

**Files:**
- Modify: `tests/natural_display_golden.rs`
- Modify: `src/generators/natural/stage.rs`
- Modify: `tests/natural_stage_graph.rs`
- Modify: `tests/relief_generation.rs`
- Modify: `docs/superpowers/specs/2026-07-29-geologic-substrate-design.md`

**Interfaces:**
- Produces no new runtime type.
- Adds quality metrics for oceanic land components that are separated from continental crust and causally supported by mantle influence or oceanic-subduction arc support.
- Changes `ReliefStage::version()` from `5` to `6`; artifact key and snapshot schema remain unchanged.

- [ ] **Step 1: Add a failing causal-ocean-island metric**

Extend `PresetMorphologyMetrics` with:

```rust
causal_oceanic_island_component_count: usize,
```

Build the mask independently from production helpers:

- formal land;
- oceanic crust;
- at least two adjacency steps from continental crust;
- either positive mantle influence or within two adjacency steps of the overriding side of an ocean—ocean subduction edge.

Count connected components with the existing test-only BFS. Assert:

```rust
if preset == WorldFormationPreset::VolcanicIslands {
    assert!(metrics.causal_oceanic_island_component_count >= 2);
}
if preset == WorldFormationPreset::Continents && seed == 42 {
    assert!(metrics.causal_oceanic_island_component_count >= 1);
}
```

- [ ] **Step 2: Run the quality matrix and verify the old generator fails the new metric**

```powershell
cargo test --test natural_display_golden quality_across_fixed_seed_set -- --exact --nocapture
```

Expected before the island implementation is applied: FAIL because existing formal land components are overwhelmingly continental-crust components and smooth hotspot support rarely exposes deep-ocean islands. When executing this task after Tasks 2–3, temporarily revert only the island calls, run the test to confirm the regression gate fails, then restore them and continue.

- [ ] **Step 3: Tune only Relief-owned constants until the fixed gates pass**

Allowed tuning variables are local hotspot envelope radii, warp strength, fBm/ridged profiles, peak exponent, bounded hotspot amplitude response, arc score threshold, and arc compact support. Do not tune crust fraction, formation corridors, hotspot count, plate count, plate motion, sea level, field ranges, or display colors to satisfy this gate.

Run after each tuning cycle:

```powershell
cargo test --test relief_generation
cargo test --test natural_display_golden quality_across_fixed_seed_set -- --exact --nocapture
```

- [ ] **Step 4: Add the failing stage-version assertion and bump to version 6**

Change the test expectation first:

```rust
assert_eq!(ReliefStage.version(), 6);
```

Run:

```powershell
cargo test --test natural_stage_graph complete_natural_graph_publishes_physical_artifacts_with_exact_stage_metadata -- --exact
```

Expected: FAIL with actual version `5`. Then return `6` from `ReliefStage::version`, update test-only `StageIdentity::new("natural.relief", 5, ...)` fixtures to version `6`, and rerun the focused graph/Relief suites.

- [ ] **Step 5: Update the older geology design's superseded hotspot clause**

In `2026-07-29-geologic-substrate-design.md`, replace the absolute prohibition on velocity-oriented hotspot morphology with a link to the approved 2026-08-03 design and this exact boundary: current velocity may shape a present-day anisotropic island group, but no age, event sequence, old-volcano entity, or historical state is generated.

- [ ] **Step 6: Commit the quality and version gate**

```powershell
git add src/generators/natural/stage.rs tests/natural_stage_graph.rs tests/natural_display_golden.rs tests/relief_generation.rs docs/superpowers/specs/2026-07-29-geologic-substrate-design.md docs/superpowers/specs/2026-08-03-causal-island-relief-design.md docs/superpowers/plans/2026-08-03-causal-island-relief.md
git commit -m "test: enforce causal ocean island morphology"
```

---

### Task 5: Regenerate and inspect all affected natural-field goldens

**Files:**
- Regenerate through the existing test: `tests/golden/natural-foundation/*.png`
- Modify only files whose generated bytes actually change.

**Interfaces:**
- Consumes: the fixed `GOLDEN_SEED`, formal field schemas, palettes, and CPU reference rasterizer.
- Produces: reviewed reference images only; no runtime code or display mask.

- [ ] **Step 1: Prove the reviewed golden test is red for algorithm-owned pixels**

```powershell
cargo test --test natural_display_golden reviewed_natural_goldens_match -- --exact --nocapture
```

Expected: FAIL for at least `elevation.png` and `current-surface.png`; downstream bedrock, hydrology, erosion, or climate images may also fail because formal land/ocean changed.

- [ ] **Step 2: Regenerate with the repository's sole update path**

```powershell
$env:SEKAI_UPDATE_NATURAL_GOLDENS='1'
cargo test --test natural_display_golden regenerate_natural_goldens -- --exact --ignored --nocapture
Remove-Item Env:SEKAI_UPDATE_NATURAL_GOLDENS
```

- [ ] **Step 3: Inspect every changed PNG**

Use `git status --short tests/golden/natural-foundation` to enumerate exact changes. Inspect each changed image at original resolution, with special attention to:

- `elevation.png` and `current-surface.png`: separated small islands/short arcs, no pepper noise or circular stamps;
- `volcanic-influence.png`: causal supports remain broad current mantle fields and are not overwritten by relief detail;
- `bedrock.png`: volcanic classifications remain related to mantle/boundary forcing;
- water/erosion images: no land or river artifacts on the closed frame.

If a visible defect remains, add a failing behavioral regression to Task 2 or 3 and fix the owning generator before accepting the PNG.

- [ ] **Step 4: Re-run golden and release morphology tests**

```powershell
cargo test --test natural_display_golden reviewed_natural_goldens_match -- --exact
cargo test --release --test natural_display_golden quality_across_fixed_seed_set -- --exact --nocapture
git diff --check
```

- [ ] **Step 5: Commit reviewed generated evidence**

```powershell
git add tests/golden/natural-foundation
git commit -m "test: review causal island relief goldens"
```

---

### Task 6: Run full native, lint, WASM, Trunk, and visual acceptance gates

**Files:**
- Modify only when a gate exposes a behavior defect and a new failing regression is added first.

**Interfaces:**
- Produces: verification evidence for the approved design and existing CI contract.

- [ ] **Step 1: Run formatting, diff, focused tests, and the full suite**

```powershell
cargo fmt --all -- --check
git diff --check
cargo test --test relief_generation
cargo test --test natural_stage_graph
cargo test --test natural_display_golden
cargo test
```

- [ ] **Step 2: Run strict compiler and Clippy gates**

```powershell
$env:RUSTFLAGS='-D warnings'
cargo check --all-features
cargo clippy -- -D warnings
Remove-Item Env:RUSTFLAGS
```

- [ ] **Step 3: Run release quality and performance gates**

```powershell
cargo test --release --test natural_display_golden
cargo test --release --test natural_performance release_default_hydro_erosion_budget -- --exact --ignored --nocapture
```

- [ ] **Step 4: Run WASM and Trunk gates with the CI backend flag**

```powershell
$env:RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
$env:RUSTDOCFLAGS='--cfg getrandom_backend="wasm_js"'
cargo check --all-features --lib --target wasm32-unknown-unknown
trunk build
Remove-Item Env:RUSTFLAGS
Remove-Item Env:RUSTDOCFLAGS
```

- [ ] **Step 5: Run and inspect the desktop application**

Start the release application, rebuild the fixed default multi-continent world, and inspect at least formal elevation, current surface, volcanic offset/influence, tectonic offset, crust kind, and land/ocean. Confirm:

- small islands occur away from continental crust but only near hotspots or oceanic arcs;
- most hotspot support remains submerged sea mountains;
- island arcs are discontinuous and follow subduction geometry;
- no global salt-and-pepper land, circular stamps, equal-spacing beads, or exposed outer frame;
- changing seed changes local island morphology while leaving system responsibilities intact.

- [ ] **Step 6: Audit scope and final repository state**

```powershell
git status --short
git diff --stat
git diff -- src/generators/natural src/world/natural tests docs/superpowers
```

Re-read `docs/superpowers/specs/2026-08-03-causal-island-relief-design.md` line by line and map every acceptance item to a passing test or inspected view. Do not claim completion if any required gate is unavailable or failing; report the exact evidence instead.

---

## Self-Review

- Spec coverage: hotspot groups, oceanic arcs, multiscale noise, strict causal masks, ownership, deterministic streams, boundary safety, quality metrics, stage invalidation, goldens, native/WASM/Trunk, and desktop inspection each have an explicit task.
- Placeholder scan: the plan contains no TBD/TODO or unspecified implementation step; tuning is constrained to an exact Relief-owned parameter list and exact behavioral gates.
- Type consistency: `ReliefNoise2d`, `FractalProfile`, `synthesize_hotspot_offset`, and `synthesize_oceanic_arc_peaks` are defined once and consumed only by private natural-generator modules. Public snapshot and generator signatures stay unchanged.
- Execution choice: the user explicitly requested autonomous implementation, so execute inline in this session with `superpowers:executing-plans`; do not dispatch subagents.
