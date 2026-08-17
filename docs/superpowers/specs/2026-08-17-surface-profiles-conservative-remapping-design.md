# Surface Profiles and Conservative Remapping Design

Date: 2026-08-17  
Phase: P1 of the complete natural-world program  
Status: locked for implementation

## 1. Outcome

P1 establishes the spatial contract that every later scientific phase uses. It
adds semantic Draft, Standard, and High quality profiles; builds their
authoritative and tectonic-control surfaces without changing the existing Draft
geometry; and provides a deterministic conservative map between two validated
geodesic spherical surfaces.

P1 does not improve tectonic or relief morphology by itself. Its purpose is to
ensure that P2-P8 can move material, scalar state, vector state, and categories
between work grids without inventing or deleting quantities.

## 2. Fixed quality profiles

`NaturalQualityProfile` is a stable serialized enum with these exact settings:

| Profile | Authoritative target | Authoritative resolved | Tectonic target | Tectonic resolved | Climate face resolution |
| --- | ---: | ---: | ---: | ---: | ---: |
| Draft | 20,000 | 20,252 | 4,842 | 4,842 | 24 |
| Standard | 80,000 | 79,212 | 20,000 | 20,252 | 32 |
| High | 200,000 | 198,812 | 20,000 | 20,252 | 48 |

The existing `SphericalSpaceSpec.target_cell_count` remains the geometric source
of truth. A strict product resolution requires the target associated with the
selected profile and records both requested and resolved values in a versioned
`NaturalResolutionPlan`. Test fixtures may construct smaller surfaces directly,
but they are not product profile resolutions and cannot be serialized as one.

The existing spherical limits currently conflate requested and resolved counts.
P1 separates `MAX_SPHERICAL_TARGET_CELL_COUNT = 200_000` from the maximum actual
geodesic allocation `MAX_SPHERICAL_CELL_COUNT = 198_812`. A High request is valid
and deterministically resolves to the latter; no surface may allocate beyond the
frequency-141 vertex, edge, or cell bounds.

Draft remains the synchronous application default during P1. Standard and High
are explicit background-quality builds until the complete pipeline passes its
release budget. No existing Draft `SphericalSurfaceSnapshot` fingerprint or
semantic hash may change.

## 3. Module and ownership boundaries

Stable world contracts live in:

```text
src/world/natural/profile.rs
src/world/spatial/remap.rs
```

Numerical construction and application live in:

```text
src/generators/spatial/conservative_remap.rs
src/generators/spatial/profile_surface.rs
```

Engine cancellation lives in:

```text
src/engine/cancellation.rs
```

The stable contracts are:

- `NaturalQualityProfile`;
- `NaturalResolutionPlan` V1;
- `ConservativeSurfaceMap` V1;
- `SurfaceOverlapWeight`;
- `TangentTransform`;
- categorical-remap ambiguity evidence.

`ProfileSurfaceBundle` is generator-owned and returned atomically. It contains
the resolution plan, authoritative surface, transient tectonic-control surface,
and control-to-authoritative conservative map. The control surface is never a
separate published world identity. P2 consumes the bundle and publishes only
its evolved authoritative tectonic result plus the recorded resolution plan.

## 4. Conservative map representation

For source cell `s` and target cell `t`, the map stores physical spherical
overlap area `A[t,s]` in square metres. Entries are canonicalized by
`(target_cell, source_cell)` and contain:

- source cell ID;
- target cell ID, implicit in target-row offsets;
- positive finite overlap area;
- a deterministic source-east/source-north to
  target-east/target-north tangent transform.

The map also stores:

- schema version;
- exact source and target `SurfaceRef` values;
- source and target per-cell areas;
- target CSR row offsets;
- maximum source-column and target-row relative closure errors;
- construction iteration count and ambiguity-free topology diagnostics.

Allocation bounds are derived from the existing maximum spherical cell count.
Deserialization is bounded before allocation, denies unknown fields, validates
all finite/range constraints, and rejects unsorted, duplicate, empty-row, or
identity-mismatched data.

Source and target radii must agree within the existing spherical metric
tolerance. P1 does not remap between different planets or differently scaled
copies of a planet.

## 5. Overlap construction algorithm

The approved algorithm is deterministic spherical convex-polygon intersection,
not nearest-neighbour interpolation.

1. Validate both surfaces and create their exact `SurfaceRef` values.
2. If the surface references match, emit the exact identity map.
3. Treat the higher-cell-count surface as the fine tessellation and the other
   as the coarse tessellation; transpose ownership when required.
4. Build a deterministic three-dimensional k-d tree over coarse cell sites.
5. For every fine cell, locate its nearest coarse site and enumerate the coarse
   cell plus two stable adjacency rings.
6. Clip the fine spherical Voronoi polygon against each candidate coarse
   polygon using oriented great-circle half-spaces.
7. Compute positive intersection area by compensated spherical triangulation.
8. If candidate overlaps do not cover the fine cell within `1e-9` relative
   error, expand adjacency rings deterministically. Failure to close is a remap
   error; it cannot fall back to nearest-neighbour weights.
9. Assemble the sparse matrix and apply deterministic alternating row/column
   scaling against the authoritative cell areas. Stop only when both margins
   are within `1e-12` relative error, or fail after 96 iterations.
10. Quantize no overlap area in the V1 map. Serialized values remain `f64`.

The balancing step corrects only floating-point clipping residuals; tests bound
the adjustment from raw geometric overlap. It is not allowed to repair missing
candidate topology or negative area.

## 6. Conservation identities

The constructed map must satisfy:

```text
sum_s A[t,s] = target_area[t]
sum_t A[t,s] = source_area[s]
sum_t target_area[t] = sum_s source_area[s]
```

The production artifact accepts maximum relative row and column errors of
`1e-10`; the public P1 product gate is `1e-6`. The tighter constructor threshold
leaves room for later serialization and field quantization without weakening
the externally promised bound.

All summation and error measurement uses deterministic compensated accumulation
in canonical order. Empty cells, non-finite values, negative weights, duplicate
entries, and a source column with no overlap are invalid.

## 7. Field remapping semantics

### 7.1 Intensive scalar

An intensive scalar uses:

```text
target[t] = sum_s A[t,s] * source[s] / target_area[t]
```

Weights are non-negative, so results are clamped only for sub-ULP arithmetic
escape from the source minimum/maximum. A bitwise-constant input takes an exact
constant fast path. `f32` output must preserve a constant exactly after
quantization.

### 7.2 Extensive quantity

An extensive per-cell amount uses:

```text
target[t] = sum_s source_amount[s] * A[t,s] / source_area[s]
```

The compensated global total before and after remapping must agree to relative
error `<= 1e-6`, using the sum of absolute input magnitudes as the scale for
signed fields. No residual is silently discarded.

### 7.3 Tangent vector

Source vectors are represented in each source cell's canonical east/north basis.
For each overlap, the implementation reconstructs the global three-dimensional
tangent vector, orthogonally projects it into the target tangent plane, and
records its target east/north components. Area-weighted accumulation then uses
the intensive-scalar row normalization.

Every returned vector is therefore tangent by construction. For an analytic
solid-body rotation field, the area-weighted target direction agreement must be
at least `0.999`.

### 7.4 Category

Categories use overlap-area majority. Ties resolve by the lowest stable category
value. The result publishes area-weighted ambiguity coverage: a target cell is
ambiguous when no category owns more than half its overlap area. Category
remapping never changes the continuous overlap matrix.

## 8. Cancellation and atomic publication

`BuildCancellation` is a cloneable engine token backed by a monotonic atomic
flag. `BuildEngine::build_with_cancellation` checks it before dependency work,
before cache restore, before every stage, and before final publication. A
cancelled build returns a failed report with stable code `engine.cancelled` and
never exposes a partial `BuildArtifacts` store or inserts an unfinished cache
entry.

The existing `BuildEngine::build` delegates to a never-cancelled token and keeps
all current semantics and hashes.

`StageRng` carries a read-only cancellation handle so long-running numerical
code can cooperate without changing deterministic random streams. It exposes
only `is_cancelled` and `check_cancelled`; cancellation state is never hashed or
serialized. The spherical surface builder and conservative-map builder check at
least once per 256 work items and during every balancing iteration.

`ProfileSurfaceBuilder` returns no bundle until both surfaces, the map, and all
cross-validation have succeeded. Cancellation or validation failure leaves the
caller's previously published bundle unchanged.

## 9. Errors

Errors distinguish:

- unsupported schema or profile;
- profile/authoritative-target mismatch;
- invalid source or target surface;
- source/target radius mismatch;
- cancellation;
- spatial search failure;
- polygon clipping degeneracy;
- uncovered fine-cell area;
- sparse allocation overflow;
- non-positive or non-finite overlap;
- row/column non-convergence;
- field length or non-finite input;
- extensive conservation failure.

Numerical errors include the affected source/target cell where applicable,
observed residual, and required threshold.

## 10. P1 quality evidence

P1 adds versioned quality metrics rather than changing P0 identities:

```text
spatial.closed-sphere-area-relative-error.v1
spatial.shared-edge-flux-cancellation-max.v1
remap.constant-scalar-max-error.v1
remap.extensive-relative-error.v1
remap.source-margin-max-relative-error.v1
remap.target-margin-max-relative-error.v1
remap.solid-body-direction-agreement.v1
remap.category-ambiguity-area-fraction.v1
```

The Draft P0 corpus is rerun unchanged. P1 cannot claim improvement in the five
known V4 tectonic/relief failures because their algorithms are not replaced yet.

## 11. Verification matrix

P1 completion requires:

1. exact profile counts and strict mismatch rejection;
2. unchanged Draft surface semantic hash for the fixed existing fixture;
3. analytic identity-map and rotated solid-body-vector fixtures;
4. Draft control-to-authoritative and Standard/High product-map tests;
5. exact constant scalar preservation after `f32` quantization;
6. extensive relative error `<= 1e-6` for positive and signed fixtures;
7. row/column closure `<= 1e-10` in every constructed artifact;
8. direction agreement `>= 0.999` and target tangency;
9. deterministic byte-identical repetition and serde rejection tests;
10. native and WASM checks;
11. cooperative cancellation during both surface and remap work;
12. atomic engine/cache publication tests;
13. release time and persistent-memory evidence for Draft, Standard, and High;
14. an inspectable P1 completion report.

Standard and High release tests may remain ignored commands because they are
background product builds, but they must be run and recorded before P1 closes.

## 12. P2 handoff

P2 receives a frozen `NaturalResolutionPlan`, authoritative surface, transient
tectonic-control surface, and verified control-to-authoritative map. P2 may
evolve crust material only on the control surface and conservatively publish it
to the authoritative surface. It may not replace the map with nearest-neighbour
sampling, use the presentation detail pyramid, change profile counts, or weaken
P1 conservation thresholds.
