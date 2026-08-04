# Spherical Tectonics and Mantle (S0B.2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generate scientifically coherent present-day plate partitioning, rigid Euler-pole motion, continental/oceanic crust, boundary kinematics, mantle hotspots, heat flow, and volcanic influence directly on the authoritative closed spherical surface, without changing planar V1 output or publishing a half-migrated product graph.

**Architecture:** Keep planar `TectonicSnapshot`, `MantleSnapshot`, `PlateVelocity`, and their wire schemas untouched. Add surface-bound spherical V2 snapshots whose IDs are meaningful only under an exact `SurfaceRef`. Reuse the disposable `NaturalTopologyIndex`, labeled random streams, dense semantic field types, graph partitioning, crust-growth core, and hotspot diffusion core. Put sphere-only Euler motion and canonical-vertex boundary connectivity behind explicit spherical types; do not add fake global 2D directions or projection coordinates. Pure spherical generators are added now; artifact/stage/UI publication remains atomic and deferred to S0B.6.

**Tech Stack:** Rust 1.85, serde/serde_json, blake3, thiserror, existing deterministic stage RNG, geodesic Voronoi surface, and spatial vector primitives; no new dependencies.

## Global Constraints

- `SphericalSurfaceSnapshot` remains the only owner of spherical cells, edges, vertices, incidence, and metric geometry.
- Every spherical natural snapshot stores and validates the exact `SurfaceRef`; equal cardinality is never sufficient.
- Preserve planar V1 JSON shape, hashes, random streams, stage versions, artifacts, field output, and reviewed images exactly.
- A spherical plate stores one Euler pole and one fixed-point angular rate. It never stores a per-cell velocity field as a second fact source.
- Local linear velocity is derived as `v(r) = omega * R * (pole × r)` and must be tangent at every queried point.
- Maximum local plate speed is 120 mm/year. Fixed-point angular-rate candidates are radius-aware and deterministic.
- Spherical boundary classification uses relative velocity at the authoritative edge midpoint, projected onto the stored cross-edge tangent normal and its along-edge tangent.
- Spherical segments connect through canonical `SurfaceVertexId`; they do not store a fake aggregate map direction.
- Closed-sphere continental and hotspot placement must not use outer-boundary distance, rectangular width/height, or a diagonal.
- Hotspot support is geodesic, monotonically non-increasing, and no larger than `pi * R`.
- No S0B.2 code may depend on UI, renderer, GPU, projection, cubed-sphere face, or circulation modules.
- Implement each behavior through a witnessed failing test, minimal code, focused verification, and a task-scoped commit.

---

## File Structure

```text
src/world/spatial/
└── natural_surface.rs                 # borrowed spherical cell/edge local-frame queries

src/world/natural/
├── mod.rs                             # V2 contract exports
├── spherical_tectonics.rs             # Euler motion and surface-bound tectonic V2 snapshot
└── spherical_mantle.rs                # surface-bound mantle V2 snapshot

src/generators/natural/
├── mod.rs                             # spherical generation-error exports
├── tectonics.rs                       # shared partition/crust core plus spherical motion/boundaries
└── mantle.rs                          # shared hotspot/field core plus spherical wrapper

tests/
├── spherical_tectonic_contracts.rs    # motion, identity, serde, and validation contracts
├── spherical_tectonic_generation.rs   # deterministic scientific generation properties
├── spherical_mantle_contracts.rs      # V2 identity and radius contracts
└── spherical_mantle_generation.rs     # global hotspot and heat-flow properties
```

---

### Task 1: Expose borrowed spherical local frames without duplicating geometry

**Files:**
- Modify: `src/world/spatial/natural_surface.rs`
- Modify: `src/world/spatial/mod.rs`
- Test: `tests/natural_surface_adapters.rs`

**Interfaces:**
- Add copyable `SphericalSurfaceCellFrame` containing `CellId` and authoritative radial `UnitVector3`.
- Add copyable `SphericalSurfaceEdgeFrame` containing `EdgeId`, canonical `[SurfaceVertexId; 2]`, ordered `[CellId; 2]`, midpoint radial, and `normal_from_first`.
- Add inherent `SphericalNaturalSurface::cell_frame` and `edge_frame` queries. Keep the geometry-neutral `NaturalSurface` trait unchanged.

- [x] **Step 1: Write RED adapter-frame tests**

Verify exact equality with the borrowed spherical snapshot, canonical endpoint IDs, owner ordering, unit/tangent frame properties, out-of-range lookups, and no new serialized or owned geometry.

Run:

```powershell
cargo test --test natural_surface_adapters spherical_local_frames
```

Expected: FAIL because the frame types and queries do not exist.

- [x] **Step 2: Implement thin frame queries**

Return values copied from one validated borrowed edge/cell record. Do not allocate, reconstruct adjacency, or derive a projection.

- [x] **Step 3: Verify and commit**

```powershell
cargo test --test natural_surface_adapters
cargo test --lib world::spatial
git diff --check
git add src/world/spatial tests/natural_surface_adapters.rs
git commit -m "feat: expose spherical natural local frames"
```

---

### Task 2: Define surface-bound spherical tectonic contracts

**Files:**
- Create: `src/world/natural/spherical_tectonics.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `tests/spherical_tectonic_contracts.rs`

**Interfaces:**
- Add `TECTONIC_SNAPSHOT_SCHEMA_V2 = 2`.
- Add `SphericalPlateRotation { pole: UnitVector3, angular_rate_prad_per_year: u64 }` with validating construction, strict deserialization, angular-vector query, maximum-speed query, and local `velocity_mm_per_year(radius, radial)`.
- Add `SphericalPlate { id, seed_cell, rotation }`.
- Add `SphericalBoundarySegment` with stable IDs, plates, kind, sorted member edges, mean strength, and optional subducting plate; deliberately no aggregate 2D direction.
- Add `SphericalTectonicSnapshot` with `SurfaceRef`, plates, existing dense `PlateIdField`/`CrustKindField`, thickness, existing edge-aligned `BoundaryRecord`, and spherical segments.

- [x] **Step 1: Write RED Euler-motion tests**

Cover fixed-point bounds, strict serde, pole speed zero, equatorial speed, tangent dot product, common angular vector for all points, and 120 mm/year validation against radii from 1 m through 100,000 km.

- [x] **Step 2: Write RED snapshot-contract tests**

Cover schema, exact spherical `SurfaceRef`, dense lengths, plate/seed ownership, field ranges, segment partition, boundary/subduction consistency, same-cardinality different-surface rejection, unknown fields, and JSON round-trip. Assert the V1 tectonic fixture retains its exact wire/hash.

Run:

```powershell
cargo test --test spherical_tectonic_contracts
```

Expected: FAIL because V2 contracts do not exist.

- [x] **Step 3: Implement immutable contracts and validation**

Reuse existing semantic field and boundary types. Keep all V2 fields private and route deserialization through validation. Self-contained validation checks allocation and semantic consistency; `validate_against(&SphericalSurfaceSnapshot)` additionally checks the exact surface identity, radius-dependent speed, seed ownership, connected plate regions, every cross-plate edge, and canonical-vertex segment connectivity.

- [x] **Step 4: Verify and commit**

```powershell
cargo test --test spherical_tectonic_contracts
cargo test --test tectonic_contracts --test tectonic_generation --test tectonic_boundaries
git diff --check
git add src/world/natural tests/spherical_tectonic_contracts.rs
git commit -m "feat: add spherical tectonic snapshot contracts"
```

---

### Task 3: Share partition and crust generation across open and closed surfaces

**Files:**
- Modify: `src/generators/natural/tectonics.rs`
- Test: `tests/tectonic_generation.rs`
- Test: `tests/spherical_tectonic_generation.rs`

**Interfaces:**
- Extract the existing plate-seed/ownership core over `NaturalTopologyIndex` without changing planar call order.
- Parameterize crust placement with a narrow domain policy: `PlanarOceanFrame` retains the exact old boundary exclusion; `ClosedSurface` uses all cells, global farthest nuclei, graph distance, true area weights, and seam-free smoothed graph noise.
- Keep crust random substreams independent of plate count and motion.

- [x] **Step 1: Write RED closed-surface crust tests**

For every formation preset, verify deterministic output, requested area within one maximum cell area, both crust kinds present, connected preset morphology envelopes, no privileged seam/pole band, and independence from plate count. Retain the existing planar byte/golden checks unchanged.

- [x] **Step 2: Refactor the minimal shared core**

Preserve the planar branch byte-for-byte in ordering and arithmetic. The closed branch must not manufacture boundary sources or reuse an all-`u64::MAX` boundary-distance sentinel as physical data.

- [x] **Step 3: Verify and commit**

```powershell
cargo test --test tectonic_generation --test tectonic_boundaries
cargo test --test spherical_tectonic_generation closed_surface_crust
cargo test --test natural_display_golden
git diff --check
git add src/generators/natural/tectonics.rs tests
git commit -m "refactor: share closed surface crust generation"
```

---

### Task 4: Generate Euler rotations and local spherical boundaries

**Files:**
- Modify: `src/generators/natural/tectonics.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_tectonic_generation.rs`

**Interfaces:**
- Add `TectonicGenerator::generate_spherical` returning `SphericalTectonicSnapshot`.
- Add a distinct `SphericalTectonicGenerationError`; do not alter planar stage-error mapping.
- Use a fixed literal spherical pole set and radius-derived fixed-point rate ladder per `TectonicActivity`.

- [x] **Step 1: Write RED rotation-assignment tests**

Verify exact plate count, non-empty connected ownership, repeatability, seed sensitivity, maximum local speed, one shared Euler vector per plate, and a minimum generated relative speed on every adjacent plate interface.

- [x] **Step 2: Implement deterministic candidate assignment**

Score each candidate only over interfaces to already assigned neighboring plates, using quantized local relative-speed energy at authoritative edge midpoints. Keep complexity proportional to candidate count times plate-boundary edges, not all cells times all plates. Use stable candidate rotation/tie order.

- [x] **Step 3: Write RED local-boundary tests**

Verify the sign of relative normal motion matches convergent/divergent classification; weak/transform decisions use the local normal/tangent ratio; subduction polarity respects crust kind/thickness; every cross-plate edge has one record; same-kind segments join only through shared canonical vertices; pole and antimeridian neighborhoods stay finite and ordinary.

- [x] **Step 4: Implement shared local classification and spherical aggregation**

Extract a geometry-independent classification kernel accepting normal and tangential relative speeds plus crust facts. Adapt the planar wrapper without changing its integer projections or results. Aggregate spherical edges with `SurfaceVertexId`, and never compute or serialize a global segment direction.

- [x] **Step 5: Verify and commit**

```powershell
cargo test --test spherical_tectonic_generation
cargo test --test tectonic_generation --test tectonic_boundaries --test natural_display_golden
cargo clippy --lib --test spherical_tectonic_generation -- -D warnings
git diff --check
git add src/generators/natural tests/spherical_tectonic_generation.rs
git commit -m "feat: generate spherical Euler plate tectonics"
```

---

### Task 5: Define and generate surface-bound spherical mantle fields

**Files:**
- Create: `src/world/natural/spherical_mantle.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/generators/natural/mantle.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/spherical_mantle_contracts.rs`
- Test: `tests/spherical_mantle_generation.rs`

**Interfaces:**
- Add `MANTLE_SNAPSHOT_SCHEMA_V2 = 2` and `SphericalMantleSnapshot { SurfaceRef, hotspots, heat_flow_mw_m2, volcanic_influence }`.
- Add `MantleGenerator::generate_spherical` and distinct `SphericalMantleGenerationError`.
- Reuse `Hotspot` and one shared field-generation core.

- [ ] **Step 1: Write RED V2 mantle contract tests**

Cover exact surface identity, strict serde, field ranges and lengths, unique hotspot sources, support no larger than `pi * R`, and rejection of a different surface with equal counts.

- [ ] **Step 2: Write RED spherical generation tests**

Verify deterministic global farthest-point sources, seed sensitivity, no boundary/pole special case, source influence exactly one, monotonic compact support by graph distance, zero outside support, ordered activity background, independent strength substream prefix, and unchanged planar mantle golden hash.

- [ ] **Step 3: Implement shared hotspot generation**

Retain planar edge-margin selection exactly. On a closed surface use global candidates and a support scale derived from `pi * R`; diffuse with the already built spherical topology and use the same compact smoothstep and bounded heat formula.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test --test spherical_mantle_contracts --test spherical_mantle_generation
cargo test --test mantle_contracts --test mantle_generation --test mantle_stage
cargo test --test natural_display_golden
git diff --check
git add src/world/natural src/generators/natural tests
git commit -m "feat: generate spherical mantle forcing"
```

---

### Task 6: Whole-slice scientific, compatibility, ownership, and performance gates

**Files:**
- Modify: `docs/superpowers/plans/2026-08-04-spherical-tectonics-mantle.md`
- Test: repository-wide commands and ignored Release measurements

- [ ] **Step 1: Run deterministic and scientific matrix**

Run multiple radii, surface frequencies, seeds, plate counts, activity levels, formation presets, and mantle biases. Record tangency, speed, interface classification, continental area, hotspot support, exact identity, and deterministic hashes.

- [ ] **Step 2: Run all compatibility gates**

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo check --target wasm32-unknown-unknown --all-features
```

- [ ] **Step 3: Run Release performance smoke**

At approximately 20,000 cells, record spherical tectonic time, mantle time, counts, dense persistent bytes, and available process-memory evidence. S0B.2 combined generation should remain comfortably inside the final S0B `2.5x`/`256 MiB` envelope and perform no repeated O(V+E+C) surface validation inside loops.

- [ ] **Step 4: Audit ownership and publication boundary**

Confirm V2 snapshots own only semantic plates/fields/events and a `SurfaceRef`; they contain no copied surface vertices, edges, cells, adjacency, projections, or render buffers. Confirm no application graph or UI path consumes these snapshots before S0B.6.

- [ ] **Step 5: Independent review, evidence, and commit**

Request read-only review for scientific signs/units, fixed-point bounds, identity trust, planar no-drift, allocation behavior, and serialization. Fix all critical/important findings, rerun gates, append exact evidence here, then commit:

```powershell
git add docs/superpowers/plans/2026-08-04-spherical-tectonics-mantle.md
git commit -m "docs: record spherical tectonics and mantle evidence"
```

---

## Explicitly Deferred

- S0B.3 consumes these V2 fields to generate spherical relief and geology.
- S0B.6 owns artifact keys, stage graph registration, cache/provenance publication, field adapters, and UI exposure.
- S0C owns spherical presentation and selection.
- No time-history archive, plate-age chronology, final circulation, ENSO, or cyclone model is introduced here.
