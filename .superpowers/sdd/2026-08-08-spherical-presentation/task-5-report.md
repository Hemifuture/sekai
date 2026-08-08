# Task 5 Report: Seam-Safe Projected Map Geometry

## Implementation summary

- Added `PreparedProjectedMap`, `ProjectedMapVertex`, `ProjectedEdgeSegment`, `SphericalMeshBudgets`, and `SphericalMeshError` as renderer-neutral, source-bound projected geometry contracts.
- Built disposable cell triangle/fragment geometry directly from each authoritative spherical cell centroid and cyclic boundary vertices. Every emitted display vertex retains the original typed `CellId`; every edge fragment retains the original typed `EdgeId`.
- Split only display polygons and edge segments at the active anti-meridian. Seam intersections are found by bounded 64-step bisection over normalized points on the original minor great-circle arc. Each bisection sample unwraps longitude continuously near its expected value instead of comparing independently wrapped longitudes.
- Added a crate-private projection entry point for an explicit latitude and relative longitude. This preserves the `+pi` versus `-pi` display side and the multiple valid longitude representations of an exact pole without exposing another public projection API.
- Represented an exact spherical pole as two disposable projected polygon corners at its adjacent arc longitudes. This fills the projection outline without clamping a near-pole direction, dropping a polar cell, or creating a semantic vertex/ID.
- Added active-projection span validation, finite/non-degenerate triangle and edge validation, checked arithmetic and `usize -> u32` conversions, distinct cell/vertex/index/edge-segment budget errors, and defaults derived from the schema-V1 `MAX_SPHERICAL_*` limits.
- Kept `SphericalPresentationSource::new` crate-private. Generated-surface/source-bound tests live in `src/view/spherical_mesh.rs`; source-independent public budget contracts live in `tests/spherical_presentation_mesh.rs`.

## Files changed

- `src/view/spherical_mesh.rs` (new): projected map contracts, spherical seam clipping, great-circle intersection, pole handling, budgets/errors, and source-bound generated-surface tests.
- `src/view/spherical_projection.rs`: crate-private explicit latitude/relative-longitude forward path shared by ordinary projection and seam/pole display geometry.
- `src/view/mod.rs`: module registration and public exports.
- `tests/spherical_presentation_mesh.rs` (new): public budget/default/checked-count contracts.
- `.superpowers/sdd/2026-08-08-spherical-presentation/task-5-report.md` (new): this report.

## RED/GREEN evidence

### Initial interface RED

Command:

```powershell
cargo test --test spherical_presentation_mesh -- --nocapture
```

Result: exit 1 as expected. `src/view/mod.rs` could not import the five not-yet-implemented Task 5 interfaces from `spherical_mesh`.

### Public count/budget GREEN

Command:

```powershell
cargo test --test spherical_presentation_mesh -- --nocapture
```

Final result: exit 0; 2 passed, 0 failed. The suite distinguishes cell, vertex, index, and edge-segment budgets, exercises checked `u32` overflow on 64-bit hosts, and verifies explicit defaults cover authoritative schema-V1 limits.

### Generated geometry RED and pole root cause

Command:

```powershell
cargo test --lib spherical_mesh -- --nocapture
```

First geometry result: exit 1. The 42-cell Equal Earth case at central meridian `0` rejected cell 30 because its exact north-pole centroid had been assigned `atan2(0, 0) == 0`; that false unique longitude produced a triangle x-span `3.1313914494403825`, larger than the active Equal Earth half-width `2.7066299836960748`.

The focused regression was strengthened to distinguish exact poles from near-pole directions. Before the exact-pole predicate fix:

```powershell
cargo test --lib view::spherical_mesh::tests::only_exact_poles_expand_to_multi_longitude_display_polygons -- --nocapture
```

Result: exit 1 as expected; the near-pole fan incorrectly expanded to four display corners instead of remaining a three-corner fan.

After expanding only exact poles into their projection-boundary longitude interval and suppressing zero-length seam-endpoint edge artifacts, the final source-bound suite passed:

```powershell
cargo test --lib spherical_mesh -- --nocapture
```

Final result: exit 0; 4 passed, 0 failed.

### Great-circle intersection mutation RED/GREEN

The generated-surface test inverses every projected edge-fragment endpoint and checks that its endpoint-angle sum equals the authoritative minor-arc angle within `2e-10` radians. To prove this assertion detects forbidden latitude/longitude interpolation, the final arc point was temporarily mutated to linearly interpolate latitude at the seam.

Command:

```powershell
cargo test --lib view::spherical_mesh::tests::generated_maps_are_seam_safe_finite_complete_and_preserve_semantic_ids -- --nocapture --test-threads=1
```

Mutation result: exit 1 as expected at authoritative `EdgeId(54)`. Restoring the bounded great-circle arc point made the same command pass: 1 passed, 0 failed.

### Explicit default RED/GREEN

Before the associated default was implemented, the public test failed to compile with `E0599`: no associated item `SphericalMeshBudgets::DEFAULT`. After adding the constant from the schema maximum-derived limits, the complete public suite passed 2/2.

## Exact final verification commands and results

```powershell
cargo test --test spherical_presentation_mesh -- --nocapture
# exit 0: 2 passed, 0 failed

cargo test --lib spherical_mesh -- --nocapture
# exit 0: 4 passed, 0 failed

cargo test --test spherical_projection -- --nocapture
# exit 0: 8 passed, 0 failed

cargo test --test spherical_picking -- --nocapture
# exit 0: 5 passed, 0 failed

cargo fmt --all -- --check
# exit 0

cargo clippy --lib --test spherical_presentation_mesh -- -D warnings
# exit 0, no warnings

git diff --check
# exit 0

cargo test
# exit 0 in 146.6 seconds: 258 library tests (257 passed, 1 ignored), followed by all integration and doc tests with no failures
```

## Self-review

- Verified generated 42- and 162-cell surfaces for Equal Earth and normalized equirectangular projections at central meridians `0`, `pi/2`, and `pi - 1e-9`.
- Verified all projected coordinates are finite, every emitted triangle has nonzero signed area, every index is in range, and vertex/triangle `CellId` sets equal the authoritative set exactly. The set equality also proves every authoritative cell has at least one triangle.
- Classified authoritative fan triangles before projection. Ordinary non-seam/non-pole cells retain exactly one output triangle per source fan; seam fragments remain bounded to the original `CellId`; exact-pole fans use only disposable duplicate display corners.
- Verified every authoritative `EdgeId` appears, no unknown ID appears, and each edge emits one or two fragments. Inverse-projected fragment endpoints remain on the original authoritative minor arc.
- Verified each triangle edge is bounded by half the active projection's full x-span, so Equal Earth uses its own bounds and equirectangular uses `[-1, 1]` rather than sharing a hard-coded limit.
- Verified directions one micro-radian to both sides of the anti-meridian round-trip through each projection and resolve to the same authoritative locator IDs.
- Confirmed clipping does not linearly interpolate latitude/longitude, clamp exact or near poles, drop a cell, mutate the surface, create IDs, or carry science/field values.
- Confirmed `SphericalPresentationSource::new` remains crate-private and the prepared map returns the exact source identity it received after validating `SurfaceRef` equality.
- Confirmed all count growth is checked before allocation/index emission, and all stored count/index conversions are checked against `u32`.
- `cargo fmt`, strict Clippy, `git diff --check`, focused/adjacent suites, and the full suite are clean.

## Concerns

No blocking concerns. Exact poles necessarily have multiple planar longitude representations; the implementation duplicates only disposable display corners with the same `CellId`. The authoritative spherical point and all science remain unchanged. The additional `spherical_projection.rs` change is crate-private and exists solely to project an explicitly unwrapped seam/pole longitude without losing its display side through ordinary wrapping.
