# P5 Coupled Geomorphic Formation Implementation Plan

Date: 2026-08-18  
Design: `docs/superpowers/specs/2026-08-18-coupled-geomorphic-formation-p5-design.md`

## Task 1: Freeze the P5 model, schemas, and identities

Files:

- Create: `src/world/natural/surface_formation.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/surface_formation_contracts.rs`

- [x] Write RED tests for strict bounded serde, component identity, checkpoint
  and model fingerprints, capabilities, reports, water/sediment bounds, wrong
  upstream identity, and malformed dense allocation.
- [x] Implement the V1 checkpoint, terrain/process fields, sediment provenance,
  solve/budget reports, and snapshot validation.
- [x] Verify focused contracts and commit.

## Task 2: Adapt P4 forcing to a validated formation terrain

Files:

- Modify: `src/generators/natural/global_circulation/forcing.rs`
- Modify: `src/generators/natural/global_circulation/generation.rs`
- Create: `tests/formation_climate_coupling.rs`

- [x] Write RED tests proving unchanged P3 terrain reproduces exact P4 forcing,
  terrain-only changes alter forcing/checkpoint, malformed terrain is rejected,
  and no preliminary/preview climate path exists.
- [x] Add a crate-private validated formation-terrain forcing boundary and reuse
  the selected P4 generator without weakening the public P4 relief identity.
- [x] Add cancellation and deterministic fingerprint coverage; commit.

## Task 3: Generalize spherical Priority-Flood hydrology for P4 rates

Files:

- Modify: `src/generators/natural/hydrology.rs`
- Modify: `src/generators/natural/spherical_hydrology.rs`
- Create: `src/generators/natural/surface_formation/hydrology.rs`
- Create: `tests/formation_hydrology.rs`

- [x] Write RED irregular-graph tests for ocean outlets, insignificant pits,
  spill lakes, residence-horizon closed sinks, flats, DAG ordering, monthly
  rate conversion, discharge/area closure, stable IDs, and cancellation.
- [x] Reuse the validated Priority-Flood/river core, add explicit P4 `mm/day`
  forcing and endorheic classification, and publish final V2 spherical
  hydrology without exposing an intermediate pass.
- [x] Run every legacy spherical hydrology regression unchanged; commit.

## Task 4: Implement the implicit tectonic-stream-power kernel

Files:

- Create: `src/generators/natural/surface_formation/stream_power.rs`
- Create: `tests/formation_stream_power.rs`

- [x] Write RED tests for the `n=1` backward-Euler closed form, large-step
  stability, base-level preservation, monotone downstream profiles, zero-source
  identity, uplift/runoff/erodibility counterfactuals, quantized accounting,
  determinism, and cancellation.
- [x] Implement the Braun-Willett downstream-stack specialization using P2
  uplift/subsidence, P3 erodibility, and P4-derived runoff.
- [x] Compare against a bounded tiny-step explicit reference and commit.

## Task 5: Add paired nonlinear hillslope transport

Files:

- Create: `src/generators/natural/surface_formation/hillslope.rs`
- Create: `tests/formation_hillslope.rs`

- [x] Write RED tests for constant equilibrium and the linear low-slope limit,
  paired mass closure,
  critical-slope response, no inversion, lithology/climate causality, closed
  coast edges, zero allocations with supplied workspace, and cancellation.
- [x] Implement the irregular spherical finite-volume Roering-style effective
  flux with donor/local-relief limiting and retained-mass diagnostics.
- [x] Verify resolution scaling and canonical edge-orientation invariance;
  commit.

## Task 6: Implement provenance-aware sediment, coast, isostasy, and sea level

Files:

- Create: `src/generators/natural/surface_formation/sediment.rs`
- Create: `src/generators/natural/surface_formation/coast.rs`
- Create: `src/generators/natural/surface_formation/isostasy.rs`
- Create: `tests/formation_sediment.rs`
- Create: `tests/formation_coast_isostasy.rs`

- [ ] Write RED tests for five-source production, capacity ordering, lake/basin
  fill, shelf/deep-ocean delivery, delta potential, no-source/no-deposit,
  global and per-source mass closure, coast eligibility/exposure, Airy signs,
  and fixed-water-volume sea level.
- [ ] Implement one upstream-to-downstream conservative sediment pass, paired
  coast exchange, local loading/unloading response, and exact retained fields.
- [ ] Add adversarial overflow/boundary/cancellation tests and commit.

## Task 7: Build the eight-step geomorphic solve and four-step fixed point

Files:

- Create: `src/generators/natural/surface_formation/mod.rs`
- Create: `src/generators/natural/surface_formation/generation.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/surface_formation_generation.rs`

- [ ] Write RED tests for the exact component sum, multirate ordering, restart
  from P3 on every outer iteration, production-climate feedback, all five
  residual components, convergence/non-convergence, deterministic repeats,
  memory ownership, and active cancellation.
- [ ] Compose the complete eight-macro-step solve and bounded four-iteration
  fixed point with reusable workspaces and no partial publication.
- [ ] Verify analytic fixtures and fixed seeds before tuning any declared
  constant; record every design amendment and commit.

## Task 8: Add exact quality gates and the typed atomic stage

Files:

- Create: `src/generators/natural/quality/surface_formation.rs`
- Create: `src/generators/natural/surface_formation_stage.rs`
- Modify: `src/generators/natural/quality/mod.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/surface_formation_quality.rs`
- Create: `tests/surface_formation_stage.rs`

- [ ] Write RED tests for every analytic/corpus metric, forged passing reports,
  same-surface wrong-relief/climate reports, exact dependencies, cache
  invalidation, cancellation during output hashing, and atomic publication.
- [ ] Implement the evaluator-issued Serialize-only product and
  `natural.surface-formation@1` graph extension.
- [ ] Prove P0-P4 graphs and frozen hashes remain unchanged; commit.

## Task 9: Freeze P5 evidence, atlas, performance, and completion

Files:

- Create: `tests/surface_formation_evidence.rs`
- Create: `tests/surface_formation_atlas.rs`
- Create: `tests/surface_formation_performance.rs`
- Create: `docs/superpowers/specs/2026-08-18-coupled-geomorphic-formation-p5-completion.md`
- Modify: this plan

- [ ] Generate deterministic Release JSON/CSV for analytic fixtures, the old
  two-pass negative baseline, and all 17 paired product seeds.
- [ ] Render fixed map/globe rows for every causal terrain, water, river,
  sediment, coast, climate, and residual field.
- [ ] Measure Draft/Standard/High wall time, conservative dense owners,
  isolated High RSS, active cancellation, and cold/warm cache behavior.
- [ ] Inspect seeds 42, 43, and 83 and fix every severe scientific or visual
  artifact without weakening a gate.
- [ ] Run fmt, full native all-target/all-feature tests, Clippy, WASM, focused
  Release suites, upstream frozen checks, and every P5 writer.
- [ ] Record exact hashes, equations, failed baseline, metrics, timing,
  limitations, schema policy, and P6 handoff; check all boxes and commit.
