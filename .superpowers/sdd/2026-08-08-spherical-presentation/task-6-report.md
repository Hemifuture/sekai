# Task 6 Report: Undeformed Globe Mesh and Orthographic Trackball Camera

## Implementation summary

- Added `PreparedGlobeMesh` and `GlobeVertex` to the existing renderer-neutral spherical mesh boundary. The mesh emits one centroid/boundary-edge triangle per authoritative cell side, stores only finite `[f32; 3]` unit positions plus raw `CellId`, uses checked `u32` indices, and reverses only triangles whose stored-position normal points inward.
- Kept globe construction source-bound while preserving the crate-private `SphericalPresentationSource::new`. The public build signature accepts exactly `SphericalPresentationSource`, `&SphericalSurfaceSnapshot`, and `SphericalMeshBudgets`.
- Added `GlobeCamera` with a normalized world-to-camera quaternion, deterministic shortest-arc trackball drag, bounded `0.55..=8.0` orthographic scale, reset, front-hemisphere visibility, and orthographic screen-to-ray conversion.
- Documented one view convention: reset looks from `+Z` toward the origin; screen y is flipped into camera-up; visibility applies the world-to-camera orientation, while rays apply its inverse. The screen-center reset ray therefore hits canonical `+Z`.
- Added `MapCamera` with separately retained pan/zoom for Equal Earth and equirectangular projections, plus `SphericalViewMode::{Map, Globe}`. View-mode switches own no camera state and cannot overwrite either map projection or globe state.
- Every camera mutator rejects/ignores non-finite input through an explicit boolean result. Globe orientation is renormalized and sign-canonicalized after every rotation mutation.

## Files changed

- `src/view/spherical_mesh.rs`: unit-globe vertex/mesh contracts, source/budget validation, finite unit-radius conversion, outward winding correction, checked indices, and crate-private generated-surface tests.
- `src/view/spherical_camera.rs`: independent map/globe cameras, normalized quaternion math, trackball mapping, visibility, screen-to-ray conversion, and coordinate-contract documentation.
- `src/view/mod.rs`: Task 6 module registration and public exports.
- `tests/spherical_presentation_mesh.rs`: public compile-time function-signature regression proving globe construction has only source/surface/budgets.
- `tests/spherical_picking.rs`: public reset/drag/zoom/visibility/ray/non-finite-input/camera-independence behavior.
- `.superpowers/sdd/2026-08-08-spherical-presentation/task-6-report.md`: this report.

## RED/GREEN evidence

### Initial interface RED

Tests were written before production implementation. The required command was then run:

```powershell
cargo test --test spherical_presentation_mesh --test spherical_picking -- --nocapture
```

Result: exit 1 as expected. `tests/spherical_picking.rs` failed with `E0432` because `GlobeCamera`, `MapCamera`, and `SphericalViewMode` did not yet exist in `sekai::view`. This was the intended missing-interface failure, not a fixture or syntax failure.

### Initial public GREEN

After the minimum camera and mesh interfaces were implemented, the same command returned exit 0: 8 picking/camera tests passed and 2 public mesh-budget tests passed, with 0 failures.

### Source-bound globe GREEN

The generated-surface tests must remain in the module because the source constructor is crate-private:

```powershell
cargo test --lib spherical_mesh -- --nocapture
```

Result: exit 0; 10 passed, 0 failed, 254 filtered out. This includes the two Task 6 tests for unit radii/outward winding/exact semantic IDs and byte identity across elevation/range/camera changes.

### Forbidden-elevation API mutation RED/GREEN

To prove the public API regression catches field coupling, a temporary forbidden `&[f32]` elevation parameter was added to `PreparedGlobeMesh::build` and only the signature regression was run:

```powershell
cargo test --test spherical_presentation_mesh globe_build_api_accepts_only_source_surface_and_budgets -- --nocapture
```

Mutation result: exit 1 with `E0308`; the test expected a three-parameter function pointer and found a four-parameter function item. After removing the temporary parameter, the final public suite passed 3/3. No mutation remains in the worktree.

## Exact final verification commands and results

```powershell
cargo test --test spherical_presentation_mesh -- --nocapture
# exit 0: 3 passed, 0 failed

cargo test --test spherical_picking -- --nocapture
# exit 0: 8 passed, 0 failed

cargo test --lib spherical_mesh -- --nocapture
# exit 0: 10 passed, 0 failed, 254 filtered out

cargo fmt --all -- --check
# exit 0

cargo test --test spherical_projection -- --nocapture
# exit 0: 8 passed, 0 failed

cargo clippy --lib --test spherical_presentation_mesh --test spherical_picking -- -D warnings
# exit 0, no warnings

git diff --check
# exit 0

cargo test
# exit 0 in 158.8 seconds: 264 library tests (263 passed, 1 ignored),
# followed by all binary, integration, and doc-test targets with no failures
```

## Explicit no-deformation proof

1. `PreparedGlobeMesh::build` has no field, elevation, relief, display range, palette, animation, or camera parameter. The public function-pointer regression locks the exact three-argument type, and the source-bound regression contains the required direct call:

   ```rust
   let globe = PreparedGlobeMesh::build(source.clone(), &surface, budgets).unwrap();
   ```

2. `PreparedGlobeMesh` stores only source identity, cell count, `Vec<GlobeVertex>`, and `Vec<u32>`. `GlobeVertex` stores only `[f32; 3]` plus raw `u32` `CellId`; no scientific/display/camera value has a storage path.
3. The no-deformation test creates two radically different finite elevation arrays and ranges, builds from the same source/surface/budgets on either side, hashes every raw position component, raw cell ID, and index with BLAKE3, and asserts equal hashes, equal vertex slices component-for-component, equal index slices, and equal `SurfaceRef` geometry identity.
4. The same test rotates and zooms a separate `GlobeCamera`, then rechecks the original globe BLAKE3 hash and geometry identity. Camera ownership and mesh ownership are disjoint, so mutations can affect only camera/uniform data.
5. The generated 162-cell test independently checks every stored radius differs from `1.0` by at most `2e-6`, every triangle normal has positive dot product with its outward direction, every index is in range, and the vertex `CellId` set exactly equals the authoritative surface cell set.

## Self-review

- Confirmed globe positions originate only from authoritative `UnitVector3` centroid/boundary values, are converted to finite `f32`, and are radius-validated before insertion.
- Confirmed inward detection uses the final stored `f32` positions. Only a negative-winding triangle swaps its second and third vertices; positive triangles are not rewritten and zero/non-finite winding is rejected.
- Confirmed every count increment and `usize -> u32` index conversion is checked against existing spherical mesh budgets and storage limits.
- Removed a temporary per-triangle allocation during review; triangle preparation is a fixed three-element array.
- Confirmed the center-ray oracle distinguishes inverse from forward orientation after a known 30-degree drag, and visibility has an additional world direction whose front/back result would flip under the wrong convention.
- Confirmed quaternion normalization is scale-safe and sign-canonical, including the deterministic antipodal trackball-axis fallback.
- Confirmed invalid screen/canvas, drag, zoom, and map pan/zoom inputs do not mutate state.
- Confirmed map reset affects only the selected projection, and changing the standalone view mode preserves both per-projection map states and globe state.
- Confirmed `SphericalPresentationSource::new` remains crate-private; generated source-bound tests were not moved to integration tests and no free-form public constructor was added.
- Confirmed focused, adjacent, formatting, strict Clippy, diff, and full-suite gates are clean.

## Concerns

No blocking concerns. `GlobeCamera::is_front_facing` treats the exact limb (`z == 0`) as visible to avoid a precision gap; fill visibility remains governed by outward CCW triangles and later GPU back-face culling. The map camera intentionally validates only finite positive zoom because Task 6 specifies numeric zoom bounds only for the globe.
