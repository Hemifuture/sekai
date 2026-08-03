# Spherical Surface Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a deterministic, closed, production-grade spherical Voronoi surface as the single authoritative source of planetary cells, vertices, edges, adjacency, and metric geometry while preserving the existing planar world path for legacy worlds.

**Architecture:** The stable `world::spatial` contract layer owns validated spherical primitives and immutable `SphericalSurfaceSnapshot` records. The `generators::spatial` layer builds an oriented class-I icosahedral Delaunay mesh and its exact spherical Voronoi dual, then publishes it through a separate artifact/stage path. Cells reference canonical vertices and edges; shared edge ownership is the sole adjacency fact. The experimental cubed-sphere remains a derived climate work grid and is not promoted to world truth.

**Tech Stack:** Rust 1.85, serde/serde_json, thiserror, blake3, existing deterministic stage engine, Cargo integration tests; no new crate dependencies.

## Global Constraints

- Keep `WorldSpec`, `PlanarSpaceSpec`, `SpatialSnapshot`, existing natural stages, current UI, and golden images behavior-compatible during this slice.
- Add a parallel spherical foundation graph; do not silently reinterpret planar saves as spherical worlds.
- `SphericalSurfaceSnapshot` is the only serialized owner of authoritative surface geometry. Rendering projections and the cubed-sphere climate grid remain disposable derived data.
- Store each surface vertex position once. Cells reference vertex and edge IDs; edges are the sole source of cell adjacency.
- Every spherical edge has exactly two distinct owning cells. There is no boundary sentinel, wrap rule, face seam, or pole special case.
- Use unit-sphere `f64` geometry and scale lengths/areas by one validated radius. Dense scientific fields remain outside this snapshot.
- Generate a class-I icosahedral mesh with frequency `f`; its exact dual counts are `cells = 10f² + 2`, `edges = 30f²`, and `vertices = 20f²`.
- Resolve requested cell counts deterministically to the nearest supported frequency, with ties selecting the lower allocation.
- Keep stable IDs contiguous and deterministic. Canonical IDs arise only from deterministic integer lattice keys and sorted edge keys, never from hash-map iteration or floating-point quantization.
- Validate schema, IDs, references, cyclic boundaries, two-owner manifold topology, geometry, Euler characteristic, total area, and stored content fingerprint at artifact boundaries.
- Reuse one implementation of vector arithmetic, central angle, tangent projection, and spherical triangle area across the authoritative surface and experimental cubed-sphere.
- Implement each behavior through a witnessed failing test, minimal implementation, passing focused tests, and a task-scoped commit.
- Do not port tectonics, geology, hydrology, rendering, picking, or climate remapping in this plan. Those are subsequent S0 plans that consume this contract.

---

## File Structure

```text
src/world/spatial/
├── mod.rs                         # planar and spherical contract re-exports
├── sphere_geometry.rs             # validated unit vector and shared spherical math
├── spherical_snapshot.rs          # immutable authoritative vertex/cell/edge snapshot
└── spherical_validation.rs        # closed-manifold and metric validation

src/generators/spatial/
├── mod.rs                         # planar and spherical builder/stage re-exports
├── geodesic_voronoi.rs            # integer-key icosahedral mesh and Voronoi dual
└── spherical_stage.rs             # spherical spec/artifact/stage integration

tests/
├── spherical_primitives.rs        # primitive and authoring-spec contracts
├── spherical_surface_contracts.rs # snapshot rejection and round-trip contracts
├── spherical_surface_generation.rs# deterministic topology and science invariants
└── spherical_foundation_build.rs  # engine artifact/cache/stage behavior
```

---

### Task 1: Add validated spherical authoring and geometry primitives

**Files:**
- Modify: `src/world/ids.rs`
- Modify: `src/world/spec.rs`
- Modify: `src/world/mod.rs`
- Create: `src/world/spatial/sphere_geometry.rs`
- Modify: `src/world/spatial/mod.rs`
- Test: `tests/spherical_primitives.rs`

**Interfaces:**
- Produces `SurfaceVertexId`.
- Produces `UnitVector3`, `SphereGeometryError`, `central_angle`, `project_tangent`, and `spherical_triangle_area_unit`.
- Produces `SphericalSpaceSpec`, `SphericalSpecError`, `MIN_SPHERICAL_CELL_COUNT`, `MAX_SPHERICAL_CELL_COUNT`, and `MAX_GEODESIC_FREQUENCY`.

- [x] **Step 1: Write the primitive RED tests**

```rust
use sekai::world::spatial::UnitVector3;
use sekai::world::{Meters, SphericalSpaceSpec, SurfaceVertexId};

#[test]
fn unit_vectors_are_canonical_and_validated_on_deserialization() {
    let point = UnitVector3::new(3.0, 0.0, 4.0).unwrap();
    assert!((point.norm() - 1.0).abs() <= 1.0e-15);
    assert_eq!(point.components(), [0.6, 0.0, 0.8]);
    assert!(UnitVector3::new(0.0, 0.0, 0.0).is_err());
    assert!(serde_json::from_str::<UnitVector3>(r#"[0.0,0.0,0.0]"#).is_err());
    assert_eq!(SurfaceVertexId::from_raw(7).raw(), 7);
}

#[test]
fn spherical_request_resolves_to_an_exact_geodesic_budget() {
    let spec = SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 20_000,
    };
    spec.validate().unwrap();
    assert_eq!(spec.resolved_frequency(), 45);
    assert_eq!(spec.resolved_cell_count(), 20_252);
}
```

- [x] **Step 2: Run the primitive test and verify RED**

Run: `cargo test --test spherical_primitives -- --nocapture`

Expected: compilation fails because the spherical IDs, vectors, and spec do not exist.

- [x] **Step 3: Implement the typed ID and canonical unit vector**

Add `SurfaceVertexId` through the existing `define_id!` macro. `UnitVector3` stores a private `[f64; 3]`, normalizes finite nonzero inputs in `new`, exposes value-returning accessors, and deserializes through its constructor. Implement shared pure operations on `UnitVector3` rather than exposing mutable component references:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct UnitVector3([f64; 3]);

impl UnitVector3 {
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, SphereGeometryError>;
    pub const fn components(self) -> [f64; 3];
    pub fn dot(self, other: Self) -> f64;
    pub fn norm(self) -> f64;
}

pub fn central_angle(a: UnitVector3, b: UnitVector3) -> f64;
pub fn project_tangent(vector: [f64; 3], radial: UnitVector3) -> [f64; 3];
pub fn spherical_triangle_area_unit(
    a: UnitVector3,
    b: UnitVector3,
    c: UnitVector3,
) -> f64;
```

Keep raw add/subtract/scale/cross helpers `pub(crate)` so both spatial generators can reuse them without making an accidental public math framework.

- [x] **Step 4: Implement deterministic spherical allocation resolution**

`SphericalSpaceSpec::validate` rejects radius outside `1..=100_000_000` meters and requested cells outside `42..=198_812`. `resolved_frequency` compares the exact integer counts immediately below and above the real-valued estimate and selects the smaller absolute count error; ties choose the lower frequency. All count arithmetic uses checked integer operations.

- [x] **Step 5: Run focused tests and commit**

Run:

```powershell
cargo test --test spherical_primitives -- --nocapture
cargo test --test world_primitives --test world_spec -- --nocapture
```

Expected: all tests pass and existing planar contracts remain unchanged.

Commit:

```powershell
git add src/world/ids.rs src/world/spec.rs src/world/mod.rs src/world/spatial/mod.rs src/world/spatial/sphere_geometry.rs tests/spherical_primitives.rs
git commit -m "feat: define spherical space primitives"
```

---

### Task 2: Define the single-source spherical surface snapshot

**Files:**
- Create: `src/world/spatial/spherical_snapshot.rs`
- Create: `src/world/spatial/spherical_validation.rs`
- Modify: `src/world/spatial/mod.rs`
- Test: `tests/spherical_surface_contracts.rs`

**Interfaces:**
- Produces `SPHERICAL_SURFACE_SCHEMA_V1`.
- Produces `SphericalSurfaceVertex`, `SphericalSurfaceCell`, `SphericalSurfaceEdge`, and `SphericalSurfaceSnapshot`.
- Produces `SphericalSurfaceValidationError` with stable variants for schema, IDs, references, manifold, metric, area, orientation, and fingerprint failures.

- [x] **Step 1: Write snapshot construction and SSOT RED tests**

Create a small test-only tetrahedral surface fixture and assert these public shapes:

```rust
pub struct SphericalSurfaceVertex {
    pub id: SurfaceVertexId,
    pub position: UnitVector3,
}

pub struct SphericalSurfaceCell {
    pub id: CellId,
    pub site: UnitVector3,
    pub centroid: UnitVector3,
    pub area: SquareMeters,
    pub boundary_vertices: Vec<SurfaceVertexId>,
    pub boundary_edges: Vec<EdgeId>,
}

pub struct SphericalSurfaceEdge {
    pub id: EdgeId,
    pub vertices: [SurfaceVertexId; 2],
    pub cells: [CellId; 2],
    pub midpoint: UnitVector3,
    pub length: Meters,
    pub center_distance: Meters,
    pub center_distances_to_midpoint: [Meters; 2],
    pub normal_from_first: UnitVector3,
}
```

Assert that `cell()`, `edge()`, `vertex()`, and `opposite_cell()` use contiguous IDs; `cell_edges()` returns the stored cyclic edge IDs; there is no serialized `neighbors` array; and `total_cell_area()` is available.

- [x] **Step 2: Run the contract test and verify RED**

Run: `cargo test --test spherical_surface_contracts -- --nocapture`

Expected: compilation fails because the snapshot records do not exist.

- [x] **Step 3: Implement immutable records, accessors, and fingerprint ownership**

`SphericalSurfaceSnapshot` keeps its record vectors and fingerprint private:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SphericalSurfaceSnapshot {
    schema_version: u16,
    radius: Meters,
    vertices: Vec<SphericalSurfaceVertex>,
    cells: Vec<SphericalSurfaceCell>,
    edges: Vec<SphericalSurfaceEdge>,
    fingerprint: [u8; 32],
}
```

`new` sorts records by ID, computes the fingerprint from canonical little-endian semantic bytes, validates, and returns the snapshot. The fingerprint hashes schema, radius, every vertex position, every cell field and cyclic ID list, and every edge field exactly once. It never includes serialization bytes, rendering data, projection coordinates, cache state, or stage timing.

- [x] **Step 4: Implement strict closed-manifold validation**

Validation must check, in deterministic order:

1. supported schema and finite positive radius;
2. contiguous IDs and unit vectors;
3. at least three boundary vertices per cell and equal vertex/edge list lengths;
4. valid, unique cell-local vertex and edge references;
5. two distinct sorted owners and distinct endpoints per edge;
6. every cyclic cell side maps to its referenced canonical edge;
7. every edge occurs exactly once in each of its two owners and nowhere else;
8. stored midpoint, arc length, center distance, midpoint distances, and tangent normal match recomputation;
9. stored cell area and centroid match spherical polygon recomputation;
10. every polygon is outward counter-clockwise around its site;
11. `V - E + F == 2` and total area matches `4πr²` within scale-aware tolerances;
12. stored fingerprint matches canonical recomputation.

Adjacency is always derived with `opposite_cell(cell_id, edge_id)`. Do not add a second neighbors vector to the cell or snapshot.

- [x] **Step 5: Test malformed deserialized snapshots**

Use `serde_json::Value` mutations to prove that validation rejects an unsupported schema, non-contiguous ID, one-owner/duplicate-owner edge, invalid vertex reference, edge missing from one owner, incorrect area, incorrect tangent normal, broken Euler topology, and altered fingerprint. Deserialize first, then call `validate`, matching the existing artifact-boundary convention.

- [x] **Step 6: Run focused tests and commit**

Run: `cargo test --test spherical_surface_contracts -- --nocapture`

Expected: all snapshot contract and rejection tests pass.

Commit:

```powershell
git add src/world/spatial/mod.rs src/world/spatial/spherical_snapshot.rs src/world/spatial/spherical_validation.rs tests/spherical_surface_contracts.rs
git commit -m "feat: define authoritative spherical surface snapshot"
```

---

### Task 3: Generate a deterministic integer-key icosahedral Delaunay mesh

**Files:**
- Create: `src/generators/spatial/geodesic_voronoi.rs`
- Modify: `src/generators/spatial/mod.rs` (private module declaration only)
- Test: unit tests in `src/generators/spatial/geodesic_voronoi.rs`

**Interfaces:**
- Produces private `GeodesicMesh`, `SiteKey`, oriented triangle, and incidence records for Task 4.
- Does not publish an incomplete builder or snapshot from `generators::spatial`.

- [x] **Step 1: Write exact-count and deterministic-ID RED tests**

```rust
#[test]
fn geodesic_frequencies_have_exact_euler_counts() {
    for (frequency, expected_sites, expected_edges, expected_triangles) in [
        (2, 42, 120, 80),
        (3, 92, 270, 180),
        (4, 162, 480, 320),
    ] {
        let mesh = GeodesicMesh::build(frequency).unwrap();
        assert_eq!(mesh.sites.len(), expected_sites);
        assert_eq!(mesh.edge_incidence.len(), expected_edges);
        assert_eq!(mesh.triangles.len(), expected_triangles);
    }
}
```

Also assert that two builds have byte-identical ordered sites/triangles/incidence and all site IDs equal their vector index.

- [x] **Step 2: Run the generation test and verify RED**

Run: `cargo test --lib geodesic_voronoi::tests::geodesic_frequencies -- --nocapture`

Expected: compilation fails because the builder does not exist.

- [x] **Step 3: Implement canonical icosahedron data and integer lattice keys**

Define 12 normalized base vertices and 20 face triples. At startup, orient each face outward by testing `dot(cross(b-a, c-a), a+b+c)`. Subdivide each face using barycentric integer triples whose sum equals frequency.

Use this non-floating canonical key scheme:

```rust
enum SiteKey {
    BaseVertex(u8),
    BaseEdge { first: u8, second: u8, weight_on_second: u16 },
    FaceInterior { face: u8, weights: [u16; 3] },
}
```

Shared base vertices and edge points therefore weld by exact integer identity. Assign `CellId` in sorted `SiteKey` order. Do not weld by epsilon, quantized floats, randomized insertion order, or a platform-dependent hash map.

- [x] **Step 4: Build oriented triangular faces and incidence maps**

Emit exactly `20f²` triangles, orient each outward, reject repeated vertices/zero area, and collect canonical Delaunay edge keys in a `BTreeMap<[CellId; 2], [triangle; 2]>`. Every Delaunay edge must have exactly two incident triangles before dual construction proceeds.

- [x] **Step 5: Run exact-count tests and commit the mesh core**

Run: `cargo test --lib geodesic_voronoi::tests::geodesic_frequencies -- --nocapture`

Expected: exact private-mesh counts, deterministic IDs, and ordered-record equality pass. No incomplete public snapshot exists yet.

Commit:

```powershell
git add src/generators/spatial/geodesic_voronoi.rs src/generators/spatial/mod.rs
git commit -m "feat: build deterministic geodesic mesh"
```

---

### Task 4: Construct and validate the spherical Voronoi dual

**Files:**
- Modify: `src/generators/spatial/geodesic_voronoi.rs`
- Modify: `src/generators/spatial/mod.rs`
- Create: `tests/spherical_surface_generation.rs`

**Interfaces:**
- Consumes the private oriented Delaunay mesh from Task 3.
- Produces public `GeodesicVoronoiBuilder`, `SphericalSurfaceBuildError`, and one fully validated `SphericalSurfaceSnapshot`.

- [x] **Step 1: Write closed-surface science RED tests**

Test frequencies `2`, `3`, `8`, and the resolved production preview near 20,000 cells. Assert:

- every edge has two distinct owners;
- every cell has five or six sides, with exactly 12 pentagons and all others hexagons;
- every surface vertex is incident to three cells;
- `V - E + F == 2`;
- summed area closes to `4πr²` within `1e-10` relative error;
- each edge tangent normal is perpendicular to its midpoint and points from `cells[0]` toward `cells[1]`;
- every cell boundary is outward counter-clockwise and contains its site;
- all arc lengths, center distances, and areas are finite and positive;
- no field contains a cubed-sphere face, row, column, projection coordinate, or boundary marker.

- [x] **Step 2: Run the science tests and verify RED**

Run: `cargo test --test spherical_surface_generation closed_surface -- --nocapture`

Expected: tests fail because the dual geometry is incomplete or does not yet validate.

- [x] **Step 3: Create one Voronoi vertex per Delaunay triangle**

Compute the unit normal of the plane through each triangle's three sites, select the hemisphere whose dot product with the triangle site sum is positive, and store it as the triangle's spherical circumcenter. Triangle ID becomes `SurfaceVertexId`; no duplicate coordinate table is stored in cells.

- [x] **Step 4: Order each cell boundary in its tangent plane**

For each Delaunay site, collect incident triangle IDs. Construct a deterministic tangent basis by selecting the Cartesian axis least aligned with the site, then sort circumcenters with `f64::total_cmp(atan2(y, x))`, using triangle ID as the tie-breaker. Reverse only when the signed outward orientation check is negative.

- [x] **Step 5: Create one canonical Voronoi edge per Delaunay edge**

The two incident triangle IDs are the Voronoi endpoints and the two Delaunay site IDs are its owners. Sort owner IDs, calculate midpoint and arc length, calculate owner-center distances, then calculate one unit tangent-plane normal oriented from `cells[0]` toward `cells[1]`. Assign `EdgeId` in sorted Delaunay-edge order and populate each cell's cyclic boundary edge IDs from consecutive circumcenters.

- [x] **Step 6: Calculate spherical cell metrics**

Triangulate each convex Voronoi polygon from its generating site. Sum robust unit-sphere triangle excesses with compensated summation, scale by `radius²`, and compute a normalized area-weighted centroid direction from the same triangles. The snapshot validator must call the same pure geometry functions; do not create a second formula in the generator.

- [x] **Step 7: Run generation and regression tests**

Run:

```powershell
cargo test --test spherical_surface_generation -- --nocapture
cargo test --test circulation_grid -- --nocapture
```

Expected: all spherical surface and pre-existing cubed-sphere grid tests pass.

- [x] **Step 8: Commit the closed Voronoi surface**

```powershell
git add src/generators/spatial/geodesic_voronoi.rs tests/spherical_surface_generation.rs
git commit -m "feat: construct closed spherical Voronoi surface"
```

---

### Task 5: Publish the spherical surface through the generation engine

**Files:**
- Create: `src/generators/spatial/spherical_stage.rs`
- Modify: `src/generators/spatial/mod.rs`
- Test: `tests/spherical_foundation_build.rs`

**Interfaces:**
- Produces `SphericalSpaceArtifact` with key `spatial.spherical-spec`.
- Produces `SphericalSurfaceArtifact` with key `world.spherical-surface`.
- Produces `SphericalSurfaceStage` with ID `spatial.spherical-voronoi`, namespace `sekai.core`, version `1`.
- Produces `spherical_foundation_graph()` without changing existing `foundation_graph()`.

- [x] **Step 1: Write stage/artifact RED tests**

Mirror the existing planar `foundation_build` coverage. Assert exact artifact keys and dependency metadata, successful validation, serde round-trip, stable content hash, cache hit on an identical rebuild, and an external invalid-spec error before stage execution.

Also prove that root-seed changes do not alter the surface artifact's semantic bytes: the mesh is a discretization chosen by the spherical spec, not hidden geological randomness. Document that the generic engine may still miss cache because its cache key conservatively includes the root seed.

- [x] **Step 2: Run the engine test and verify RED**

Run: `cargo test --test spherical_foundation_build -- --nocapture`

Expected: compilation fails because the artifacts and stage do not exist.

- [x] **Step 3: Implement typed artifacts and stage errors**

Use stable diagnostic codes:

- `spherical-spatial.invalid-spec`
- `spherical-spatial.build-failed`
- `spherical-spatial.invalid-snapshot`

The stage validates input, calls `GeodesicVoronoiBuilder`, validates the result again, and atomically returns a `SphericalSurfaceArtifact`. It consumes no random draws and emits one informational diagnostic when the resolved cell count differs from the author-requested target.

- [x] **Step 4: Add the separate foundation graph**

```rust
pub fn spherical_foundation_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<SphericalSpaceArtifact>()
        .stage(SphericalSurfaceStage)
        .build()
}
```

Do not add both planar and spherical outputs under the same artifact key and do not alter the current app graph in this task.

- [x] **Step 5: Run focused tests and commit**

Run:

```powershell
cargo test --test spherical_foundation_build -- --nocapture
cargo test --test foundation_build -- --nocapture
```

Expected: both parallel foundation paths pass.

Commit:

```powershell
git add src/generators/spatial/mod.rs src/generators/spatial/spherical_stage.rs tests/spherical_foundation_build.rs
git commit -m "feat: publish spherical surface stage"
```

---

### Task 6: Consolidate shared spherical math without changing circulation results

**Files:**
- Modify: `src/generators/natural/circulation/math.rs`
- Modify: `src/generators/natural/circulation/grid.rs`
- Modify: `src/generators/natural/circulation/operators.rs` if imports require it
- Modify: `src/generators/natural/circulation/dynamics.rs` if imports require it
- Test: existing `tests/circulation_grid.rs`
- Test: existing `tests/circulation_operators.rs`
- Test: existing `tests/circulation_comparison.rs`

**Interfaces:**
- Consumes `world::spatial` sphere geometry primitives.
- Preserves all current public `CubedSphereGrid`, solver, fixture, and snapshot interfaces and numerical outputs.

- [x] **Step 1: Add a no-drift regression witness**

Record current deterministic grid fingerprints for small resolutions already covered by the circulation tests, and retain existing operator/conservation assertions. This is a refactor witness, not a new scientific tolerance.

- [x] **Step 2: Run the witness before refactoring**

Run:

```powershell
cargo test --test circulation_grid --test circulation_operators --test circulation_comparison -- --nocapture
```

Expected: tests pass and establish the pre-refactor baseline.

- [x] **Step 3: Replace duplicate sphere formulas with shared functions**

Make circulation `math.rs` either import and narrowly adapt the shared functions or delete functions that have direct shared equivalents. Preserve raw `[f64; 3]` solver storage where changing it would add conversions in hot loops. There must be one formula each for normalization, central angle, tangent projection, and spherical triangle area.

- [x] **Step 4: Run the no-drift suite and commit**

Run:

```powershell
cargo test --test circulation_grid --test circulation_operators --test circulation_thermodynamics --test circulation_steady --test circulation_transient --test circulation_comparison -- --nocapture
```

Expected: all fingerprints and numerical assertions remain unchanged.

Commit:

```powershell
git add src/generators/natural/circulation tests/circulation_grid.rs
git commit -m "refactor: share spherical geometry math"
```

---

### Task 7: Verify scale, serialization stability, and repository compatibility

**Files:**
- Modify: `tests/spherical_surface_generation.rs`
- Modify: `tests/spherical_foundation_build.rs`
- Modify: `docs/superpowers/plans/2026-08-03-spherical-surface-foundation.md`

- [x] **Step 1: Add boundary-budget and production-scale tests**

Test minimum and maximum supported requested counts without performing oversized invalid allocations. Add an ignored Release measurement that builds the resolved `20_252`-cell surface, prints elapsed time plus serialized byte size, validates it, and reports per-cell bytes. Do not assert wall-clock timing in CI.

- [x] **Step 2: Verify deterministic serialization across repeated builds**

At frequencies `2`, `8`, and `45`, build twice, compare fingerprints and JSON bytes, deserialize, validate, and compare reserialized bytes. Include radii `1`, `6_371_000`, and `100_000_000` meters to exercise scale-aware tolerances.

- [x] **Step 3: Run formatting, lint, focused, and full native tests**

Run:

```powershell
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --test spherical_primitives --test spherical_surface_contracts --test spherical_surface_generation --test spherical_foundation_build -- --nocapture
cargo test --all-targets --all-features
```

Expected: every command exits `0`.

- [x] **Step 4: Verify the WebAssembly compilation boundary**

Run: `cargo check --target wasm32-unknown-unknown --lib`

Expected: exits `0`, or, if the target is not installed, record that exact environmental limitation and do not claim the check passed.

- [x] **Step 5: Run and record the Release measurement**

Run:

```powershell
cargo test --release --test spherical_surface_generation production_scale_measurement -- --ignored --nocapture
```

Record the observed machine, resolved count, elapsed time, JSON size, and any scientific residuals in this plan's execution notes. Timing is evidence only and never enters snapshot data or fingerprints.

- [x] **Step 6: Mark checklist state and commit verification evidence**

Update only completed checkboxes and append an `Execution Evidence` section with exact command outcomes. Then commit:

```powershell
git add docs/superpowers/plans/2026-08-03-spherical-surface-foundation.md tests/spherical_surface_generation.rs tests/spherical_foundation_build.rs
git commit -m "test: verify spherical surface foundation"
```

## Execution Evidence

Executed on 2026-08-04 in the linked worktree `.worktrees/spherical-circulation`, starting from `dcf5fecf79cb8af1b4ddc2c50f07d90788457ede`.

- Task 1 through Task 6 checklist completion is supported by the task reports, the SDD ledger, and the reviewed commit chain `586ad8d` through `dcf5fec`; the final native suite below revalidated their current repository compatibility.
- Boundary-contract mutation witness: after temporarily removing the builder's leading `space.validate()?`, `cargo test --test spherical_surface_generation builder_rejects_cell_counts_immediately_outside_the_allocation_budget -- --nocapture` exited `1` because target `41` incorrectly produced an `Ok` snapshot. Restoring the validation produced exit `0` (`1 passed`). The same GREEN run covered `198_813` without allocating the maximum supported mesh.
- Serialization-contract mutation witness: after temporarily omitting the authoritative fingerprint from JSON, `cargo test --test spherical_surface_generation canonical_serialization_is_stable_across_frequency_and_radius_budgets -- --nocapture` exited `1` at the real deserialization boundary with `missing field fingerprint`. Restoring the schema produced exit `0` (`1 passed`) across all nine frequency/radius cases.
- `cargo fmt -- --check`: exit `0`.
- `cargo clippy --all-targets --all-features -- -D warnings`: exit `0`.
- `cargo test --test spherical_primitives --test spherical_surface_contracts --test spherical_surface_generation --test spherical_foundation_build -- --nocapture`: exit `0`; suites reported `7`, `22`, `6`, and `6` passed respectively, with only the explicit production measurement ignored.
- `cargo test --all-targets --all-features`: exit `0`; every executed suite reported zero failures, and ignored measurement/evidence gates remained ignored.
- `cargo check --target wasm32-unknown-unknown --lib`: exit `0`; the installed `wasm32-unknown-unknown` target compiled the library boundary successfully.
- `cargo test --release --test spherical_surface_generation production_scale_measurement -- --ignored --nocapture`: exit `0`; validation returned `ok`.
- Release observation: resolved count `20_252`; build elapsed `136.1077 ms`; canonical JSON `30_188_052` bytes; `1490.620778` bytes per cell; relative total-area residual `0e0`.
- Measurement machine: Microsoft Windows 11 Pro `10.0.22631`, x86-64, Intel(R) Core(TM) i9-14900KF; Release profile; `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1 (c980f4866 2026-06-30)`.
- `git diff --check`: exit `0` before documentation capture and rerun as the final pre-commit whitespace gate.

---

## Follow-on S0 Plans

This foundation deliberately stops at a validated authoritative surface. Continue S0 in independently reviewable plans, in this dependency order:

1. **S0B — topology/metric adapters and natural-process migration:** move tectonics, geology, relief, hydrology, and erosion from planar coordinates to the authoritative cell/edge metric interface while preserving each stage's scientific ownership.
2. **S0C — spherical presentation:** add a 3D globe, explicit 2D projections, seam-only display splitting, and unified picking that always returns the authoritative `CellId`.
3. **S0D — conservative climate remapping:** construct sparse overlap weights between `SphericalSurfaceSnapshot` and the derived `CubedSphereGrid`, with constant-field, global-integral, vector-tangent, and coastline-mask conservation tests.
4. **C0 onward — layered climate:** begin the approved time-evolving atmosphere/ocean route only after S0B–S0D establish real terrain forcing and a visible WYSIWYG spherical world.
