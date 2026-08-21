# P5 Coupled Geomorphic Formation Completion Record

Date: 2026-08-19  
Design: `2026-08-18-coupled-geomorphic-formation-p5-design.md`  
Plan: `../plans/2026-08-18-coupled-geomorphic-formation-p5.md`  
Status: implementation complete; one declared performance gate is not met and is
recorded below as an open deviation rather than silently relaxed

## 1. Delivered boundary

P5 publishes one atomic `world.natural-surface-formation` product built from the
authoritative sphere, P2 evolved tectonics, P3 substrate and primary relief, the
resolved climate input, the frozen P4 circulation, and one resolved formation
specification. The product contains:

- the nine causal elevation components and the exact final elevation they
  reconstruct;
- the physically solved sea level, realized water volume, and land/ocean
  classification;
- the final drainage product: receivers, basins, lakes, monthly runoff and
  discharge, Strahler order, and stable directed river reaches;
- sediment thickness, five-source provenance, throughput, shelf delivery,
  deep-ocean delivery, endorheic storage, and delta potential;
- the selected P4 circulation re-solved on the converged formation terrain;
- the bounded fixed-point report, sediment budget report, capability set, and
  checkpoint identity;
- an inseparable locked quality verdict.

The retired two-pass `PriorityFloodStreamPowerV1` modifier stays in the tree as a
compatibility path only. It can never own the P5 artifact key and is never a
silent fallback.

## 2. Algorithm conformance and declared extensions

Retained published methods:

- Barnes-Lehman-Mulla Priority-Flood depression filling and watershed ordering on
  the irregular spherical graph;
- the Braun-Willett O(N) downstream-stack implicit stream-power update, used in
  its `n = 1` closed form;
- Roering-Kirchner-Dietrich nonlinear critical-slope hillslope transport;
- Davy-Lague/Yuan explicit erosion, transport, deposition, and downstream mass
  balance;
- Cordonnier-style coupling of drainage graph, tectonic uplift, and stream power.

Declared Sekai extensions, each independently tested and included in the model
fingerprint:

- the irregular spherical finite-volume paired hillslope operator that conserves
  one mass packet across a bedrock-to-colluvium density change;
- the bounded effective formation-runoff proxy;
- the bounded annual formation-precipitation envelope (design amendment,
  section 7 of the design): P4 admits mean daily rates far above the published
  `20,000 mm yr-1` envelope every other natural product shares, so P5 derives one
  monthly envelope, scaled per cell by a single factor across all twelve months,
  and both hydrology runoff and hillslope precipitation forcing read only that
  envelope;
- the capacity-limited five-source provenance ledger with shelf accommodation,
  delta potential, and endorheic terminal storage;
- the map-scale wind/current coastal exposure proxy and sediment shielding;
- local Airy loading response instead of elastic flexure;
- the bounded four-iteration climate-surface fixed point.

## 3. Governing identity and atomicity

Every published cell satisfies the exact retained identity

```text
final = primary + tectonic - fluvial - hillslope_erosion + hillslope_deposition
        + routed_deposition - coastal_erosion + coastal_deposition + isostatic
```

bit for bit. The compositor accumulates each component in `f64`, quantizes once
to `f32`, and rebuilds the working elevation from the identity after every
operator, so the elevation the next operator sees is exactly the elevation that
will be published. A retained elevation outside the publishable range is a typed
failure, never a silent clamp.

Nothing is published until the fixed point converges. A non-converged solve
returns `SurfaceFormationGenerationError::NotConverged` carrying the best
normalized residual and leaves any previous product untouched.

## 4. Fixed point behaviour

Each outer iteration restarts the eight `12,500 yr` macro steps from the
immutable P3 terrain, so four iterations integrate one `100,000 yr` horizon, not
four. The published tectonic displacement is bounded by the horizon-limited net
uplift of every cell, which the integration test checks directly.

Across the 17-seed Draft corpus the fixed point closes in three or four outer
iterations, with final normalized residuals from `0.0564` to `0.9921`. Seed 42
closes in three iterations at `0.6383`:

| component | value | scale | normalized |
| --- | --- | --- | --- |
| area-weighted elevation RMS | `0.9187 m` | `100 m` | `0.0092` |
| changed receiver area fraction | `0.000702` | `0.05` | `0.0140` |
| area-weighted `log1p(discharge)` RMS | `0.09575` | `0.15` | `0.6383` |
| area-weighted sediment-cover RMS | `2.0295 m` | `10 m` | `0.2030` |
| changed land/ocean area fraction | `0.0000575` | `0.005` | `0.0115` |

Iteration zero compares the first candidate against the immutable P3 state, so a
world can never be declared converged on its first solve. The binding component
is the normalized `log1p(discharge)` residual: the drainage network keeps
responding to the rebuilt circulation after the terrain itself has settled.

## 5. Locked quality gates

`sekai.surface-formation-v1` publishes fourteen metrics in one canonical order,
with per-profile bounds and a subject fingerprint bound to the exact formation
checkpoint:

| metric | bound |
| --- | --- |
| component-identity-mismatch-count | `<= 0` |
| deposited-sediment-enrichment-ratio | `>= 1.25` |
| final-land-fraction-absolute-change | `<= 0.03` |
| fixed-point-normalized-residual | `<= 1.0` |
| fluvial-incision-support-enrichment-ratio | `>= 1.5` |
| land-outlet-path-area-fraction | `>= 0.95` |
| largest-network-strahler-order | `>= 3` Draft, `>= 4` Standard/High |
| primary-final-elevation-correlation | `>= 0.90` |
| provenance-mass-relative-error | `<= 1e-7` |
| receiver-adjacency-violation-count | `<= 0` |
| river-reach-count | `>= 1` |
| sediment-mass-relative-error | `<= 1e-8` |
| through-ocean-land-river-count | `<= 0` |
| water-volume-relative-error | `<= P3 water bound` |

Corpus result over the 17 Draft seeds, every metric passing on every seed:

| metric | minimum | mean | maximum |
| --- | --- | --- | --- |
| component-identity-mismatch-count | `0` | `0` | `0` |
| deposited-sediment-enrichment-ratio | `2.908` | `3.229` | `3.515` |
| final-land-fraction-absolute-change | `2.08e-6` | `1.38e-3` | `2.27e-3` |
| fixed-point-normalized-residual | `0.0564` | `0.6189` | `0.9921` |
| fluvial-incision-support-enrichment-ratio | `2.960` | `3.125` | `3.268` |
| land-outlet-path-area-fraction | `1.000` | `1.000` | `1.000` |
| largest-network-strahler-order | `4` | `4.29` | `5` |
| primary-final-elevation-correlation | `0.99933` | `0.99947` | `0.99958` |
| provenance-mass-relative-error | `2.42e-16` | `8.03e-16` | `1.75e-15` |
| receiver-adjacency-violation-count | `0` | `0` | `0` |
| river-reach-count | `4,140` | `4,812` | `5,783` |
| sediment-mass-relative-error | `2.31e-16` | `5.94e-16` | `1.36e-15` |
| through-ocean-land-river-count | `0` | `0` | `0` |
| water-volume-relative-error | `6.30e-9` | `2.19e-8` | `4.05e-8` |

Sediment closure lands roughly eight orders of magnitude inside its `1e-8`
bound and provenance closure nine orders inside its `1e-7` bound. Every land
cell on every seed reaches a real outlet terminal, and the deepest network on
every seed reaches Strahler order `4` or `5`, above the Draft requirement of
`3`.

The outlet-path metric walks the receiver chain with a cycle-safe colored
traversal, so a cyclic drainage graph can never be reported as an outlet path.
The receiver gate checks authoritative adjacency and centimetre-quantized
non-uphill drainage; equal-height flats are ordered by the solver's own flood
dequeue rank, which is not part of the published product, so the gate checks the
published monotonicity instead of reconstructing that rank.

The evaluator rejects a same-surface relief that did not produce the product by
comparing the retained primary elevation and water inventory. The public product
factory always re-measures, so a forged all-pass report can never acquire an
artifact identity.

## 6. Deterministic 17-seed evidence

The release writer regenerates `target/natural-quality/p5/evidence.json` and
`evidence.csv` for the Draft corpus `42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59,
61, 71, 73, 83, 89, 97` on a `20,252`-cell authoritative sphere with fingerprint
`0d09df7aa131d120490202741b0fd3184919ea9681f16537a14f81f0e5806f2e`.

- `evidence.json`: `93,207 B`, blake3
  `8b9b22b9db045ecc25109b7226210055f3c2b26e3928184e22cded55e075d453`;
- seed 42 artifact: `67,693,614 B` of canonical JSON, blake3
  `a20d89ac6520f040fbdd41d004a3062be4efa9ec8aa462540a80b24ab6278f49`;
- seed 42 checkpoint fingerprint
  `21e5e2638b5dd7c0815c3b40a87f32c00c5865e3415ed98370602b71c6ba650d`, state
  fingerprint
  `7203c04a79dd9724702d60c3509a5c2ed8a29dfab0e7b73b22e58f73c7d3767d`;
- re-running seed 42 in the same process reproduces an equal artifact, including
  both fingerprints;
- seed 42 publishes `1,000` basins, `321` lakes, and `4,263` river reaches.

The writer also records the retired two-pass baseline structurally: for each
locked P5 gate it names why the old modifier cannot report it at all, including
the absent nine-component inventory, the absent fixed-point residual, the absent
five-source provenance, the fixed local deposition ceiling, and the unre-solved
sea level. That baseline was not re-run numerically because the old generator
consumes the retired preliminary-climate and spherical-relief contracts rather
than the P3/P4 products P5 is built on; the comparison recorded is therefore
structural, which is the honest form of it.

## 7. Performance, memory, cancellation, and cache

Release measurements on the development machine, seed 42, wall clock of the
complete `NaturalSurfaceFormationArtifact::generate` call including every
mandated P4 re-solve:

| profile | cells | outer iterations | measured | declared gate | verdict |
| --- | --- | --- | --- | --- | --- |
| Draft | 20,252 | 3 | `10.94 s` | `<= 15 s` | pass |
| Standard | 79,212 | 4 | `92.03 s` | `<= 90 s` | **fail by 2.3%** |
| High | 198,812 | 4 | `290.99 s` | `<= 300 s` | pass |

Conservative dense-owner inventory and isolated peak RSS delta stay far below
their `1 GiB` limits:

| profile | dense-owner inventory | peak RSS delta |
| --- | --- | --- |
| Draft | `20,008,784 B` | `73,322,496 B` |
| Standard | `78,261,264 B` | `269,762,560 B` |
| High | `196,426,064 B` | `668,241,920 B` |

Per-iteration split at Standard:

| phase | time |
| --- | --- |
| eight-macro-step geomorphic solve | `0.96 s` |
| P4 circulation re-solve on the candidate terrain | `20.95 s` |
| final hydrology on the candidate terrain and climate | `0.07 s` |

**Open deviation.** Standard is the only missed gate. It is missed because the
design mandates one full cold P4 solve per outer iteration, and P4's own Standard
budget is `30 s`; four cold re-solves cannot fit a `90 s` end-to-end budget.
`91%` of Standard wall clock is the frozen P4 solver, and the complete P5
geomorphic work is `3.8 s`. High passes only because its four re-solves happen to
land under the wider `300 s` budget, not because the coupling is cheaper there.
The design already names the remedy: section 6 permits an outer iteration to
warm-start the circulation "when an exact compatible checkpoint is available".
Implementing that warm start is a P4-boundary change with its own acceptance
work, so it is recorded here as the required follow-up rather than absorbed by
weakening the gate or by reducing the iteration budget.

An earlier profile of this compositor spent half of Draft wall clock
re-validating immutable upstream products inside the macro-step loop. Each
kernel now has a crate-private `*_from_validated*` entry that the compositor
uses after validating the surface, tectonics, substrate, relief, work domain,
and frozen climate exactly once. Draft fell from `26.8 s` to `10.89 s` with no
change to any published value.

The isolated High run in a fresh child process reports a `669,208,576 B` peak
RSS delta, also inside the `1 GiB` limit.

Active cancellation returns `SurfaceFormationGenerationError::Cancelled` in
`37 us` after `512` observed polls, far inside the `250 ms` requirement, and
publishes nothing. The Draft graph is a `18.15 s` cold build with six misses and
a `0.12 s` warm rebuild with six hits, and both publish the same product hash.

## 8. Atlas and manual inspection

The release atlas renders nineteen causal rows for all seventeen seeds as an
equirectangular map beside an oblique globe: primary elevation, final elevation,
final-minus-primary change, tectonic displacement, fluvial erosion, hillslope
erosion, hillslope deposition, routed sediment deposition, coastal erosion,
coastal deposition, isostatic response, surface-water class, mean annual
discharge, Strahler order, sediment thickness, dominant provenance, delta
potential, formation precipitation, and shelf delivery.

Seeds `42`, `43`, and `83` were inspected by hand. No severe artifact was found:
no cubed-face seam, no polygon-aligned global drainage ring, no pole artifact,
no isolated one-cell lake field, and no land river crossing ocean, which the
`through-ocean-land-river-count` gate also reports as exactly zero on every
seed. Drainage networks fill every continent, lakes appear inland, isostatic
response tracks the loaded and unloaded margins, and provenance classes follow
their source lithologies.

Two honest observations from the same sheets:

- hillslope erosion and deposition are visually negligible beside fluvial
  incision. At Draft the authoritative cell spacing is roughly `140 km`, so
  slope transport is a subgrid process there; the operator is still exactly
  paired and mass-closed, and its share grows with resolution. This is a
  resolution limitation, not a defect.
- the published formation precipitation shows a strong high-precipitation band
  in the southern hemisphere across the corpus. P5 re-solves the frozen P4
  equation without modifying it, so this pattern belongs to the P4 model on
  these terrains. It is recorded here for P4/P6 follow-up rather than corrected
  inside the P5 boundary.

## 9. Verification

Default-suite coverage (native debug, `cargo test --workspace --all-targets
--all-features`):

- `surface_formation_contracts` freezes strict bounded serde, component
  identity, checkpoint and model fingerprints, capabilities, and reports;
- `formation_climate_coupling`, `formation_hydrology`, `formation_stream_power`,
  `formation_hillslope`, `formation_sediment`, and `formation_coast_isostasy`
  cover each kernel against analytic fixtures, counterfactuals, mass closure,
  adversarial malformed input, and active cancellation;
- `surface_formation_generation` covers the exact component sum, the immutable
  P3 restart and horizon-bounded displacement, the rebuilt production climate,
  every residual component, the conservative dense-owner inventory, a
  deterministic non-converging single-iteration budget, and active cancellation;
- `surface_formation_quality` covers the complete locked gate inventory, a
  same-surface wrong relief, a forged all-pass report, deterministic repeats,
  and cancelled evaluation;
- `surface_formation_stage` covers the locked artifact key, stage identity,
  exact dependency boundary, unchanged P0-P4 graph and hashes, warm all-hit
  rebuild, and formation-spec-only invalidation;
- `water_volume_sea_level` proves the new cancellable physical sea-level solve
  is bit-identical to the frozen operator.

Release-only writers and gates (`--ignored`):

- `surface_formation_evidence::write_surface_formation_evidence` regenerates the
  deterministic 17-seed JSON/CSV record, asserts every hard metric passes on
  every seed, and re-runs seed 42 to prove byte-identical repeats;
- `surface_formation_atlas::render_surface_formation_atlas` renders the fixed
  19-row causal map/globe sheet for all 17 seeds plus its manifest;
- `surface_formation_performance::measure_surface_formation_performance` records
  Draft/Standard/High wall clock, dense ownership, isolated High RSS in a fresh
  child process, active cancellation, and cold/warm cache behaviour;
- `draft_wall_clock_stays_within_the_declared_gate`,
  `standard_wall_clock_stays_within_the_declared_gate`, and
  `high_wall_clock_stays_within_the_declared_gate` assert each declared time
  budget on its own, so the missed Standard budget is visible without hiding the
  remaining measurements;
- `surface_formation_generation::repeated_complete_solves_are_byte_identical`
  compares two complete solves byte for byte.

Analytic fixtures are verified by the default-suite kernel tests rather than by
the JSON writer: the `n = 1` backward-Euler closed form, base-level
preservation, large-step stability against the rejected explicit method, the
linear low-slope hillslope limit, exact no-op identities, capacity ordering,
lake and shelf accommodation, Airy signs, and the fixed-water-volume solve all
assert against closed-form expectations in their own suites. The release writer
records product-corpus evidence, not a second copy of those fixtures.

Static and full-suite verification of the frozen implementation:

- `cargo fmt --all -- --check`: clean;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean;
- `cargo check --target wasm32-unknown-unknown --all-features --lib`: clean;
- `cargo test --workspace --all-targets --all-features --no-fail-fast`:
  `175` targets, `1,490` passing tests, `0` failures, `47` explicitly ignored
  release-only or writer tests;
- `cargo test --workspace --doc`: `9` passing, `0` failures, `8` ignored
  examples, including the two `compile_fail` proofs that a decoded snapshot can
  never become a `NaturalSurfaceFormationArtifact`.

Release runs regenerated after the final kernel, gate, and compositor changes:
the 17-seed evidence writer, the 17-seed atlas writer, the performance writer,
the focused Draft and High wall-clock gates, and the focused cancellation gate.

`standard_wall_clock_stays_within_the_declared_gate` currently fails. That is
the deliberate, visible record of the open deviation in section 7; it is neither
suppressed nor weakened.

## 10. Limitations and P6 handoff

P5 deliberately does not implement soil, vegetation, groundwater, explicit
evapotranspiration, snow, sea ice, or glaciers. `ExplicitEvapotranspirationV1`,
`GroundwaterFlowV1`, and `GlacialErosionV1` are declared `Unavailable`, and the
glacial term is exactly zero.

Declared proxies that P6 must not reinterpret as final physics:

- effective formation runoff is a bounded fraction of bounded precipitation, not
  a solved water balance;
- the `1,000 yr` endorheic residence horizon is a formation proxy for lakes that
  cannot reach their spill level, not modelled lake evaporation;
- the bounded annual formation-precipitation envelope caps forcing at the
  published `20,000 mm yr-1` field bound; it is a bound of the P5 forcing, not a
  claim that the circulation produced less rain;
- hillslope diffusivity, coastal exposure, and the Airy response are map-scale
  closures, not measured local coefficients.

Open work handed forward:

1. the Standard wall-clock budget, whose remedy is the circulation warm start
   the design already permits;
2. P6 versioning of the compound formation model once C3 clouds, surface
   energy/moisture exchange, snow, sea ice, and glaciers exist.

P6 receives the immutable final terrain and its causal components, formation
climate, final hydrology, lakes, sediment mass and provenance, coastal and delta
state, and the complete checkpoint identity.
