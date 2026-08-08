# Task 4 Report: Shared Spherical Picking

## Implementation summary

Added the renderer-neutral `SphericalEntityLocator`, bound to a validated `SphericalPresentationSource` and authoritative `SphericalSurfaceSnapshot`. It caches only unit sites and each cell's incident edge IDs, endpoint directions, and midpoint directions. Cell lookup is a click-only O(n) maximum-dot scan with exact `f64::total_cmp` ordering and lower-ID ties. Edge lookup only evaluates cached incident edges, measures the minor great-circle segment distance, validates a finite `0.0..=PI` tolerance, and resolves equal distances by lower `EdgeId`.

Added normalized `UnitRay`, structured `RayError`, `RaySphereHit`, and a stable unit-sphere quadratic intersection that returns the nearest non-negative hit. Public exports are available from `sekai::view`.

## Files changed

- `src/view/spherical_picking.rs` (new): locator, ray primitive, geometry helpers, and crate-local source-dependent tests.
- `src/view/mod.rs`: spherical picking module and public exports.
- `tests/spherical_picking.rs` (new): public ray and projection behavior tests.

## RED/GREEN evidence

1. RED: `cargo test --test spherical_picking -- --nocapture` failed with unresolved `intersect_unit_sphere` and `UnitRay` imports before the ray API existed.
2. GREEN: the same command passed all 4 public tests after the minimal ray API and unit-sphere intersection implementation.
3. RED: `cargo test --lib spherical_picking -- --nocapture` failed with unresolved `CachedCell`, `CachedEdge`, and `SphericalEntityLocator` imports before locator implementation.
4. GREEN: the crate-local locator suite passed after the source-bound cache and deterministic lookup implementation.
5. Segment-distance regression: with the minor-arc projection intentionally mutated to return edge-midpoint distance, `cargo test --lib spherical_picking::tests::equal_dot_and_equal_incident_edge_distances_choose_lowest_stable_ids -- --nocapture` failed (`None` instead of `Some(EdgeId(2))`) at a true segment endpoint with zero tolerance. Restoring the projection-based minor-arc distance made the final suite pass.

## Test commands and results

- `cargo test --lib spherical_picking -- --nocapture` — passed: 3 tests.
- `cargo test --test spherical_picking -- --nocapture` — passed: 4 tests.
- `cargo test --test spherical_foundation_build -- --nocapture` — passed: 6 tests.
- `cargo clippy --test spherical_picking -- -D warnings` — passed with no warnings.
- `cargo test` — passed with exit code 0 (254 library tests plus all integration and doc tests).

The first full-suite attempt was stopped by the command's 120-second execution cap while progressing through the spherical hydrology tests; it showed no test failure. The final rerun used a longer cap and completed with exit code 0.

## Self-review

- Confirmed no locator field carries polygon geometry or field payloads.
- Confirmed source/snapshot identity mismatch is rejected before caching.
- Confirmed site ties use exact floating ordering and lower `CellId`.
- Confirmed edge selection inspects only the selected cell's incident cache, not a global edge set; endpoint regression proves this is segment rather than midpoint picking.
- Confirmed tolerance is finite and bounded by `PI`; misses and non-incident edges return `None`.
- Confirmed ray origins and directions validate/normalize, and intersection chooses the nearest non-negative solution.

## Concerns and follow-up

No blocking concerns. The locator deliberately uses O(n) cell lookup only at discrete pick time; do not add a spatial index until a 20k-cell click-picking benchmark has been recorded and demonstrates the need.
