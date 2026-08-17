# P4 Global Atmosphere-Ocean Implementation Plan

Date: 2026-08-17  
Design: `docs/superpowers/specs/2026-08-17-global-atmosphere-ocean-p4-design.md`

## Task 1: Publish the cubed-sphere as a reconstructable spherical work domain

Files:

- Modify: `src/generators/natural/circulation/grid.rs`
- Create: `src/world/natural/global_circulation.rs`
- Create: `src/generators/natural/climate_work_domain.rs`
- Create: `tests/climate_work_domain.rs`

- [x] Write RED tests for lossless cubed-sphere conversion, exact topology and
  area identity, stable fingerprints, profile resolutions, forward/reverse
  conservative overlaps, bounded allocation, cancellation, and determinism.
- [x] Implement `ClimateWorkDomainSnapshot` and the cancellable builder.
- [x] Verify Draft/Standard/High domains and commit.

## Task 2: Define strict C0 layer, capability, checkpoint, and public schemas

Files:

- Extend: `src/world/natural/global_circulation.rs`
- Create: `tests/global_circulation_contracts.rs`

- [x] Write RED tests for fixed C1/C2 layouts, integrator identity, capability
  tri-state, checkpoint fingerprints, strict serde, bounded monthly arrays,
  vector tangency, monthly identities, and surface mismatch.
- [x] Implement `ClimateModelProfile`, `ClimateLayerLayout`,
  `ProductionIntegratorId`, `ClimateCheckpoint`, capabilities, solve/budget
  reports, and `GlobalCirculationSnapshot`.
- [x] Run focused tests and strict Clippy; commit.

## Task 3: Build exact P3-derived climate forcing and reverse projection

Files:

- Create: `src/generators/natural/global_circulation/forcing.rs`
- Create: `src/generators/natural/global_circulation/project.rs`
- Create: `tests/global_circulation_forcing.rs`

- [x] Write RED tests for physical land/bathymetry causality, constant and
  bounded intensive remap, conservative precipitation projection, tangent
  vector transport, axial-tilt phase, mountain response, and wrong-input
  rejection.
- [x] Implement one forcing builder and one semantic diagnostic projector.
- [x] Prove no preliminary-climate or renderer dependency; commit.

## Task 4: Implement reusable layered state and paired C2 physics

Files:

- Create: `src/generators/natural/global_circulation/state.rs`
- Create: `src/generators/natural/global_circulation/tendency.rs`
- Create: `tests/layered_circulation_physics.rs`

- [x] Write RED analytic tests for fixed layer roles, tangent dynamics, paired
  momentum/heat/moisture exchange, positive layer depth, deep-reservoir
  timescale, coastal permeability, and complete budget accounting.
- [x] Implement the shared C1/C2 tendency system without integrator branching.
- [x] Add cancellation polls and reusable workspace; commit.

## Task 5: Add monotone second-order conservative transport

Files:

- Modify: `src/generators/natural/circulation/operators.rs`
- Create: `tests/circulation_second_order_transport.rs`

- [x] Write RED tests for linear-field accuracy, extrema preservation,
  positivity, paired flux closure, seam invariance, reversal symmetry, and
  zero per-step cell-sized allocation with a supplied workspace.
- [x] Implement piecewise-linear reconstruction, stable limiter, and outgoing
  positivity scaling while retaining first-order reference APIs.
- [x] Run all existing operator/transient regressions; commit.

## Task 6: Implement and compare RK3, IMEX, and split-explicit integrators

Files:

- Create: `src/generators/natural/global_circulation/rk3.rs`
- Create: `src/generators/natural/global_circulation/imex.rs`
- Create: `src/generators/natural/global_circulation/split_explicit.rs`
- Create: `src/generators/natural/global_circulation/comparison.rs`
- Create: `tests/global_circulation_integrators.rs`
- Create: `tests/global_circulation_comparison.rs`

- [x] Write RED equilibrium, convergence-order, stability, linear-residual,
  deterministic, cancellation, and known artificial-comparison tests.
- [x] Implement the same-equation explicit reference and both production
  candidates with fixed work budgets.
- [x] Run Release comparisons on all approved fixtures; select only a candidate
  passing every locked metric. Do not relax thresholds.
- [x] Record candidate failures and commit the winner as the only product path.

## Task 7: Generate and validate C2 seasonal circulation

Files:

- Create: `src/generators/natural/global_circulation/mod.rs`
- Create: `src/generators/natural/quality/global_circulation.rs`
- Create: `tests/global_circulation_generation.rs`
- Create: `tests/global_circulation_quality.rs`

- [ ] Write RED tests for all public semantic fields, component identities,
  formation convergence, wind belts, vertical shear, basin gyres, thermocline,
  humidity/precipitation causality, cross-resolution statistics, unavailable
  C3/C4 capabilities, determinism, and cancellation.
- [ ] Implement the bounded annual/monthly formation driver and quality report.
- [ ] Tune declared physical/numerical constants only; commit.

## Task 8: Publish typed stages and isolated P4 graph

Files:

- Create: `src/generators/natural/global_circulation_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/global_circulation_stage.rs`

- [ ] Write RED tests for keys, stage identities, exact dependencies, cache
  restore/selective invalidation, malformed inputs, cancellation, and atomic
  publication.
- [ ] Add `ClimateWorkDomainArtifact`, `GlobalCirculationArtifact`, both stages,
  and `global_circulation_graph` extending P3.
- [ ] Verify V4 and P2/P3 graphs/hashes remain unchanged; commit.

## Task 9: Freeze evidence, atlas, performance, and P4 completion

Files:

- Create: `tests/global_circulation_evidence.rs`
- Create: `tests/global_circulation_atlas.rs`
- Create: `tests/global_circulation_performance.rs`
- Create: `docs/superpowers/specs/2026-08-17-global-atmosphere-ocean-p4-completion.md`
- Modify: this plan

- [ ] Write deterministic Release JSON/CSV for fixtures and 17 P3 seeds.
- [ ] Render fixed map/globe rows for lower/upper wind, shear, surface current,
  SST, thermocline, humidity, precipitation, and solver/remap diagnostics.
- [ ] Measure C1 n24, C2 n32/n48, memory, cancellation, and cold/cache behavior.
- [ ] Inspect seeds 42, 43, and 83 and fix every severe artifact.
- [ ] Run fmt, all-target/all-feature check/test/Clippy, WASM, focused/adjacent
  suites, frozen P0/P2/P3 evidence, and all P4 Release writers.
- [ ] Record integrator decision, hashes, metrics, timing, limitations, and P5
  handoff; check all boxes and commit.
