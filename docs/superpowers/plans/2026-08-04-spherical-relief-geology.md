# Spherical Relief and Geology (S0B.3) Implementation Plan

> **Execution rule:** implement task-by-task with witnessed RED tests, the smallest GREEN change, focused regression checks, and a task-scoped commit. Do not publish the spherical path to the app graph before S0B.6.

**Goal:** Generate explainable present-day relief and geologic fields directly on the authoritative closed spherical surface, consuming only validated spherical tectonic and mantle semantics, while preserving every planar V1/V2/V3 wire, random stream, hash, and image.

**Architecture:** Keep `SphericalSurfaceSnapshot` as the only geometry/topology owner and bind both new outputs to its exact `SurfaceRef`. Share only genuinely geometry-independent field equations and graph kernels. Put three-dimensional spherical noise, local great-circle hotspot trails, and overriding-side island arcs behind a sphere-specific morphology module. Relief remains an exact sum of crust, tectonic, volcanic, and regional components; geology remains the sole writer of bedrock and material-property fields. Neither module reads a projection, cubed-sphere face, renderer, or UI state.

**Science/product position:** This is a deterministic current-state synthesis, not archived geologic history and not a coupled viscoelastic lithosphere solver. Compact graph-distance kernels approximate finite tectonic belts; 3D coherent noise sampled on unit radial vectors supplies seamless long-wavelength heterogeneity; local Euler velocity and geodesic geometry orient hotspot chains and island arcs. These choices retain the causal signs and spatial relationships needed by ecology and climate while keeping generation interactive and smoothly replaceable by richer internal solvers later.

**Tech stack:** Rust 1.85, serde/serde_json, existing deterministic stage RNG, `noise` crate Perlin basis, authoritative geodesic Voronoi surface, and existing spatial vector primitives; no new dependencies.

## Global constraints

- Add `RELIEF_SCHEMA_V4 = 4` as the first surface-bound relief contract and `GEOLOGIC_SNAPSHOT_SCHEMA_V2 = 2` as the first surface-bound geology contract. Older planar contracts remain byte-compatible.
- Every new snapshot carries and validates the exact spherical `SurfaceRef`; equal cell/edge counts never imply compatible identity.
- New dense and nested JSON sequences use streaming allocation limits before allocation. Unknown fields are rejected only in the new strict wires; old permissive decoding is unchanged.
- `elevation = crust_base + tectonic_offset + volcanic_offset + regional_offset` must hold exactly for every cell after bounded reconciliation.
- Spherical relief has no exposed-world-boundary ocean frame and no hidden seam or pole branch.
- Boundary morphology consumes authoritative member edges, local edge frames, local relative Euler motion, crust semantics, and graph/geodesic distance. It must not consume a segment-wide fake 2D direction.
- The mantle snapshot remains the only source of hotspot position, strength, support, heat flow, and influence. Relief may add morphology but may not rewrite mantle facts.
- Hotspot chains point in the local positive plate-velocity direction away from the current mantle-fixed source; the current edifice remains dominant and relief is zero outside mantle support.
- Subduction topography places the trench on the descending side and volcanic arc candidates on the overriding side, displaced inland by local tangent geometry. Arc sparsity is deterministic and resolution-limited.
- Geologic fields stay cell-aligned and derive only from validated crust, boundary, mantle, and relief semantics. There is one writer for each published geologic field.
- Do not add UI, artifacts, cache keys, stage-graph publication, projection coordinates, render buffers, climate, hydrology, erosion, or time-history storage in S0B.3.
- At approximately 20,000 cells, keep the eventual whole S0B path inside the approved `2.5x` planar time and `256 MiB` extra-working-memory envelope. Build topology at most once per public generator and avoid repeated cell/edge-sized allocation inside loops.

## Target file structure

```text
src/world/natural/
├── relief.rs                         # shared relief field invariants, frozen planar wire
├── spherical_relief.rs               # surface-bound relief V4 contract
├── geology.rs                        # shared geology field invariants, frozen planar wire
└── spherical_geology.rs              # surface-bound geology V2 contract

src/generators/natural/
├── relief.rs                         # frozen planar adapter plus shared graph kernels
├── relief_noise.rs                   # existing 2D noise plus deterministic 3D sampler
├── spherical_island_relief.rs        # sphere-only hotspot trails and island arcs
├── spherical_relief.rs               # closed-surface relief orchestration
├── geology.rs                        # frozen planar adapter plus shared equations
└── spherical_geology.rs              # closed-surface geology orchestration

tests/
├── spherical_relief_contracts.rs
├── spherical_relief_generation.rs
├── spherical_geologic_contracts.rs
├── spherical_geologic_generation.rs
└── spherical_relief_geology_matrix.rs
```

---

### Task 1: Share validation without weakening trust boundaries

**Files:**
- Modify: `src/world/natural/relief.rs`
- Modify: `src/world/natural/geology.rs`
- Modify: `src/world/natural/spherical_tectonics.rs`
- Modify: `src/world/natural/spherical_mantle.rs`
- Test: `tests/relief_contracts.rs`
- Test: `tests/geologic_contracts.rs`
- Test: existing spherical contract tests

- [ ] Extract private/package-visible dense-field and semantic validation helpers that accept explicit cell counts and schema limits. Keep all existing planar constructors, serializers, deserializers, errors, and accepted legacy inputs unchanged.
- [ ] Add narrow validated-surface entry points for already-validated spherical upstream snapshots so a combined generator can avoid redundant whole-surface validation without exposing an unchecked public API.
- [ ] Freeze planar relief/geology JSON and representative hashes before and after the refactor.
- [ ] Verify with focused planar and spherical contract tests, then commit `refactor: share relief and geology field validation`.

### Task 2: Define the surface-bound spherical relief contract

**Files:**
- Create: `src/world/natural/spherical_relief.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `tests/spherical_relief_contracts.rs`

- [ ] Write RED tests for schema V4, exact `SurfaceRef`, dense lengths, all numeric bounds, land/ocean classification, exact component identity, strict unknown-field rejection, round-trip stability, and rejection of a different equal-count surface.
- [ ] Write RED allocation tests for every dense field at the maximum and maximum-plus-one size, ensuring failure occurs before oversized allocation.
- [ ] Implement immutable private fields, checked construction, getters, strict bounded wire decoding, self-validation, and `validate_against(surface, tectonic, mantle)` with exact upstream identity matching.
- [ ] Keep the contract free of copied cells, edges, adjacency, projection state, or renderer data.
- [ ] Verify planar relief compatibility and commit `feat: add spherical relief snapshot contracts`.

### Task 3: Extract geometry-independent relief synthesis without planar drift

**Files:**
- Modify: `src/generators/natural/relief.rs`
- Test: `tests/relief_generation.rs`
- Test: `tests/natural_display_golden.rs`

- [ ] Introduce narrow internal read views for crust ownership/kind, boundary effects, mantle influence, and topology rather than a universal context object.
- [ ] Extract the shared crust-base field, local boundary source amplitudes, compact graph diffusion, and final bounded reconciliation. Preserve the planar call order, integer arithmetic, iteration order, and random labels exactly.
- [ ] Keep `apply_closed_ocean_frame` exclusively in the planar adapter.
- [ ] Add component-level no-drift tests, run the existing planar goldens/hashes, and commit `refactor: share relief graph synthesis`.

### Task 4: Add deterministic seamless spherical regional relief

**Files:**
- Modify: `src/generators/natural/relief_noise.rs`
- Test: `tests/spherical_relief_generation.rs`

- [ ] Write RED tests for deterministic `ReliefNoise3d`, seed sensitivity, finite/bounded samples, continuity across an arbitrary longitude cut, ordinary pole statistics, and rotation-equivalent sampling.
- [ ] Sample coherent 3D Perlin octaves at authoritative unit radial cell centers. Limit useful octaves from representative cell spacing so unresolved frequencies cannot alias into cell noise.
- [ ] Area-weight the global recentering with authoritative cell areas, then quantize and clamp through the existing regional-offset bounds. Do not use longitude/latitude, a seam coordinate, or cubed-sphere faces.
- [ ] Verify no changes to existing `ReliefNoise2d` output and commit the noise work with the spherical relief generator task.

### Task 5: Generate causal spherical boundary, hotspot, and arc relief

**Files:**
- Create: `src/generators/natural/spherical_island_relief.rs`
- Create: `src/generators/natural/spherical_relief.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_relief_generation.rs`

- [ ] Write RED sign tests: collisions uplift both sides; rifts lower the axis; ridges uplift oceanic divergence; subduction creates a descending-side trench and overriding-side uplift; transform relief remains weak. Effects must decay monotonically outside compact graph-distance belts.
- [ ] Write RED hotspot tests: current source is the strongest edifice, the older chain follows positive local Euler velocity, chain cells remain on the source plate, and volcanic offset is zero outside authoritative mantle support.
- [ ] Write RED island-arc tests: peaks occur only on the overriding side, are displaced inland from the trench using local tangent/great-circle direction, are sparse local maxima, and remain ordinary across poles and an arbitrary display seam.
- [ ] Implement a spherical generator that validates the surface and upstream identities once, builds one `NaturalTopologyIndex`, runs shared relief cores, applies sphere-only morphology, applies 3D regional relief, reconciles the exact component sum, and returns V4.
- [ ] Use source-centered tangent projection only as a local calculation. Store no tangent basis, trail direction, or copied geometry in the snapshot.
- [ ] Verify deterministic repeatability, seed sensitivity, component bounds, no outer-boundary correction, multiple mesh resolutions/radii, planar no-drift, and commit `feat: generate spherical relief`.

### Task 6: Define surface-bound geology and share the material equations

**Files:**
- Create: `src/world/natural/spherical_geology.rs`
- Modify: `src/world/natural/geology.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/generators/natural/geology.rs`
- Create: `src/generators/natural/spherical_geology.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_geologic_contracts.rs`
- Test: `tests/spherical_geologic_generation.rs`

- [ ] Write RED V2 contract tests for exact surface identity, upstream V2 identity, strict/bounded decoding, dense lengths, bedrock/crust compatibility, normalized property fields, and equal-count different-surface rejection.
- [ ] Extract geology's geometry-independent boundary influence, province diffusion, local-relative-relief, bedrock classification, and material-property formulas behind narrow semantic views. Preserve all planar equations, iteration order, random streams, and output hashes.
- [ ] Implement the spherical wrapper using the already validated spherical topology and V4 relief. It may derive fields but must never mutate relief, crust, mantle, or topology facts.
- [ ] Verify bedrock categories, fracture response near active boundaries, geothermal response near mantle heat, erosion resistance/permeability envelopes, deterministic province coherence, planar no-drift, and commit `feat: generate spherical geology`.

### Task 7: Whole-slice scientific, compatibility, ownership, and performance gates

**Files:**
- Create: `tests/spherical_relief_geology_matrix.rs`
- Modify: this plan with measured evidence

- [ ] Run a deterministic matrix spanning minimum/Earth/maximum radii, several mesh frequencies, seeds, plate counts, tectonic activities, world-formation presets, and mantle biases. Freeze representative relief/geology hashes only after scientific review.
- [ ] Test pole bands and several arbitrary longitude cuts statistically; a closed surface must have no seam-specific or boundary-specific morphology.
- [ ] Confirm the four relief components retain exact identity and scientifically correct sign/side relationships; confirm hotspot/arc orientation from local frames; confirm geology consumes only validated upstream fields.
- [ ] Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets --all-features`, and `cargo check --target wasm32-unknown-unknown --all-features`.
- [ ] Measure Release generation near 20,000 cells, report relief time, geology time, persistent semantic bytes, and working-set delta. Audit topology construction and O(C/E) allocations.
- [ ] Audit that no S0B.3 type owns surface geometry and no app/stage/UI path consumes half-migrated snapshots.
- [ ] Complete a read-only scientific/code review, close all Critical/Important findings, rerun fresh gates, append exact evidence, and commit `docs: record spherical relief and geology evidence`.
- [ ] Fast-forward merge the reviewed branch into `main`, remove only the verified S0B.3 worktree/branch, then continue to S0B.4.

## Explicitly deferred

- S0B.4: spherical preliminary-climate forcing.
- S0B.5: closed-sphere hydrology and erosion, including current-state time stepping.
- S0B.6: artifacts, stage graph, cache/provenance publication, field adapters, compatibility loading, and product/UI exposure.
- S0C/S0D: spherical presentation and conservative remapping to circulation grids.
- C0-C4: layered atmosphere/ocean circulation, ENSO, cyclones, and long-running coupled climate.
- Full lithosphere rheology, flexure, slab thermomechanics, absolute plate chronology, hotspot motion history, and archived geologic ages. The contracts are deliberately semantic so richer internal solvers can replace the present approximations later without creating new field owners.

## Completion definition

S0B.3 is complete only when surface-bound V4 relief and V2 geology generate directly from the authoritative sphere; exact upstream identity and bounded decoding are enforced; relief component identity and causal scientific signs pass; poles/cuts have no special behavior; planar outputs remain frozen; full compatibility and Release performance gates pass; ownership/publication audits are clean; and independent review reports no unresolved Critical or Important issue.
