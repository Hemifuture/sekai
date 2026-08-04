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

- [ ] Write a frozen planar V1 JSON/hash regression around representative monthly and annual fields.
- [ ] Add `PRELIMINARY_CLIMATE_SCHEMA_V2`, `MonthlyVector3Field`, and narrow package-visible scalar/monthly validation helpers. Do not make planar V1 strict or bounded retroactively.
- [ ] Reuse the existing `ClimateValidationError` for geometry-independent finite/range/summary failures; keep spherical identity and tangency errors in a separate type.
- [ ] Run all planar climate contracts/generation/stage/golden tests and commit `refactor: share preliminary climate field semantics`.

### Task 2: Define the strict surface-bound V2 climate contract

**Files:**
- Create: `src/world/natural/spherical_climate.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `tests/spherical_climate_contracts.rs`

- [ ] Write RED tests for V2 schema, exact `SurfaceRef`, every dense length/range, monthly/annual identities, strict unknown-field rejection, exact round trip, and equal-count different-surface rejection.
- [ ] Write streaming maximum and maximum-plus-one allocation tests for latitude, maritime, both monthly scalar fields, monthly vector3, every scalar summary, and prevailing vector3.
- [ ] Cross-validate latitude against `asin(radial.z)`, wind tangency against authoritative radials, ocean maritime identity, all-land/all-ocean behavior, and exact spherical relief compatibility.
- [ ] Keep all fields private and immutable; expose zero-copy slices and typed cell/month getters. Store no surface cells, edges, adjacency, bases, fluxes, or renderer state.
- [ ] Verify planar V1 decoding remains unchanged and commit `feat: add spherical preliminary climate contracts`.

### Task 3: Freeze and expose only shared physical formulas

**Files:**
- Modify: `src/generators/natural/climate.rs`
- Test: existing planar climate tests

- [ ] Make only the existing lapse rate, monthly declination, daily-mean insolation, annual sea-level temperature, circulation-band components, and evaporation response package-visible.
- [ ] Preserve every expression, constant, call order, grid algorithm, and planar result. Do not introduce a universal climate context or make rectangular-grid helpers shared.
- [ ] Add focused hemisphere/insolation and planar no-drift checks; commit with the first spherical generator slice that consumes these helpers.

### Task 4: Generate spherical latitude, maritime influence, temperature, and tangent wind

**Files:**
- Create: `src/generators/natural/spherical_climate.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_climate_generation.rs`

- [ ] Write RED tests that latitude is exactly derived from unit radials, full-sphere extent reaches both polar bands, and northern/southern seasonal phases reverse.
- [ ] Write RED wind tests for low-latitude easterlies, midlatitude westerlies, monthly band migration, finite speed bounds, strict cell tangency, ordinary pole/cut statistics, and zero longitude/projection dependencies.
- [ ] Write RED maritime tests for all-ocean `1`, all-land `0`, ocean cells `1`, monotone graph-distance decay, and no exposed-boundary artifact.
- [ ] Derive local east/north only as disposable three-dimensional tangent bases around canonical `+Z`; use a smooth zero-speed limit where zonal direction is undefined at the exact poles.
- [ ] Reuse shared monthly insolation and thermal formulas, apply authoritative local elevation lapse correction directly per cell, and allocate monthly fields once.

### Task 5: Implement paired monotone spherical moisture transport

**Files:**
- Create: `src/generators/natural/spherical_moisture.rs`
- Test: module unit tests
- Test: `tests/spherical_climate_generation.rs`

- [ ] Write RED operator tests showing one shared edge is applied once, donor loss equals receiver gain plus explicit condensation, global vapor-plus-condensate closes, reversing owner labels preserves physics, and no supported outgoing fan can make vapor negative.
- [ ] Precompute monthly edge-normal speed from an analytic tangent wind at each authoritative edge midpoint. Use `speed × shared-edge length` as conductance and cell area for a local outgoing CFL limiter.
- [ ] Use first-order donor upwinding. Keep f64 transport mass internally, reuse vapor/delta/outgoing/condensate/flow buffers, and quantize only published f32 climate fields.
- [ ] Make ocean evaporation and land recycling explicit sources. Make background/convective and positive upwind elevation change explicit condensation; do not hide loss at an external boundary or in an unexplained decay factor.
- [ ] Write causal tests for warm-ocean moisture supply, moisture-scale response, windward enhancement, downstream rain shadow, nonnegative vapor/precipitation, and annual bounds.

### Task 6: Orchestrate and validate spherical V2 generation

**Files:**
- Modify: `src/generators/natural/spherical_climate.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_climate_generation.rs`

- [ ] Implement `ClimateGenerator::generate_spherical(surface, relief, spec)`: validate each public input once, build one topology index, derive monthly forcing, compute exact summaries, construct V2, and cross-validate the completed snapshot.
- [ ] Reject same-count wrong-surface relief and malformed specs before generating work arrays.
- [ ] Verify deterministic byte repeatability, spec sensitivity, several radii/resolutions, all-land/all-ocean safety, no mutation of upstreams, and planar no drift.
- [ ] Commit `feat: generate spherical preliminary climate`.

### Task 7: Whole-slice scientific, ownership, and performance gates

**Files:**
- Create: `tests/spherical_climate_matrix.rs`
- Create: `tests/spherical_climate_performance.rs`
- Modify: this plan with exact evidence

- [ ] Run a deterministic matrix spanning minimum/Earth/maximum radii, coarse/medium meshes, climate-spec extremes, ocean/continent mixtures, and multiple upstream seeds. Freeze representative V2 hashes after review.
- [ ] Confirm latitude/temperature/season signs, tangent wind, maritime causality, moisture nonnegativity, paired edge budgets, windward/rain-shadow response, and no seam/pole statistical outlier.
- [ ] Measure Release generation near 20,000 cells: total time, per-month transport time if practical, persistent V2 bytes, peak/working-set delta, and diagnostics. Confirm O(months × iterations × edges) time and O(cells + edges) work memory.
- [ ] Audit that S0B.4 owns no geometry, does not consume geology/circulation/UI state, and is not published through a half-migrated stage or field path.
- [ ] Run focused planar climate/golden regressions, `cargo fmt --all -- --check`, all-target/all-feature Clippy with warnings denied, all-target/all-feature tests, and the all-feature WASM check.
- [ ] Complete a fresh read-only scientific/code review, close all Critical/Important findings, rerun fresh gates, append exact evidence, and commit `docs: record spherical preliminary climate evidence`.
- [ ] Fast-forward merge the reviewed branch into `main`, remove only the verified S0B.4 worktree/branch, then continue to S0B.5.

## Explicitly deferred

- S0B.5 closed-sphere hydrology and current-state erosion stepping.
- S0B.6 artifacts, stage graph, cache/provenance publication, field adapters, compatibility loading, and app/UI exposure.
- S0C spherical presentation and S0D conservative remapping between authoritative surface fields and circulation grids.
- C0–C4 selected transient layered atmosphere/ocean circulation, coupled thermodynamics, ENSO-like variability, cyclones, ocean heat transport, and final-climate replacement fields.
- Higher-order tracer reconstruction, Runge–Kutta weather integration, vertical atmospheric layers, explicit cloud microphysics, sea ice, snow, vegetation feedback, and stored climate history.

## Completion definition

S0B.4 is complete only when surface-bound V2 preliminary climate generates directly from the authoritative sphere; the snapshot is exact-identity-bound, strictly decoded, and geometry-free; latitude and wind use real spherical frames; moisture transfer is paired, conservative apart from explicit sources/sinks, and positivity-preserving; monthly fields and summaries remain bounded and causal; planar V1 output is frozen; performance and full repository gates pass; ownership/publication audits are clean; and a fresh read-only review reports no unresolved Critical or Important issue.
