# Natural Surface Adapters (S0B.1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a fingerprinted, geometry-neutral, read-only surface contract for natural processes, with planar no-drift and closed-sphere implementations, then make the shared natural topology index consume that contract without migrating scientific stages prematurely.

**Architecture:** `world::spatial` owns stable surface identity and metric query contracts. Lightweight planar and spherical adapters borrow their authoritative snapshots and never copy or serialize geometry. `generators::natural::NaturalTopologyIndex` remains a disposable build-time cache, but its constructor is generalized over the new contract. Existing planar call sites retain a compatibility constructor so current natural outputs and random streams do not change during this slice.

**Tech Stack:** Rust 1.85, serde/serde_json, blake3, thiserror, existing deterministic stage engine and spatial builders; no new dependencies.

## Global Constraints

- Preserve all current planar natural outputs, stage versions, artifact keys, field hashes, screenshots, and random substreams.
- `SphericalSurfaceSnapshot` remains the sole serialized owner of spherical geometry; adapters and topology indexes are borrowed or disposable.
- Do not introduce projection coordinates, latitude/longitude storage, cubed-sphere face IDs, rendering data, or GPU types into the contract.
- Do not change plate kinematics, relief, climate, hydrology, erosion, stage graphs, or the UI in S0B.1.
- A surface identity includes geometry kind, geometry schema, cell count, edge count, and deterministic content fingerprint.
- Existing planar schema V1 serialization stays byte-shape compatible; its fingerprint is computed, not added as a serialized field.
- The generic topology path must reproduce the old planar index exactly, including length normalization and tie behavior.
- Implement each behavior through a witnessed failing test, minimal code, focused verification, and a task-scoped commit.

---

## File Structure

```text
src/world/spatial/
├── mod.rs                         # re-export surface identity and metric contracts
├── snapshot.rs                    # planar semantic fingerprint query, no wire change
├── surface_ref.rs                 # stable SurfaceRef and validation
└── natural_surface.rs             # trait plus planar/spherical borrowed adapters

src/generators/natural/
└── topology.rs                    # contract-driven disposable topology index

tests/
├── surface_ref_contracts.rs       # identity, wire validation, mismatch behavior
└── natural_surface_adapters.rs    # planar/spherical metric conformance
```

---

### Task 1: Add stable surface identity and a planar semantic fingerprint

**Files:**
- Create: `src/world/spatial/surface_ref.rs`
- Modify: `src/world/spatial/snapshot.rs`
- Modify: `src/world/spatial/mod.rs`
- Test: `tests/surface_ref_contracts.rs`

**Interfaces:**
- Produces `SurfaceGeometryKind`, `SurfaceRef`, `SurfaceRefError`.
- Adds `SpatialSnapshot::fingerprint() -> [u8; 32]` without changing serialized fields.
- Adds `SurfaceRef::for_planar(&SpatialSnapshot)` and `SurfaceRef::for_spherical(&SphericalSurfaceSnapshot)`.

- [ ] **Step 1: Write RED identity tests**

Cover:

- deterministic planar fingerprint and `SurfaceRef` construction;
- different planar geometry with equal cell cardinality has a different fingerprint;
- spherical identity reuses the authoritative stored fingerprint;
- geometry kind, schema, cell count, and edge count are serialized explicitly;
- non-contiguous or malformed wire values cannot create a valid `SurfaceRef`;
- JSON round-trip is stable and unknown fields are rejected.

Run:

```powershell
cargo test --test surface_ref_contracts
```

Expected: FAIL because the identity types and planar fingerprint do not exist.

- [ ] **Step 2: Implement the minimal identity contract**

Implement a validated immutable value with private fields and getters. Use manual `Deserialize` or a validating wire type so malformed counts, unsupported geometry schema, and a zero/invalid fingerprint cannot bypass construction. Hash planar semantic fields manually in stable ID order with explicit integer and IEEE-754 byte encoding; include schema, bounds, boundary condition, all cell fields, polygon order, neighbors, and all edge fields.

Do not add a fingerprint field to `SpatialSnapshot` and do not use `Debug` output or map iteration as hash input.

- [ ] **Step 3: Verify and commit**

Run:

```powershell
cargo test --test surface_ref_contracts
cargo test --lib world::spatial
git diff --check
```

Commit:

```powershell
git add src/world/spatial/surface_ref.rs src/world/spatial/snapshot.rs src/world/spatial/mod.rs tests/surface_ref_contracts.rs
git commit -m "feat: add stable natural surface identity"
```

---

### Task 2: Define the minimal read-only natural-surface metric contract

**Files:**
- Create: `src/world/spatial/natural_surface.rs`
- Modify: `src/world/spatial/mod.rs`
- Test: `tests/natural_surface_adapters.rs`

**Interfaces:**
- Produces `NaturalSurface`, `SurfaceCellMetrics`, `SurfaceEdgeMetrics`, and `NaturalSurfaceError`.
- Produces `PlanarNaturalSurface<'a>` and `SphericalNaturalSurface<'a>` borrowed adapters.

The contract exposes only:

- `surface_ref`, `is_closed`, `cell_count`, `edge_count`, and total area;
- dense cell lookup with area and a deterministic unitless three-coordinate shape embedding;
- dense edge lookup with owners, boundary length, graph traversal length, and center distance when defined;
- short characteristic length and maximum useful great-distance scale.

It does not yet expose plate motion or stable edge endpoint connectivity; those enter S0B.2 with their own witnessed consumer.

- [ ] **Step 1: Write RED adapter conformance tests**

Planar fixture assertions:

- counts, areas, owners, boundary status, Euclidean center distance, and normalized first two shape coordinates match the current snapshot;
- the third shape coordinate is constant, so three-dimensional distance ranking is exactly the old two-dimensional ranking;
- the planar adapter is not closed and preserves the old short/long scale definitions.

Spherical fixture assertions:

- every edge has two owners and a positive center distance;
- no cell is exposed as a boundary cell and the adapter reports closed;
- total area agrees with `4πR²` in the authoritative tolerance;
- shape coordinates are finite, bounded, and derived from authoritative centroids;
- maximum useful distance is `πR`.

Run:

```powershell
cargo test --test natural_surface_adapters
```

Expected: FAIL because the contract and adapters do not exist.

- [ ] **Step 2: Implement borrowed adapters**

Construct adapters through validating public constructors and crate-visible constructors for already validated stage inputs. Store only a borrowed snapshot and one computed `SurfaceRef`. Return small copyable metric records; never allocate a neighbors vector or clone geometry per query.

For planar compatibility, graph traversal length remains the current shared-edge length in this slice. For the sphere, use stored center distance as graph traversal length. Record both meanings explicitly to prevent accidental scientific conflation.

- [ ] **Step 3: Verify and commit**

Run:

```powershell
cargo test --test natural_surface_adapters
cargo test --test spherical_surface_contracts
cargo test --test spherical_surface_generation
git diff --check
```

Commit:

```powershell
git add src/world/spatial/natural_surface.rs src/world/spatial/mod.rs tests/natural_surface_adapters.rs
git commit -m "feat: add natural surface metric adapters"
```

---

### Task 3: Generalize the disposable natural topology index with planar no-drift

**Files:**
- Modify: `src/generators/natural/topology.rs`
- Test: `src/generators/natural/topology.rs`
- Test: `tests/natural_surface_adapters.rs`

**Interfaces:**
- Adds `NaturalTopologyIndex::from_surface(&impl NaturalSurface)`.
- Keeps `NaturalTopologyIndex::new(&SpatialSnapshot)` as a planar compatibility entry point during S0B.1.
- Replaces two-coordinate cached centers with three-coordinate `quantized_shape_positions` while preserving the planar first two quantized values and all planar ordering outcomes.

- [ ] **Step 1: Write RED topology tests**

Add tests that:

- compare the generic planar index to fixed pre-refactor arcs, owners, area weights, boundary flags, quantized positions, short scale, and long scale;
- prove edge iteration order does not change the index;
- build a spherical index and verify symmetric arcs, positive costs, paired owners, zero boundary flags, and deterministic farthest-point seeds;
- prove the third planar coordinate contributes zero to squared-distance rankings.

Run:

```powershell
cargo test --lib generators::natural::topology
```

Expected: FAIL because the generic constructor and three-coordinate cache do not exist.

- [ ] **Step 2: Implement the generic index path**

Build arcs from dense edge metric records. Normalize traversal lengths by the adapter-provided maximum scale, normalize cell areas by adapter total area, and quantize adapter-provided shape coordinates with the existing deterministic rounding protocol. Keep owner sorting, arc sorting, queue ordering, tie rotations, and quantization constants unchanged.

Update only the internal crust nucleus distance calculation to sum three squared coordinate deltas. With the planar adapter's constant third coordinate, its integer arithmetic and results remain identical.

- [ ] **Step 3: Prove planar behavior did not drift**

Run focused natural suites before the whole repository:

```powershell
cargo test --test tectonic_generation --test tectonic_boundaries --test relief_generation
cargo test --test geologic_generation --test hydro_erosion_generation
cargo test --test natural_display_golden
```

Expected: all PASS without accepting new planar goldens.

- [ ] **Step 4: Commit**

```powershell
git add src/generators/natural/topology.rs tests/natural_surface_adapters.rs
git commit -m "refactor: drive natural topology through surface metrics"
```

---

### Task 4: Add contract audit gates and S0B.1 evidence

**Files:**
- Modify: `docs/superpowers/plans/2026-08-04-natural-surface-adapters.md`
- Test: repository-wide commands only

- [ ] **Step 1: Run formatting, lint, native, feature, and WASM gates**

Run:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo check --target wasm32-unknown-unknown --all-features
```

- [ ] **Step 2: Run Release performance smoke checks**

Use the existing spherical foundation performance fixtures and a Release planar natural build. Record machine, command, cell count, elapsed time, and peak-memory evidence where the repository harness supports it. S0B.1 must not introduce a measurable repeated per-cell allocation in topology construction.

- [ ] **Step 3: Audit ownership and dependency direction**

Search and record that:

- `natural_surface.rs` imports no generator, UI, renderer, climate-grid, or GPU module;
- neither adapter owns `Vec<SpatialCell>`, `Vec<SphericalSurfaceCell>`, vertices, edges, or adjacency;
- `NaturalTopologyIndex` remains private, un-serialized, and absent from artifact/public snapshot types;
- no natural stage has yet switched to spherical production before its scientific model is migrated.

- [ ] **Step 4: Record evidence and commit**

Append exact gate results and any measured baselines under an `Execution Evidence` section in this plan.

```powershell
git add docs/superpowers/plans/2026-08-04-natural-surface-adapters.md
git commit -m "docs: record natural surface adapter evidence"
```

---

## Follow-on Plans

After S0B.1 is independently reviewed and merged, continue in the approved order:

1. **S0B.2:** versioned surface-bound natural snapshots, spherical Euler-pole tectonics, global crust formation, and mantle hotspots.
2. **S0B.3:** local-edge-motion relief and geology on the sphere.
3. **S0B.4:** preliminary spherical cell-graph climate forcing with tangent winds and conservative nonnegative moisture transport.
4. **S0B.5:** closed-sphere hydrology and erosion with ocean outlets and explicit endorheic basins.
5. **S0B.6:** spherical natural stage graph, cache/provenance integration, compatibility loading, fields, and final S0B acceptance.

S0C presentation begins only after these fields are authoritative enough to display without a second synthetic world.
