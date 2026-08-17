# P4 Global Atmosphere-Ocean Circulation Design

Date: 2026-08-17  
Status: locked implementation child of the approved complete natural-world and layered-climate designs

## 1. Scope and decision

P4 delivers C0, C1, and C2 of the approved layered climate architecture on the
immutable P3 world. It does not reuse the empirical preliminary-climate wind
bands as physical truth and it does not promote `BalancedSteadySolver`, which
the frozen comparison evidence proved was not WYSIWYG-equivalent to the RK3
transient equations.

The existing explicit RK3 transient shallow-water implementation remains the
small-grid truth reference. P4 implements two production candidates under the
same state, spatial operators, forcing, local physics, and diagnostic projector:

1. matrix-free IMEX Crank-Nicolson for linear gravity-wave, Coriolis, drag, and
   layer-exchange terms, with conservative slow transport and local physics;
2. split-explicit RK3, with slow physics on a large step and the existing fast
   shallow-water kernel on deterministic substeps.

The production candidate is selected only after the locked RK3 agreement gates
pass. Speed cannot qualify an inaccurate candidate. The losing implementation
remains a test strategy and has no product/UI mode.

## 2. Existing foundation and compatibility

The following existing components remain authoritative and are reused rather
than rewritten:

- `CubedSphereGrid` geometry, paired shared edges, tangent vectors, and closed
  area accounting;
- `CirculationOperators` gradient, divergence, Coriolis, and conservative edge
  flux conventions;
- `TransientShallowWaterSolver` as the V1 single-layer explicit RK3 reference;
- `CirculationSnapshot` V1 and all frozen solver-comparison evidence;
- P1 `ConservativeSurfaceMap` and typed remap operators;
- P3 `PrimaryReliefSnapshot` elevation, physical sea level, bathymetry, and
  land/ocean classification.

P4 adds new schemas and stage identities. It does not mutate or reinterpret V1
circulation experiments, preliminary climate V1/V2, P0 evidence, or P2/P3
artifacts.

## 3. Climate work domain and exact P3 coupling

The cubed sphere becomes a reconstructable work domain, never a second world
surface. `CubedSphereGrid` publishes a lossless conversion of its canonical
vertices, quadrilateral cells, shared edges, areas, and tangent normals into a
validated `SphericalSurfaceSnapshot` V2. V1 remains the strict spherical
Voronoi contract; V2 adds a distinct `SphericalGeodesicV2` identity for generic
closed geodesic finite-volume meshes and deliberately does not claim that
cubed-sphere cell sites generate Voronoi bisectors. Both schemas retain the
same manifold, metric, orientation, bounded-allocation, area-closure, and
fingerprint checks. Conservative remapping accepts both explicit spherical
families. The grid and converted-surface fingerprints are both retained and
cross-validated.

Cubed-sphere seam welding uses exact integer face-lattice topology keys rather
than rounded floating-point coordinates. This is required for the locked
`n=24/32/48` production resolutions and preserves all pre-existing V1 grid
fingerprints at the reference fixtures.

`ClimateWorkDomainSnapshot` contains:

- quality profile and face resolution (`24`, `32`, or `48`);
- authoritative P3 `SurfaceRef`;
- validated cubed-sphere surface geometry;
- authoritative-to-climate and climate-to-authoritative conservative overlap
  maps;
- overlap count, balance statistics, and conservative-closure diagnostics.

P3-to-climate forcing uses exact spherical polygon overlaps:

- elevation and other intensive scalars use bounded area-weighted remapping;
- land fraction is the overlap-weighted physical P3 land mask, not crust kind;
- bathymetry is elevation relative to the solved physical sea level;
- vector fields are transported in three dimensions and projected tangent;
- output precipitation and other fluxes are remapped as extensive monthly
  amounts before returning to per-area rates.

Deleting this work domain and rebuilding it cannot change P3 or public climate
semantics.

## 4. C0 contracts

### 4.1 Fixed model profiles

`ClimateModelProfile` is a closed enum, not an arbitrary layer count:

- `C1SingleLayerV1`: one active atmosphere and one active mixed-layer ocean;
- `C2LayeredV1`: lower and upper active atmosphere, mixed-layer and thermocline
  active ocean, plus one slow deep-ocean heat reservoir.

`ClimateLayerLayout` stores stable semantic roles, not public numeric layer
indices. It validates exact role inventory, positive reference thickness,
density/heat capacity, and bounded coupling coefficients.

### 4.2 Integrator and checkpoint identity

`ProductionIntegratorId` has `ImexCrankNicolsonV1` and
`SplitExplicitRk3V1`. `ClimateCheckpoint` includes grid, forcing, model,
algorithm, quantization, and input fingerprints. Any mismatch is a typed cold
start, never a guessed migration.

### 4.3 Capabilities and stable output

P4 publishes `GlobalCirculationSnapshot` on the authoritative surface with
monthly climatologies for:

- near-surface wind;
- upper wind;
- vertical wind shear;
- surface ocean current;
- air and sea-surface temperature;
- thermocline temperature and depth;
- specific humidity and precipitation;
- lower/upper atmosphere height anomalies;
- sea-surface and thermocline height anomalies;
- deep-ocean temperature/heat-reservoir proxy.

Public fields are named by scientific meaning. Internal layer indices and the
cubed-sphere face layout do not leak downstream. The snapshot carries explicit
`SeasonalMeanV1` and `VerticalStructureV1` capabilities, model/integrator
identity, solve statistics, convergence history, mass/energy budgets, remap
closure, and immutable input fingerprints.

## 5. Shared layered equations

Each active shallow-water layer uses the common finite-volume form:

```text
d eta_l / dt = -H_l div(u_l)
               + lambda_l (eta_eq_l - eta_l)
               + sum exchange_eta(l, k)

d u_l / dt = -g'_l grad(eta_l)
             - f k x u_l
             - r_l u_l
             + external_l
             + sum exchange_momentum(l, k)
```

Lower-atmosphere temperature and humidity, both atmospheric momentum fields,
mixed-layer/thermocline temperature, and ocean momentum use paired exchange
terms. Every pair is accumulated once with equal and opposite extensive
budgets after heat-capacity/density conversion.

The deep-ocean reservoir has no horizontal velocity. It exchanges heat only
with the thermocline on a declared slow timescale. It cannot create or destroy
energy outside the explicit radiative/surface budgets.

Ocean edge permeability comes from the overlap-weighted P3 land fraction.
Normal transport is zero through fully blocked coastal edges; fractional cells
use one shared symmetric permeability. No solver-specific coastline logic is
allowed.

## 6. Time integration

### 6.1 RK3 reference

The explicit classic RK3 path evaluates the complete shared C1/C2 tendency
system and is used only at bounded small resolutions. Its quantized outputs are
the agreement reference and it remains independently runnable.

### 6.2 IMEX candidate

IMEX uses a centered Crank-Nicolson update for linear height/momentum,
Coriolis, drag, and paired vertical exchange. Local tangent `I + aI + bJ`
blocks are inverted analytically. Eliminating momentum yields a matrix-free
scalar/multilayer Helmholtz operator solved by bounded restarted GMRES with a
declared diagonal preconditioner, tolerance, iteration count, and residual.

Transport and nonlinear local sources use one paired-edge, piecewise-linear
finite-volume step. Green-Gauss gradients are limited with the
Barth-Jespersen one-ring limiter; nonnegative tracers additionally scale each
donor's outgoing fan to its available extensive amount. Because the sampled
cell velocity is only discretely near-solenoidal, a final deterministic
mass-conserving bound redistribution clips cell means to their original
one-ring extrema and redistributes the clipped extensive residual according to
remaining bound capacity. This projection is explicit, allocation-free in the
supplied workspace, and never changes an edge flux asymmetrically. A failed
linear solve, non-positive layer thickness, or budget excess is a typed
failure; the solver cannot silently fall back to different physics.

### 6.3 Split-explicit candidate

Split-explicit uses a slow advective/thermodynamic step and deterministic fast
RK3 substeps chosen from the same gravity-wave and Coriolis stability limits as
the reference. Slow tendencies are held consistently over the fast cycle and
paired exchange is applied once per slow step.

### 6.4 Second-order transport

Production scalar transport uses piecewise-linear reconstruction with a stable
Barth-Jespersen-style monotonic limiter on the cubed-sphere adjacency graph.
One shared edge flux updates donor and receiver together. A positivity limiter
scales the complete outgoing fan, preserving nonnegative humidity and positive
layer amounts without cell-order dependence. First-order upwind remains a
reference operator.

## 7. C2 physical closure

The fixed C2 layout is:

| Role | Primary meaning | Key coupling |
|---|---|---|
| Lower atmosphere | near-surface weather-bearing layer | surface exchange, upper-atmosphere momentum/heat/moisture |
| Upper atmosphere | free-tropospheric/first-baroclinic response | lower-layer exchange and radiative equilibrium |
| Mixed-layer ocean | SST and wind-driven surface current | wind stress, thermocline and surface heat exchange |
| Thermocline ocean | subsurface heat and pressure memory | mixed-layer entrainment and deep-ocean exchange |
| Deep reservoir | slow ocean heat background | thermocline heat only |

Orographic forcing uses P3 elevation; ocean forcing uses physical bathymetry.
Seasonal radiation uses the authored axial tilt and temperature/moisture scale.
C2 does not yet include C3 clouds, soil moisture, snow, glaciers, vegetation,
or sea ice; their capability states remain explicitly unavailable.

## 8. Stage graph and atomicity

`ClimateWorkDomainStage` depends only on the authoritative spherical surface
and natural quality profile. `GlobalCirculationStage` depends on the work
domain, P3 relief, resolved climate input, and authoritative surface.

The isolated P4 graph appends both stages to `primary_relief_graph`. Stage IDs
and artifacts are:

- `natural.climate-work-domain@1` -> `world.climate-work-domain`;
- `natural.global-circulation@1` -> `world.global-circulation`.

All work arrays and checkpoints are private until the complete public snapshot
and quality report validate. Cancellation or any error publishes neither a
partial work domain nor partial climate.

## 9. Acceptance gates

### 9.1 Numerical and scientific hard gates

- shared-edge volume/tracer/heat exchanges close to relative `<= 1e-6`;
- closed no-source annual layer-mass drift is `<= 1e-8` on the analytic fixture;
- every velocity is tangent and every full-land ocean velocity is zero;
- layer depths are positive, humidity is nonnegative, and all fields are finite;
- reverse output remap closes extensive monthly precipitation to `<= 1e-6`;
- component, monthly-summary, shear, and thermocline identities are exact after
  publication quantization;
- repeated fixed-input output and diagnostics are byte deterministic;
- cancellation latency is `<= 250 ms` once a solver work loop is active.

### 9.2 Production candidate agreement

Against the same-equation RK3 small-grid reference, for every fixture and month:

- wind and current vector correlation `>= 0.995`, normalized RMSE `<= 0.05`;
- air/SST correlation `>= 0.999`, absolute area-weighted bias `<= 0.1 C`;
- precipitation correlation `>= 0.98`, annual total bias `<= 1%`;
- capability set, formation cycle, and conservation interpretation match.

Any failed metric disqualifies the candidate. Thresholds are not relaxed to
select a winner.

### 9.3 C2 morphology and climate structure

The fixed water/two-basin/Earth-like fixtures and paired 17 P3 seeds must show:

- equatorial lower-atmosphere easterlies and midlatitude westerlies with
  hemisphere/season reversal where forcing demands it;
- upper winds and nonzero, finite vertical shear rather than copied lower wind;
- basin-confined surface currents, no through-land flow, and paired gyre
  circulation responding to wind stress and rotation;
- warm mixed layer over a cooler thermocline in eligible low/midlatitude ocean,
  positive bounded thermocline depth, and slower deep-reservoir response;
- warm-ocean moisture supply, orographic precipitation enhancement, and a
  downstream rain-shadow signal;
- no independent cubed-face seam, pole spike, global ring, or P3 coastline
  displacement in map/globe atlases.

### 9.4 Product budgets

Release targets remain those already approved:

- C1 `n=24 <= 10 s`;
- C2 `n=32 <= 30 s`;
- C2 `n=48 <= 120 s`;
- C2 `n=48` core state, workspace, and climate output `<= 512 MiB`.

Standard and High execution is asynchronous and cancellable. The product may
display only the last valid snapshot while updating; it may not display a
different approximate climate.

## 10. Evidence and completion

P4 completion requires analytic tests, candidate/RK3 comparison JSON, a
17-seed quality report, deterministic evidence hashes, fixed map/globe atlases
for all public wind/ocean/thermal/moisture fields, Release performance and
memory records, native/WASM/full-workspace gates, and manual review of at least
seeds 42, 43, and 83. The completion record must state the winning integrator,
every failed candidate, all declared procedural closures, and the exact P5
formation handoff.
