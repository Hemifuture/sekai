# P5 Coupled Geomorphic Formation Design

Date: 2026-08-18  
Status: locked implementation child of the complete natural-world design  
Inputs: frozen P1 surface, P2 evolved tectonics, P3 substrate/primary relief,
and P4 production circulation

## 1. Scope and decision

P5 replaces the old spherical `PriorityFloodStreamPowerV1` two-pass modifier as
the production terrain-formation path. The old solver remains a compatibility
and negative-baseline implementation only. It cannot own the new
`world.natural-surface-formation` product and is never a silent fallback.

P5 publishes one atomic, mutually consistent formation state containing:

- final world-scale terrain and physical sea level;
- ocean, lake, and dry-land classification;
- a deterministic drainage DAG, basins, lakes, monthly runoff/discharge,
  Strahler order, and stable directed river reaches;
- tectonic displacement, fluvial incision, hillslope erosion/deposition,
  routed sediment deposition, coastal erosion/deposition, and local isostatic
  response as separate causal fields;
- conservative sediment mass, provenance, ocean/shelf delivery, endorheic
  storage, delta potential, and closure evidence;
- the exact selected P4 equation evaluated on the converged formation terrain;
- a bounded fixed-point report, model/checkpoint identity, capabilities, and
  quality evidence.

P5 does not implement soil, vegetation, groundwater, explicit
evapotranspiration, snow, sea ice, or glaciers. The glacial term in the program
equation is exactly zero and the capability is `Unavailable`; P6 versions the
formation model when C3 cryosphere and surface feedback become available.

## 2. Why the old implementation is insufficient

The existing spherical hydro-erosion code has useful foundations: integer
Priority-Flood ordering on an irregular closed mesh, stable flat routing, a
single-flow drainage DAG, monthly discharge accumulation, lake/basin records,
Strahler order, stable river IDs, and a conservative one-pass sediment ledger.
Those pieces are retained where their contracts remain true.

Its erosion law is not retained. It converts discharge and slope to two bounded
response curves, multiplies by a fixed `300 m` incision ceiling, deposits with a
fixed `50 m` local ceiling, and executes exactly one erosion pass between two
hydrology solves. It has no tectonic-rate integration, implicit stream-power
solve, hillslope transport, coastal response, isostasy, coupled climate
feedback, convergence residual, or transport-capacity state. It is therefore a
bounded current-slice modifier, not the geomorphic formation solver required by
the program.

## 3. Scientific basis and implementation identity

The production algorithm is named
`priority-flood-fastscape-sediment-hillslope-coast-isostasy-v1`.

Recognizable published components are kept separate from Sekai extensions:

- Barnes, Lehman, and Mulla Priority-Flood supplies depression filling and
  watershed ordering on an irregular graph
  (`10.1016/j.cageo.2013.04.024`);
- the Braun-Willett O(N) downstream-stack implicit method advances the
  detachment-limited stream-power equation without an explicit stability limit
  (`10.1016/j.geomorph.2012.10.008`);
- Cordonnier et al. motivate coupling a drainage graph, tectonic uplift, and
  stream-power erosion for large-scale terrain structure
  (`10.1111/cgf.12820`);
- Roering-Kirchner-Dietrich nonlinear slope transport motivates the bounded
  critical-slope hillslope flux (`10.1029/1998WR900090`);
- Davy-Lague/Yuan et al. motivate explicit erosion, sediment transport,
  deposition, and an efficient downstream mass balance
  (`10.1029/2008JF001146`, `10.1029/2018JF004867`).

Sekai does not claim to implement complete FastScape, SPACE, an Exner shallow
water solver, or a calibrated coastal morphodynamic model. Its irregular
spherical finite-volume hillslope operator, effective runoff proxy,
capacity-limited sediment ledger, map-scale coastal exchange, local Airy
response, fixed-point climate coupling, and strict artifact schemas are named
extensions. Every extension is tested independently and included in the model
fingerprint.

## 4. Authoritative inputs and identity

The compound formation stage consumes exactly:

- `SphericalSurfaceArtifact` for cells, edges, metric, and area;
- `NaturalQualityProfileArtifact` for resolution identity only;
- `EvolvedTectonicArtifact` for uplift and subsidence rates;
- `GeologicSubstrateArtifact` for density, lithology, erodibility,
  permeability, fracture, and sediment-source class;
- `PrimaryReliefArtifact` for the immutable constructional terrain and water
  inventory;
- `ClimateWorkDomainArtifact`, the resolved climate input, and the frozen P4
  `GlobalCirculationArtifact`;
- one resolved P5 formation specification.

The P5 checkpoint fingerprints the complete upstream identities, exact
formation equation/constants, fixed-point iteration, retained final fields,
and selected P4 integrator/model. A surface, relief, substrate, tectonic,
climate, work-domain, or model mismatch forces a cold rebuild. View state,
palette, camera, and terrain-detail LOD are not dependencies.

## 5. Governing elevation identity

For every authoritative cell, after `f32` retention:

```text
final_elevation
  = primary_elevation
  + tectonic_displacement
  - fluvial_erosion
  - hillslope_erosion
  + hillslope_deposition
  + routed_sediment_deposition
  - coastal_erosion
  + coastal_deposition
  + isostatic_response
```

All non-signed erosion/deposition fields are finite and nonnegative. No later
clamp may break this identity: if an elevation safety bound is reached, the
owning process is reduced before quantization and its sediment/source ledger is
recomputed from the retained increment.

The modeled formation horizon is `100,000 yr`, divided into eight deterministic
`12,500 yr` geomorphic macro steps. It is a declared coarse-grained formation
horizon, not a world age or a fabricated historical timeline. Draft, Standard,
and High use the same horizon, equations, and constants.

## 6. Bounded outer climate-surface fixed point

The engine graph stays acyclic. Climate/terrain feedback lives only inside the
atomic P5 stage:

```text
frozen P3 terrain + current production climate
  -> complete 8-step geomorphic solve from the same P3 initial state
  -> physical water-volume sea-level solve
  -> selected P4 forcing and circulation on the candidate terrain
  -> final hydrology on candidate terrain and candidate climate
  -> normalized fixed-point residual
```

Outer iteration zero uses the frozen P4 product. Later iterations rebuild
forcing from the candidate elevation, sea level, land mask, bathymetry, and
terrain gradient, then run the same selected split-explicit P4 generator. They
may warm-start when an exact compatible checkpoint is available, but may never
use the preliminary climate or a cheaper preview equation.

Each geomorphic solve restarts from immutable P3 terrain. Outer iterations are
therefore fixed-point iterations, not repeated application of another
`100,000 yr` of erosion. A candidate is published only after its newly solved
climate and final hydrology agree with the terrain that produced them.

The stage performs at most four outer iterations. The convergence report takes
the maximum of independently normalized quantities:

- spherical-area-weighted elevation RMS divided by `100 m`;
- changed receiver area fraction divided by `0.05`;
- area-weighted `log1p(discharge)` RMS divided by `0.15`;
- area-weighted sediment-cover RMS divided by `10 m`;
- changed land/ocean area fraction divided by `0.005`.

Every component must be `<= 1`. Non-convergence is a typed failure carrying the
best report; no last iterate or partial artifact is published.

## 7. Hydrology and depression semantics

Elevations are quantized to centimetres only for topology-changing comparisons.
Priority keys are `(filled_height_cm, CellId)`, adjacency is visited in stable
`CellId` order, and the flood dequeue rank breaks flats without random
perturbation. A receiver is a real neighbor and has a strictly earlier drainage
key, proving the graph acyclic.

Cells below the current physical sea level are ocean terminals. A depression
shallower than the greater of `1 m` and one retained elevation quantum is a
numerically insignificant pit and is routed across its filled surface without
publication as a lake. Deeper connected depressions become lake candidates.

For every candidate, the solver compares spill volume with `1,000 yr` of
catchment effective runoff. A depression that cannot reach its spill level in
that declared residence horizon becomes an endorheic `ClosedSink`; otherwise it
has one stable spill cell and downstream receiver. This is an explicit P5
formation proxy, not a claim to model lake evaporation. P6 replaces the proxy
with the final water balance while retaining the same outlet categories.

Monthly P4 precipitation is a mean `mm day-1` rate. P5 multiplies by the exact
climatological month duration and the bounded formation-runoff coefficient:

```text
runoff_fraction = 0.15 + 0.70 * (1 - relative_permeability)
monthly_runoff_mm = precipitation_mm_day * days_per_month * runoff_fraction
```

Ocean runoff is zero. This is deliberately named effective formation runoff;
soil storage, evapotranspiration, snow, and groundwater are unavailable until
their owning phases. Monthly water volume and contributing cell area are
accumulated once in upstream-to-downstream order. Annual discharge equals the
sum of monthly volumes divided by the exact climatological year.

Basins, lakes, and river segments have contiguous stable IDs assigned from
sorted terminal/cell tuples. Lakes contain no fabricated internal river
segments. A segment is emitted only along a real receiver edge above the fixed
discharge threshold, and its length is the authoritative great-circle
center-to-center distance. Strahler order is computed over the same emitted
network.

## 8. Implicit tectonic-stream-power solve

The net external rock forcing is

```text
U_i = (uplift_rate_i - subsidence_rate_i) / 1000   [m yr-1].
```

For non-ocean cells with receiver `r`, the fluvial equation is

```text
dh_i/dt = U_i - K_i A_i^m max((h_i - h_r) / L_i - S_t, 0)^n
m = 0.5
n = 1
S_t = 1e-5
```

`A_i` is the routed effective area in square metres, `L_i` is receiver-edge
length, and the reference erodibility is `K0 = 5e-6 yr-1`. The local
coefficient is

```text
K_i = K0
    * (0.25 + 1.50 * substrate_erodibility_i)
    * sqrt(clamp(annual_runoff_i / 1000 mm, 0.10, 4.0)).
```

The units follow the `m=0.5` area convention. The solver first applies retained
tectonic displacement, then visits cells downstream-to-upstream. For an active
reach, with `c = dt K_i sqrt(A_i) / L_i`, the `n=1` backward-Euler solution is

```text
h_i_new = (h_i_forced + c * (h_r_new + L_i S_t)) / (1 + c).
```

If the result is not above threshold, the reach is inactive and no incision is
recorded. Ocean/lake base levels and all retained receiver elevations are never
crossed. This is the Braun-Willett implicit method specialized to the locked
`n=1` equation, not an explicit response curve or a post hoc valley carving
filter.

## 9. Hillslope transport

Land-land shared edges carry one paired finite-volume sediment flux from high
to low elevation. The coarse-grid effective nonlinear law is

```text
q_edge = D_eff * edge_length * slope
       / max(1 - (slope / S_c)^2, 0.10)            [m3 yr-1]
S_c = tan(32 deg)
D0 = 5,000 m2 yr-1
```

`D_eff` is `D0` times a bounded lithology/fracture/weathering factor derived
from the same P3 substrate and P4 annual precipitation. The large value is
explicitly an unresolved world-cell diffusivity; it is not a measured local
soil-creep coefficient.

Flux is limited by donor area, local relief, and the critical-slope target so a
step cannot invert a pair or export more material than retained. Both cells are
updated from the same quantized mass transfer. This provides conservative
diffusion and a bounded landslide surrogate without random scars. P9 may derive
subcell scar texture but cannot alter the authoritative mass or drainage.

## 10. Sediment production, transport, and deposition

Fluvial, hillslope, and coastal removal are converted to mass using the P3
cell-specific substrate density. Deposits use a fixed coarse alluvial bulk
density of `1,800 kg m-3`. Five source-rock mass channels follow the P3
`SedimentSourceKind` inventory; every local deposit publishes normalized
fractions and a stable dominant class.

For each drainage cell in upstream-to-downstream order:

```text
available = upstream_mass + locally_produced_mass
capacity  = C0 * discharge * dt_seconds
          * sqrt(max(slope, 0) / (max(slope, 0) + 0.001))
C0 = 0.5 kg m-3
deposited = min(max(available - capacity, 0), local_accommodation)
outgoing  = available - deposited
```

Lake and closed-basin accommodation derives from the Priority-Flood spill
volume. Non-lake floodplain accommodation is bounded by local filled-surface
relief and `50 m` per macro step. A capacity or elevation limit reduces
deposition, never discards the remainder.

At an ocean receiver, sediment first enters adjacent shelf accommodation. The
retained shelf fraction decreases with annual wind/current exposure; its
ratio to marine transport capacity is published as `delta_potential` in
`0..=1`. Remaining mass becomes explicit deep-ocean delivery. Endorheic
terminal storage is a deposited mass, not an unexplained export.

The global signed ledger is evaluated from retained `f32` layer changes and
`f64` masses:

```text
produced sediment mass
  = land/lake/shelf deposited mass
  + deep-ocean delivery
  + final in-transit mass.
```

Relative closure must be `<= 1e-8`; each provenance channel must close to
`<= 1e-7`. Transport is not allowed to hide an erosion or deposition error by
placing full tendencies on the expected side of the budget.

## 11. Coastal response and local isostasy

Only current land-ocean edges participate in coastal exchange. Annual P4
near-surface wind and surface-current vectors are projected onto authoritative
edge-normal/alongshore directions. A bounded wave-exposure proxy controls a
maximum `2e-5 m yr-1` bedrock-coast erosion rate, multiplied by local
erodibility and reduced by existing sediment cover. Removed mass enters the
same shelf/alongshore sediment ledger; deposition is never painted
independently of a source.

P5 uses local Airy unloading/loading, not elastic flexure:

```text
isostatic_response_m
  = removed_mass / (rho_mantle * cell_area)
  - deposited_mass / (rho_mantle * cell_area)
rho_mantle = 3300 kg m-3.
```

This response is applied once from the retained process mass, separately
published, and included in the elevation identity. The fixed P3 liquid-water
inventory is solved against every candidate terrain with the existing exact
piecewise-linear water-volume operator. P5 may move the coastline, but it may
not tune sea level to an authored land fraction.

## 12. Public schema and atomic product

`NaturalSurfaceFormationSnapshot` V1 contains:

- `SurfaceFormationCheckpoint` and model fingerprint;
- `FormationTerrainFields` with the exact component inventory from section 5;
- sea level, realized water volume, and `LandOceanField`;
- final `SphericalHydrologySnapshot`;
- sediment thickness, five provenance fractions, throughput, shelf delivery,
  deep-ocean delivery, endorheic storage, and delta potential;
- final formation `GlobalCirculationSnapshot`;
- `FormationSolveReport`, `SedimentBudgetReport`, and capability set.

Portable snapshots use strict bounded serde and validate every self-contained
invariant. `NaturalSurfaceFormationArtifact` is Serialize-only and can be
created publicly only by running the authoritative inputs through the locked
generator/evaluator. Contextual validation recomputes upstream identities,
terrain component sums, water volume, receiver adjacency, river lengths,
sediment budgets, climate/terrain forcing identity, model fingerprint, and
quality thresholds.

The engine stage ID is `natural.surface-formation@1`; the artifact key is
`world.natural-surface-formation`. No intermediate terrain, hydrology, or
climate iterate enters the engine cache.

All dense loops poll cancellation at most every 256 cells or 64 KiB of
serialization. Cancellation after work begins must complete within `250 ms`.
Failure or cancellation publishes nothing and leaves the last valid product
untouched.

## 13. Acceptance gates

### 13.1 Analytic and conservation gates

- Priority-Flood, flat routing, lakes, closed sinks, and receiver DAG match
  analytic irregular-graph fixtures;
- every receiver is an authoritative neighbor and every land cell reaches an
  ocean, lake, or declared closed sink;
- monthly/annual runoff and discharge close to `<= 1e-9` relative in `f64`;
- the implicit `n=1` update matches its closed form, preserves base level, and
  remains stable for time steps that make the rejected explicit method fail;
- zero uplift/erosion/diffusion/coast inputs leave terrain byte-identical;
- paired hillslope and coastal transfers conserve retained mass;
- global sediment closure is `<= 1e-8`, provenance closure `<= 1e-7`, and
  physical water-volume closure remains within the P3 bound;
- the exact retained component sum reconstructs final elevation;
- deterministic repeats are byte-identical and wrong upstream/model identities
  are rejected.

### 13.2 Causal counterfactuals

- higher P4 precipitation raises runoff/discharge without changing topology;
- higher permeability lowers effective formation runoff;
- higher P3 erodibility increases incision under equal uplift/runoff;
- zero discharge or sub-threshold slope produces zero fluvial incision;
- higher uplift steepens/raises the steady profile rather than painting an
  unrelated ridge;
- nonlinear hillslope transport increases near the critical slope and remains
  paired;
- sediment cannot deposit without upstream/local production;
- low capacity/lake accommodation increases deposition while high capacity
  increases throughput;
- coastal erosion requires an actual land-ocean edge and nonzero exposure;
- changing only candidate terrain changes the rebuilt production forcing and
  final climate checkpoint.

### 13.3 Corpus morphology gates

Across the paired 17-seed corpus:

- every nontrivial land world has at least one basin and river reach, and at
  least `95%` of land area belongs to a valid outlet path;
- the largest river network reaches Strahler order `>= 3` at Draft and `>= 4`
  at Standard/High when land area exceeds `10%`;
- fluvial-incision mass is enriched by at least `1.5x` over the joint
  high-discharge/high-slope support relative to its land-area fraction;
- deposited sediment is enriched by at least `1.25x` in lakes, floodplains,
  terminal basins, deltas, and shelves relative to eligible area;
- area-weighted P3/final elevation correlation remains `>= 0.90`, while final
  drainage relief and river-valley incision are nontrivial;
- final land fraction changes by at most `0.03` absolute and no water-volume
  target is violated;
- no isolated one-cell lake, cyclic river, polygon-aligned global drainage
  ring, cubed-face seam, pole artifact, or through-ocean land river is visible
  in the fixed atlas;
- all fixed-point components pass within four outer iterations.

The old two-pass product is recorded as a failed baseline. P5 is not accepted
merely because it looks more detailed; it must pass the scientific and causal
gates above.

### 13.4 Performance and memory

Release end-to-end targets, including any selected P4 re-solves, are:

- Draft `<= 15 s`;
- Standard `<= 90 s`;
- High `<= 300 s`;
- High conservative dense-owner inventory and isolated peak RSS delta each
  `<= 1 GiB`;
- active cancellation `<= 250 ms`;
- a warm identical graph is all-hit and returns the same result hash.

## 14. Evidence and P6 handoff

P5 completion requires analytic tests, strict schema/identity negatives,
old-versus-new baseline evidence, a deterministic 17-seed JSON/CSV report,
fixed 2D/globe atlases, Release time/RSS/cancellation/cache records, native and
WASM checks, and manual inspection of seeds 42, 43, and 83.

P6 receives immutable final P5 terrain, formation climate, hydrology, lakes,
sediment/provenance, coast/delta state, and checkpoint identity. It versions the
compound formation equation to add C3 clouds, surface energy/moisture exchange,
snow, sea ice, and glaciers, then runs the required final non-mutating climate
and hydrology consistency passes. It must not reinterpret the P5 effective
runoff proxy as final evapotranspiration or groundwater truth.
