# Evolved Tectonics V5 Implementation Plan

Date: 2026-08-17
Design: `docs/superpowers/specs/2026-08-17-evolved-tectonics-v5-design.md`

## Global constraints

- Preserve `SphericalTectonicSnapshot` V3, `SphericalTectonicStage@4`, and the
  exact P0 baseline hashes.
- Work only in the isolated `feat/spherical-presentation` worktree.
- Use test-driven development: add a focused failing test, observe the intended
  failure, implement the smallest coherent slice, then re-run adjacent tests.
- Never replace the P1 conservative map with nearest/barycentric material
  projection.
- Do not weaken the locked 17-seed thresholds to obtain green output.
- Keep the repository buildable after every task and commit each bounded slice.
- P2 may expose causes and compatibility views; it may not implement P3 relief
  or P4 circulation early.

### Task 1: Strict evolved snapshot, material, forcing, and budget contracts

Files:

- Create: `src/world/natural/evolved_tectonics.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/evolved_tectonic_contracts.rs`

- [x] **Step 1: Write RED contract tests**

  Cover schema constants, strict round-trip, nested V3 compatibility view,
  exact lengths, finite/non-negative material values, zero-area/volume
  consistency, derived thickness bounds, forcing sentinel/range rules, material
  equations, lineage equation, surface/profile identity, allocation limits, and
  nested unknown-field rejection.

- [x] **Step 2: Verify RED**

  Run `cargo test --test evolved_tectonic_contracts -- --nocapture` and confirm
  failure is due to missing V5 types/APIs.

- [x] **Step 3: Implement strict immutable contracts**

  Add `EvolvedTectonicSnapshot`, `SphericalCrustMaterialState`,
  `SphericalTectonicForcingState`, `SphericalTectonicMaterialBudget`, and
  `SphericalTectonicLineageBudget` with bounded custom deserialization and
  cross-validation against an authoritative surface.

- [x] **Step 4: Verify contracts and V3 compatibility**

  Run the new test plus `spherical_tectonic_contracts` and
  `natural_quality_profiles`.

- [x] **Step 5: Commit**

  `git commit -m "feat: define evolved tectonic contracts"`

### Task 2: Extensive transient material columns and exact ledger primitives

Files:

- Modify: `src/generators/natural/spherical_tectonics/model.rs`
- Modify: `src/generators/natural/spherical_tectonics/initial_state.rs`
- Modify: all local `CrustSample` fixtures/builders
- Create: `tests/evolved_tectonic_material.rs`

- [x] **Step 1: Write RED material-column tests**

  Prove pure continental/oceanic initialization, component thickness
  derivation, compensated totals, rigid-copy bit preservation, mixed-category
  tie behavior, and exact initial budget capture.

- [x] **Step 2: Verify RED**

  Run the focused test and record the missing extensive representation.

- [x] **Step 3: Add material columns without changing V4 semantics**

  Extend transient samples with both area/volume components, centralize
  construction/validation/derived compatibility fields, and add a compensated
  `EvolutionMaterialLedger`. Existing V4 branches must not read the new fields.

- [x] **Step 4: Re-run V4 hashes early**

  Run focused spherical tectonic tests and the Release P0 baseline writer;
  assert both frozen hashes are unchanged.

- [x] **Step 5: Commit**

  `git commit -m "feat: track tectonic material columns"`

### Task 3: Conservative V5 process and resampling semantics

Files:

- Modify: `src/generators/natural/spherical_tectonics/processes/mod.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/subduction.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/collision.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/spreading.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/relaxation.rs`
- Modify: `src/generators/natural/spherical_tectonics/resample.rs`
- Modify: `src/generators/natural/spherical_tectonics/runner.rs`
- Extend: `tests/evolved_tectonic_material.rs`

- [x] **Step 1: Add RED analytic process fixtures**

  Demonstrate that V4 overlap resampling loses represented continental area;
  require V5 to preserve it, consume oceanic before continental at subduction,
  conserve collision volume, thin by pure shear without volume loss, create
  age-zero ocean at spreading, and leave material unchanged during relaxation.

- [x] **Step 2: Observe the intended failures**

  Run focused integration/unit filters and confirm each failure points at the
  V4 conflation rather than fixture construction.

- [x] **Step 3: Implement separate V4/V5 semantics**

  Keep the legacy entry points byte-stable. Add conservative process variants,
  fractional-pivot categorical allocation, bounded component-volume water
  filling, named ocean coverage closure, and ledger updates.

- [x] **Step 4: Add resample/property tests**

  Exercise overlaps, gaps, mixed pivot cells, infeasible thickness bounds,
  signed residuals, deterministic ties, repeated resamples, and cancellation.

- [x] **Step 5: Verify focused and V4 regression suites**

  Run material, spherical tectonic generation/causality, and P0 baseline tests.

- [x] **Step 6: Commit**

  `git commit -m "feat: conserve evolved crust material"`

### Task 4: Anisotropic connected plates and bounded lineage evolution

Files:

- Modify: `src/generators/natural/spherical_tectonics/initial_state.rs`
- Modify: `src/generators/natural/spherical_tectonics/resample.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/collision.rs`
- Modify: `src/generators/natural/spherical_tectonics/processes/rifting.rs`
- Modify: `src/generators/natural/spherical_tectonics/runner.rs`
- Create: `tests/evolved_tectonic_morphology.rs`

- [x] **Step 1: Freeze RED V4 defects**

  On the locked 17 seeds record at least one V4 plate above 45% and a regular
  macro triple-junction fraction above 35%.

- [x] **Step 2: Implement connected anisotropic initial domains**

  Add stable per-lineage low-frequency edge costs in `[0.70, 1.30]` and a
  multi-source shortest-path assignment whose predecessor paths guarantee
  connectivity.

- [x] **Step 3: Bound collision transfer and fragment oversized plates**

  Reject transfers above the hard cap, mechanically split lineages above the
  40% trigger, preserve all material bits/totals, and update the exact lineage
  ledger.

- [x] **Step 4: Run analytic and 17-seed morphology gates**

  Require connected plates, max share `<= 0.45`, regular-120 fraction
  `<= 0.35`, final plate-count bounds, and byte-identical repetition.

- [x] **Step 5: Commit**

  `git commit -m "feat: evolve bounded anisotropic plates"`

### Task 5: Present-day tectonic forcing fields

Files:

- Create: `src/generators/natural/spherical_tectonics/forcing.rs`
- Modify: `src/generators/natural/spherical_tectonics.rs`
- Create: `tests/evolved_tectonic_forcing.rs`

- [x] **Step 1: Write RED handcrafted-boundary tests**

  Construct ocean-continent, ocean-ocean, continent-continent, divergence, and
  transform fixtures. Require correct side/sign, positive collision shortening,
  zero transform forcing, minimum boundary distance, and event-age semantics.

- [x] **Step 2: Verify RED**

  Confirm V4 cannot expose the requested fields.

- [x] **Step 3: Implement final-contact forcing evaluation**

  Rebuild contacts after final resampling, apply the locked transfer curves as
  rates, preserve active orogenic forcing, and validate all finite/unit bounds.

- [x] **Step 4: Run analytic and corpus causality tests**

  Require both 80% transect gates, age-depth rank correlation `>= 0.70`, and
  transform/convergent uplift ratio `<= 0.5`.

- [x] **Step 5: Commit**

  `git commit -m "feat: publish active tectonic forcing"`

### Task 6: P1 conservative authoritative publisher

Files:

- Create: `src/generators/natural/evolved_tectonics.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/spatial/profile_surface.rs`
- Create: `tests/evolved_tectonic_publication.rs`

- [x] **Step 1: Write RED control-to-authority tests**

  Require exact P1 source/target identities, extensive closure for all material
  components, masked ocean-age remap, tangent lineation transport, category
  ambiguity evidence, derived thickness, authoritative boundary reconstruction,
  and deterministic bytes.

- [x] **Step 2: Verify RED**

  Confirm the old one-shot `project_current_state` fails the new conservation
  contract.

- [x] **Step 3: Implement the V5 generator/publisher**

  Add `EvolvedTectonicGenerator::generate`, consume a validated
  `ProfileSurfaceBundle`, run only the control evolution, remap by field
  semantics, canonicalize authority domains, build the nested V3 compatibility
  view, and assemble strict budgets.

- [x] **Step 4: Verify Draft/Standard/High publication and cancellation**

  Run Draft synchronously; exercise cancellable Standard/High paths and confirm
  no partial snapshot escapes.

- [x] **Step 5: Commit**

  `git commit -m "feat: publish conservative evolved tectonics"`

### Task 7: V5 stage, graph, and versioned quality report

Files:

- Create: `src/generators/natural/evolved_tectonic_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `src/generators/natural/quality/evolved_tectonics.rs`
- Modify: `src/generators/natural/quality/mod.rs`
- Create: `tests/evolved_tectonic_stage.rs`
- Create: `tests/evolved_tectonic_quality.rs`

- [x] **Step 1: Write RED typed-stage/graph tests**

  Require exact dependencies, artifact key, stage identity/version, profile and
  surface mismatch rejection, cache determinism/invalidation, cooperative
  cancellation, atomic publication, and legacy graph coexistence.

- [x] **Step 2: Implement profile artifact and V5 stage**

  Complete the supplied authoritative surface with P1 control/map data inside
  the stage, invoke V5, and publish only the validated evolved artifact.

- [x] **Step 3: Implement versioned P2 metrics**

  Record material fraction/retention, max plate area, subduction/collision
  causality, ocean age-depth correlation, transform ratio, triple-junction
  regularity, material/lineage closure, remap ambiguity, and non-finite count.

- [x] **Step 4: Add the legacy-relief coast harness**

  Measure the fixed 17-seed median buffered coast/plate overlap and require
  `<= 0.35`; mark the harness explicitly as a P3 revalidation dependency, not a
  V5 graph input.

- [x] **Step 5: Verify native, WASM, and legacy graphs**

  Run stage/quality suites, spherical legacy graph suites, all-target check,
  and WASM check.

- [x] **Step 6: Commit**

  `git commit -m "feat: integrate evolved tectonic stage"`

### Task 8: Release evidence, atlas, review, and P2 completion

Files:

- Create: `tests/evolved_tectonic_evidence.rs`
- Create: `tests/evolved_tectonic_performance.rs`
- Create or extend: `tests/evolved_tectonic_atlas.rs`
- Create: `docs/superpowers/specs/2026-08-17-evolved-tectonics-v5-completion.md`
- Modify: this plan

- [x] **Step 1: Add ignored Release evidence/performance writers**

  Write deterministic JSON/CSV evidence and diagnostic atlases only under
  `target/natural-quality/p2`. Record exact hashes, sizes, profile timings,
  cancellation latency, per-seed metrics, aggregate metrics, and material
  budgets.

- [x] **Step 2: Run every P2 gate fresh**

  Run formatting, all-target/all-feature check, warning-free Clippy, focused
  P2 suites, adjacent V4 suites, WASM, Release 17-seed evidence, profile
  performance, and P0 baseline regeneration.

- [x] **Step 3: Inspect atlases and conduct direct review**

  Inspect material, plate, forcing, age, and budget diagnostic views. Review
  contract safety, equations, paper/extension labeling, determinism,
  cancellation, performance, and compatibility. Fix every Critical or
  Important issue and re-run affected gates.

- [x] **Step 4: Write completion record and freeze P3 handoff**

  Record exact evidence and explicitly state remaining visual limitations: P2
  supplies better causes but P3-P9 are still required for finished terrain.

- [x] **Step 5: Check all boxes and commit**

  `git commit -m "docs: record evolved tectonics v5"`
