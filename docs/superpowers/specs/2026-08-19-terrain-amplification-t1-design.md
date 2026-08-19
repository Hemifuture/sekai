# Terrain Amplification T1 — Frozen Design (2026-08-19)

Status: frozen for P6 M1 Task 3. Deviations discovered during implementation
must be recorded here as explicit amendment entries.

## 1. Scope

T1 is a deterministic, local, presentation-only refinement of the published
formation product. It never mutates T0 artifacts, never enters the physical
ledger, and is never read back by any solver. Its single deliverable is the
pure function

```text
sample(position: UnitVector3, lod: AmplificationLod) -> AmplifiedSample
AmplifiedSample { elevation_m: f32, regime: SurfaceRegime }
```

- `position` is a three-dimensional unit vector. Latitude/longitude never
  appear in the sampling path: they carry a ±180° seam and polar
  singularities, both already observed as artifacts in equirect evidence
  renders. All noise is evaluated in R³ on the sphere (existing
  `SphericalNoise3d` machinery), which is seamless by construction.
- The function is stateless after construction, `Send + Sync`, and free of
  interior mutability, so M2 may evaluate chunks concurrently in any order.
- `elevation_m` must stay within the existing authoritative bounds
  `ELEVATION_MIN_M..=ELEVATION_MAX_M` and be finite for every input.

## 2. Input assembly

`AmplificationInputs` is built once per published world from the same build
outcome the display document uses. Every field below exists today; no new
upstream data is introduced.

| Input | Source (single source of truth) |
| --- | --- |
| Final elevation, sea level, land/ocean | `FormationTerrainFields` (P5 product) |
| Sediment blanket thickness | `FormationSedimentFields::sediment_thickness_m` |
| Crust kind, crust age, lineation east/north, orogeny kind/age | `EvolvedTectonicSnapshot::compatibility()` |
| Substrate erodibility | `GeologicSubstrateSnapshot::erodibility` |
| Annual precipitation | `formation_annual_precipitation_mm` over the product's end-state circulation (`formation_climate()`) |
| River reaches: per-cell receiver, discharge, Strahler order | P5 `SphericalHydrologySnapshot` |
| Shelf break, floodplain accommodation | `FORMATION_SHELF_BREAK_DEPTH_M`, `FORMATION_FLOODPLAIN_ACCOMMODATION_M` (reused constants) |

## 3. T0 interpolation

The cell centers of the product surface are exactly the vertices of the
subdivided icosahedral lattice, so scalar fields are interpolated on that
triangulation:

1. Locate the containing base icosahedron face (20 dot-product tests),
   gnomonically project into it, and resolve the local subdivision triangle
   by index arithmetic — the standard geodesic discrete-global-grid
   addressing scheme (Sahr, White & Kimerling 2003). A one-time validation
   pass asserts agreement with the authoritative adjacency.
2. Interpolate with spherical barycentric weights (Langer, Belyaev & Seidel
   2006). This is uniform everywhere, including around the 12 pentagon
   cells, because interpolation runs on the triangular primal lattice, not
   on the polygonal duals.
3. Weights are smoothstep-remapped (`w ↦ w²(3−2w)`, renormalized) to remove
   the gradient crease of plain barycentric interpolation at triangle edges
   — the standard smooth-interpolation trick from texture filtering. The
   result interpolates cell values exactly at vertices and is C1 across
   edges for display purposes. This biased smoothing is acceptable only
   because T1 is presentation-only (§1).

Continuity class: C1 everywhere except along carved river center lines,
where the carve operator is C0 with bounded gradient (§7).

## 4. Regimes

Each sample resolves one of four regimes from interpolated T0 fields; all
conditioning is expressed per-regime. Regime boundaries blend over explicit
transition bands — never hard switches — so no regime edge can imprint a
visible line of its own.

- **Ocean floor**: below sea level − shelf-transition band.
- **Continental shelf**: between the coast and
  `FORMATION_SHELF_BREAK_DEPTH_M`, preserving the P5 shelf plateau.
- **Coastal band**: within one warp wavelength of the interpolated
  coastline.
- **Land interior**: everything else.

## 5. Conditioning table (frozen; each row cites its geologic basis)

Notation: factors multiply a per-regime base amplitude `A₀` and shape the
fractal parameters. All factors are clamped to the stated range and are
smooth (C1) in their inputs. Initial numeric values are starting points to
be calibrated on screen in Task 4; the *structure, drivers, directions and
bounds* below are the frozen contract.

| # | Driver (T0 field) | Effect | Direction & bound | Geologic basis |
| --- | --- | --- | --- | --- |
| C1 | Local relief proxy: slope magnitude of interpolated final elevation over one cell spacing | Detail amplitude `f_relief ∈ [0.05, 1]` | Amplitude grows with relief; near-zero on plains | Fine-scale roughness scales with local relief/slope in DEM statistics; depositional plains are smooth because they are burial surfaces (Turcotte 1997; Gagnon, Lovejoy & Schertzer 2006) |
| C2 | Orogeny age (Myr), where orogeny kind ≠ None | Ridge-noise weight and anisotropy strength `f_orog = exp(−age/80 Myr)` | Monotone decay; half-life reuses the T0 orogenic precedent (80 Myr, `directed_noise.rs`) | Post-orogenic relief decay over 10⁷–10⁸ yr (Himalaya vs Appalachians; Burbank & Anderson, *Tectonic Geomorphology*) |
| C3 | Lineation east/north (tectonic grain) | Anisotropic ridge alignment via sparse Gabor along the lineation tangent | Anisotropy only where C2 active; isotropic fallback | Ridge-and-valley / fold-belt alignment with structural strike (Appalachians, Zagros); Gabor noise is the documented anisotropic primitive (Lagae et al. 2009) |
| C4 | Substrate erodibility | Splits into two effects: amplitude ceiling `f_erod ∈ [0.4, 1]` decreasing with erodibility; dissection texture frequency increasing with erodibility | Two monotone effects in opposite channels, never one combined knob | Resistant lithologies hold cliffs and high local relief; weak lithologies lower relief but raise drainage density / fine texture (Schumm 1956 badlands; Horton 1945) |
| C5 | Annual precipitation | Dissection (valley-texture) weight follows a **non-monotone** Langbein–Schumm curve peaking in the semi-arid band (~300–400 mm/yr equivalent), reduced under hyper-arid and humid conditions | Peaked response, bounded [0, 1]; explicitly NOT monotone | Maximum sediment yield / drainage texture at semi-arid effective precipitation (Langbein & Schumm 1958; Abrahams 1984) |
| C6 | C4 high **and** C5 in semi-arid band | Badlands micro-texture gate (adds one extra dissection octave) | Only when both gates hold | Badlands form in weak rock + semi-arid climate (Schumm 1956, Perth Amboy) |
| C7 | Sediment blanket thickness | Amplitude damping `f_sed = exp(−thickness/D)` toward smooth fills | Monotone damping; floors at alluvial-plain smoothness | Burial smoothing: abyssal plains are turbidite-buried hills; alluvial plains bury bedrock relief (Goff & Jordan 1988 sediment damping term) |
| C8 | Ocean floor: crust age gradient magnitude (spreading-rate proxy) + lineation | Abyssal-hill field: anisotropic Gabor aligned with lineation; amplitude 50–300 m, wavelength band 2–10 km at full LOD; rougher where the age gradient indicates slow spreading; damped by C7 | Bounded to the measured abyssal-hill envelope | Abyssal hills are Earth's most common landform; their stochastic model and spreading-rate dependence are established (Goff & Jordan 1988; Malinverno 1991) |
| C9 | Coastal band: local coastal relief | Domain-warp magnitude for the coastline and near-shore detail: rugged (ria/fjord-like) where coastal relief is high, subdued where low and sediment-rich | Bounded so warp displacement < one T0 cell spacing (T0 land fraction preserved, §8) | Tectonic coast classification: collision coasts rugged, trailing-edge depositional coasts smooth (Inman & Nordstrom 1971); coastline fractality D ≈ 1.2–1.33 (Mandelbrot 1967) |
| C10 | Base spectral character per regime | Hurst exponent H: land interior blends 0.5 (young mountains) → 0.8 (plains) via C1/C2; per-octave persistence derived as `p = 2^(−H)` | H ∈ [0.4, 0.85] | Measured self-affine topography spectra: profile β ≈ 2, H clustered near 0.5 in mountains, higher (smoother fine-scale) in low-relief terrain (Sayles & Thomas 1978; Huang & Turcotte 1989; Gagnon et al. 2006). Note `p = 2^(−H)`, so the folklore p = 0.5 corresponds to H = 1 — smoother than real terrain; persistence is therefore derived from H, never hard-coded |

## 6. Spectral budget and LOD ladder

- Base detail wavelength λ₀ = 2 × mean cell spacing of the published tier
  (Draft ≈ 318 km, Standard ≈ 160 km); each LOD level adds one octave:
  λ_min(L) = λ₀ · 2^(−L).
- Anti-aliasing rule: when evaluated for a raster or mesh with footprint s,
  octaves with wavelength < 2s are skipped (Nyquist clamp) — standard
  procedural practice (Ebert et al. 2002). The M1 bake at 4096×2048
  (equatorial pixel ≈ 9.8 km) therefore uses L such that λ_min ≈ 20 km:
  L = 4 at Draft, L = 3 at Standard.
- Hard cap for M2: L ≤ 13 (λ_min ≈ 39 m at Draft λ₀), revisited in the M2
  plan with measured per-sample cost.

## 7. Rivers

- Carving applies only along published P5 reaches; T1 invents no channels.
  (Sub-T0 fine hydrology is an M2 topic and is out of scope here.)
- Channel/valley width scales as `w = k·Q^0.5` from P5 mean annual
  discharge, modulated by Strahler order — hydraulic geometry (Leopold &
  Maddock 1953). Valley cross-section: V-profile where C1 relief is high,
  widening toward a floodplain profile bounded by
  `FORMATION_FLOODPLAIN_ACCOMMODATION_M` where relief is low (Génevaux
  et al. 2013 carve-and-blend operators).
- Monotone-descent invariant: along each reach, carved bed elevation is
  non-increasing downstream, enforced analytically by interpolating the P5
  node elevations monotonically before subtracting the profile; carving is
  subtractive only (`min(base, carve)`), so it can never dam a valley.

## 8. Determinism and invariants

Seed derivation: world root seed → labeled substreams (existing
discipline) with frozen labels `t1.warp`, `t1.continental-detail`,
`t1.dissection`, `t1.badlands`, `t1.abyssal-hills`, `t1.coast`. Layers are
mutually uncorrelated and cannot collide with T0 labels.

Frozen probe fingerprint: 256 probe directions defined by the spherical
Fibonacci lattice (golden-angle formula, i = 0..255) — the formula is the
definition, no stored coordinates. The implementation test evaluates
`sample` at every probe for the M1 LOD and hashes the little-endian f32
elevations with blake3. The value is recorded here by amendment when Task 3
lands and must never change silently afterwards.

Invariant tests enumerated for Task 3 (all must exist before Task 4):

1. Determinism: probe fingerprint stable across runs and threads.
2. Seamlessness: antipodal/meridian probe pairs straddling the atlas seam
   agree with direct evaluation (no lat/lon anywhere in the path).
3. Bounds: every probe result finite and inside
   `ELEVATION_MIN_M..=ELEVATION_MAX_M`.
4. Land-fraction preservation: classifying the probe set (extended to 16k
   Fibonacci points) by amplified elevation vs sea level changes the T0
   area-weighted land fraction by ≤ 1 percentage point.
5. River monotone descent: for every reach, sampled bed elevations along
   the polyline are non-increasing downstream.
6. Conditioning directions: synthetic single-driver sweeps confirm each
   table row's stated direction (and C5's peaked shape).
7. Regime blending: no sample sequence crossing a regime boundary exhibits
   a first-difference spike beyond the authored bound.

## 9. Scientific review record (2026-08-19, pre-freeze)

The draft table was reviewed against planetary-geomorphology literature
before freezing; three substantive corrections and two rejections resulted:

1. **Corrected — precipitation is not a monotone dissection driver.** The
   working draft (and the earlier chat sketch) had "more rain → more
   gullies". The established Langbein–Schumm relation peaks in the
   semi-arid band and declines under vegetated humid climates; C5 now
   encodes the peaked curve explicitly.
2. **Corrected — erodibility must split into two channels.** A single
   "erodibility → smoother" knob contradicts badlands, where the weakest
   rocks produce the finest, densest dissection. C4 separates the amplitude
   ceiling (down with erodibility) from texture frequency (up with
   erodibility), and C6 gates the badlands end-member on climate, matching
   Schumm's observations.
3. **Corrected — persistence is derived, not folklore.** p = 0.5 per octave
   implies H = 1, smoother than any measured landscape. C10 derives
   persistence from measured Hurst exponents (H ≈ 0.4–0.85) via
   p = 2^(−H).
4. **Rejected — invented "erosion-look" filters.** No ridged-multifractal
   or thermal-erosion post-filters are applied without a named driver from
   the table: every visible structure must trace to a T0 physical field or
   a cited stochastic landform model (abyssal hills, hydraulic geometry).
5. **Deferred — seamounts and volcanic edifices.** A grounded stochastic
   model exists (Wessel 2001 seamount statistics), but the formation
   product currently publishes no hotspot/volcanic flux field to condition
   on; adding one is a T0 change and therefore out of T1's charter.
   Recorded as a candidate alongside the Task 6 gate.

Constraint philosophy: T1 may only *redistribute* detail below the T0 cell
scale; every macro-scale truth (continents, land fraction, shelf plateaus,
river topology, sea level) is inherited and protected by the §8 invariants.
This is what guarantees "no unpredictable results": any deviation beyond
the stated bounds is a test failure, not a matter of taste.

## 10. Amendment log

- (empty — first entry will record the frozen probe fingerprint value from
  Task 3.)
