# Sekai Complete Natural-World Pipeline Design

> Status: approved for autonomous implementation
>
> Date: 2026-08-17
>
> Branch baseline: `feat/spherical-presentation` / `a5cb45b`
>
> Supersedes no existing contract. This document composes the approved spherical,
> tectonic, climate, hydrology, and presentation designs into the next product
> program and versions each changed artifact independently.

## 1. Decision summary

Sekai will finish one causal, spherical, deterministic natural-world pipeline
before making a final Gleba comparison. The implementation follows a
science-gated sequence rather than a visual-first sequence:

```text
authoring inputs
  -> authoritative spherical surface and quality profile
  -> evolved plates, crust, and boundary processes
  -> mantle, lithology, isostasy, and tectonic uplift forcing
  -> bounded coupled surface formation
       seasonal atmosphere-ocean circulation
       hydrology and lakes
       fluvial, hillslope, coastal, and glacial processes
       sediment transport and isostatic response
  -> frozen final terrain, basins, and coastline
  -> final climate, hydrology, cryosphere, and hazards
  -> weathering, soil, ecology, and vegetation
  -> derived real-color material and terrain presentation
  -> scientific report, visual atlas, and Gleba comparison
```

The project keeps its strong typed-artifact stage graph. Mutually dependent
surface-formation processes are isolated inside one bounded, atomic stage with
explicit residuals and a maximum iteration count. No stage publishes partial or
unvalidated output.

The existing `CurrentSliceV1` planar compatibility path remains frozen. New
spherical artifacts receive new schema and stage versions. Presentation never
writes back into scientific truth.

## 2. Scope

### 2.1 Included

- authoritative closed-sphere geometry and explicit quality profiles;
- evolved plate tectonics, crust mass, ocean age, orogeny, rifting, spreading,
  subduction, collision, transform boundaries, passive margins, and hotspots;
- lithology, rock strength, crustal isostasy, mantle forcing, and uplift rate;
- large-scale terrain formation from uplift, fluvial incision, hillslope
  transport, sediment routing, basin filling, coastal response, and glacial
  erosion;
- monthly global winds, ocean currents, temperature, humidity, precipitation,
  sea-surface temperature, thermocline state, sea ice, and snow;
- two active atmosphere layers, two active ocean layers, and a slow deep-ocean
  heat reservoir at the approved medium-complexity ceiling;
- bounded climate-surface feedback, final hydrology, lakes, wetlands,
  groundwater proxies, glaciers, and hydroclimatic hazards;
- weathering, soil properties, net primary productivity, biome, vegetation,
  and disturbance response;
- diagnostic views and a separate finished natural-color view in both 2D and
  3D;
- scientific, deterministic, performance, artifact, and perceptual acceptance;
- a final comparison with the documented current Gleba product.

### 2.2 Excluded

- magic, population, culture, settlements, states, and history;
- real-time weather prediction or individual simulated storms;
- a full three-dimensional non-hydrostatic GCM;
- arbitrary user-programmable solver layers or numerical schemes;
- scientific claims about Gleba internals that cannot be observed or sourced;
- copying Gleba code, private algorithms, palettes, assets, or map layouts;
- treating an attractive screenshot as proof of a scientifically valid field.

## 3. Current-state audit

The production spherical graph currently publishes:

```text
surface -> tectonics V4 -> mantle -> relief V3 -> geology
        -> preliminary climate -> bounded hydro-erosion
```

These artifacts establish valuable contracts, but only the closed-surface and
presentation foundations meet the intended architectural role.

The current seed-42 `Continents` result demonstrates the principal upstream
failure: requested initial continental crust is 38.0%, evolved continental
crust is 19.9%, and sea level is lowered to approximately -1673.5 m to force
38.0% visible land. This exposes extensive oceanic crust as low-relief land.

Other known gaps are:

- tectonic evolution runs on a 4,842-cell control surface before projection to
  the 20,252-cell product surface;
- old orogenies decay until their median coarse relief is close to zero;
- current erosion is a bounded two-pass modifier rather than a landscape
  evolution solver;
- production climate uses analytic monthly thermal winds and moisture routing,
  not the experimental global atmosphere-ocean dynamics;
- the experimental steady circulation solver is not equivalent to the
  transient reference, while the explicit transient reference is too slow for
  production;
- no production soil, ecology, vegetation, glacier, true-color material, terrain
  lighting, or atmosphere stage exists;
- the elevation renderer uses one scalar per flat cell, a five-stop diverging
  palette, a max-outlier symmetric range, and no terrain normal or illumination.

Therefore existing downstream class names do not count as completion of later
program phases. The program is currently entering P0, with P1 substantially
implemented and P2 requiring replacement/calibration.

## 4. Non-negotiable principles

1. **One truth per semantic domain.** The authoritative spherical surface owns
   global cell identity. Work grids and render tiles are reproducible derived
   assets with explicit mappings.
2. **Causal fields before colors.** Tectonic uplift, climate fluxes, discharge,
   erosion, soil, and vegetation are stored and validated before presentation.
3. **No fake preview physics.** Interactive presentation may show the last valid
   snapshot while a new build runs, but it may not substitute a different
   scientific model.
4. **Bounded feedback.** Every feedback loop has a residual, maximum iteration
   count, cancellation points, and an atomic publication boundary.
5. **Determinism.** Stage and entity random streams derive only from stable
   identities. Thread scheduling, visible layers, and diagnostic collection do
   not affect world facts.
6. **Conservation and provenance.** Crust, water, sediment, heat, moisture, and
   remapped fluxes expose budgets and error bounds in a quality report.
7. **Quality is measured at several scales.** Contract tests alone cannot certify
   morphology. Every scientific stage supplies analytic fixtures, multi-seed
   distributions, atlases, and scale-aware artifact metrics.
8. **Compatibility is explicit.** Existing serialized worlds never silently
   change. New algorithms increment stage/schema versions and old artifacts are
   rejected or migrated by declared policy.

## 5. Spatial and resolution architecture

### 5.1 Quality profiles

The product exposes semantic quality profiles rather than arbitrary numerical
knobs:

| Profile | Authoritative target | Resolved geodesic cells | Tectonic control | Climate cubed-sphere | Intended use |
|---|---:|---:|---:|---:|---|
| Draft | 20,000 | 20,252 | 4,842 | `n=24` | fast authoring and tests |
| Standard | 80,000 | 79,212 | 20,252 | `n=32` | default final product target |
| High | 200,000 | 198,812 | 20,252 | `n=48` | asynchronous export/inspection |

The existing serialized `target_cell_count` remains the geometric source of
truth. `NaturalQualityProfile` resolves the coordinated work-grid settings. A
world stores the selected profile and resolved counts so a future release cannot
silently reinterpret it.

The Standard profile becomes the default only after its complete native build
passes the product budget. Until then Draft remains the interactive default and
Standard remains an explicit background-quality build.

### 5.2 Derived work domains

- `SphericalSurfaceSnapshot` remains the authoritative global topology.
- `TectonicControlSurface` is transient and carries no published identity.
- `ClimateGrid` is a transient cubed-sphere finite-volume work grid.
- `ConservativeSurfaceMap` records source/target fingerprints, overlap weights,
  vector tangent transforms, and conservation error.
- `TerrainDetailPyramid` is a read-only derived presentation artifact. It may
  add subcell normals, displacement, material variation, and anti-aliasing from
  published drivers, but hydrology, climate, and ecology cannot read it.

Continuous extensive quantities use conservative area remapping. Intensive
scalars use bounded area-weighted interpolation. Tangent vectors are transported
in three dimensions and projected into the target tangent plane. Categories use
stable majority/nearest-compatible rules and publish ambiguity coverage.

### 5.3 Scale semantics

The authoritative terrain represents world-scale relief, drainage ownership,
and named river networks. The detail pyramid represents unresolved ridges,
channels, rock texture, and material roughness for display. It must preserve the
authoritative mean elevation, drainage direction at parent outlets, coast
classification, and material fractions within declared tolerances.

## 6. Runtime artifact graph

The final graph is acyclic at the engine level:

```text
SphericalSurfaceArtifact
  -> EvolvedTectonicArtifact
  -> GeologicSubstrateArtifact
  -> PrimaryReliefArtifact
  -> NaturalSurfaceFormationArtifact
       {final terrain, formation climate, hydrology, sediment, cryosphere}
  -> FinalSeasonalClimateArtifact
  -> FinalHydrologyArtifact
  -> ClimateVariabilityArtifact
  -> ExtremeClimateArtifact
  -> SoilArtifact
  -> EcologyArtifact
  -> SurfaceMaterialArtifact
  -> TerrainDetailArtifact
  -> NaturalQualityReportArtifact
```

`NaturalSurfaceFormationStage` is the only intentionally compound stage. Its
internal modules remain independently testable, but they cannot publish
mutually inconsistent intermediate world facts.

## 7. P0: baseline and quality infrastructure

### 7.1 Deliverables

- a versioned `NaturalQualityReport` with per-stage scientific, morphology,
  conservation, deterministic, performance, and visual metrics;
- a deterministic atlas generator for Draft and Standard profiles;
- a fixed corpus of analytic fixtures plus 17 paired product seeds;
- current V4 baseline reports retained as negative evidence;
- machine-readable acceptance thresholds with metric direction and scope;
- a command that fails when any required metric is absent, stale, or outside
  its approved range.

### 7.2 Rules

Metrics may not silently drop samples. Empty denominators report `Unavailable`
with a reason. Aggregate success cannot hide a seed that violates a hard safety
bound. Threshold changes require a versioned design amendment and before/after
evidence.

## 8. P1: surface, profiles, and remapping

P1 adds the quality-profile contract and reusable conservative mappings without
changing existing Draft geometry hashes.

Acceptance includes:

- closed-sphere area error within the existing analytic tolerance;
- paired shared-edge flux cancellation within floating-point roundoff;
- constant scalar preservation exactly after quantization;
- extensive flux conservation relative error no greater than `1e-6`;
- tangent vectors remain tangent and solid-body rotation direction agreement is
  at least `0.999` by area;
- Draft remains within its existing synchronous geometry budget;
- Standard and High builds are cancellable and publish atomically.

## 9. P2: evolved tectonics V5

### 9.1 Model changes

V5 retains rigid spherical plate motion and event ordering, but separates four
quantities that V4 conflates:

- crust material mass/area and thickness;
- plate ownership and motion;
- instantaneous tectonic uplift/subsidence forcing;
- accumulated topographic response.

Continental material cannot disappear through resampling or ownership changes.
Subduction consumes eligible oceanic crust first; continental collision transfers
or thickens terranes under an explicit bounded mass budget. Rifting thins
continental crust before creating oceanic crust. Spreading creates young oceanic
crust with age zero and a lineation direction. Relaxation changes topography, not
material category, unless a named metamorphic or melting process says otherwise.

The published snapshot adds uplift rate, subsidence rate, boundary distance,
event age, and material-budget diagnostics. Orogenic forcing persists while a
convergent boundary is active; post-orogenic topography is later controlled by
geomorphic erosion rather than an arbitrary visual half-life.

### 9.2 Earth-like acceptance

Across the paired 17-seed `Continents` corpus:

- evolved continental area median remains in `0.30..=0.45`;
- per-seed continental retention remains in `0.75..=1.15` of initialized area;
- no final active plate owns more than 45% of spherical area for the normal
  12-plate preset;
- at least 80% of sampled ocean-continent subduction transects place the trench
  on the descending side and positive uplift on the overriding side;
- at least 80% of continental-collision transects have positive shortening and
  positive uplift forcing;
- young oceanic crust is shallower than old oceanic crust with area-weighted
  rank correlation at least `0.70`;
- transform-boundary median absolute uplift is at most half the convergent
  median;
- regular near-120-degree triple junctions are at most 35% of measured junctions;
- median coast/plate-boundary overlap is at most 35% after final formation;
- all material and lineage budgets close within declared quantization error.

Formation presets may use separately approved distributions but cannot change
equations, event direction, budget accounting, or acceptance semantics.

## 10. P3: substrate and primary relief

`GeologicSubstrateArtifact` publishes crust kind, thickness, density, ocean age,
lithology, erodibility, permeability, heat flow, volcanic influence, and
sediment-source class.

`PrimaryReliefArtifact` publishes:

- Airy/isostatic base elevation;
- dynamic tectonic offset;
- uplift/subsidence rate in `mm yr-1`;
- hotspot and volcanic construction;
- passive-margin shelf/slope profile;
- initial sea level and water-volume budget;
- initial land/ocean classification.

Sea level is selected from water volume and basin capacity for the physical
profile. An author-requested target land fraction remains a separate authoring
constraint and may report infeasibility; it cannot expose oceanic crust merely
to force an exact percentage.

The Earth-like gate requires a bimodal hypsometry, realistic continental
freeboard and ocean depth ordering, connected convergent mountain forcing,
trench/ridge adjacency, and cross-scale spectral energy. Exact values live in
the P3 child specification and report schema, not in a renderer palette.

## 11. P4: C0-C2 global atmosphere-ocean circulation

The approved layered climate design remains normative:

- the explicit RK3 transient shallow-water implementation remains the small-grid
  truth reference;
- production compares IMEX and split-explicit candidates under identical
  equations, operators, forcing, and diagnostics;
- the winner must meet the existing RK3 agreement gates and cannot be selected
  only for speed;
- C2 publishes two active atmosphere layers, a mixed-layer ocean, a thermocline
  layer, and a slow deep-ocean heat reservoir;
- public fields use stable meanings such as near-surface wind, upper wind,
  vertical shear, surface current, SST, thermocline depth, humidity, and
  precipitation; internal layer indices never leak downstream.

The existing product performance targets remain: C1 `n=24` under 10 seconds,
C2 `n=32` under 30 seconds, and C2 `n=48` under 120 seconds on the recorded
reference class, with asynchronous cancellation and last-valid-snapshot display.

## 12. P5: geomorphic formation

The formation core evolves elevation with explicit, unit-bearing terms:

```text
dh/dt = uplift
      - K(lithology, climate) * A^m * max(slope - threshold, 0)^n
      + hillslope_diffusion
      + sediment_deposition
      + coastal_response
      + glacial_term
      + isostatic_response
```

Required components are:

- depression handling that distinguishes ocean outlets, closed basins, lakes,
  and numerically insignificant pits;
- deterministic drainage DAG, discharge, Strahler order, and stable river
  segment identities;
- stream-power incision with erodibility derived from lithology and runoff;
- conservative sediment production, transport capacity, deposition, basin fill,
  delta potential, and shelf delivery;
- hillslope diffusion/landsliding bounded by local relief and material;
- coastal erosion/deposition at the world-map scale;
- multirate stepping and convergence based on normalized elevation, drainage,
  sediment, and coastline residuals.

The formation stage runs a fixed maximum of four outer climate-surface
iterations. Each outer iteration may use a warm-started production climate
solution, but never a different preview model. Non-convergence is a typed build
failure with the best residual report; it does not publish the last iterate.

## 13. P6: C3 surface feedback and cryosphere

C3 adds cloud, convection, radiation, surface exchange, soil-moisture proxy,
evapotranspiration, roughness, sea ice, snow, glacier mass balance, and ice-flow
potential. These components participate in the bounded formation stage where
they alter long-term terrain or basin state.

The final published terrain is followed by one final C3 climate solution and one
final hydrology solution. Those final passes cannot modify terrain. If their
diagnostics exceed the formation-stage consistency tolerance, the entire
formation build fails rather than publishing inconsistent facts.

Glacier acceptance includes accumulation/ablation ordering, latitude/elevation
eligibility, flow down ice-surface potential, bounded mass balance, and fjord/U-
valley response only where sustained ice and relief support it.

## 14. P7: final water, variability, and extremes

`FinalHydrologyArtifact` publishes oceans, lakes, rivers, wetlands, groundwater
recharge proxy, discharge seasonality, sediment delivery, and floodplain
potential on the frozen terrain.

C4 then publishes:

- generalized equatorial atmosphere-ocean modes only when basin and planetary
  eligibility tests pass;
- tropical and extratropical cyclone climatology;
- drought, flood, heat, cold, wildfire-weather, storm-surge, and coastal-erosion
  statistics;
- explicit `Unavailable`, `EvaluatedNotApplicable`, or `Available` capability
  state for every optional phenomenon.

These are climatological statistics, not invented historic events.

## 15. P8: weathering, soil, ecology, and vegetation

The soil stage consumes lithology, slope, climate, hydrology, sediment, and
disturbance statistics. It publishes depth, texture class/fractions, drainage,
organic matter proxy, fertility, salinity, moisture regime, and erodibility.

The ecology stage publishes biome, NPP, seasonality, canopy/ground cover,
roughness, albedo, rooting depth, flammability, wetland class, and recovery
timescale. It may use the climate-vegetation proxy already converged in C3, but
it cannot call the climate solver or overwrite climate fields.

Acceptance checks energy/water eligibility, treeline and aridity ordering,
coastal/wetland consistency, biome contiguity without forced blobs, and
multi-seed diversity without violating climate constraints.

## 16. P9: finished natural presentation

Diagnostic views remain available and scientifically literal. A separate
`NaturalSurfaceMaterialArtifact` derives finished appearance from immutable
facts:

- deep/shallow water color from depth, suspended sediment, ice, and sky;
- land base color from soil, exposed lithology, vegetation, moisture, and snow;
- coast, river, lake, wetland, glacier, and volcanic overlays with semantic LOD;
- analytic terrain normals, slope-aware hillshade, ambient/solar illumination,
  ocean specular response, and atmosphere;
- a terrain detail pyramid conditioned by uplift direction, drainage, lithology,
  erosion, and material cover;
- physically 1x globe geometry by default, with clearly labelled optional
  visual exaggeration modes that never alter picking or world facts.

Natural view never uses the diagnostic five-stop elevation palette. Diagnostic
range defaults use robust area-weighted percentiles while still exposing exact
min/max and clipped coverage.

Static scenes upload unchanged geometry and fields once. LOD transitions cannot
move coastlines, rivers, or selected cells. Native and WASM share semantic
content even when GPU precision or optional atmosphere quality differs.

## 17. P10: final validation and Gleba comparison

The final gate runs only after P0-P9 pass. It produces:

- machine-readable scientific and performance reports for analytic fixtures and
  all paired product seeds;
- a fixed 2D/3D atlas at named projections, cameras, seasons, and layers;
- ablation images for relief-only, climate-only, material-only, and lighting;
- artifact metrics for polygon imprint, coast regularity, mountain connectivity,
  drainage density, relief spectra, color gamut, clipping, and LOD stability;
- blinded pairwise visual ratings that separate macro geography, terrain detail,
  climate readability, natural color, and overall coherence;
- a documented feature matrix against publicly observable Gleba capabilities.

Because Gleba does not publish comparable internal fields or identical seeded
worlds, Sekai may claim that it passes stronger public scientific tests, but may
not claim numerical superiority over unknown Gleba internals. Visual comparison
uses matched output conditions where possible and states every mismatch.

Completion requires all hard scientific gates, no severe visual artifact, and a
finished natural view that is preferred to the frozen Sekai V4 baseline by at
least 80% of blinded comparisons. Gleba preference is reported honestly rather
than made a release-blocking claim that could be gamed.

## 18. Error, cancellation, and publication

Errors distinguish invalid input, unsupported capability, remap failure,
non-finite state, solver instability, linear-solve failure, budget violation,
non-convergence, incompatible cache/checkpoint, and presentation-resource
failure.

Each numerical error includes stage, component, iteration/time/month/layer,
residual, threshold, and bounded spatial context. The engine may reduce a time
step, discard an invalid warm start, or change a preconditioner, but cannot
relax acceptance thresholds or substitute different physics.

Builds carry a content generation ID. Cancellation is cooperative and bounded.
Only a complete validated artifact set atomically replaces the previous world.
Presentation failure does not invalidate scientific artifacts; scientific
failure retains the previous complete world and reports the failed input
fingerprint.

## 19. Testing strategy

Every child plan uses test-driven development and includes:

1. analytic unit tests for formulas, units, and limiting cases;
2. contract tests for schema, validation, identity, and unique ownership;
3. property tests for conservation, positivity, monotonicity, and topology;
4. deterministic repetition and native/WASM quantized-hash checks;
5. standard scientific fixtures with quantitative thresholds;
6. paired multi-seed morphology/statistics tests;
7. release performance and peak-memory tests;
8. atlas generation and human-inspectable evidence;
9. stage invalidation and atomic-publication tests;
10. an end-to-end acceptance run before the phase is considered complete.

Tests must fail for the known V4 defects before a replacement is implemented.
Golden images supplement but never replace semantic assertions.

## 20. Program order and gates

The implementation order is fixed:

```text
P0 -> P1 -> P2 -> P3 -> P4 -> P5 -> P6 -> P7 -> P8 -> P9 -> P10
```

A later phase may be designed while an earlier phase runs, but production code
for a consumer does not start until its upstream artifact and acceptance report
are frozen. Each phase has its own specification, implementation plan, focused
commit series, and completion record.

The repository must remain buildable after every task. Existing unrelated
changes are preserved. Legacy planar golden outputs remain frozen unless a
separate migration is explicitly approved.

## 21. Definition of done

The program is complete only when:

- Standard quality builds a complete natural world from a fixed seed;
- continental material, water, sediment, atmospheric/oceanic budgets, and
  remapping errors satisfy their versioned limits;
- tectonic events visibly and quantitatively control the correct landforms;
- terrain contains coherent mountain systems, basins, river networks, sediment
  landforms, and climate-conditioned glacial/coastal features across scales;
- monthly global winds and ocean currents come from the approved production
  transient equations and pass reference agreement;
- final climate, hydrology, cryosphere, soil, and ecology are mutually
  consistent and publish capability/quality reports;
- the natural view shows finished real-color terrain, water, ice, vegetation,
  lighting, and atmosphere without exposing cell polygons at normal viewing
  scales;
- diagnostic views remain exact, inspectable, and independent of natural-view
  styling;
- native tests, formatting, linting, WASM checks, release performance gates,
  deterministic hashes, atlases, and end-to-end acceptance all pass;
- the final effect has been opened and visually inspected in the running product;
- the Gleba comparison report states supported evidence, limitations, wins, and
  remaining deficits without unsupported claims.
