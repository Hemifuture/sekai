# Geologic Substrate and Primary Relief P3 Implementation Plan

**Goal:** Publish a strict V5-derived geologic substrate and water-volume-consistent primary relief with fixed 17-seed quality evidence.

**Architecture:** Add immutable world contracts first, then pure generators, quality evaluators, typed stages/graph, and ignored evidence/atlas/performance writers. Keep V4 artifacts and product graph unchanged.

**Tech stack:** Rust 2024, existing typed artifact graph, spherical topology, P2 material/forcing, existing mantle and conditioned relief primitives, serde/thiserror, image/blake3 test support.

## Task 1: Define strict substrate contracts

Files:

- Create: `src/world/natural/primary_relief.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/geologic_substrate_contracts.rs`

- [x] Write RED tests for schema, bounded wire allocation, sediment-source decoding, exact dense lengths, density/range validation, and evolved cross-validation.
- [x] Implement `SedimentSourceKind`, its field, and `GeologicSubstrateSnapshot`.
- [x] Run focused tests and strict Clippy.
- [x] Commit: `feat: define geologic substrate contracts`

## Task 2: Generate the V5-derived substrate

Files:

- Create: `src/generators/natural/geologic_substrate.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/random.rs`
- Create: `tests/geologic_substrate_generation.rs`

- [x] Write RED analytic tests for density mixing, causal lithology priority, hotspot causality, deterministic streams, and cancellation.
- [x] Generate mantle, copied crust facts, density, lithology, erodibility, permeability, fracture, and sediment-source fields.
- [x] Cross-validate against the exact V5 snapshot and surface.
- [x] Commit: `feat: generate causal geologic substrate`

## Task 3: Define primary-relief and water-budget contracts

Files:

- Extend: `src/world/natural/primary_relief.rs`
- Create: `tests/primary_relief_contracts.rs`
- Create: `tests/water_volume_sea_level.rs`

- [x] Write RED tests for strict serde, component identity, compatibility mapping, water closure, physical classification, and author-constraint status.
- [x] Implement `LandFractionConstraintStatus` and `PrimaryReliefSnapshot`.
- [x] Implement and analytically test the stable piecewise-linear water-volume solve.
- [x] Commit: `feat: define physical primary relief`

## Task 4: Generate isostatic and causal primary relief

Files:

- Create: `src/generators/natural/primary_relief.rs`
- Modify: `src/generators/natural/spherical_relief.rs`
- Create: `tests/primary_relief_generation.rs`

- [x] Write RED tests for Airy density/thickness monotonicity, Parsons-Sclater age ordering, forcing signs, passive-margin support, hotspot construction, component closure, determinism, and cancellation.
- [x] Implement density-aware base, bounded dynamic response, hotspot construction, passive-margin distance profile, conditioned detail, safety reconciliation, and physical sea level.
- [x] Reuse existing primitives only through explicit causal inputs.
- [x] Commit: `feat: generate physical primary relief`

## Task 5: Add P3 quality metrics and corpus evaluator

Files:

- Create: `src/generators/natural/quality/primary_relief.rs`
- Modify: `src/generators/natural/quality/mod.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/primary_relief_quality.rs`

- [x] Write RED tests for exact metric inventory, unavailable semantics, hard/statistical separation, and recomputation from raw corpus samples.
- [x] Implement all locked P3 gates without averaging per-world summaries.
- [x] Tune implementation constants only; do not relax thresholds to make the corpus pass.
- [x] Commit: `feat: measure primary relief quality`

## Task 6: Publish typed stages and graph

Files:

- Create: `src/generators/natural/primary_relief_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/primary_relief_stage.rs`

- [x] Write RED tests for artifact keys, exact dependencies, stage identities, cache restore, malformed/mismatched inputs, cancellation, and atomic publication.
- [x] Add `GeologicSubstrateArtifact`, `PrimaryReliefArtifact`, both stages, and `primary_relief_graph` including the P2 stage.
- [x] Verify the frozen V4 graph and hashes remain unchanged.
- [x] Commit: `feat: integrate substrate and primary relief`

## Task 7: Freeze P3 evidence, atlas, and performance

Files:

- Create: `tests/primary_relief_evidence.rs`
- Create: `tests/primary_relief_atlas.rs`
- Create: `tests/primary_relief_performance.rs`

- [ ] Write deterministic Release JSON/CSV under `target/natural-quality/p3`.
- [ ] Render fixed map/globe rows for density, lithology, base, forcing response, volcanic/passive/detail components, elevation, and physical water.
- [ ] Measure Draft completion and Standard/High cancellation.
- [ ] Inspect at least seeds 42, 43, and 83; fix every severe artifact.

## Task 8: Verify and complete P3

Files:

- Create: `docs/superpowers/specs/2026-08-17-substrate-primary-relief-p3-completion.md`
- Modify: this plan

- [ ] Run fmt, all-target/all-feature check/test/Clippy, WASM, focused/adjacent suites, P0 baseline, P2 evidence, and all P3 Release writers.
- [ ] Conduct direct equation/provenance/determinism/cancellation review.
- [ ] Record exact hashes, metrics, timing, atlas review, limitations, and P4 handoff.
- [ ] Check all boxes and commit: `docs: record substrate and primary relief`
