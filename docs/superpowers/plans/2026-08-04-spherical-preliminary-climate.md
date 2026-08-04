# Spherical Preliminary Climate Forcing (S0B.4) Implementation Plan

> **Execution rule:** implement task-by-task with witnessed RED tests, the smallest GREEN change, focused planar regressions, and task-scoped commits. Do not publish the spherical path to artifacts, the production stage graph, fields, app, or UI before S0B.6.

**Goal:** Generate deterministic monthly temperature, precipitation, maritime influence, and three-dimensional tangent prevailing wind directly on the authoritative closed spherical surface, so S0B.5 hydrology has scientifically causal current-slice forcing without pretending to be the final layered atmosphere/ocean model.

**Architecture:** Keep `SphericalSurfaceSnapshot` as the sole geometry/topology owner. Add a strict surface-bound V2 climate snapshot containing only semantic cell fields and the exact `SurfaceRef`. Reuse the existing geometry-independent insolation, thermal, circulation-band, evaporation, and summary formulas without changing planar V1 behavior. Put spherical frame construction and a monotone edge-flux moisture relaxation behind sphere-only modules. Each undirected edge is visited once and produces one paired finite-volume transfer; explicit condensation is the only transport sink and ocean/land recycling is an explicit source.

**Science/product position:** This remains preliminary climatological forcing for current-state hydrology. It is not archived weather, a shallow-water dynamical core, the already selected time-evolution circulation solver, ENSO, cyclones, or ocean circulation. Spherical Voronoi C-grid methods place scalar state in cells and normal flux on shared edges; conservative tracer schemes add limiters to retain monotonicity. S0B.4 deliberately uses a first-order upwind donor flux with a local outgoing limiter: it is more diffusive than higher-order atmospheric transport, but is positivity-preserving, explainable, deterministic, and proportionate for twelve monthly hydrology inputs. The V2 semantic contract lets a richer internal solver replace it later without changing field ownership.

**Scientific references:**

- Ringler, Thuburn, Klemp, and Skamarock, “A unified approach to energy conservation and potential vorticity dynamics for arbitrarily-structured C-grids,” *JCP* 229 (2010), DOI `10.1016/j.jcp.2009.12.007`: spherical Voronoi cells with shared edge-normal fluxes.
- Skamarock and Gassmann, “Conservative Transport Schemes for Spherical Geodesic Grids,” *MWR* 139 (2011), DOI `10.1175/MWR-D-10-05056.1`: conservative finite-volume tracer transport and positive/monotone limiting on irregular spherical Voronoi meshes.
- NASA’s Earth energy-budget reference and the existing approved preliminary-climate design supply the latitude/season insolation basis; NOAA references in that design supply circulation bands, lapse-rate, maritime, and orographic-rainfall approximations.

**Tech stack:** Rust 1.85, serde/serde_json, the existing authoritative geodesic Voronoi surface, `NaturalTopologyIndex`, existing fixed-point `ClimateSpec`, and existing spatial vector primitives; no new dependencies.

## Global constraints

- Add `PRELIMINARY_CLIMATE_SCHEMA_V2 = 2` only for the surface-bound spherical snapshot. Existing planar `PreliminaryClimateSnapshot` V1 JSON, accepted legacy input, fields, random-independent output, artifacts, stage graph, cache behavior, and goldens remain unchanged.
- The full sphere always derives latitude from authoritative unit radials and the canonical `+Z` planetary spin axis used by the existing circulation fixtures. `ClimateSpec.south_latitude_centideg` and `north_latitude_centideg` remain planar map-extent controls and do not crop or remap a closed sphere. Axial tilt, temperature offset, and moisture scale remain shared author inputs.
- Store spherical wind as global `[f32; 3]` vectors. Every monthly and annual vector must be finite, speed-bounded, and tangent to the authoritative cell radial within a strict floating-point tolerance. Do not store longitude, local east/north bases, projection coordinates, cubed-sphere faces, or edge flux work arrays.
- The new snapshot carries the exact content-fingerprinted spherical `SurfaceRef`; equal cell/edge counts never imply identity compatibility. Every new dense sequence is stream-bounded before oversized allocation, and new wires reject unknown fields.
- Every published annual summary is derived from and revalidated against the twelve monthly values. Temperature and precipitation use the existing units and envelopes; precipitation and internal vapor are nonnegative.
- Maritime influence is `1` on ocean cells, decays by closed-surface graph distance onto land, returns exactly `1` for all-ocean worlds and exactly `0` for all-land worlds, and never references a map boundary.
- Each shared edge produces at most one directed upwind vapor transfer per relaxation step. Donor loss, receiver gain, and explicit condensed mass must close as one paired budget. Per-donor outgoing limiting must prevent negative vapor for arbitrary supported meshes and radii.
- Edge transport uses authoritative shared-edge length, cell area, edge midpoint, and `normal_from_first`. Orographic condensation uses receiver-minus-donor elevation along the actual upwind edge. There is no rectangular grid, bilinear map sampling, longitude seam, pole branch, or external moisture boundary.
- Build one disposable `NaturalTopologyIndex` at most once per public spherical generation. Allocate all O(C/E) work buffers outside month/iteration inner loops and reuse them.
- Do not add artifacts, stages, cache keys, field registrations, display adapters, product controls, hydrology, erosion, final-climate fields, circulation-grid remapping, or renderer code in S0B.4.

## Target file structure

```text
src/world/natural/
├── climate.rs                         # frozen planar V1 plus shared scalar/monthly semantics
└── spherical_climate.rs               # strict surface-bound V2 climate contract

src/generators/natural/
├── climate.rs                         # frozen planar adapter plus shared physical functions
├── spherical_moisture.rs              # paired monotone cell-edge vapor transport
└── spherical_climate.rs               # spherical frame, temperature, wind, maritime, orchestration

tests/
├── spherical_climate_contracts.rs
├── spherical_climate_generation.rs
├── spherical_climate_matrix.rs
└── spherical_climate_performance.rs
```

---

### Task 1: Share climate field validation without planar drift

**Files:**
- Modify: `src/world/natural/climate.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `tests/climate_contracts.rs`
- Test: `tests/climate_generation.rs`

- [x] Write a frozen planar V1 JSON/hash regression around representative monthly and annual fields.
- [x] Add `PRELIMINARY_CLIMATE_SCHEMA_V2`, `MonthlyVector3Field`, and narrow package-visible scalar/monthly validation helpers. Do not make planar V1 strict or bounded retroactively.
- [x] Reuse the existing `ClimateValidationError` for geometry-independent finite/range/summary failures; keep spherical identity and tangency errors in a separate type.
- [x] Run all planar climate contracts/generation/stage/golden tests and commit `refactor: share preliminary climate field semantics`.

### Task 2: Define the strict surface-bound V2 climate contract

**Files:**
- Create: `src/world/natural/spherical_climate.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `tests/spherical_climate_contracts.rs`

- [x] Write RED tests for V2 schema, exact `SurfaceRef`, every dense length/range, monthly/annual identities, strict unknown-field rejection, exact round trip, and equal-count different-surface rejection.
- [x] Write streaming maximum and maximum-plus-one allocation tests for latitude, maritime, both monthly scalar fields, monthly vector3, every scalar summary, and prevailing vector3.
- [x] Cross-validate latitude against `asin(radial.z)`, wind tangency against authoritative radials, ocean maritime identity, all-land/all-ocean behavior, and exact spherical relief compatibility.
- [x] Keep all fields private and immutable; expose zero-copy slices and typed cell/month getters. Store no surface cells, edges, adjacency, bases, fluxes, or renderer state.
- [x] Verify planar V1 decoding remains unchanged and commit `feat: add spherical preliminary climate contracts`.

### Task 3: Freeze and expose only shared physical formulas

**Files:**
- Modify: `src/generators/natural/climate.rs`
- Test: existing planar climate tests

- [x] Make only the existing lapse rate, monthly declination, daily-mean insolation, annual sea-level temperature, circulation-band components, and evaporation response package-visible.
- [x] Preserve every expression, constant, call order, grid algorithm, and planar result. Do not introduce a universal climate context or make rectangular-grid helpers shared.
- [x] Add focused hemisphere/insolation and planar no-drift checks; commit with the first spherical generator slice that consumes these helpers.

### Task 4: Generate spherical latitude, maritime influence, temperature, and tangent wind

**Files:**
- Create: `src/generators/natural/spherical_climate.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_climate_generation.rs`

- [x] Write RED tests that latitude is exactly derived from unit radials, full-sphere extent reaches both polar bands, and northern/southern seasonal phases reverse.
- [x] Write RED wind tests for low-latitude easterlies, midlatitude westerlies, monthly band migration, finite speed bounds, strict cell tangency, ordinary pole/cut statistics, and zero longitude/projection dependencies.
- [x] Write RED maritime tests for all-ocean `1`, all-land `0`, ocean cells `1`, monotone graph-distance decay, and no exposed-boundary artifact.
- [x] Derive local east/north only as disposable three-dimensional tangent bases around canonical `+Z`; use a smooth zero-speed limit where zonal direction is undefined at the exact poles.
- [x] Reuse shared monthly insolation and thermal formulas, apply authoritative local elevation lapse correction directly per cell, and allocate monthly fields once.

### Task 5: Implement paired monotone spherical moisture transport

**Files:**
- Create: `src/generators/natural/spherical_moisture.rs`
- Test: module unit tests
- Test: `tests/spherical_climate_generation.rs`

- [x] Write RED operator tests showing one shared edge is applied once, donor loss equals receiver gain plus explicit condensation, global vapor-plus-condensate closes, reversing owner labels preserves physics, and no supported outgoing fan can make vapor negative.
- [x] Precompute monthly edge-normal speed from an analytic tangent wind at each authoritative edge midpoint. Use `speed × shared-edge length` as conductance and cell area for a local outgoing CFL limiter.
- [x] Use first-order donor upwinding. Keep f64 transport mass internally, reuse vapor/delta/outgoing/condensate/flow buffers, and quantize only published f32 climate fields.
- [x] Make ocean evaporation and land recycling explicit sources. Make background/convective and positive upwind elevation change explicit condensation; do not hide loss at an external boundary or in an unexplained decay factor.
- [x] Write causal tests for warm-ocean moisture supply, moisture-scale response, windward enhancement, downstream rain shadow, nonnegative vapor/precipitation, and annual bounds.

### Task 6: Orchestrate and validate spherical V2 generation

**Files:**
- Modify: `src/generators/natural/spherical_climate.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_climate_generation.rs`

- [x] Implement `ClimateGenerator::generate_spherical(surface, relief, spec)`: validate each public input once, build one topology index, derive monthly forcing, compute exact summaries, construct V2, and cross-validate the completed snapshot.
- [x] Reject same-count wrong-surface relief and malformed specs before generating work arrays.
- [x] Verify deterministic byte repeatability, spec sensitivity, several radii/resolutions, all-land/all-ocean safety, no mutation of upstreams, and planar no drift.
- [x] Commit the generator in reviewable thermal/wind and conservative-moisture slices.

### Task 7: Whole-slice scientific, ownership, and performance gates

**Files:**
- Create: `tests/spherical_climate_matrix.rs`
- Create: `tests/spherical_climate_performance.rs`
- Modify: this plan with exact evidence

- [x] Run a deterministic matrix spanning minimum/Earth/maximum radii, coarse/medium meshes, climate-spec extremes, and distinct all-land/all-ocean/continent/mountain upstream relief fields. S0B.4 is deliberately RNG-free, so exact upstream fields replace meaningless seed variation. Freeze representative V2 hashes after review.
- [x] Confirm latitude/temperature/season signs, tangent wind, maritime causality, moisture nonnegativity, paired edge budgets, windward/rain-shadow response, and no seam/pole statistical outlier.
- [x] Measure Release generation near 20,000 cells: total time, persistent V2 bytes, working-set delta, and diagnostics. Confirm O(months × iterations × edges) time and O(cells + edges) work memory.
- [x] Audit that S0B.4 owns no geometry, does not consume geology/circulation/UI state, and is not published through a half-migrated stage or field path.
- [x] Run focused planar climate/golden regressions, `cargo fmt --all -- --check`, all-target/all-feature Clippy with warnings denied, all-target/all-feature tests, and the all-feature WASM check.
- [x] Complete a fresh read-only scientific/code review, close all Critical/Important findings, rerun fresh gates, append exact evidence, and commit `docs: record spherical preliminary climate evidence`.
- [x] Fast-forward merge the reviewed branch into `main`, remove only the verified S0B.4 worktree/branch, then continue to S0B.5.

## Verification evidence (2026-08-04)

- Planar compatibility: the representative V1 snapshot remains byte-stable at BLAKE3 `a6f42228c9e520fdaa83cf9d2757178b2a8d103578127a46757131d812d88862`; planar contract, generation, stage, field, display, and golden coverage passed unchanged.
- Strict V2 contract: schema/kind/range/length/summary rejection, unknown-field rejection, bounded streaming decode, exact `SurfaceRef`, equal-count/different-surface rejection, latitude identity, monthly and annual wind tangency, all-land/all-ocean maritime identities, exact round trip, and zero-copy access all passed.
- Scientific properties: the tests witness reversed hemispheric seasons, low-latitude easterlies, midlatitude westerlies, monthly band migration, maritime seasonal moderation and graph-distance decay, explicit warm-ocean supply, moisture-scale response, positive-upwind orographic condensation, downstream rain shadow, nonnegative vapor/precipitation, paired transport closure, arbitrary-fan positivity, and ordinary cut/pole statistics.
- Frozen V2 matrix hashes:
  - minimum radius / 42 cells / cold all-land: `f00d5b7e0768597c4f0112c69940ddd6378945a9a0acb5d7640cbf0a5bc3a6b6`
  - regional radius / 92 cells / high-tilt all-ocean: `7835c9d0a6107c12220e3878b26a059afdf8ea7ab04ab955de856b2d6075b185`
  - Earth radius / 162 cells / mixed continents: `43f5ad23c89ee39cbe7142e6aab0f6b4d6003308f467e9055e0cdc37ea8fc532`
  - maximum radius / 642 cells / mountain arc: `aa8efb1b8d4ba180bb8faf451d013f732c7c86afb178fa1e90530dd1ef873821`
- Release performance at 20,252 cells and 60,750 shared edges: `133.604 ms` total for all twelve months, `5,508,808 bytes` of complete persistent V2 storage, and `5,169,152 bytes` final working-set delta on the final reference Windows run. The measured loop is structurally `O(12 × (edges + 48 × edges))`; reusable work storage is `O(cells + edges)`. Per-month transport was not separately instrumented because the whole-slice time is already small and instrumentation would perturb the short measurement.
- Ownership audit: the snapshot stores only semantic dense fields plus one exact `SurfaceRef`; it stores no cells, edges, adjacency, bases, flux work arrays, projection, stage, artifact, cache, renderer, or UI state. The generator consumes only the authoritative sphere, spherical relief, and shared `ClimateSpec`; it builds one disposable topology index and does not consume geology or the transient circulation solver.
- Review corrections: the outgoing limiter was changed to preserve ordinary wind-speed response below its CFL ceiling; condensation now uses the receiver cell's authoritative latitude while edge wind remains evaluated at the edge midpoint; the performance byte count includes the complete snapshot header and identity. No Critical or Important finding remains.
- Fresh commands exited `0`: `cargo fmt --all -- --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo test --all-targets --all-features` (225-test library batch with one pre-existing ignored stress test, plus all integration/binary/bench targets); `cargo check --all-features --lib --target wasm32-unknown-unknown`; and the ignored Release performance gate invoked explicitly.
- Integration: reviewed head `615f381` was fast-forwarded into `main`; the clean `spherical-preliminary-climate` worktree and merged feature branch were then removed.

## Explicitly deferred

- S0B.5 closed-sphere hydrology and current-state erosion stepping.
- S0B.6 artifacts, stage graph, cache/provenance publication, field adapters, compatibility loading, and app/UI exposure.
- S0C spherical presentation and S0D conservative remapping between authoritative surface fields and circulation grids.
- C0–C4 selected transient layered atmosphere/ocean circulation, coupled thermodynamics, ENSO-like variability, cyclones, ocean heat transport, and final-climate replacement fields.
- Higher-order tracer reconstruction, Runge–Kutta weather integration, vertical atmospheric layers, explicit cloud microphysics, sea ice, snow, vegetation feedback, and stored climate history.

## Completion definition

S0B.4 is complete only when surface-bound V2 preliminary climate generates directly from the authoritative sphere; the snapshot is exact-identity-bound, strictly decoded, and geometry-free; latitude and wind use real spherical frames; moisture transfer is paired, conservative apart from explicit sources/sinks, and positivity-preserving; monthly fields and summaries remain bounded and causal; planar V1 output is frozen; performance and full repository gates pass; ownership/publication audits are clean; and a fresh read-only review reports no unresolved Critical or Important issue.
