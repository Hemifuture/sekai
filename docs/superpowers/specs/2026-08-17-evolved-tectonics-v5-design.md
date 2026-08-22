# Evolved Tectonics V5 Design

Date: 2026-08-17
Program phase: P2
Status: locked for implementation

## 1. Outcome and scope

P2 replaces the production candidate's V4 tectonic publication path with a
conservative V5 path while leaving the frozen V4 graph and its P0 byte baseline
intact. V5 evolves crust on the P1 tectonic-control surface, publishes onto the
P1 authoritative surface only through the retained conservative map, and adds
explicit material, forcing, lineage, and remap diagnostics.

P2 ends at tectonic cause fields. It does not synthesize final substrate,
isostatic relief, climate, erosion, soil, ecology, or natural-view shading.
Those remain P3-P9 work. The global atmosphere and ocean circulation contract
remains locked for P4, P6, and P7.

## 2. Baseline diagnosis

The V4 generator is a serious Cortial-inspired implementation, not random
height noise. It contains:

- rigid Euler rotations on a closed sphere;
- a 2 Myr step and 128 bounded iterations;
- oceanic subduction polarity, trench and overriding-side uplift curves;
- discrete terrane collision and suturing;
- oceanic gap filling with age zero and ridge lineation;
- stochastic plate rifting;
- 10-60 step resampling based on maximum angular displacement;
- continental erosion, oceanic cooling/damping, and trench filling.

The weak result is caused by representation and publication defects rather
than the absence of tectonic vocabulary:

1. `CrustKind` and `thickness_km` are treated as sample attributes, not
   conserved quantities. When moving samples overlap, V4 counts each occupied
   anchor once and loses the material represented by the other samples.
2. Collision transfers ownership and later watershed reconstruction can allow
   one lineage to absorb most of the sphere. The existing 642-cell corpus
   contains final plates owning 60-78% of the surface.
3. V4 initializes plate seeds with a farthest-point distribution and only
   perturbs seed positions. The result is still a geometric Voronoi partition;
   51.4% of measured macro triple-junction angles sit within 10 degrees of 120
   degrees in the existing corpus.
4. Above 5,000 cells V4 evolves an independently created control mesh and
   performs a one-shot barycentric/category projection. That projection is not
   the P1 conservative overlap map and has no extensive budget.
5. Accumulated elevation is used as both the tectonic cause and its response.
   A downstream stage cannot distinguish active uplift, active subsidence, or
   inherited relief.
6. The current snapshot has no material or lineage closure evidence. A valid
   wire payload can therefore be scientifically non-conservative.

These are algorithm/data-model defects. Renderer tuning cannot repair them.

## 3. Algorithm provenance and permitted extensions

The reference is Cortial, Peytavie, Galin, and Guerin, *Procedural Tectonic
Planets*, Computer Graphics Forum 38(2), 2019,
<https://doi.org/10.1111/cgf.13614>. The authors describe their method as a
procedural heuristic for plausible interactive authoring, not a predictive
geodynamic simulation.

V5 retains the following reference semantics:

- one rigid geodetic rotation per live plate;
- the 2 Myr discrete step;
- oceanic crust preferentially descends at convergence and older oceanic crust
  descends for ocean-ocean convergence;
- subduction uplift depends on boundary distance, relative speed, and
  descending relief;
- collision is a discrete terrane suture/transfer event;
- spreading creates age-zero oceanic material and ridge-parallel lineation;
- rifting creates two to four diverging child lineages;
- resampling cadence is bounded by maximum plate displacement;
- the Appendix A distance, elevation, and rate constants remain recognizable
  and unit-bearing.

The following are deliberate Sekai V5 extensions and must never be described as
literal Cortial equations:

- extensive continental/oceanic reference area and volume tracers;
- exact material and lineage ledgers;
- anisotropic graph-distance initial plate domains;
- a maximum live-plate area invariant with deterministic mechanical
  fragmentation;
- conservative P1 control-to-authoritative remapping;
- separated present-day forcing and accumulated response;
- bounded pure-shear continental extension before oceanic gap creation;
- explicit cancellation, validation, quality metrics, and atomic publication.

V5 is therefore a conservative, testable extension of the reference procedural
model. It is not a mantle-convection solver and must not be marketed as one.

## 4. Compatibility and artifact boundary

V4 remains available unchanged:

- `SphericalTectonicSnapshot` schema V3 remains strict;
- `SphericalTectonicStage` remains version 4;
- `world.spherical-tectonics` remains the legacy artifact key;
- the P0 V4 JSON/CSV hashes must remain byte-identical.

V5 introduces:

- `EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1 = 1`;
- `EvolvedTectonicSnapshot`;
- `EvolvedTectonicArtifact` with key `world.evolved-tectonics`;
- `EvolvedTectonicStage` with identity
  `sekai.core/natural.evolved-tectonics@5`;
- a strict `NaturalQualityProfileArtifact` input for the V5 graph.

`EvolvedTectonicSnapshot` contains a V3 `SphericalTectonicSnapshot` as its
compatibility/current-state view. Existing plate, boundary, geology, and relief
consumers can be migrated one phase at a time without weakening the new
contract. The wrapper additionally stores the exact `NaturalResolutionPlan`,
material fields, forcing fields, and diagnostics. Its authoritative
`SurfaceRef` must match all nested fields.

The V5 stage accepts the already-built authoritative
`SphericalSurfaceArtifact`. It asks P1 to complete that exact surface with the
profile's transient control surface and conservative map. It never creates a
second authoritative world and never publishes the control surface as a world
identity.

## 5. Material representation

### 5.1 Transient sample columns

Every moving control sample carries two material components:

```text
continental_reference_area_m2
continental_volume_m3
oceanic_reference_area_m2
oceanic_volume_m3
```

All four values are finite and non-negative. Reference area is the surface
footprint represented by a moving material sample. Volume is crustal volume,
not mass; density is introduced by P3 substrate. A pure initialized sample has
one component equal to its control-cell area and the other equal to zero.

Component mean thickness is derived, never independently invented:

```text
thickness_km = volume_m3 / reference_area_m2 / 1000
```

Zero-area components must also have zero volume. Non-zero continental
components remain within 20-80 km and non-zero oceanic components within
3-15 km after each committed/resampled state.

The legacy `kind` is a compatibility category selected by the greater local
reference area, with continental winning an exact tie. The legacy thickness is
the selected component's derived thickness. Category selection never mutates
the extensive fields.

### 5.2 Initial state

Initial continental reference area is selected by the existing area-weighted
continental quantile and differs from the requested fraction by at most one
control cell. Initial volumes are exactly reference area times the initialized
thickness field. Initial material totals are recorded before the first motion
step.

### 5.3 Named process rules

- **Rigid motion:** changes position and ownership geometry only; all four
  material quantities are bit-preserved.
- **Oceanic subduction:** consumes the descending sample's eligible oceanic
  component first. A mixed sample retains its continental component. Any
  forced continental consumption must be separately named and budgeted; the
  normal ocean-continent path cannot consume continental material.
- **Continental collision:** transfers terrane ownership without changing
  material totals. Overlapping continental columns may be stacked during the
  conservative resample, increasing local thickness while preserving volume.
- **Rifting/extension:** applies the bounded pure-shear factor `beta` already
  derived from extension speed. Continental reference area is multiplied by
  `beta` and continental volume is unchanged, so the column thins. Extension
  stops at 20 km. It does not switch the compatibility category to oceanic.
- **Spreading:** only uncovered divergent gaps create oceanic material. New
  oceanic reference area is the gap-cell area, volume is area times 7 km, age
  is zero, and lineation is ridge-parallel.
- **Relaxation:** changes accumulated topographic response and ages, never
  material area, volume, or category.
- **Resampling:** preserves continental reference area and both material
  volumes. Sphere coverage is closed by creating or consuming oceanic
  reference area only, and that adjustment is explicitly recorded.

### 5.4 Dense conservative control resampling

V5 uses the moved-sample field as the data term, but the material solve is
different from V4's occupied-anchor MBO correction:

1. Sum source continental reference area and both component volumes with
   compensated accumulation. Overlaps do not collapse the sum.
2. Diffuse the categorical phase for three graph-heat steps as in V4.
3. Sort cells by phase, geometric affinity, stable source identity, then cell
   ID.
4. Allocate full continental cells until the target is bracketed. At most one
   pivot cell contains fractional continental/oceanic reference area so the
   continental total closes to floating-point tolerance.
5. Set oceanic reference area to `cell_area - continental_area` in every cell.
   The difference from the incoming oceanic total is a named coverage-closure
   source or sink.
6. Distribute each component volume with bounded proportional water filling
   using interpolated thickness as weights and the public thickness limits as
   lower/upper bounds.
7. Correct only the final eligible cell by the remaining compensated residual.
   If the requested total is outside the bounded feasible interval, fail.

This operator preserves a continuous material tracer while leaving a strict
categorical compatibility view. It has no nearest-neighbour fallback for
extensive totals.

## 6. Plate domains and lineage conservation

### 6.1 Initial domains

V5 retains deterministic seed selection but replaces geometric nearest-center
assignment with a multi-source anisotropic shortest-path partition. Each
lineage has an independently seeded low-frequency spherical cost field. Edge
cost is the canonical graph traversal cost multiplied by a bounded factor in
`[0.70, 1.30]`. Stable `(cost, lineage, cell)` ordering makes the result
deterministic and each domain connected by construction.

This is the actual warped-distance partition that V4's comments intended but
its center-only perturbation did not implement.

### 6.2 Collision and maximum plate area

Collision transfers only the connected terrane that actually intersects. A
transfer is rejected if it would make the receiver own more than 45% of the
sphere. After every conservative resample, a live lineage above 40% is
mechanically fragmented into two to four anisotropic connected children using
the existing rift construction and divergent rotations. The 40% trigger leaves
one-cell quantization and later transfer headroom below the hard 45% published
invariant.

Fragmentation changes ownership/motion only. Material quantities are copied
without change. No lineage ID is reused.

### 6.3 Lineage ledger

The published lineage budget records:

```text
initial_lineages
allocated_lineages
retired_lineages
final_live_lineages
terrane_transfer_count
mechanical_fragmentation_count
```

and validates exactly:

```text
initial_lineages + allocated_lineages
    = retired_lineages + final_live_lineages
```

Final live IDs are canonicalized for the compatibility snapshot, while the
ledger counts transient never-reused identities.

## 7. Present-day forcing

V5 computes a fresh final contact set after the last resample. It publishes
these dense authority fields:

```text
uplift_rate_mm_per_year       >= 0
subsidence_rate_mm_per_year   >= 0
shortening_rate_mm_per_year   >= 0
boundary_distance_m           >= 0
event_age_myr                 >= 0 or -1 sentinel
```

The fields have these meanings:

- oceanic-subduction descending cells receive subsidence/trench forcing;
- the overriding side receives positive uplift derived from the locked
  distance/speed/elevation transfer curve;
- active continental-collision cells on both sides receive positive shortening
  and uplift even before/after the one-step ownership transfer;
- divergent continental cells receive extensional subsidence, while a newly
  spread ocean cell receives ridge construction but not convergent uplift;
- transform contacts contribute zero uplift, subsidence, and shortening;
- `boundary_distance_m` is the minimum great-circle distance to a present-day
  active event reference;
- active-event cells have event age zero; inherited orogeny uses its stored
  age; cells with no event history use `-1`.

Forcing is instantaneous cause. `tectonic_elevation_m` remains the accumulated
coarse response used only by the V3 compatibility view. P3 consumes forcing and
material separately.

## 8. Control-to-authoritative publication

Publication uses only `ProfileSurfaceBundle.control_to_authoritative_map()`:

- all four material quantities use `remap_extensive_f64`;
- elevation, boundary distance, event age, and non-negative rates use bounded
  intensive remapping, with weighted masks for sentinel/phase-specific ages;
- lineation uses three-dimensional tangent transport and target projection;
- owner, orogeny, and compatibility crust categories use stable overlap
  majority and publish ambiguity coverage;
- ocean age uses an oceanic-reference-area weighted numerator and denominator;
- compatibility thickness is derived from remapped component area and volume;
- final authority plate domains are canonicalized and boundary records are
  recomputed from the authoritative topology and rigid rotations.

Every extensive remap records source total, target total, absolute error, and
relative error. A field that exceeds P1's `1e-6` relative limit aborts
publication. There is no barycentric or nearest-cell material fallback.

## 9. Snapshot contracts

### 9.1 `SphericalCrustMaterialState`

The strict dense contract stores the four extensive fields. Validation checks:

- exact authoritative cell count;
- finite non-negative values;
- zero-area/zero-volume consistency;
- derived thickness ranges for every non-zero component;
- compensated totals equal the material budget's final authoritative totals
  within its recorded quantization bound.

### 9.2 `SphericalTectonicForcingState`

Validation checks exact dense lengths, finite values, non-negativity, event-age
sentinel semantics, and zero transform forcing when cross-validated against the
authoritative boundaries.

### 9.3 `SphericalTectonicMaterialBudget`

The budget records initial, process-source/sink, post-control, and final
authority totals for both area and volume, plus control-resample and authority
remap residuals. The core equations are:

```text
continental_area_final_control
  = continental_area_initial
  + rift_extension_area_gain
  - named_continental_consumption
  + continental_area_residual

continental_volume_final_control
  = continental_volume_initial
  - named_continental_consumption_volume
  + continental_volume_residual

oceanic_area_final_control
  = oceanic_area_initial
  + spreading_area_created
  + coverage_area_created
  - subducted_oceanic_area
  - coverage_area_consumed
  + oceanic_area_residual

oceanic_volume_final_control
  = oceanic_volume_initial
  + spreading_volume_created
  + coverage_volume_created
  - subducted_oceanic_volume
  - coverage_volume_consumed
  + oceanic_volume_residual
```

Authority totals then equal control totals plus the separately measured remap
residual. Relative material residuals must be `<= 1e-9` at control resolution
and `<= 1e-6` after P1 authority remap. Named normal-operation continental
consumption must be zero.

## 10. Determinism, cancellation, and failure policy

- The V5 stage seed is isolated by stage identity/version. V4 draws and hashes
  must not change.
- Every anisotropic field uses labeled counter/substreams and stable lineage or
  cell identities. Iteration order and thread scheduling cannot affect facts.
- Evolution checks `StageRng::check_cancelled` at least once per step, during
  domain solves, and at bounded field-remap intervals.
- The stage publishes one `EvolvedTectonicArtifact` only after nested snapshot,
  material, forcing, budget, lineage, P1 map, and quality validation all pass.
- Unsupported schema, mismatched surface/profile, non-finite data, infeasible
  bounded material allocation, missing eligible oceanic material, broken
  lineage closure, or exceeded remap error is a hard failure.

## 11. Earth-like acceptance protocol

The fixed corpus is the P0 17 seeds:

```text
42, 3, 7, 11, 19, 23, 29, 31, 43,
47, 59, 61, 71, 73, 83, 89, 97
```

Each uses Draft profile, Earth radius, `Continents`, 12 initial plates, and
initial continental fraction 0.38. Intrinsic V5 gates are:

1. corpus median evolved continental reference-area fraction is
   `0.30..=0.45`;
2. every seed retains `0.75..=1.15` of initialized continental reference area;
3. every seed's largest live plate owns at most 45% of spherical area;
4. at least 80% of sampled ocean-continent subduction transects put positive
   subsidence/trench forcing on the descending side and positive uplift on the
   overriding side;
5. at least 80% of sampled continental-collision transects have positive
   shortening and positive uplift on both participants;
6. area-weighted Spearman rank correlation between ocean age and depth is at
   least `0.70` (equivalently older ocean is deeper);
7. transform median absolute uplift is at most half the convergent median;
8. no more than 35% of measured macro triple-junction angles are within 10
   degrees of 120 degrees;
9. all material remap/ledger residuals and lineage equations pass their strict
   limits;
10. repeated fixed-seed artifacts are byte-identical.

The coast/plate-boundary gate needs a formed surface. P2 evaluates it through a
read-only compatibility harness using the existing relief generator and requires
the 17-seed median buffered overlap to be at most 35%. P3 must re-run the same
gate against its physical primary relief before replacing that harness; P2 does
not make the legacy relief a V5 production dependency.

The corpus also records, without weakening gates, plate count, category
ambiguity, event sample counts, minimum/median/maximum material thickness,
control/authority duration, and artifact size.

## 12. Verification and P2 completion

P2 is complete only after all of the following are fresh and green:

- RED tests demonstrate V4 material loss, giant-plate, regular-junction, and
  missing-forcing defects before V5 implementation;
- strict contract, malformed serde, analytic material-process, resampling,
  forcing, conservative publication, stage, and cancellation tests;
- the full 17-seed Release evidence writer and atlas;
- unchanged P0 V4 JSON/CSV hashes;
- native formatting, all-target/all-feature check, warning-free Clippy, and
  WASM check;
- Release performance measurements for Draft, Standard, and High, with
  Standard/High allowed as cancellable background builds;
- direct code/algorithm conformance review with no unresolved Critical or
  Important finding;
- an inspectable P2 completion record containing exact hashes, metrics,
  timings, known limitations, and the P3 handoff.

## 13. 修订记录

- **A1（2026-08-22，T0 测高校准 L2，规格
  `2026-08-21-t0-hypsometric-calibration-design.md` §4）。** 三处偏离本文的
  冻结文本，均由该规格的 Task 1 诊断驱动（陆壳厚度谱坍缩为 32–35 km
  尖峰、无造山根）：
  1. **初始陆壳清单（§5.1 / `initial_crust_samples`）**：连续陆壳格元的
     厚度不再是 `24 km + 28 km × fbm`，而是把同一 fbm 信号在陆壳格元上的
     面积加权秩映射到 `CRUST1_PLATFORM_THICKNESS_QUANTILES_KM`（CRUST1.0
     台地/地盾/克拉通类型群的 21 点分位表，world 层唯一事实源）。噪声仍决定
     厚薄台地的位置；两个名义常量退役。该映射同时服务 legacy 球面控制面
     路径（共用 `initial_crust_samples`）。
  2. **碰撞缩短增厚（§5.3 具名过程规则新增一条）**：`apply_collision_v5`
     在每个大陆碰撞事件上记录参与样本的法向汇聚速率，随后对每个汇聚样本
     施加有界纯剪切缩短 `MaterialColumn::shorten_continental_pure_shear`
     （`β = 1 + 汇聚量 / CONTINENTAL_COLLISION_ZONE_WIDTH_M`，上限
     `MAXIMUM_STEP_STRETCH_FACTOR`，与裂谷拉张同一常量；厚度封顶
     `CONTINENTAL_CRUST_MAX_THICKNESS_KM`）。面积损失记入账本新项
     `collision_shortening_continental_area_loss_m2`（总量上限与裂谷增面
     对称：初始陆壳面积的 15%），体积位守恒；兼容高程不随厚度抬升——造山高程由 P3 的 Airy 柱从
     加厚的物质导出，再写进兼容高程会在 P3 的陆壳继承项上双计（首轮
     实施曾如此，seed 59 的峰顶因此越过 `ELEVATION_MAX_M`）；拉张侧既有
     的 Airy 沉降调整保留，记为不对称债务。§9.3 预算方程的陆壳面积项相应变为
     `initial + rift_gain − collision_loss − consumed`。科学依据：England &
     McKenzie 1982 薄粘性板增厚；Cortial et al. 2019 汇聚增厚；McKenzie
     1978 纯剪切运动学（与拉张互为逆过程）。
  3. **守恒重采样的陆壳体积分配（§5.4）**：原"赢家厚度偏好 + 均匀位移
     闭合总体积"改为**锚点沉积**：每个源样本是一个整包，沉积到其锚点
     格元；格元的半拉格朗日赢家先占位，其余同锚样本与超出格元配额的面积
     作为整包、按自身厚度沿拓扑弧广度优先流向最近的欠填陆壳格元
     （`deposit_continental_volumes`）。不再存在全局闭合位移，薄陆缘与
     厚造山根得以跨重采样保留；洋壳体积分配不变。这是本文 §5.3"重叠陆壳
     柱可在守恒重采样中堆叠"承诺的实现形式（一阶施主格守恒重映射）。
  - **诊断**：`SEKAI_V5_TRACE=1` 在初始态、每次重采样后与终态打印面积加权
    陆壳厚度清单（均值/sd/p05/p50/p95/极值）与沉积外流份额，与 P5 的
    `SEKAI_P5_TRACE` 同例。
  - 实测效果（草稿档 seed 42）：初始 sd 1.47 km → 4.14 km；终态 sd 2.63 →
    4.55 km；碰撞增厚把最大厚度推到 57 km；重采样均匀位移
    （每次 −0.2…−0.9 km）归零。P2/v5 全部指纹与证据按 T0 规格 §6 刷新。
