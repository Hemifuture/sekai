# Closed-Sphere Hydrology and Current-State Erosion (S0B.5) Implementation Plan

> **Execution rule:** implement task-by-task with witnessed RED tests, the smallest GREEN change, focused planar regressions, and task-scoped commits. Do not publish the spherical path to artifacts, the production stage graph, fields, app, or UI before S0B.6.

**Goal:** Generate deterministic runoff, drainage, lakes, basins, rivers, bounded fluvial incision, conservative sediment routing, and the final post-process hydrology directly on one authoritative closed spherical surface.

**Architecture:** Keep `SphericalSurfaceSnapshot` as the sole geometry/topology owner. Add strict surface-bound V2 hydrology, surface-process, and atomic hydro-erosion snapshots that carry one exact `SurfaceRef` and only semantic facts. Refactor the existing planar implementation into one geometry-independent hydrology/erosion core over `NaturalSurface` and one disposable `NaturalTopologyIndex`; retain narrow planar and spherical adapters with different outlet policies and upstream contracts. The combined spherical solve builds the topology once and performs the fixed current-slice sequence `initial hydrology -> erosion/deposition -> final hydrology` without publishing the first pass.

**Science/product position:** Priority-Flood, a stable single-flow drainage DAG, discharge/slope/resistance stream-power response, and conservative sediment routing remain the efficient current-state formation model. The sphere changes physical measures and terminal semantics, not the causal model. Ocean cells are base-level outlets. If a planet has no ocean, connected local-minimum plateaus become explicit endorheic terminals instead of inventing a map edge or collapsing the whole sphere into a hidden single outlet. Because S0B.5 has no calibrated lake evaporation, groundwater balance, or elapsed formation time, it does not fabricate an equilibrium lake level for those all-land terminals; they are published honestly as `ClosedSink` basins. A future time-evolution or richer lake-balance solver may replace the internal operator while preserving the same present-state ownership and stable downstream semantics.

**Scientific references:**

- Barnes, Lehman, and Mulla, “Priority-Flood: An Optimal Depression-Filling and Watershed-Labeling Algorithm for Digital Elevation Models,” *Computers & Geosciences* 62 (2014), DOI `10.1016/j.cageo.2013.04.024`: deterministic graph-compatible depression filling with `O(n log n)` floating-height behavior.
- Barnes, Lehman, and Mulla, “An Efficient Assignment of Drainage Direction Over Flat Surfaces in Raster Digital Elevation Models,” *Computers & Geosciences* 62 (2014), DOI `10.1016/j.cageo.2013.01.009`: stable flat routing; Sekai uses Priority-Flood rank on the irregular Voronoi graph to obtain a strict DAG.
- O'Callaghan and Mark, “The Extraction of Drainage Networks from Digital Elevation Data,” *Computer Vision, Graphics, and Image Processing* 28 (1984), DOI `10.1016/S0734-189X(84)80011-0`: interpretable steepest single-flow drainage.
- Whipple and Tucker, “Dynamics of the Stream-Power River Incision Model,” *JGR* 104 (1999), DOI `10.1029/1999JB900120`: first-order discharge/slope control of fluvial incision.
- Davy and Lague, “Fluvial Erosion/Transport Equation of Landscape Evolution Models Revisited,” *JGR Earth Surface* 114 (2009), DOI `10.1029/2008JF001146`: explicit erosion, transport, and deposition accounting.

**Tech stack:** Rust 1.97, serde/serde_json, the authoritative geodesic Voronoi surface, existing `NaturalSurface`, `NaturalTopologyIndex`, `HydroErosionSpec`, spherical relief/geology/preliminary climate V2 snapshots, and existing typed natural fields; no new dependencies.

## Global constraints

- Existing planar `HydrologySnapshot`, `SurfaceProcessSnapshot`, and `HydroErosionSnapshot` V1 JSON, accepted legacy input, fields, hashes, stage behavior, cache behavior, and display goldens remain unchanged.
- New spherical snapshots use schema V2, exact content-fingerprinted spherical `SurfaceRef`, strict unknown-field rejection, streaming allocation limits, private immutable fields, and zero-copy semantic getters. Equal counts never imply identity compatibility.
- Reuse `SurfaceWaterKind`, `SurfaceWaterField`, `StrahlerOrderField`, `DrainageBasin`, `Lake`, and `RiverSegment` as the single semantic record definitions. New strict V2 wire adapters reconstruct those records without making permissive V1 decoding retroactively strict.
- Store no cells, edges, adjacency, projection coordinates, longitude/latitude, cubed-sphere faces, renderer data, or disposable flood/routing work arrays in natural snapshots.
- Ocean classification is still the formal current surface relative to the spherical relief sea level. Ocean cells have no receiver and no local runoff. There is no `ExternalBoundary` terminal class.
- Ocean-present worlds seed Priority-Flood with every ocean cell. Ocean-free closed worlds detect connected plateaus with no lower adjacent cell, choose the lowest `CellId` in each plateau as a terminal, and seed a deterministic multi-terminal endorheic solve. Every nonterminal receiver is an authoritative adjacent `CellId`, and the receiver graph is acyclic.
- Quantize elevations to centimeters before classification-sensitive routing. Keep the existing stable heap/rank/`CellId` ordering and serial upstream-to-downstream accumulation.
- Compute local and accumulated water from authoritative spherical cell areas. Compute slope and every published spherical river-segment length from authoritative center-to-center great-circle distance. Never use chord distance, edge polygon length as channel length, or a projected map coordinate.
- Keep effective runoff as the existing explicit precipitation/permeability proxy. Do not silently add soil, vegetation, snow, evapotranspiration, groundwater, or a fictitious elapsed duration.
- Keep incision bounded and quantized. Sediment volume is conserved as local deposition plus explicit transfer to either the ocean reservoir or an endorheic terminal reservoir. A closed planet has no field described as sediment leaving the planet.
- The final snapshot contains the post-erosion surface and only the second-pass hydrology. The first pass is private forcing and can never be observed as a competing truth.
- Build one `NaturalTopologyIndex` per public spherical combined generation and reuse it across both hydrology passes and erosion. Allocate `O(cells + edges)` work buffers outside inner loops; no per-cell topology clones.
- Do not add artifacts, stages, cache keys, field registrations, product controls, display adapters, renderer code, ecology, final climate, ocean circulation, or historical/time-series storage in S0B.5.

## Target file structure

```text
src/world/natural/
├── hydrology.rs                       # frozen V1 storage plus shared semantic validation
├── spherical_hydrology.rs             # strict surface-bound V2 hydrology contract
├── surface_process.rs                 # frozen V1 storage plus shared process validation
├── spherical_surface_process.rs       # strict V2 erosion/deposition and terminal ledger
├── hydro_erosion.rs                   # frozen planar V1 composite
└── spherical_hydro_erosion.rs         # strict atomic spherical V2 composite

src/generators/natural/
├── hydrology.rs                       # planar adapter plus one shared hydrology core
├── spherical_hydrology.rs             # spherical outlet/contract adapter
├── erosion.rs                         # planar adapter plus one shared erosion core
├── spherical_erosion.rs               # spherical metric/ledger adapter
├── hydro_erosion.rs                   # frozen planar orchestration
└── spherical_hydro_erosion.rs         # one-index atomic spherical orchestration

tests/
├── spherical_hydrology_contracts.rs
├── spherical_hydrology_generation.rs
├── spherical_hydro_erosion_contracts.rs
├── spherical_hydro_erosion_generation.rs
├── spherical_hydro_erosion_matrix.rs
└── spherical_hydro_erosion_performance.rs
```

---

### Task 1: Freeze planar behavior and expose only shared semantic views

**Files:**
- Modify: `src/world/natural/hydrology.rs`
- Modify: `src/world/natural/surface_process.rs`
- Test: existing planar hydrology/erosion contracts and generation tests

- [x] Add representative planar V1 byte/hash regressions before refactoring.
- [x] Extract narrow package-visible validation/data views for hydrology fields, topology-dependent metric identities, and surface-process component/mass identities. Preserve V1 struct layout, serialized field order, permissive decoding, error behavior, and formulas.
- [x] Generalize only metric reads to `NaturalSurface`; retain exact planar values through `PlanarNaturalSurface` adapter equivalence.
- [x] Run planar contracts, generation, stages, fields, displays, and goldens; commit `refactor: share hydro erosion semantics`.

### Task 2: Define the strict surface-bound V2 hydrology contract

**Files:**
- Create: `src/world/natural/spherical_hydrology.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `tests/spherical_hydrology_contracts.rs`

- [x] Write RED tests for schema/kind, exact `SurfaceRef`, all dense lengths/ranges/summaries, canonical receivers/basins/lakes/rivers, strict unknown fields at every nesting level, exact round trip, and equal-count different-surface rejection.
- [x] Stream-bound every dense and record sequence. Bound the aggregate count of all nested lake member cells, not merely each lake independently.
- [x] Reuse shared semantic records through strict private V2 wires; add an aligned `river_segment_length_m: Vec<f64>` derived from authoritative center distance.
- [x] Cross-validate receiver adjacency, discharge/area accumulation, lake area/volume, basin roots/outlet kinds, river receiver identity, and exact river length against the referenced sphere.
- [x] Keep all persistent data semantic and geometry-free; commit `feat: add spherical hydrology contracts`.

### Task 3: Refactor one hydrology core and implement closed-sphere outlets

**Files:**
- Modify: `src/generators/natural/hydrology.rs`
- Create: `src/generators/natural/spherical_hydrology.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_hydrology_generation.rs`

- [x] Write RED fixtures for ocean outlets, a topographic bowl and spill outlet, all-ocean safety, all-land multiple minima, a connected flat minimum plateau, receiver adjacency/DAG, and deterministic bytes.
- [x] Refactor Priority-Flood, receiver selection, lake routing, runoff, accumulation, basin labeling, and Strahler construction into one core over `NaturalSurface`, `NaturalTopologyIndex`, precipitation values, and an explicit outlet policy.
- [x] Preserve the planar no-ocean single-sink policy only inside the frozen planar adapter. Implement spherical all-land terminal detection by connected quantized local-minimum plateaus with stable representative IDs.
- [x] Build spherical lake/basin aggregates from true areas and river lengths/slopes from great-circle center distance. Validate the completed V2 snapshot against the exact surface.
- [x] Confirm no code path observes `boundary_cells` or creates an external outlet for the sphere; commit `feat: generate closed-sphere hydrology`.

### Task 4: Define the strict V2 surface-process and sediment-terminal ledger

**Files:**
- Create: `src/world/natural/spherical_surface_process.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `tests/spherical_hydro_erosion_contracts.rs`

- [x] Write RED tests for V2 schema/identity, bounded fields, surface component identity, ocean immutability, finite nonnegative sediment volumes, strict/bounded decode, and wrong-surface rejection.
- [x] Store erosion depth, deposition thickness, current surface elevation, and per-cell sediment throughput once. Store terminal transfer as two orthogonal totals: `sediment_ocean_delivery_m3` and `sediment_endorheic_storage_m3`.
- [x] Enforce `eroded volume = deposited volume + ocean delivery + endorheic storage` using authoritative spherical areas and compensated sums.
- [x] Do not reuse V1's “leaves the modeled world” wording for a closed planet; commit `feat: add spherical surface process contracts`.

### Task 5: Refactor one erosion core and route sediment on spherical metrics

**Files:**
- Modify: `src/generators/natural/erosion.rs`
- Create: `src/generators/natural/spherical_erosion.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_hydro_erosion_generation.rs`

- [x] Write RED tests that incision increases with discharge/slope and decreases with resistance, zero strength/flat flow produces zero incision, ocean cells remain unchanged, and low-energy/lake/terminal cells retain more sediment.
- [x] Refactor stream energy, bounded incision, topological routing, and deposition into one core over semantic relief/hydrology views and `NaturalSurface` metrics.
- [x] Use authoritative center distance for slope and true area for eroded/deposited volumes. Classify residual terminal sediment by first-pass basin terminal: ocean delivery versus endorheic storage.
- [x] Preserve the planar total `sediment_export_m3` byte-for-byte as the sum of terminal transfers in the V1 adapter.
- [x] Validate the spherical surface-process V2 snapshot after every generated current-state update; commit `feat: generate spherical fluvial erosion`.

### Task 6: Orchestrate one-index atomic spherical two-pass generation

**Files:**
- Create: `src/world/natural/spherical_hydro_erosion.rs`
- Create: `src/generators/natural/spherical_hydro_erosion.rs`
- Modify: both natural module exports
- Test: both spherical composite test files

- [x] Write RED composite tests for exact shared surface identity, upstream relief/geology/climate identity, runoff forcing identity, ocean/current-surface agreement, lake-depth identity, and rejection of mixed same-count surfaces.
- [x] Implement `HydroErosionGenerator::generate_spherical(surface, relief, geology, climate, spec)`: validate public inputs once, build one surface view/topology, run initial hydrology, erosion/deposition, final hydrology, and construct one atomic V2 output.
- [x] Ensure only final hydrology is stored; verify rerunning hydrology independently on the stored current surface reproduces it exactly.
- [x] Verify deterministic bytes, no upstream mutation, no projection dependency, and planar no drift; commit `feat: generate atomic spherical hydro erosion`.

### Task 7: Whole-slice scientific, compatibility, and performance gates

**Files:**
- Create: `tests/spherical_hydro_erosion_matrix.rs`
- Create: `tests/spherical_hydro_erosion_performance.rs`
- Modify: this plan with exact evidence

- [x] Run a deterministic matrix spanning minimum/Earth/maximum radii, coarse/medium meshes, all-land/all-ocean/mixed relief, wet/dry climate, hard/soft substrate, and hydro-erosion spec extremes. Freeze representative V2 hashes after review.
- [x] Confirm ocean outlet semantics, explicit multiple endorheic basins, flat-plateau determinism, receiver adjacency/DAG, true-area accumulation, exact geodesic river lengths, stream-power causal signs, sediment ledger closure, and final two-pass consistency.
- [x] Measure Release generation near 20,000 cells: total two-pass time, persistent V2 bytes, working-set delta, and diagnostics. Confirm `O(cells log cells + edges)` per hydrology pass and `O(cells + edges)` work memory with one topology index.
- [x] Audit ownership: no geometry duplication, external boundary, stage/artifact/UI publication, hidden first-pass truth, fake elapsed time, or history storage.
- [x] Run focused planar hydrology/erosion/stage/golden regressions, `cargo fmt --all -- --check`, all-target/all-feature Clippy with warnings denied, all-target/all-feature tests, and the all-feature WASM check.
- [x] Complete a fresh read-only scientific/code review, close all Critical/Important findings, rerun fresh gates, append exact evidence, and commit `docs: record spherical hydro erosion evidence`.
- [x] Fast-forward merge the reviewed branch into `main`, remove only the verified S0B.5 worktree/branch, then continue to S0B.6.

## Verification evidence (2026-08-04)

### Compatibility and deterministic identity

- Frozen planar Hydrology V1 BLAKE3: `5530d416b1c84d865610e463c4393e63cc41dfa01cdf1da869f25f5f489b0b6c`.
- Frozen planar Hydro-Erosion V1 BLAKE3: `59f982f1902fcaa81e601d91d270909526e0d9a5986a043c675f2924710b6b8f`.
- Frozen spherical V2 matrix hashes:
  - minimum radius, dry/hard, all land, 42 cells: `e43ff5de8494499e18e06246c6e00ef56cfa6ed39693404feff093a9cb00fd0b`;
  - Earth radius, wet/soft, all ocean, 42 cells: `291855a73f860fb82b7096fc2c3211bae8dcd772153312b8d11c690533e4b300`;
  - Earth radius, moderate mixed relief, 162 cells: `d23c5afaf6228c7e0745b49909a9723212f6e38d59bf57b8d1ac1c2db54b8eec`;
  - Earth radius, wet/soft mixed relief, 642 cells: `04560c868114798c95574583c2dea8c232130ea4d9c3db3169899e1c394ff930`;
  - maximum radius, wet/hard, all land, 162 cells: `2e1758ef9cd99f2f2c8bbac2017e607af7393ea7676021491fc3f2e8ebf486f0`.
- Focused planar contracts/generation/stages/rule stages and `natural_display_golden` passed; the reviewed PNGs did not change.

### Release performance

Command: `cargo test --release --test spherical_hydro_erosion_performance -- --ignored --nocapture`.

- authoritative sphere: 20,252 cells / 60,750 edges;
- fixed two-hydrology-pass plus erosion solve: `69.150 ms`;
- persistent V2 in-memory estimate: `3,508,248 bytes`;
- strict JSON V2 bytes: `5,515,076 bytes`;
- observed working-set delta: `3,739,648 bytes`;
- output diagnostics: 240 basins, 0 lakes, 9,342 published river reaches;
- budgets passed: 5,000 ms, 128 MiB persistent, 64 MiB serialized.

The implementation performs two `O(cells log cells + edges)` Priority-Flood passes and one `O(cells + edges)` erosion/routing pass. One `NaturalTopologyIndex` is shared by all three; work arrays remain linear in cells/edges.

### Full gates and ownership review

- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace --all-targets --all-features`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo test --workspace --all-targets --all-features`: passed; 226 library tests passed, one pre-existing large Voronoi stress test remained ignored, and every integration/binary/benchmark target passed.
- `cargo test --workspace --doc`: passed; eight pre-existing documentation examples remained ignored.
- `cargo check --workspace --all-features --lib --target wasm32-unknown-unknown`: passed.
- Ownership search and changed-file audit found no S0B.5 artifact, stage, cache, projection, field, app, UI, renderer, fake elapsed-time, or history publication. Only the final hydrology is serialized in the atomic output.
- Fresh scientific/code review found zero unresolved Critical or Important issues. The review explicitly checked centimeter-quantized sea-level classification against the shared `LandOceanKind` fact source; a spherical-only floating-point exception was rejected because it would create conflicting land/ocean semantics.

## Explicitly deferred

- S0B.6 artifacts, stage graph, cache/provenance publication, field adapters, compatibility loading, and app/UI exposure.
- Soil, vegetation, snow/ice, evapotranspiration, groundwater, dynamic lake water balance, flood events, channel width/depth hydraulics, sediment grain classes, delta/coastal morphology, and marine bathymetric deposition.
- A calibrated landscape-evolution timescale, implicit FastScape-style stepping, stored erosion history, or historical events. A later internal time-evolution solver still publishes only current state through the same ownership boundary.
- S0C spherical presentation, S0D climate remapping, C0-C4 atmosphere/ocean circulation, ENSO-like variability, cyclones, and final climate.

## Completion definition

S0B.5 is complete only when strict surface-bound V2 hydrology and current-surface snapshots generate directly from the authoritative sphere; ocean and all-land endorheic terminal semantics are explicit; all water and erosion measures use true spherical area and great-circle center distance; receiver, lake, basin, runoff, surface, and sediment identities cross-validate; the fixed two-pass result is atomic and deterministic; planar V1 output is frozen; performance and full repository gates pass; ownership/publication audits are clean; and a fresh read-only review reports no unresolved Critical or Important issue.
