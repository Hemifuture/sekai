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

Its canonical fingerprint covers both complete directed conservative maps,
including schema, source/target identities and areas, CSR row offsets, every
overlap weight and tangent transform, and solve statistics. Consequently a
checkpoint cannot be resumed against a different-but-still-balanced remap.

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
algorithm, quantization, and input fingerprints. The input fingerprint includes
the exact climate work-domain fingerprint, not only the two endpoint surfaces.
Any mismatch is a typed cold start, never a guessed migration.

The model fingerprint is the canonical identity of the complete active
equation set, not merely the layer inventory. It covers the pair-specific
exchange table, pressure/drag/relaxation coefficients, horizontal viscosity,
bathymetric drag, Boussinesq steric constants, moisture and ocean bounds,
initial upper-humidity fraction, accelerated-formation schedule and residual
scales, and the declared discrete transport/Reynolds-stress semantics. A
change to any of these constants is a different checkpoint model.

### 4.3 Capabilities and stable output

P4 publishes `GlobalCirculationSnapshot` on the authoritative surface with
monthly climatologies for:

- near-surface wind;
- upper wind;
- vertical wind shear;
- surface ocean current;
- air and sea-surface temperature;
- thermocline temperature and depth;
- specific humidity, total precipitation, and its independently published
  water-limited orographic component;
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
             + div(K_l grad(u_l))
             + external_l
             + sum exchange_momentum(l, k)
```

Revision 2026-09-02 (milestone A2, `2026-09-02-p4-zonal-asymmetry-design.md`):
the lower atmosphere is shallow water over topography (Vallis 2017 §3.1). Its
pressure gradient still reads the layer top `eta`, but the transported
thickness is `H_ref - z_b + eta` with the terrain floor
`z_b = land_fraction * max(elevation - sea_level, 0)`, capped so the layer
keeps at least `H_ref / 6`. Its Rayleigh rate is
`r_lower = r_sea * (1 + (rho - 1) * land_fraction)` with `rho = 3`, the
grassland-to-open-sea bulk drag ratio (Garratt 1992 §4.1). Both terms are
per-cell constants of the bound forcing and enter the equation-model
fingerprint (v10).

Revision 2026-09-02 (milestone A3, `2026-09-02-p4-land-evapotranspiration-design.md`):
land evaporates back the non-runoff share of its own precipitation,
`E_land = land_fraction * (1 - runoff_fraction(kappa)) * P` with the P5 runoff
partition (steady bucket balance, Manabe 1969), applied as one Picard pass in
the moisture step; its latent heat is taken from the lower atmosphere. Orographic
lifting and the orographic quality metrics use the over-flow wind (the C2 upper
layer) because the terrain-aware lower layer flows around ridges. Forcing
fingerprint v4, equation-model fingerprint v11.

Revision 2026-09-03 (milestone A4, `2026-09-03-p4-water-heat-correction-design.md`):
the monthly equilibrium targets are no longer the instantaneous monthly gray
equilibrium (which has no heat inertia and diverges in polar night). Each cell
solves the linear energy-balance equation `C dT/dt = ASR(t) - [ASR_ann +
B (T - T_eq,ann)]` (Budyko 1969; North & Coakley 1979) exactly per month and
publishes the month means of its periodic solution: the mixed-layer target with
`C = C_ml`, the lower-air target with `C = C_air + (1 - land) C_ml`. The
12-month mean of every target equals the annual gray target `T_eq,ann`
(with the orographic lapse), so the annual-mean initial state is that target
(clamped once per role), and the TOA gray longwave is linearized about the
annual state, `OLR = ASR_ann + B_ann (T_s - T_eq,ann)`, so seasonal storage
appears as a seasonal TOA imbalance instead of being forced to zero every
month. Forcing fingerprint v5, equation-model fingerprint v12.

Lower-atmosphere temperature and humidity, both atmospheric momentum fields,
mixed-layer/thermocline temperature, and ocean momentum use paired exchange
terms. Every pair is accumulated once with equal and opposite extensive
budgets after heat-capacity/density conversion.

The paired update is projected onto the two actual `f32` tendency lattices,
never retained on only one side. A nearest representable pair is accepted only
when its relative extensive imbalance is `<=5e-7` and its retained exchange
magnitude differs from the requested flux by `<=1e-3`; otherwise a bounded
neighbouring-ULP search selects the closest balanced pair. If neither side can
represent a sub-ULP exchange, both retain zero. These two tolerances and this
projection semantic are part of the equation-model fingerprint.

The deep-ocean reservoir has no horizontal velocity. It exchanges heat only
with the thermocline on a declared slow timescale. It cannot create or destroy
energy outside the explicit radiative/surface budgets.

Unresolved horizontal eddy mixing is a shared finite-volume momentum term,
not a post-processing filter. It uses positive-permeability edges, parallel
transports both endpoint velocities to the shared edge, applies the same
opposite edge impulse, and projects the accumulated acceleration tangent at
the cell center. The physical kinematic viscosities are `1e6 m2 s-1` for both
atmosphere layers and `1e3 m2 s-1` for both ocean layers. Closed coastal edges
therefore exchange neither tracer nor momentum. The thermocline additionally
uses bathymetric bottom drag with a `90 day` reference timescale scaled by
`1000 m / max(depth, 1000 m)` and by water fraction; changing P3 bathymetry
thus changes the ocean solution while preserving the same land mask.

C2 also diagnoses the unresolved monthly-mean baroclinic eddy momentum flux
that the accelerated `7,200 s` climatological continuation cannot spin up as
explicit multi-hundred-day weather. Because this background eddy field has
multi-month memory, an area-weighted fit to the exact annual-mean forcing
`T_eq = T0 + b sin^2(phi)` supplies `DeltaT = max(-b, 0)`; the resolved
pressure/radiative terms still own monthly seasonality. The unresolved eddy
velocity uses the available-potential-energy scale
`U_e = min(sqrt(g H DeltaT / T0), 65 m s-1)`. This is a total horizontal eddy
RMS scale, not a Held-Hou mean angular-momentum wind. Synoptic Eady activity
vanishes with `|f|` at the equator, so the regular column-distributed stress is
`u'v' = C U_e^2 sin(phi)|sin(phi)| cos^2(phi)`, with `C=2/3`. Since the
latitude factor has maximum magnitude `1/4`, the retained covariance is at
most `U_e^2/6`, one third of the Cauchy bound `|u'v'| <= 0.5 U_e^2`.
Its spherical zonal divergence
is proportional to `2 |sin(phi)| cos(phi) (3 sin^2(phi) - 1)`: it is finite and
zero at both the equator and poles, decelerates the subtropics, accelerates the
extratropics, and contains no authored acceptance latitude bands. After `f32`
retention, a layer-uniform angular
acceleration removes the residual global axial torque to relative `<=1e-6`.
The same acceleration profile acts in both resolved atmosphere layers, which
is the vertically unresolved column closure representable by C2; each layer's
quantized profile is independently projected to zero global axial torque.
Declared conservative lower/upper exchange still controls resolved shear.
The closure is C2-only, is recomputed from the exact
bound annual thermal forcing, and does not alter height or layer mass. A tested mass-only Eady overturning trial
was rejected because it could not carry heat, humidity, and momentum with the
exchanged mass; no such source exists in the production equation.

Consequently, every internal production layer-height/amount term is the
conservative divergence path advanced by the selected fast RK3 subsystem. The
only remaining slow height term is declared external thickness relaxation and
is recorded in the signed external amount ledger. The annual closed fixture
disables that external relaxation and advances all internal height terms for
twelve climatological months. It therefore exercises the complete selected
no-source layer-mass path; it is not a partial substitute that omits a slow
internal mass exchange.

Lower-atmosphere Rayleigh friction uses the standard one-day boundary-layer
timescale; the upper free-tropospheric layer uses ten days. This distinction is
also required by the accelerated formation procedure: five-day lower drag left
an unresolved inertial phase after only eight Draft continuation cycles and
made the sign of the monthly near-surface jet depend on cycle phase rather than
the bound forcing. The one-day value damps that numerical memory while the
shared pressure, Coriolis, eddy-stress, and exchange terms determine the wind.

`OceanMixedLayer.eta` is the published free-surface height anomaly, not an
internal-interface displacement. Its pressure mode therefore uses standard
gravity `g = 9.80665 m s-2`; using the thermocline reduced gravity here would
amplify the public SSH response by roughly `g / g'`. For a vertically uniform
100 m Boussinesq mixed layer with `rho = rho0 (1 - alpha T)`, depth-averaging
the hydrostatic horizontal pressure gradient gives
`du/dt = -g grad(eta) + 0.5 g alpha H grad(T)`, with Earth-like seawater
`alpha = 2e-4 K-1`. Thus a warm column initially accelerates water toward the
warm region and builds a positive steric surface displacement. The separate
thermocline height is an internal-interface/thickness anomaly: its fixed
reduced gravity already closes that baroclinic pressure response, so the same
surface-temperature gradient is not applied a second time to thermocline
momentum.

Ocean edge permeability comes from the overlap-weighted P3 land fraction.
Normal transport is zero through fully blocked coastal edges; fractional cells
use one shared symmetric permeability. No solver-specific coastline logic is
allowed. Partial coastal cells also receive a one-day maximum unresolved
shelf/island form-drag tendency scaled linearly by land fraction. This drag is
part of the same shared momentum equation used by all three integrators; only
exact full-land velocity is zeroed after a step as an invariant check, and no
fractional post-step damping is permitted.

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
remaining bound capacity. Gradient stencils, extrema, fluxes, and final
redistribution all use the same graph of strictly positive-permeability edges,
and the correction is performed independently in each connected component;
two fully separated ocean basins therefore cannot exchange clipped residual.
This projection is explicit, allocation-free in the supplied workspace, and
never changes an edge flux asymmetrically. A failed
linear solve, non-positive layer thickness, or budget excess is a typed
failure; the solver cannot silently fall back to different physics.

### 6.3 Split-explicit candidate

Split-explicit uses a slow advective/thermodynamic step and deterministic fast
RK3 substeps chosen at a conservative `0.20` gravity-wave/advection/Coriolis CFL
from the same stability limits as the reference. Transport, radiative/local
physics, diagnosed Reynolds stress, and paired heat/moisture exchange are
evaluated once and held consistently over the fast cycle. Horizontal momentum
diffusion and conservative paired momentum exchange are re-evaluated at every
RK stage together with pressure/divergence/Coriolis; freezing either
velocity-dependent term over the full macro step admits a collocated
equatorial grid mode at High resolution, while freezing momentum exchange also
failed the locked ocean-current comparison. The monotone finite-volume
transport operator is evaluated over the actual `7,200 s` slow-step horizon
and converted back to a frozen tendency. This is essential: evaluating its
outgoing-fan positivity limiter over an arbitrary one-second horizon and then
extrapolating that tendency would bypass the limiter at high resolution.

### 6.4 Second-order transport

Production scalar transport uses piecewise-linear reconstruction with a stable
Barth-Jespersen-style monotonic limiter on the cubed-sphere adjacency graph.
One shared edge flux updates donor and receiver together. A positivity limiter
scales the complete outgoing fan, preserving nonnegative humidity and positive
layer amounts without cell-order dependence. First-order upwind remains a
reference operator.

The shared C1/C2 tendency invokes this operator for every active-layer
temperature and for both lower- and upper-atmosphere specific humidity where
the profile provides them. IMEX excludes these
nonlinear terms from its matrix-free linear operator and advances them
explicitly. The selected split-explicit path freezes one evaluated slow
tendency over each macro step, as required by its comparison contract. After
the conservative humidity transport, the physical condensation sink is
limited to the transported water actually available over that macro step; the
same removed sink is subtracted from precipitation, so positivity cannot
create an unreported water source.

The selected path does not project the terminal temperature or moisture fields
onto a requested global total. Instead it integrates signed, quantized external
source/sink ledgers and compares them directly with the terminal extensive
state change. Conservative transport and paired internal exchange have zero
declared external contribution, so any leak in those operators remains visible
to the public closure gate rather than being absorbed by a correction.

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
C2 converts condensation of a lower-atmosphere mixing ratio through the fixed
column mass to `kg m-2 s-1 == mm s-1`. Its resolved orographic term is
`q max(u dot grad(z), 0) / 800 m`, capped at `0.02 m s-1` uplift and multiplied
by P3 land fraction; the same conservative humidity tendency creates the
precipitation sink. The water-limited orographic contribution is retained as a
separate monthly extensive field, is conservatively projected, and is
validated cell-by-cell not to exceed total precipitation. Liquid
mixed-layer equilibrium is bounded at `-2 C` and subsurface ocean equilibrium
at `-5 C` rather than allowing impossible supercooled liquid values.

Revision 2026-09-03 (milestone A4 §4): ocean whose ice-free annual gray target
is below the liquid mixed-layer floor carries a static sea-ice prior (North
1975 ice line, diagnosed once, no albedo feedback): surface albedo
`P4_SEA_ICE_SURFACE_ALBEDO = 0.60` (Perovich et al. 2002), TOA radiation
weighted onto the lower atmosphere as over land, zero evaporation, and the
air–mixed-layer heat exchange reduced to the conductive fraction
`(k_ice / h_ice) / (rho c_p C_H U)` (Untersteiner 1961; Maykut & Untersteiner
1971; Large & Pond 1982). Momentum exchange is untouched. Forcing fingerprint
v6, equation-model fingerprint v13.

The C1 single lower layer retains its declared effective hypsometric pressure
coupling of `30 m2 s-2 K-1`. C2 uses a first-baroclinic pair: the upper
coupling is `-25 m2 s-2 K-1` and the lower coupling is derived as
`+25 * 4000 / 6000 = +16.666... m2 s-2 K-1`, so the fixed 6 km/4 km column has
zero depth-integrated internal pressure force. This replaces an earlier
`+30/-25` pair whose nonzero column force conflated internal shear with a
barotropic acceleration. The bounded values retain baroclinic shear inside
the shallow-water depth validity range; no layer-thickness clipping is used.

The product formation driver is a deterministic accelerated climatological
continuation: sequential January-to-December forcing phases each receive one
`7,200 s` split-explicit macro adjustment per cycle. The public acceptance is
normalized cycle residual `<= 0.25`; the deterministic implementation and
comparison corpus use a `0.24` internal guard so publication cannot depend on
rounding at the acceptance boundary. Draft/Standard/High hard maxima are
`8/10/12`; a
nonconverged state is a typed failure. Monthly state fields are the converged
phase endpoints, while precipitation is the frozen-slow macro-step mean. This
is a procedural climatology closure, not a claim that each adjustment step is
a literal 30-day weather integration.

Annual-cycle convergence is not a dimensionally mixed state norm. For every
named prognostic scalar/vector field it computes a spherical-area-weighted RMS
change, divides by a positive physical scale, and takes the maximum over
fields. Height uses that layer's reference thickness; temperature/deep heat
uses `30 K`; atmosphere speed uses `20 m s-1`; ocean speed uses `2 m s-1`;
lower and upper specific humidity use `0.02`. Thus an unconverged humidity or
wind field cannot be hidden by a numerically larger height field.
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
partial work domain nor partial climate. The product artifact accepts only the
locked winning integrator, and its quality report is bound to the complete
checkpoint fingerprint (state plus forcing/input/integrator identity). A
same-surface report from a different climate or relief input is rejected.

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

Formation budgets accumulate signed per-cell extensive changes and compare
the complete formation interval with integrated declared external sources and
sinks. Height relaxation is area-weighted volume rate, evaporation and
precipitation are area-weighted mass rates, and radiative/thermal relaxation is
area- and heat-capacity-weighted signed power. Each ledger uses the increment
actually retained after `f32` quantization and water-availability limiting.
Transport and paired vertical exchange contribute exactly zero to the expected
external budget. The absolute value is applied only to the final global
closure. This avoids both catastrophic subtraction of two planet-scale totals,
the incorrect conversion of unbiased per-step `f32` roundoff into
resolution-dependent one-way drift, and a false pass caused by treating an
internal conservation error as a declared source.

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
- nontrivial but bounded free-surface response:
  `0.01 m <= sea-surface-height-max-absolute-m <= 6 m` over ocean cell-months;
  this rejects both a zeroed field and the tens-of-metres internal-interface
  response produced by accidentally using reduced gravity for public SSH;
- warm-ocean moisture supply, orographic precipitation enhancement, and a
  downstream rain-shadow signal;
- `orographic-uplift-enrichment-ratio >= 1.20`. For every land cell/month with
  wind speed at least `0.5 m s-1`, the support predicate requires an eligible
  land neighbor at least `0.15` upstream, another at least `0.15` downstream,
  and downstream-minus-upstream elevation at least `50 m`. The metric is
  `(supported orographic amount / all-land orographic amount) /
  (supported land-month area / all land-month area)`. Both denominators are
  conditioned on land, so ocean area cannot inflate the score; an orographic
  field uniform over land has enrichment near one and must fail;
- when the exact bound forcing has at least `0.5 C` January/July equilibrium
  air-temperature amplitude, at least `65%` correct hemispheric phase outside
  10 degrees, with latitude/temperature Pearson correlation retained as an
  unbounded layout-sensitive diagnostic. Below that forcing amplitude both
  seasonal metrics are explicitly `Unavailable` with zero samples and a locked
  reason; this is a valid conditional outcome, not a fabricated pass. In
  particular, the V1-valid zero-axial-tilt input must still publish a product;
- warmest-ocean-quartile humidity exceeds coldest-ocean-quartile humidity by
  at least `10%` of the ocean mean; raw SST/humidity correlation remains an
  unbounded advection-sensitive diagnostic;
- no independent cubed-face seam, pole spike, global ring, or P3 coastline
  displacement in map/globe atlases.

### 9.4 Product budgets

Release targets remain those already approved:

- C1 `n=24 <= 10 s`;
- C2 `n=32 <= 30 s`;
- C2 `n=48 <= 120 s`;
- C2 `n=48` core state, workspace, and climate output `<= 512 MiB`, checked
  both by a mechanically derived owner inventory and by 1 ms process-RSS
  sampling with the pre-generation baseline subtracted.

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
