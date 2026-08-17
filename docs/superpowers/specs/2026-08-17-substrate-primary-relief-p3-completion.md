# P3 Geologic Substrate and Primary Relief Completion Record

Date: 2026-08-17  
Branch: `feat/spherical-presentation`  
Design: [2026-08-17-substrate-primary-relief-p3-design.md](2026-08-17-substrate-primary-relief-p3-design.md)  
Plan: [2026-08-17-substrate-primary-relief-p3.md](../plans/2026-08-17-substrate-primary-relief-p3.md)

## Outcome

P3 is complete. The authoritative Draft sphere now has two immutable,
surface-bound artifacts after evolved tectonics V5:

- `GeologicSubstrateArtifact`, containing the causal mantle, copied V5 crust
  facts, density, lithology, fracture, erodibility, permeability, and sediment
  source;
- `PrimaryReliefArtifact`, containing density-aware isostatic relief, the
  signed V5-derived dynamic response, hotspot construction, passive-margin
  response, conditioned regional detail, an exact component sum, physical
  water inventory, sea level, and land/ocean classification.

The frozen V4 graph remains unchanged. P3 is available only through the new
`primary_relief_graph` and versioned stage identities.

## Algorithm fidelity and extension classification

The implementation review compared the equations in the child design directly
against the generator and contract code.

| Part | Classification | Review result |
|---|---|---|
| Continental isostatic base | Density-aware local Airy balance | The implemented reference column, density term, thickness units, and `250 m` freeboard match the locked equation exactly. |
| Oceanic thermal depth | Parsons-Sclater piecewise empirical relation | The `<= 70 Myr` square-root branch and the older exponential branch use the locked coefficients without retuning. Density/thickness buoyancy is a separately named correction. |
| Ocean water inventory | NOAA/NGDC `1.335e18 m3` Earth reference | Inventory is scaled by spherical area. Sea level is solved from basin capacity, never from requested land percentage. |
| Bath-tub solve | Sekai numerical operator | Stable `(elevation, CellId)` ordering, compensated sums, quantized publication, and recomputed closure implement the specified monotone piecewise-linear equation. |
| Dynamic tectonic relief | Declared Sekai procedural extension | The locked `0.65 * accumulated + 250 * (uplift - subsidence)` response is unchanged. The inherited coarse response is projected onto active net-forcing sign exactly as the design amendment requires. |
| Substrate lithology/properties | Declared Sekai procedural extension | Deterministic causal priority and bounded property recipes are implemented as specified; they are not presented as a published predictive geology model. |
| Passive margins and regional detail | Declared Sekai procedural extensions | Both remain separate bounded components. Neither can change crust, forcing, water inventory, or authoritative classification. |

The published Airy and Parsons-Sclater equations were not silently modified.
The project-specific parts are explicit, named, unit-bearing, independently
bounded, and covered by causal tests. Therefore the honest answer to “is the
algorithm modified?” is: the cited core equations are implemented directly;
the surrounding world-construction model contains declared Sekai extensions,
not hidden changes to those equations.

Uplift and subsidence rates remain owned by the exact upstream
`EvolvedTectonicArtifact` rather than being copied into P3 as a second source of
truth. `PrimaryReliefStage` consumes that artifact explicitly and publishes its
derived dynamic component. Downstream formation must depend on the same V5
artifact when it needs the rates.

## Contract and implementation review

- Every dense field has exact authoritative-surface cardinality and a bounded
  deserialization allocation.
- Every copied crust fact and the density recipe are cross-validated against
  the exact V5 snapshot.
- Relief validates the exact component identity, compatibility projection,
  physical-water closure, surface fingerprint, authored land constraint, and
  finite safety ranges.
- Generation uses isolated stage identities and labelled random substreams.
  Repeated fixed-seed substrate, relief, diagnostics, JSON, and CSV are byte
  deterministic.
- Cancellation is polled during substrate construction, primary component
  construction, inherited detail synthesis, and publication. Stage tests prove
  cancelled or malformed builds publish no partial artifact.
- The stage graph has exact typed dependencies, cache restoration, selective
  invalidation, and no accidental V4 substitution.

## Seventeen-seed Release quality evidence

Evidence directory: `target/natural-quality/p3`.

| Corpus metric | Value | Samples | Gate | Result |
|---|---:|---:|---:|---|
| Coast/plate-boundary overlap | `0.22311321531197517` | 17 | `<= 0.35` | Pass |
| Continental-ocean median separation | `5669.5 m` | 344,284 | `>= 2500 m` | Pass |
| Convergent positive dynamic fraction | `0.9977628635346756` | 1,788 | `>= 0.80` | Pass |
| Hotspot positive construction fraction | `1.0` | 68 | `>= 0.80` | Pass |
| Old-young ocean depth separation | `4125.75 m` | 95,410 | `>= 600 m` | Pass |
| Physical land-area fraction median | `0.4025236666202545` | 17 | `0.20..=0.55` | Pass |
| Regional-detail RMS ratio | `0.021551929914420936` | 344,284 | `0.01..=0.30` | Pass |
| Subduction negative dynamic fraction | `1.0` | 6,426 | `>= 0.80` | Pass |

Every per-world hard gate passed: component closure, elevation safety,
non-finite count, upstream P2 hard status, plate-area bound, and water-volume
closure. No quality threshold was relaxed during tuning.

Deterministic evidence hashes:

- P3 `evidence.json`: `8da45485208674530e4fd2426904a636b00f35d3f27ef93981fccc22b606b690`
- P3 `metrics.csv`: `55a7dbe37a366e52bae092814a198ea886580511d0b38cbc5dc2df04b46f38c8`
- P3 `performance.json`: `a74315b2874263170ae017fda1f1ea333b304070cd29dadee6eb4d6b4a78463a`

Frozen upstream evidence was regenerated and remained byte-identical:

- P0/V4 JSON: `4c1a0a8dfe0d41a45bb4f4e4ff36beb888167424db513e948bce53c5a1cac083`
- P0/V4 CSV: `a763d5b4bd5c176794c3a08e5e66bc00953d93ab72e3ae8862df2124a61bee3f`
- P2 JSON: `d6af0f68da189291d46e0af36d4f6875bd73671f2d12b28a56c0f16e47ebce97`
- P2 CSV: `f8cbc4c506970f020afbd4450bd0152121f6c49ebb74b9ab170957f65802150a`

## Performance and cancellation

Release measurements on this working environment:

| Profile | Authority/control cells | Surface bundle | Completed P3 pipeline | Cancellation latency |
|---|---:|---:|---:|---:|
| Draft | 20,252 / 4,842 | `0.535225 s` | `2.204695 s` | Not applicable |
| Standard | 79,212 / 20,252 | `2.155890 s` | Intentionally cancelled | `0.356074 s` |
| High | 198,812 / 20,252 | `4.964492 s` | Intentionally cancelled | `0.836864 s` |

The Draft pipeline breakdown was `0.497577 s` evolved tectonics,
`0.331786 s` substrate, `0.723678 s` primary relief, and `0.647606 s`
quality evaluation. The serialized primary artifact was 1,470,572 bytes.
Standard and High cancellation was requested while the exact upstream V5 phase
was active; both returned the stable cancelled result within the two-second
budget, so no downstream P3 artifact could publish.

## Atlas review

The Release atlas contains all 17 seeds. Each sheet has fixed map and oblique
globe columns for density, lithology, isostatic base, dynamic response,
volcanic construction, passive margin, regional detail, elevation, and
physical water.

The required manual review covered:

- seed 42: `1dc9b3938b75864a62fd66202ab0b34676a0bea31e09bd1611cd821c89ca4a16`;
- seed 43: `cf31a03287a09ab8c0c27d3e0b1ca0c20b0c1041880513e5cde43275ff121b0c`;
- seed 83: `d4f538d3c05b9cbcd75d9b097b410be900505c9ad3c71cfdf8b195a8848fee1b`.

Map/globe coastlines and fields agree; ocean-age depth bands are continuous;
positive and negative tectonic responses follow causal support; hotspot
construction stays local; passive-margin bands are connected; and physical
water classification matches the solved sea level. No severe seam, global
stripe, isolated height wall, projection disagreement, or water inversion was
found.

Draft cell facets and the diagnostic raster-Voronoi projection remain visibly
coarse. That is an honest P1/P3 sampling limitation, not a terrain defect to
hide with destructive smoothing. P9 owns analytic normals, detail LOD,
materials, lighting, and the final continuous presentation.

## Verification record

The following gates passed from a clean source state after the P3 commits:

- `cargo fmt --all -- --check`;
- `cargo check --all-targets --all-features`;
- `cargo test --all-targets --all-features --quiet` (final full run exited 0
  after 426.6 seconds);
- `cargo clippy --all-targets --all-features -- -D warnings`;
- `cargo check --workspace --all-features --lib --target wasm32-unknown-unknown`
  with the repository's `getrandom_backend="wasm_js"` configuration;
- all focused P3 contract, generation, quality, stage, evidence, atlas, and
  performance tests;
- all P0, P2, and P3 ignored Release evidence writers.

One earlier default-parallel library run ended in Windows
`STATUS_ACCESS_VIOLATION` without a Rust failure. A single-thread diagnostic run
passed 443 tests with one ignored, the immediate default-parallel retry passed
the same 443 tests, and the final all-target run passed. The event was therefore
recorded as a non-reproducible native test-process incident rather than hidden
or attributed to a specific algorithm.

## Limits and P4 handoff

P3 is a deterministic Earth-like initial relief model, not a predictive mantle
or landscape simulator. It intentionally does not yet contain atmospheric
circulation, ocean transport, erosion, drainage, sediment redistribution,
cryosphere, soils, vegetation, finished materials, or product lighting.

P4 must consume the immutable P1 surface mappings, V5 forcing, P3 elevation,
bathymetry, and physical ocean mask. It must implement the already locked
global-wind and ocean-circulation design: explicit RK3 remains the small-grid
truth reference; IMEX and split-explicit production candidates use identical
equations and operators; C2 publishes two active atmosphere layers, mixed-layer
and thermocline ocean states, a slow deep-ocean heat reservoir, near-surface
and upper winds, shear, surface currents, SST, thermocline depth, humidity, and
precipitation. Climate may not rewrite P3 relief or replace physical sea level
with an author percentile.
