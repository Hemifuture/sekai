# Geologic Substrate and Primary Relief P3 Design

Date: 2026-08-17
Program phase: P3
Status: locked for implementation

## 1. Outcome and boundary

P3 converts the conservative V5 material and forcing fields into a physical
geologic substrate and a first formed, water-volume-consistent relief. It
publishes two new artifacts:

```text
world.evolved-tectonics
  -> world.geologic-substrate
  -> world.primary-relief
```

P3 does not run climate, drainage, erosion, sediment transport, cryosphere,
soil, ecology, or finished shading. The current product graph remains unchanged
until the complete P3-P9 chain can replace it atomically.

## 2. Scientific references and classification

Oceanic thermal subsidence follows the piecewise empirical plate-cooling
relations reported by Parsons and Sclater (1977), DOI
<https://doi.org/10.1029/JB082i005p00803>. For age `t` in Myr, depth is:

```text
t <= 70: d = 2500 + 350 sqrt(t) metres
t > 70:  d = 6400 - 3200 exp(-t / 62.8) metres
```

Continental relief uses density-aware Airy column balance. Equal mass at the
compensation depth is applied to the V5 thickness and substrate density, with
mantle density 3300 kg/m3, a 35 km / 2800 kg/m3 continental reference column,
and 250 m reference freeboard. This is a local isostatic approximation, not an
elastic-flexure or dynamic-mantle solution.

The global liquid-water inventory uses the NOAA/NGDC Earth ocean estimate of
`1.335e18 m3` at Earth radius and scales with spherical area, preserving an
equivalent global water layer. The bath-tub volume solve is a Sekai numerical
operator; it is not a claim that real sea level ignores self-gravitation,
sediment displacement, ice, or dynamic topography.

The bounded response time that converts present forcing rates into an initial
dynamic-relief contribution, the passive-margin graph profile, deterministic
lithology assignment, and conditioned regional detail are explicit Sekai
procedural extensions. They must remain named, unit-bearing, bounded, and
separate from the cited equations.

## 3. `GeologicSubstrateSnapshot`

The strict schema publishes, on the authoritative surface:

- the exact dominant V5 crust category;
- area-weighted crust thickness in km;
- ocean age with the continental sentinel unchanged;
- volume-weighted crust density in kg/m3;
- broad bedrock/lithology category;
- normalized fracture intensity, erodibility, and relative permeability;
- sediment-source category;
- the complete spherical mantle/hotspot snapshot, exposing heat flow and
  volcanic influence without duplicate fields.

The density recipe uses 2800 kg/m3 continental and 2950 kg/m3 oceanic material,
volume weighted in mixed cells. Every copied crust value is cross-validated
against `EvolvedTectonicSnapshot`; the artifact cannot become a second source
of tectonic truth.

Bedrock selection is causal and deterministic:

1. strong present volcanic influence selects volcanic cover;
2. strongly shortened/uplifted continental cells select metamorphic rock;
3. subsiding low-fracture continental cells may select sedimentary cover;
4. remaining continental cells select crystalline basement;
5. remaining oceanic cells select oceanic mafic rock.

Erodibility and permeability start from the selected lithology and are modified
only by bounded fracture and basin terms. Sediment-source category is a stable
mapping from lithology, not a future sediment-deposit prediction.

## 4. `PrimaryReliefSnapshot`

The snapshot stores a V4-shaped compatibility relief plus separate causal
components:

```text
isostatic_base_m
dynamic_tectonic_offset_m
volcanic_construction_m
passive_margin_offset_m
conditioned_regional_detail_m
elevation_m = exact component sum
```

The compatibility view maps base, tectonic, and volcanic directly and combines
passive margin plus regional detail into its bounded regional component.

### 4.1 Isostatic base

For continental material:

```text
h = 250
  + [ (rho_m - rho_c) T
      - (rho_m - 2800) 35 ] / rho_m * 1000 metres
```

where thickness `T` is in km. Oceanic material starts from the Parsons-Sclater
depth and receives the corresponding density/thickness buoyancy correction
relative to a 7 km / 2950 kg/m3 column. Mixed cells blend continental and
oceanic bases by their V5 reference-area fractions.

### 4.2 Dynamic tectonic response

The accumulated V5 tectonic response remains the long-lived state. Present
uplift and subsidence rates add a bounded initial response over a declared
`0.25 Myr` characteristic interval:

```text
dynamic = 0.65 * accumulated_response
        + 250 * (uplift_rate - subsidence_rate)
```

with rates in mm/year and result in metres. It is clamped only to the public
tectonic-component safety envelope. Transform forcing remains exactly zero by
the P2 contract.

The V5 compatibility field is an absolute accumulated coarse response, while
P3 publishes a signed dynamic contribution. On cells with nonzero normal
forcing, P3 projects the inherited response onto the net forcing's sign before
applying the equation: net uplift cannot inherit a negative dynamic
contribution and net subsidence cannot inherit a positive one. Cells without a
net normal forcing are unchanged. Quality sampling uses the same net-sign
definition at multi-boundary junctions. This named causal projection prevents a
legacy reference elevation from reversing the present V5 cause; it does not
change the rate multiplier, crust, or final safety bounds.

### 4.3 Volcanic and passive-margin construction

The existing spherical hotspot-chain construction is reused with the P3 stage's
isolated deterministic stream. Passive-margin seeds are crust-transition edges
that are neither active convergent/divergent/transform boundaries nor strongly
forced. A bounded graph-distance profile raises the oceanward shelf/rise and
lowers the continental edge; it never changes crust category.

### 4.4 Conditioned regional detail

The existing resolution-limited spherical broad/Gabor field is reused only as
conditioned detail. It is oriented by V5 lineation and orogeny, decays with
event/ocean age, has zero authority over crust, forcing, sea level, or material,
and remains a separately published component.

## 5. Water-volume sea level

P3 never selects sea level from a desired land percentile. It solves the
monotone piecewise-linear equation:

```text
sum_i area_i * max(sea_level - elevation_i, 0) = water_inventory_m3
```

using stable `(elevation, CellId)` order and compensated sums. The final stored
water volume is recomputed from the quantized sea level and must close to the
inventory within `1e-6` relative error. Land/ocean classification remains the
existing centimetre-quantized semantic.

`ReliefSpec.target_land_fraction` is retained only as an authoring constraint.
The artifact stores requested and physical fractions plus
`Satisfied`/`Infeasible`; it never moves physical sea level. Satisfaction uses
the larger of 0.02 area fraction or one maximum-cell quantization fraction.

## 6. Quality gates

The fixed P0 17-seed Draft corpus uses the same surface, formation, and default
specifications as P2. Per-world hard gates are:

1. exact substrate/evolved identity and zero non-finite values;
2. water-volume relative error `<= 1e-6`;
3. component closure maximum error `<= 0.02 m`;
4. every final elevation remains within `-11,000..=9,000 m`;
5. maximum final plate-area and all upstream P2 hard gates remain valid.

Corpus morphology/science gates are:

1. median dominant-continental minus dominant-ocean elevation separation is
   at least 2500 m;
2. median physical land area is `0.20..=0.55`;
3. at least 80% of sampled active convergent uplift cells have positive dynamic
   relief;
4. at least 80% of descending subduction cells have negative dynamic relief;
5. median old-ocean (`>= 80 Myr`) depth exceeds young-ocean (`<= 20 Myr`)
   depth by at least 600 m;
6. regional-detail RMS is `0.01..=0.30` of total elevation RMS;
7. at least 80% of hotspot source cells have positive volcanic construction;
8. median buffered coast/plate-boundary overlap is at most 0.35.

Statistical metrics may fail on an individual sparse world and are recomputed
over original corpus samples. No absent denominator silently passes.

## 7. Determinism, cancellation, and failure policy

- Stage identities are `sekai.core/natural.geologic-substrate@1` and
  `sekai.core/natural.primary-relief@1`.
- The new graph consumes P2, resolved geology/formation, relief authoring, and
  the authoritative surface through exact typed dependencies.
- Every long dense loop polls the monotonic cancellation signal at bounded
  intervals.
- Unsupported schema, surface mismatch, copied-field disagreement, invalid
  density/lithology, impossible component bound, water-solve failure, or hard
  quality failure prevents publication.
- Fixed-seed JSON is byte-identical. Evidence, atlas, and timing output remain
  ignored beneath `target/natural-quality/p3`.

## 8. P4 handoff and known limits

P3 hands P4 a fixed initial terrain, physical sea level, land/ocean mask,
substrate, permeability, heat flow, and tectonic/volcanic causes. P4 then solves
the already locked coupled global atmosphere-ocean C0-C2 system. P3 has no
preview wind, latitude-band climate substitute, or decorative ocean current.

P3 relief is pre-erosion. Drainage, rivers, sediment redistribution, coastal
response, glacier carving, and final spectral detail remain P5-P9 work, so P3
is not compared visually with Gleba as a finished product.

## 9. 修订记录

- **A1（2026-08-22，T0 测高校准 L0，规格
  `2026-08-21-t0-hypsometric-calibration-design.md` §4）。** §4.2 的
  `accumulated_response` 在**衬底 `crust_kind == Oceanic` 的格元上恒为 0**，
  只保留速率响应 `DYNAMIC_RATE_RESPONSE_M_PER_MM_PER_YEAR × (U − S)`；
  陆壳格元不变。原因：v5 兼容场 `tectonic_elevation_m` 在洋壳上由
  `oceanic_plate_cooling_elevation_m` 初始化并在松弛中继续加深，是与 §4.1
  Parsons–Sclater 基底同一物理量的第二次入账（实测被继承场在洋壳上均值
  −2437 m，经权重与符号投影后动力项均值 −1470 m，使浴缸海面落到
  −1374 m）；v5 设计 §7 亦明文该场只供 V3 兼容视图使用。§2 对 GDH1 的
  引用更正：本文与代码采 Parsons & Sclater 1977。P3 十四项质量门禁的
  锁定边界不变，证据数值按 T0 规格 §6 刷新。
