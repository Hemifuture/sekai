# Spherical Relief and Geology (S0B.3) Implementation Plan

> **Execution rule:** implement task-by-task with witnessed RED tests, the smallest GREEN change, focused regression checks, and a task-scoped commit. Do not publish the spherical path to the app graph before S0B.6.

**Goal:** Generate explainable present-day relief and geologic fields directly on the authoritative closed spherical surface, consuming only validated spherical tectonic and mantle semantics, while preserving every planar V1/V2/V3 wire, random stream, hash, and image.

**Architecture:** Keep `SphericalSurfaceSnapshot` as the only geometry/topology owner and bind both new outputs to its exact `SurfaceRef`. Share only genuinely geometry-independent field equations and graph kernels. Put three-dimensional spherical noise, local great-circle hotspot trails, and overriding-side island arcs behind a sphere-specific morphology module. Relief remains an exact sum of crust, tectonic, volcanic, and regional components; geology remains the sole writer of bedrock and material-property fields. Neither module reads a projection, cubed-sphere face, renderer, or UI state.

**Science/product position:** This is a deterministic current-state synthesis, not archived geologic history and not a coupled viscoelastic lithosphere solver. Compact graph-distance kernels approximate finite tectonic belts; 3D coherent noise sampled on unit radial vectors supplies seamless long-wavelength heterogeneity; local Euler velocity and geodesic geometry orient hotspot chains and island arcs. These choices retain the causal signs and spatial relationships needed by ecology and climate while keeping generation interactive and smoothly replaceable by richer internal solvers later.

**Tech stack:** Rust 1.85, serde/serde_json, existing deterministic stage RNG, the existing `noise` crate (frozen planar Perlin plus sphere-only 3D OpenSimplex), authoritative geodesic Voronoi surface, and existing spatial vector primitives; no new dependencies.

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

- [x] Extract private/package-visible dense-field and semantic validation helpers that accept explicit cell counts and schema limits. Keep all existing planar constructors, serializers, deserializers, errors, and accepted legacy inputs unchanged.
- [x] Add narrow validated-surface entry points for already-validated spherical upstream snapshots so a combined generator can avoid redundant whole-surface validation without exposing an unchecked public API.
- [x] Freeze planar relief/geology JSON and representative hashes before and after the refactor.
- [x] Verify with focused planar and spherical contract tests; the compatible extractions landed with the task-scoped contract and synthesis commits.

### Task 2: Define the surface-bound spherical relief contract

**Files:**
- Create: `src/world/natural/spherical_relief.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `tests/spherical_relief_contracts.rs`

- [x] Write RED tests for schema V4, exact `SurfaceRef`, dense lengths, all numeric bounds, land/ocean classification, exact component identity, strict unknown-field rejection, round-trip stability, and rejection of a different equal-count surface.
- [x] Write RED allocation tests for every dense field at the maximum and maximum-plus-one size, ensuring failure occurs before oversized allocation.
- [x] Implement immutable private fields, checked construction, getters, strict bounded wire decoding, self-validation, and `validate_against(surface, tectonic, mantle)` with exact surface-bound upstream matching.
- [x] Keep the contract free of copied cells, edges, adjacency, projection state, or renderer data.
- [x] Verify planar relief compatibility and commit `feat: add spherical relief snapshot contracts`.

### Task 3: Extract geometry-independent relief synthesis without planar drift

**Files:**
- Modify: `src/generators/natural/relief.rs`
- Test: `tests/relief_generation.rs`
- Test: `tests/natural_display_golden.rs`

- [x] Introduce narrow internal read views for crust ownership/kind, boundary effects, mantle influence, and topology rather than a universal context object.
- [x] Extract the shared crust-base field, local boundary source amplitudes, compact graph diffusion, and final bounded reconciliation. Preserve the planar call order, integer arithmetic, iteration order, and random labels exactly.
- [x] Keep `apply_closed_ocean_frame` exclusively in the planar adapter.
- [x] Add component-level no-drift tests, run the existing planar goldens/hashes, and commit `refactor: share relief graph synthesis`.

### Task 4: Add deterministic seamless spherical regional relief

**Files:**
- Modify: `src/generators/natural/relief_noise.rs`
- Test: `tests/spherical_relief_generation.rs`

- [x] Write RED tests for deterministic `ReliefNoise3d`, seed sensitivity, finite/bounded samples, continuity across an arbitrary longitude cut, ordinary pole statistics, and axis-bias-resistant sampling.
- [x] Sample coherent 3D OpenSimplex octaves at authoritative unit radial cell centers. This trades a modest constant-factor cost for less lattice-direction bias than Perlin; limit useful octaves from representative cell spacing so unresolved frequencies cannot alias into cell noise.
- [x] Area-weight the global recentering with authoritative cell areas, then quantize and clamp through the existing regional-offset bounds. Do not use longitude/latitude, a seam coordinate, or cubed-sphere faces.
- [x] Verify no changes to existing `ReliefNoise2d` output and commit the noise work with the spherical relief generator task.

### Task 5: Generate causal spherical boundary, hotspot, and arc relief

**Files:**
- Create: `src/generators/natural/spherical_island_relief.rs`
- Create: `src/generators/natural/spherical_relief.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_relief_generation.rs`

- [x] Write RED sign tests: collisions uplift both sides; rifts lower the axis; ridges uplift oceanic divergence; subduction creates a descending-side trench and overriding-side uplift; transform relief remains weak. Effects must decay monotonically outside compact graph-distance belts.
- [x] Write RED hotspot tests: the current source remains a dominant edifice, the older chain follows positive local Euler velocity, chain cells remain on the source plate, and volcanic offset is zero outside authoritative mantle support.
- [x] Write RED island-arc tests: peaks occur only on the overriding side, are displaced inland from the trench using local tangent/great-circle direction, are sparse local maxima, and remain ordinary across poles and an arbitrary display seam.
- [x] Implement a spherical generator that validates the surface and upstream identities once, builds one `NaturalTopologyIndex`, runs shared relief cores, applies sphere-only morphology, applies 3D regional relief, reconciles the exact component sum, and returns V4.
- [x] Use source-centered tangent projection only as a local calculation. Store no tangent basis, trail direction, or copied geometry in the snapshot.
- [x] Verify deterministic repeatability, seed sensitivity, component bounds, no outer-boundary correction, multiple mesh resolutions/radii, planar no-drift, and commit `feat: generate spherical relief`.

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

- [x] Write RED V2 contract tests for exact surface identity, upstream V2 identity, strict/bounded decoding, dense lengths, bedrock/crust compatibility, normalized property fields, and equal-count different-surface rejection.
- [x] Extract geology's geometry-independent boundary influence, province diffusion, local-relative-relief, bedrock classification, and material-property formulas behind narrow semantic views. Preserve all planar equations, iteration order, random streams, and output hashes.
- [x] Implement the spherical wrapper using the already validated spherical topology and V4 relief. It may derive fields but must never mutate relief, crust, mantle, or topology facts.
- [x] Verify bedrock categories, fracture response near active boundaries, geothermal response near mantle heat, erosion resistance/permeability envelopes, deterministic province coherence, planar no-drift, and commit `feat: generate spherical geology`.

### Task 7: Whole-slice scientific, compatibility, ownership, and performance gates

**Files:**
- Create: `tests/spherical_relief_geology_matrix.rs`
- Modify: this plan with measured evidence

- [x] Run a deterministic matrix spanning minimum/Earth/maximum radii, several mesh frequencies, seeds, plate counts, tectonic activities, world-formation presets, and mantle biases. Freeze representative relief/geology hashes only after scientific review.
- [x] Test pole bands and arbitrary longitude cuts statistically; a closed surface must have no seam-specific or boundary-specific morphology.
- [x] Confirm the four relief components retain exact identity and scientifically correct sign/side relationships; confirm hotspot/arc orientation from local frames; confirm geology consumes only validated upstream fields.
- [x] Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets --all-features`, and `cargo check --target wasm32-unknown-unknown --all-features`.
- [x] Measure Release generation near 20,000 cells, report relief time, geology time, persistent semantic bytes, and working-set delta. Audit topology construction and O(C/E) allocations.
- [x] Audit that no S0B.3 type owns surface geometry and no app/stage/UI path consumes half-migrated snapshots.
- [x] Complete a fresh read-only scientific/code review, close all Critical/Important findings, rerun fresh gates, append exact evidence, and commit `docs: record spherical relief and geology evidence`.
- [x] Fast-forward merge the reviewed branch into `main`, remove only the verified S0B.3 worktree/branch, then continue to S0B.4.

## Verification evidence (2026-08-04)

### Scientific and deterministic matrix

The fixed matrix covers radii from `1 m` to `100,000 km`, `42` through `642` cells, `2` through `64` plates, quiet/moderate/active tectonics, four formation profiles, quiet/moderate/active mantle outcomes, multiple seeds, and exact repeat generation. Every case validates exact `SurfaceRef` identity, the four-component elevation equation, bounded material fields, zero volcanic relief outside mantle support, source-cell volcanic bedrock, and an area-weighted regional mean below `0.05 m`.

| Case | Cells | Relief hash | Geology hash |
|---|---:|---|---|
| minimum-radius-quiet | 42 | `c88766ca2693c0146eee4408803b288ff7786d1ebaf45a8e27ff0602c059d8f4` | `b61c39e0df6d7c39658d433a8a5078b841ed02d5615c15a0ffc30fb1ba0fc1a7` |
| regional-great-island | 92 | `4425c3202f6d85288e11ed87998f1b3146f58bceae7c54a20eace2a4292f0ade` | `a3555806fe66eec5b2508bd9e80231134520e814c92495f085abfdb7f892feba` |
| earth-continents | 162 | `e1bdd2f43f3ac5495a81f91bc586f54729989e4c8cfa3e118df8b1dd743278b9` | `20870e5522a429f91aa971d4b4b921a9a2fba8638a49c3b9dd63cab8ae3c5c4e` |
| maximum-radius-volcanic | 642 | `5cb52cc0c50d804d53292ead817d95045952d3a9bb177a2ea1f8b8d04d0c6073` | `a1e82e23df49ef044f5659849bc6543d920246284ed62f2ea812027d28c05cca` |

Regional relief samples sphere-only 3D OpenSimplex on unit radials, so it has no longitude, seam, pole branch, or projection face. Hotspot morphology uses source-centered geodesic/tangent coordinates normalized by authoritative support radius; the final review corrected an earlier whole-planet noise scale so local physical wavelengths no longer vary with planet radius. Island arcs choose an overriding-side neighbor using the local great-circle tangent away from the trench. Statistical cut/pole checks and causal boundary/hotspot tests pass.

### Compatibility, trust, and ownership

- Public validation checks the authoritative surface and every supplied spherical upstream; crate-private validated-surface paths only avoid duplicate full-surface scans after that trust boundary.
- V4 relief and V2 geology carry the exact content-fingerprinted spherical `SurfaceRef`. Strict new wires stream-bound every dense sequence before oversized allocation and reject unknown fields; all existing planar relief/geology wires retain their prior decoding behavior and byte/hash compatibility.
- `SphericalSurfaceSnapshot` remains the sole geometry/topology owner. The new snapshots contain only semantic dense fields plus `SurfaceRef`; source search confirms there is no app, stage, engine-publication, UI, projection, or renderer consumer in S0B.3.
- Planar relief/geology regressions and reviewed natural-display goldens pass unchanged. No planar random label, iteration order, field equation, or closed-ocean-frame behavior moved into the spherical adapter.

### Performance and fresh gates

Release measurement at `20,252` cells and `60,750` edges:

- relief: `63.351 ms`
- geology: `53.459 ms`
- combined: `116.810 ms` (budget `5,000 ms`)
- relief persistent semantic bytes: `486,048`
- geology persistent semantic bytes: `567,056`
- combined persistent semantic bytes: `1,053,104`
- measured working-set delta: `1,265,664 bytes` (budget `256 MiB`)
- diagnostics: `0`

Fresh post-review commands all exited `0`: formatting, all-target/all-feature Clippy with warnings denied, all-target/all-feature tests, and the all-feature `wasm32-unknown-unknown` check. The final local read-only review found and closed the hotspot physical-scale issue above; no unresolved Critical or Important issue remains.

## Explicitly deferred

- S0B.4: spherical preliminary-climate forcing.
- S0B.5: closed-sphere hydrology and erosion, including current-state time stepping.
- S0B.6: artifacts, stage graph, cache/provenance publication, field adapters, compatibility loading, and product/UI exposure.
- S0C/S0D: spherical presentation and conservative remapping to circulation grids.
- C0-C4: layered atmosphere/ocean circulation, ENSO, cyclones, and long-running coupled climate.
- Full lithosphere rheology, flexure, slab thermomechanics, absolute plate chronology, hotspot motion history, and archived geologic ages. The contracts are deliberately semantic so richer internal solvers can replace the present approximations later without creating new field owners.

## Completion definition

S0B.3 is complete only when surface-bound V4 relief and V2 geology generate directly from the authoritative sphere; exact upstream identity and bounded decoding are enforced; relief component identity and causal scientific signs pass; poles/cuts have no special behavior; planar outputs remain frozen; full compatibility and Release performance gates pass; ownership/publication audits are clean; and a fresh read-only review reports no unresolved Critical or Important issue.
