# Terrain Amplification & LOD Plan (P6, 2026-08-19)

## Context

The 2026-08-19 audit and formation UI delivery established the three-tier
direction the user approved:

- **T0 — global physical truth** (existing P1→P5 chain): continents, mountain
  belts, climate, river networks, water-volume sea level. Draft solves 20,252
  cells (≈159 km); Standard (80k) and High (200k) resolutions are already in
  the quality-profile design but not selectable in the UI. Uniform global
  simulation below ~10 km is neither storable nor useful (1 m ≈ 5.1×10¹⁴
  cells ≈ 10²+ PB), so fine detail must never come from raising T0's N.
- **T1 — conditioned amplification** (new): a deterministic, local, pointwise
  function `sample(unit_vector, lod) → elevation/material` that interpolates
  T0 fields and adds multi-octave, domain-warped noise whose amplitude,
  roughness, and anisotropy are modulated by T0 physics (orogeny age,
  erodibility, precipitation, slope), plus carved valleys along the published
  P5 river reaches. This is the Gleba technique with physics-driven
  conditioning instead of hand-authored spots. No global solve, seamless by
  construction (3D noise on the sphere, no lat/lon seams), seed-deterministic.
- **T2 — LOD runtime** (future milestone M2): quadtree chunking with
  view-dependent sampling density down to ~1 m near the camera, GPU
  displacement, chunk cache. LOD is the delivery mechanism; T1's local
  determinism is the enabler.

Two audit follow-ups fold in naturally: the too-straight oceanic structures
(plate-boundary geometry made visible by thin detail noise) are first
attacked visually by T1's domain warp, with a decision gate on a structural
T0 fix; and hillshading arrives with the baked amplified view.

Per `AGENTS.md`: every algorithm task below is delivered only when its output
is visible and operable in the UI, and final acceptance is the user's.

## Milestone M1 tasks

- [x] Task 1 — Quality tier selector in the UI: 档位 (Draft/Standard; High
      listed as 实验性·离线级) drives `FORMATION_QUALITY_PROFILE`-equivalent
      state, the formation surface cache keys on (profile, radius), the panel
      names the active tier and expected build time (Standard ≈ 3–5 min on
      the worker thread), and the inert 目标陆地面积 slider is disabled with
      an explanatory tooltip while the formation pipeline is active.
      Verify (UI): switch 档位 to Standard, rebuild without freezing, watch
      the cell count rise to ~80k and the world gain visible detail.
- [x] Task 2 — Amplification design spec: freeze the short T1 contract in
      docs/superpowers/specs/ — sampling domain (3D unit vectors, never
      lat/lon), T0 field interpolation scheme and its continuity class,
      conditioning table (which T0 fields modulate which noise parameters and
      monotonicity directions), octave/frequency budget per LOD, seed
      derivation (world seed + fixed labels via the existing labeled-stream
      discipline), and the determinism fingerprint (hash of a frozen probe
      set).
      Verify: spec committed; probe-set fingerprint test enumerated in the
      spec before any implementation lands.
- [ ] Task 3 — T1 core `sample()` module implementing the frozen spec on the
      existing in-crate machinery (`SphericalNoise3d`, `FractalProfile`,
      labeled substreams): T0 barycentric interpolation over the geodesic
      lattice, conditioned multi-octave detail, domain-warped coastline and
      ridge break-up. Unit tests: determinism fingerprint, cross-meridian and
      polar seamlessness, conditioning monotonicity, continuity at cell
      borders. Per AGENTS.md this task is *not delivered* until Task 4 puts
      it on screen — the checkbox here only tracks the commit.
      Verify: tests green (delivery deferred to Task 4).
- [ ] Task 4 — Baked amplified display layer: after the worker-thread build,
      bake an equirect color+hillshade texture (initial budget 4096×2048,
      ≤3 s worker-side at Draft) from `sample()` with the hypsometric ramp
      and sun-shaded relief, and add a 显示模式 toggle (格元视图 / 放大视图)
      on both the 2D map and the globe. Entity inspection stays on 格元视图
      in this milestone.
      Verify (UI): toggle 放大视图 — smooth coastlines with no hex
      staircase, straight ocean ridge lines visibly broken up by the warp,
      hillshaded mountains; toggle back for inspection.
- [ ] Task 5 — River carving and channels: rasterize the published P5 river
      reaches into `sample()` as analytic valley profiles and draw the reach
      polylines (width by Strahler order) in the amplified view.
      Verify (UI): rivers follow the drainage the inspector reports, valleys
      visibly carve the amplified terrain.
- [ ] Task 6 — Decision gate on structural T0 de-regularization: with
      Tasks 4–5 on screen, judge with the user whether plate-boundary
      domain-warp roughening and small-scale oceanic age perturbation are
      still needed in P2v5 itself. If yes, that work gets its own spec
      amendment plus evolved/P5 evidence refresh (it changes artifact
      fingerprints); if no, record the waiver here.
      Verify: explicit user decision recorded.
- [ ] Task 7 — Gates and acceptance: fmt/clippy/wasm/full regression, plan
      checkboxes reconciled, user acceptance steps written, artifact report
      updated with before/after captures.
      Verify: gates green; user walks the steps personally.

## Milestone M2 (separate plan when M1 is accepted)

Chunked cube-sphere quadtree with view-dependent `sample()` density to ~1 m
near the camera, GPU displacement and chunk cache, biome/material shading on
top of the circulation fields, and per-chunk constrained fine hydrology
synthesis below T0 resolution.

## Provenance of every load-bearing technique

None of the techniques below are invented here; each is an established
result or a shipped production system. What *is* authored by us is the
composition — in particular the Task 2 conditioning table that maps our P5
physical fields onto noise parameters — which is exactly the part the spec
freeze and the Task 6 user gate exist to validate.

- **T0 geodesic icosahedral grid (existing surface)** — standard in global
  atmospheric modelling since Sadourny, Arakawa & Mintz, *Integration of the
  nondivergent barotropic vorticity equation with an icosahedral-hexagonal
  grid for the sphere*, Monthly Weather Review 96 (1968); operational today
  in DWD/MPI's ICON (Zängl et al., QJRMS 141, 2015) and NCAR's MPAS
  (Skamarock et al., MWR 140, 2012 — spherical centroidal Voronoi meshes).
  The 12 pentagons are the Goldberg-polyhedron property (Goldberg, 1937).
- **Task 3/4 conditioned multi-octave noise** — fBm and procedural noise:
  Perlin, *An Image Synthesizer*, SIGGRAPH 1985; Perlin, *Improving Noise*,
  SIGGRAPH 2002; spatially varying roughness ("heterogeneous terrain") is
  Musgrave's multifractal line: Musgrave, Kolb & Mace, *The Synthesis and
  Rendering of Eroded Fractal Terrains*, SIGGRAPH 1989, canonized in Ebert
  et al., *Texturing & Modeling: A Procedural Approach* (3rd ed., 2002).
- **Task 3/4 domain warping** — Perlin & Hoffert, *Hypertexture*, SIGGRAPH
  1989; Quilez, *Domain Warping* (iquilezles.org); first-party production
  evidence read during the 2026-08-19 audit: Factorio Space Age's Gleba
  expressions warp every biome coordinate (`gleba_wobble_x/y`, official
  wube/factorio-data repository).
- **Terrain amplification as a research line** — Paris, Galin, Peytavie,
  Guérin & Gain, *Terrain Amplification with Implicit 3D Features*, ACM TOG
  38(5), SIGGRAPH Asia 2019 (doi 10.1145/3342765); successor *Terrain
  Amplification using Multi Scale Erosion*, ACM TOG 2024 (doi
  10.1145/3658200); surveyed in Galin et al., *A Review of Digital Terrain
  Modeling*, Eurographics STAR 2019.
- **Task 5 river carving with analytic valley primitives** — Génevaux,
  Galin, Guérin, Peytavie & Beneš, *Terrain Generation Using Procedural
  Models Based on Hydrology*, ACM TOG 32(4), SIGGRAPH 2013 (doi
  10.1145/2461912.2461996): hierarchical drainage graph, then terrain
  assembled by blending/carving river patches — our variant substitutes the
  P5-published physical reach network for their synthetic graph.
- **Task 3 spherical interpolation** — Langer, Belyaev & Seidel, *Spherical
  Barycentric Coordinates*, Eurographics SGP 2006; conservative/barycentric
  remapping is likewise standard in climate regridding.
- **M2 chunked LOD planet runtime** — Ulrich, *Rendering Massive Terrains
  Using Chunked Level of Detail Control*, SIGGRAPH 2002 course; Losasso &
  Hoppe, *Geometry Clipmaps*, SIGGRAPH 2004; Cignoni et al., *P-BDAM:
  Planet-Sized Batched Dynamic Adaptive Meshes*, IEEE Vis 2003; Cozzi &
  Ring, *3D Engine Design for Virtual Globes*, 2011 (cube-sphere quadtrees;
  the Cesium lineage); open-source reference implementation: Proland
  (INRIA, Bruneton & Neyret).
- **M2 lazily evaluated deterministic chunks in production** — Factorio's
  own noise pipeline (FFF-390, read during the audit); Hello Games,
  *Building Worlds in No Man's Sky Using Math(s)*, GDC 2017; Outerra's
  published pipeline (coarse Earth DEM + on-GPU fractal refinement).
- **T0 physical chain (unchanged)** — already cited in the P5 completion
  document: Barnes, Lehman & Mulla 2014 (priority-flood); Braun & Willett
  2013 (O(N) implicit stream power); Roering, Kirchner & Dietrich 1999
  (nonlinear hillslope); Davy & Lague 2009 / Yuan et al. 2019 (sediment).

## Non-goals (M1)

- No changes to T0 artifacts, fingerprints, or quality evidence (until the
  Task 6 gate says otherwise).
- No entity picking or simulation reads from amplified data — T1 is
  presentation-only; the P5 product remains the sole authority.
- No attempt at real-time LOD or 1 m sampling this milestone.
