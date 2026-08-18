# P4 Global Atmosphere-Ocean Completion Record

Date: 2026-08-18  
Design: `2026-08-17-global-atmosphere-ocean-p4-design.md`  
Status: complete and frozen for the P5 handoff

## 1. Delivered boundary

P4 publishes a reconstructable cubed-sphere climate work domain and an atomic
`world.global-circulation` product derived from the authoritative sphere, P3
relief, and authored climate input. C1 publishes one active atmosphere and one
mixed-layer ocean. C2 publishes lower/upper atmosphere,
mixed-layer/thermocline ocean, a slow deep-ocean heat reservoir, and twelve
monthly samples of wind, current, temperature, humidity, precipitation,
orographic precipitation, layer-height anomaly, sea-surface height, and
thermocline depth. It also publishes strict checkpoint, capability, solve,
budget, remap, and quality evidence.

C3 clouds/sea ice and C4 soil/snow/glacier/vegetation feedback remain explicitly
unavailable. P4 is the large-scale climatological circulation boundary consumed
by P5; it is not the finished natural-world product or the final renderer.

## 2. Algorithm conformance and declared extensions

The implementation retains recognizable published numerical methods:

- classic third-order Runge-Kutta is the bounded small-grid truth reference;
- production uses additive slow/fast split-explicit RK3;
- scalar advection is Green-Gauss piecewise-linear finite volume with a
  Barth-Jespersen one-ring limiter and one paired flux per shared edge;
- positivity scales the complete outgoing donor fan and performs deterministic
  conservative bound redistribution independently inside each open-edge
  connected component;
- source/product projection uses conservative spherical-polygon overlaps;
- heat, moisture, and momentum exchange are paired in extensive units;
- mixed-layer free-surface pressure uses full gravity, while the thermocline
  internal interface uses reduced gravity;
- the mixed-layer steric term is the depth-mean Boussinesq closure
  `+0.5 g alpha H grad(T)` evaluated from the prognostic temperature.

Sekai-specific extensions are explicit parts of the versioned equation rather
than hidden replacements:

- immutable cubed-sphere/geodesic work-domain reconstruction and canonical
  two-way conservative maps;
- the fixed C1/C2 layer and pair-specific exchange contract;
- P3-derived fractional coastal permeability and bathymetric thermocline drag;
- accelerated monthly climatological continuation;
- bounded, water-limited orographic condensation and a separately published
  orographic precipitation component;
- horizontal eddy viscosity shared by every compared integrator;
- a C2 annual-mean APE/Eady column Reynolds-stress closure. It diagnoses the
  annual thermal forcing, uses a smooth spherical analytic divergence with no
  authored acceptance-latitude bands, applies the same column acceleration to
  both atmosphere layers, and projects each retained layer profile to zero
  global axial torque;
- strict public identity, cancellation, quality, budget, and checkpoint
  boundaries.

The production transport operator is evaluated over the real `7,200 s` macro
horizon before conversion to a frozen slow tendency. Condensation is limited to
transported available water and the identical retained correction is applied to
precipitation. Formation closure integrates only signed, quantized external
source/sink ledgers; transport and paired internal exchange have zero declared
external contribution, so a leak remains visible instead of being absorbed by
a terminal projection.

Draft, Standard, and High change only surface/work-grid resolution and maximum
formation-cycle budget. They do not select different physical constants.

## 3. Integrator decision

The Release same-equation corpus selects `SplitExplicitRk3V1` in all four
C1/C2 open/coastal fixtures and all 12 months. Across the selected reports the
worst values are:

| Agreement metric | Worst selected value | Gate |
|---|---:|---:|
| Wind/current vector correlation | 0.9992012497 | >= 0.995 |
| Wind/current normalized RMSE | 0.0474717400 | <= 0.05 |
| Air/SST scalar correlation | 0.9999969336 | >= 0.999 |
| Air/SST absolute bias, C | 0.0076453327 | <= 0.1 |
| Precipitation correlation | 0.9999881472 | >= 0.98 |
| Annual precipitation bias | 0.0012309489 | <= 0.01 |

The largest closed annual no-source layer-mass drift is
`3.0969580014e-9`, below `1e-8`. Formation procedure, capability identity,
and conservation semantics also match the RK3 reference.

IMEX remains an intentionally retained losing test implementation. It does not
meet its locked linear residual tolerance and cannot own a P4/P5 product or act
as a runtime fallback. The only selected product ID is
`split-explicit-rk3-v1`.

The comparison artifact is
`target/p4/integrator-comparison.json`: 146,311 bytes, BLAKE3
`93b9ee096622fa9309149b9be7301133012124116e0ede95d2501ec18a2ef6e6`.

## 4. Deterministic 17-seed evidence

The final Release writer generated paired P3/P4 Draft products for 17 fixed
seeds (`20,252` authoritative cells, cubed-sphere `n=24`, `3,456` climate
cells). Every public product passes all 16 locked quality metrics.

- `target/natural-quality/p4/evidence.json`: 90,997 bytes, BLAKE3
  `cd92d747c077d39381db8a5be693b22dcaf29b5f993e68ade2072517e31f8257`;
- `target/natural-quality/p4/metrics.csv`: 35,119 bytes, BLAKE3
  `e095e4677d699eec3c21ff6e147bb8ba0a430efc2cffffa8a16c4bcfe2b1daa9`.

The corpus converges in one to three formation cycles. Its final normalized
annual-cycle residual is `0.1859921020..0.2398630206` against the `0.25`
publication limit. Selected physical summaries are wind RMS
`3.51694..5.80959 m/s`, surface-current RMS
`0.0121416..0.0321110 m/s`, global air temperature
`7.1733..10.1835 C`, and precipitation
`6.22348..10.08944 mm/day`.

| Quality metric | Minimum | Mean | Maximum |
|---|---:|---:|---:|
| Cubed-face seam speed ratio | 0.494551 | 0.948345 | 1.947562 |
| Low-latitude easterly fraction | 0.991876 | 0.997939 | 1.000000 |
| Midlatitude westerly fraction | 0.575006 | 0.700445 | 0.862577 |
| Mixed layer warmer than thermocline | 1.000000 | 1.000000 | 1.000000 |
| Full-land ocean-current leakage, m/s | 0.000000 | 0.000000 | 0.000000 |
| Ocean gyre circulation fraction | 0.403846 | 0.653324 | 0.759259 |
| Orographic precipitation response | 0.180660 | 0.285163 | 0.414043 |
| Rain-shadow correlation | 0.264312 | 0.341908 | 0.471821 |
| Orographic uplift enrichment ratio | 1.246700 | 1.428330 | 1.617420 |
| Positive thermocline-depth fraction | 1.000000 | 1.000000 | 1.000000 |
| Maximum absolute ocean SSH, m | 0.198257 | 0.365523 | 0.790205 |
| Seasonal latitude/temperature correlation | 0.610312 | 0.772388 | 0.825050 |
| Correct hemispheric season fraction | 0.798109 | 0.836404 | 0.876746 |
| Vertical-shear RMS, m/s | 3.252420 | 4.200090 | 5.526010 |
| Warm-ocean humidity contrast | 0.986708 | 1.331100 | 1.868540 |
| SST/humidity correlation, diagnostic | 0.820465 | 0.905409 | 0.949697 |

The seasonal metrics are forcing-conditional. When the exact January/July
forcing amplitude is below `0.5 C`, including a valid zero-axial-tilt input,
both are published as the locked `Unavailable` result rather than as a
fabricated pass.

Maximum formation-budget errors over all 17 products are atmosphere amount
`1.1960942436e-9`, ocean amount `7.3174661417e-13`, moisture
`4.6120364240e-9`, thermal energy `6.0349230109e-9`, and paired exchange
`1.0843634564e-7`.

## 5. Performance, memory, cancellation, and cache

Release command:
`cargo test --release --test global_circulation_performance measure_global_circulation_performance -- --ignored --exact --nocapture`.

| Product | Time | Limit | Conservative dense-owner bound | Peak RSS delta |
|---|---:|---:|---:|---:|
| C1 Draft, n24 | 1.275570 s | 10 s | 28,000,032 B | 17,518,592 B |
| C2 Standard, n32 | 21.138335 s | 30 s | 190,451,232 B | 123,265,024 B |
| C2 High, n48 | 68.048291 s | 120 s | 476,437,152 B | 305,467,392 B |

The owner inventory is a mechanically derived conservative simultaneous-owner
upper bound, not a claim that every allocator has exactly that live-byte
count. The independent 1 ms RSS measurement is the second hard gate. A fresh
High child process retains all prepared upstream owners while measuring the
public quality-gated C2 product; it completes in `67.535666 s` with a
`297,553,920 B` RSS delta. Both measures remain below `512 MiB`.

Active cancellation is requested `582.2 us` after entering work and completes
in `1.787 ms`, below `250 ms`. The final audit also found and fixed a subtle
engine-output cancellation defect: `std::io::ErrorKind::Interrupted` caused
`Write::write_all` to retry artifact hashing. Cancellation now returns a
non-retryable typed error, while dense validation and streaming JSON hashing
poll cooperatively.

A cold five-stage Draft graph completes in `7.156088 s` with `0/5` cache
hits/misses; the identical warm graph completes in `122.793 ms` with `5/0`.
Both produce result hash
`6b7d925bb106c3d3b674e746e5e203a42705766d20c0d9c4903b541ee1332cec`.

`target/natural-quality/p4/performance.json` is 2,866 bytes with BLAKE3
`a6e91ec645be06d13843db5a91b16f5d6181db053edb052b2cce0126e328fa6c`.

## 6. Atlas and manual inspection

The frozen atlas has 19 rows and January/July equirectangular plus globe
columns. It includes P3 elevation; lower and upper wind; vertical shear;
surface current; air, mixed-layer, thermocline, and deep-ocean temperature;
thermocline depth; humidity; total and orographic precipitation; lower and
upper height anomaly; sea-surface height; thermocline height; solver residual;
and remap error.

Seeds 42, 43, and 83 were inspected at full atlas resolution after the final
equation and quality changes. The review found:

- no visible straight cubed-face seam, equatorial checkerboard, or pole spike;
- coherent low-latitude easterlies and midlatitude westerlies, with distinct
  upper/lower wind rather than a copied field;
- basin-confined currents without through-land flow;
- terrain-linked precipitation, orographic enhancement, and rain shadow;
- bounded sub-metre SSH instead of the former reduced-gravity tens-of-metres
  internal-interface response.

Representative identities are:

| Seed | Checkpoint fingerprint | Artifact BLAKE3 | Atlas PNG BLAKE3 |
|---:|---|---|---|
| 42 | `4f3e9abcc7d1fbfa24a32d5dc6046da386b43f3b995bb7eb17d118a737fdfbf8` | `08cc6987175657758cb1f8e4a1f5bfb9fbead06406a2a4101a949eeb86cd2a18` | `ddad3b1273fdbbf6918f7b091eed5da64b7fa4d2c76c446dc483135676e3b188` |
| 43 | `530ded07bdf35b1791240f90cc712fe62247fe7d49ba2ca67c6cabf969542248` | `e5e8ed87ad9dfd3e70f7287b0c0f3dc48034c12a686da02a291159628971d9e0` | `5660f316dfc3260712fe83f6c277d78cc24f39ce5d336a8e26e056b7b5ae5340` |
| 83 | `784df40f9bcee6dd182349f52faada200e3bf5edc7645f91c2f72989befeb392` | `5c75365fee136a9756a0667a296c317d8de05fe721ac672c3288d40c6d1641b4` | `2a4a2a23c4ecd88081f8e090b8c880bbb161c44dc15e9fce9b3a5770cb6de7d2` |

The broad zonal bands are an intentional P4 large-scale backbone, not a claim
of final local realism. P5 drainage/erosion and later weather, soil, ecology,
and material stages must add causal regional structure; rendering may not hide
this limitation with synthetic noise.

## 7. Final verification

The frozen implementation passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --all-targets --all-features -- -D warnings`;
- `cargo check --all-features --lib --target wasm32-unknown-unknown`;
- `cargo test --quiet --all-targets --all-features` in `790.7 s`, including
  `462` passing and one explicitly ignored library test plus every integration,
  binary, benchmark, GPU, spherical, terrain, and climate target;
- the Release integrator comparison suite and writer;
- the Release 17-seed product-quality JSON/CSV writer;
- the Release 19-row atlas writer with artifact-hash matching;
- the Release wall-clock, mechanically bounded memory, isolated High RSS,
  cancellation, cold-cache, and warm-cache writer;
- `git diff --check` after the documentation freeze.

All evidence above was regenerated after the final physical equation, schema,
identity, cancellation, quality, and transport changes.

## 8. Schema policy and P5 handoff

The checkpoint and global-circulation V1 schemas were developed only on this
unmerged, unpublished feature branch. Intermediate branch artifacts are not a
persistence contract. V1 freezes at the first integration of this completed
record; no backward-compatibility promise is made for transient pre-freeze V1
JSON or fingerprints.

P5 receives exact P3 relief plus P4 monthly near-surface/upper wind, surface
ocean current, air/SST/thermocline/deep temperature, lower/upper humidity,
total/orographic precipitation, layer heights, SSH, quality, budget, remap,
forcing, model, and checkpoint identity. P5 must implement bounded coupled
geomorphic formation without mutating or bypassing P4. Any terrain feedback
must return through the explicitly versioned P5 iteration boundary.

P4 is accelerated climatological continuation, not literal multi-year weather
integration. It deliberately has no clouds, sea ice, soil moisture, snowpack,
glaciers, vegetation feedback, ENSO-like variability, or resolved storms.
Those capabilities remain unavailable until their owning stages implement and
validate them.
