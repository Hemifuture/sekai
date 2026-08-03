# Spherical Circulation Solver Comparison Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build two scientifically comparable closed-sphere atmosphere–ocean circulation solvers, measure their Release performance and field agreement, and produce the evidence needed to choose one WYSIWYG production solver.

**Architecture:** A validated `world::natural::circulation` contract layer owns specifications, forcing, and snapshots. A separate `generators::natural::circulation` package owns cubed-sphere geometry, one shared finite-volume operator set, one shared thermodynamic kernel, two solver strategies, deterministic fixtures, and read-only comparison/reporting. Both solvers consume the same immutable objects and differ only in steady iteration versus time integration.

**Tech Stack:** Rust 1.85, serde/serde_json, thiserror, blake3, Criterion already present in the repository, Cargo integration tests, native and `wasm32-unknown-unknown` builds.

## Global Constraints

- Preserve the existing planar production world, stage graph, `PreliminaryClimateSnapshot`, UI, and golden images during this experiment.
- Use no new crate dependencies.
- Use `f64` for spherical geometry and accumulated diagnostics, and `f32` for dense prognostic state.
- Cap the experimental grid at `6 × 64² = 24,576` cells and reject oversized allocation before mutation.
- Keep the dependency direction `solver -> operators -> {grid, physics}` and `comparison -> {grid, physics}`; `grid` and `physics` remain independent.
- Keep one implementation each of forcing, gradient, divergence, flux, Coriolis, tangent projection, thermodynamics, validation, and snapshot formatting.
- Both solvers consume the same `CirculationSpec`, `PlanetForcing`, `CubedSphereGrid`, and `CirculationOperators` and produce the same `CirculationSnapshot`.
- Do not use solver-specific empirical forcing, post-comparison fitting, GPU execution, runtime randomness, system time, or thread scheduling in numerical results.
- Use deterministic stable IDs and checked allocation arithmetic; timing values never participate in hashes.
- Implement every production behavior through a witnessed failing test followed by minimal code and a green test.
- Benchmark `n = 12`, `n = 24`, and `n = 32`; `n = 24` is the production-scale comparison point.

---

## File Structure

```text
src/world/natural/circulation/
├── mod.rs          # public contract re-exports
├── spec.rs         # CirculationSpec and validation budgets
├── forcing.rs      # immutable PlanetForcing and content fingerprint
└── snapshot.rs     # shared monthly output and solver statistics

src/generators/natural/circulation/
├── mod.rs              # public experiment API and CirculationSolver trait
├── math.rs             # private three-dimensional vector primitives
├── grid.rs             # immutable cubed-sphere geometry/topology
├── operators.rs        # sole finite-volume spatial operator implementation
├── thermodynamics.rs   # sole scalar forcing/transport implementation
├── fixtures.rs         # deterministic aqua/two-basin/Earth-like forcing
├── steady.rs           # BalancedSteadySolver orchestration only
├── transient.rs        # TransientShallowWaterSolver orchestration only
└── comparison.rs       # immutable metrics and suite report

src/bin/circulation_compare.rs       # Release measurement/report CLI
tests/circulation_contracts.rs       # contract and allocation validation
tests/support/mod.rs                  # shared integration-test support module
tests/support/circulation.rs          # reusable numeric helpers and deterministic fixtures
tests/circulation_grid.rs            # closed-sphere geometry invariants
tests/circulation_operators.rs       # analytic operator tests
tests/circulation_thermodynamics.rs  # forcing and scalar conservation
tests/circulation_steady.rs          # steady solver behavior
tests/circulation_transient.rs       # transient solver behavior
tests/circulation_comparison.rs      # metric/report correctness
tests/circulation_performance.rs     # ignored Release measurement smoke test
```

---

### Task 1: Add the single-source circulation contracts

**Files:**
- Create: `src/world/natural/circulation/mod.rs`
- Create: `src/world/natural/circulation/spec.rs`
- Create: `src/world/natural/circulation/forcing.rs`
- Create: `src/world/natural/circulation/snapshot.rs`
- Modify: `src/world/natural/mod.rs`
- Test: `tests/circulation_contracts.rs`

**Interfaces:**
- Consumes: existing serde, thiserror, blake3, `CLIMATE_MONTH_COUNT`.
- Produces: `CirculationSpec`, `CirculationSpecError`, `PlanetForcing`, `ForcingError`, `CirculationSnapshot`, `CirculationSnapshotError`, `CirculationSolverId`, `CirculationSolveStats`, `CIRCULATION_SCHEMA_V1`.

- [ ] **Step 1: Write failing specification and forcing tests**

```rust
use sekai::world::natural::{CirculationSpec, PlanetForcing, CLIMATE_MONTH_COUNT};

#[test]
fn circulation_spec_rejects_grid_and_iteration_budgets_before_allocation() {
    let mut spec = CirculationSpec::default();
    spec.face_resolution = 65;
    assert!(spec.validate().is_err());
    spec.face_resolution = 24;
    spec.max_steady_iterations = 0;
    assert!(spec.validate().is_err());
}

#[test]
fn forcing_is_dense_finite_and_content_addressed() {
    let count = 24;
    let monthly = vec![[280.0; CLIMATE_MONTH_COUNT]; count];
    let first = PlanetForcing::new(
        [7; 32], vec![0.0; count], vec![0.0; count], vec![0.3; count],
        vec![1.0; count], monthly.clone(), monthly,
        vec![[0.01; CLIMATE_MONTH_COUNT]; count],
    ).unwrap();
    let second = first.clone();
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.cell_count(), count);
}
```

- [ ] **Step 2: Run the contract test and verify RED**

Run: `cargo test --test circulation_contracts -- --nocapture`

Expected: compilation fails because `CirculationSpec` and `PlanetForcing` do not exist.

- [ ] **Step 3: Implement validated specifications and immutable forcing**

Implement these exact public shapes, keeping vectors private and exposing slices:

```rust
pub const CIRCULATION_SCHEMA_V1: u16 = 1;
pub const MAX_CUBED_SPHERE_FACE_RESOLUTION: u16 = 64;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CirculationSpec {
    pub face_resolution: u16,
    pub planet_radius_m: f64,
    pub rotation_rate_rad_s: f64,
    pub gravity_m_s2: f64,
    pub atmosphere_reference_depth_m: f32,
    pub atmosphere_reduced_gravity_m_s2: f32,
    pub ocean_reference_depth_m: f32,
    pub ocean_reduced_gravity_m_s2: f32,
    pub atmosphere_drag_s_inv: f32,
    pub ocean_drag_s_inv: f32,
    pub layer_relaxation_s_inv: f32,
    pub thermal_relaxation_s_inv: f32,
    pub max_steady_iterations: u16,
    pub max_formation_years: u16,
    pub convergence_tolerance: f32,
    pub cfl_limit: f32,
}

impl Default for CirculationSpec {
    fn default() -> Self {
        Self {
            face_resolution: 24,
            planet_radius_m: 6_371_000.0,
            rotation_rate_rad_s: 7.292_115_9e-5,
            gravity_m_s2: 9.806_65,
            atmosphere_reference_depth_m: 8_000.0,
            atmosphere_reduced_gravity_m_s2: 0.3125,
            ocean_reference_depth_m: 500.0,
            ocean_reduced_gravity_m_s2: 0.02,
            atmosphere_drag_s_inv: 2.314_814_8e-6,
            ocean_drag_s_inv: 3.858_024_7e-7,
            layer_relaxation_s_inv: 7.716_049_5e-7,
            thermal_relaxation_s_inv: 3.858_024_7e-7,
            max_steady_iterations: 96,
            max_formation_years: 4,
            convergence_tolerance: 1.0e-4,
            cfl_limit: 0.45,
        }
    }
}
```

Implement `CirculationSpec` deserialization through a private wire representation followed by `validate`, so invalid serialized input cannot bypass construction checks. `CirculationSpec::fingerprint()` must hash the schema version and canonical little-endian bytes of every validated field.

`PlanetForcing::new` must validate equal nonzero lengths, finite values, ranges `land_fraction/albedo/moisture ∈ [0,1]`, and compute one blake3 fingerprint from grid fingerprint plus canonical little-endian field bytes.

- [ ] **Step 4: Add the shared snapshot contract and validation tests**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CirculationSolverId { BalancedSteadyV1, TransientShallowWaterV1 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CirculationSolveStats {
    pub iterations_or_steps: u64,
    pub formation_years: u16,
    pub final_residual: f64,
    pub relative_mass_error: f64,
    pub dense_state_bytes: u64,
}
```

`CirculationSnapshot::new` must accept the spec/forcing/grid fingerprints, solver ID, stats, and cell-major monthly wind, current, air temperature, surface temperature, humidity, precipitation, atmospheric-height anomaly, and sea-height anomaly arrays. Test length mismatch, NaN rejection, negative humidity/precipitation rejection, fingerprint preservation, and serde round-trip.

- [ ] **Step 5: Run tests and commit**

Run: `cargo test --test circulation_contracts -- --nocapture`

Expected: all circulation contract tests pass.

Commit:

```powershell
git add src/world/natural/circulation src/world/natural/mod.rs tests/circulation_contracts.rs
git commit -m "feat: define spherical circulation contracts"
```

---

### Task 2: Build deterministic closed cubed-sphere geometry

**Files:**
- Create: `src/generators/natural/circulation/mod.rs`
- Create: `src/generators/natural/circulation/math.rs`
- Create: `src/generators/natural/circulation/grid.rs`
- Modify: `src/generators/natural/mod.rs`
- Test: `tests/circulation_grid.rs`

**Interfaces:**
- Consumes: `CirculationSpec`, blake3.
- Produces: `CubedSphereGrid`, `SphericalCell`, `SphericalEdge`, `CubedSphereGridError`; accessors `cells()`, `edges()`, `cell_count()`, `fingerprint()`, `minimum_center_distance_m()`.

- [ ] **Step 1: Write the closed-topology RED tests**

```rust
use sekai::generators::natural::circulation::CubedSphereGrid;

#[test]
fn cubed_sphere_is_closed_and_has_euler_counts() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    assert_eq!(grid.cell_count(), 24);
    assert_eq!(grid.edges().len(), 48);
    assert!(grid.edges().iter().all(|edge| edge.cells().len() == 2));
    for cell in grid.cells() {
        assert_eq!(cell.edges().len(), 4);
        assert_eq!(cell.neighbors().len(), 4);
    }
}

#[test]
fn cubed_sphere_area_closes_to_the_analytic_sphere() {
    let radius = 6_371_000.0;
    let grid = CubedSphereGrid::new(12, radius).unwrap();
    let found: f64 = grid.cells().iter().map(|cell| cell.area_m2()).sum();
    let expected = 4.0 * std::f64::consts::PI * radius * radius;
    assert!(((found - expected) / expected).abs() <= 1.0e-10);
}
```

- [ ] **Step 2: Run the grid test and verify RED**

Run: `cargo test --test circulation_grid -- --nocapture`

Expected: compilation fails because `CubedSphereGrid` does not exist.

- [ ] **Step 3: Implement the private vector math and face mapping**

`math.rs` must provide only crate-private pure functions over `[f64; 3]`: `add`, `sub`, `scale`, `dot`, `cross`, `norm`, `normalize`, `project_tangent`, `central_angle`, and `spherical_triangle_area_unit`. Use the robust unit-sphere triangle formula:

```rust
let numerator = a_dot_b_cross_c.abs();
let denominator = 1.0 + dot(a, b) + dot(b, c) + dot(c, a);
2.0 * numerator.atan2(denominator)
```

Define six right-handed `(normal, u_axis, v_axis)` bases. Map an equiangular face coordinate with:

```rust
let alpha = -FRAC_PI_4 + grid_coordinate * FRAC_PI_2;
normalize(add(normal, add(scale(u_axis, alpha.tan()), scale(v_axis, beta.tan()))))
```

- [ ] **Step 4: Implement immutable cells/edges and seam welding**

Create all face quads in stable `face,row,col` order. Weld a vertex by each normalized component rounded to `1e-13`; weld an edge by its sorted pair of welded vertex IDs. Split each spherical quad into triangles `(0,1,2)` and `(0,2,3)` for area. After construction, require every edge to have two owners, derive neighbors, edge midpoint, great-circle length, center distance, and canonical tangent normal from the first owner toward the second. Hash stable IDs and quantized geometry into the grid fingerprint.

- [ ] **Step 5: Add seam, determinism, and budget tests, run, and commit**

Add assertions that repeated `n=12` builds have identical fingerprints, adjacency is reciprocal, normals are tangent, resolution `0` and `65` fail before allocation, and minimum distance is positive.

Run: `cargo test --test circulation_grid -- --nocapture`

Expected: all grid tests pass.

Commit:

```powershell
git add src/generators/natural/circulation src/generators/natural/mod.rs tests/circulation_grid.rs
git commit -m "feat: add deterministic cubed sphere grid"
```

---

### Task 3: Implement one shared finite-volume operator set

**Files:**
- Create: `src/generators/natural/circulation/operators.rs`
- Modify: `src/generators/natural/circulation/mod.rs`
- Create: `tests/support/mod.rs`
- Create: `tests/support/circulation.rs`
- Test: `tests/circulation_operators.rs`

**Interfaces:**
- Consumes: immutable `CubedSphereGrid`, `[f32]`, `[[f32; 3]]`.
- Produces: `CirculationOperators<'grid>` with `gradient`, `divergence`, `coriolis`, `tangentize`, and `advect_scalar_conservative`.

- [ ] **Step 1: Write analytic constant-field RED tests**

```rust
mod support;

use support::circulation::area_weighted_rms;

#[test]
fn constant_scalar_has_zero_gradient_and_solid_rotation_is_divergence_free() {
    let grid = CubedSphereGrid::new(12, 6_371_000.0).unwrap();
    let operators = CirculationOperators::new(&grid);
    let gradient = operators.gradient(&vec![3.5; grid.cell_count()]).unwrap();
    assert!(gradient.iter().flatten().all(|value| value.abs() < 1.0e-10));

    let velocity = grid.cells().iter().map(|cell| {
        let r = cell.center_unit();
        [-r[1] as f32 * 10.0, r[0] as f32 * 10.0, 0.0]
    }).collect::<Vec<_>>();
    let divergence = operators.divergence(&velocity).unwrap();
    assert!(area_weighted_rms(&grid, &divergence) < 2.0e-6);
}
```

Create `tests/support/circulation.rs` with reusable, non-asserting `magnitude([f32; 3]) -> f64` and compensated `area_weighted_rms(&CubedSphereGrid, &[f64]) -> f64` helpers. Re-export it from `tests/support/mod.rs`; later solver tests must import these helpers rather than copy them.

- [ ] **Step 2: Run the operator test and verify RED**

Run: `cargo test --test circulation_operators -- --nocapture`

Expected: compilation fails because `CirculationOperators` does not exist.

- [ ] **Step 3: Implement Green–Gauss gradient and edge-flux divergence**

For every edge, linearly interpolate the scalar or vector between its two centers using center distances. Accumulate `edge_value × outward_normal × edge_length / cell_area` to each owner with opposite signs. Project vector interpolation onto the edge-midpoint tangent plane. Reject mismatched input lengths and non-finite input before mutation.

- [ ] **Step 4: Implement shared Coriolis and conservative upwind transport**

Use `f = 2 Ω center_z` and `-f (r × u)` for Coriolis acceleration. For scalar transport, compute one canonical signed volume flux per edge, select the upstream scalar, apply equal and opposite extensive-mass changes, divide by cell area, and return both the new field and relative global mass error. Dry-face permeability is supplied as a `[0,1]` edge multiplier and is not inferred inside the operator.

- [ ] **Step 5: Add transport conservation tests, run, and commit**

Test that a uniform scalar remains uniform, tangent projection removes radial components, solid-body advection preserves total scalar to `1e-6`, and zero permeability produces zero cross-edge change.

Run: `cargo test --test circulation_operators -- --nocapture`

Expected: all operator tests pass.

Commit:

```powershell
git add src/generators/natural/circulation/operators.rs src/generators/natural/circulation/mod.rs tests/support tests/circulation_operators.rs
git commit -m "feat: add shared spherical circulation operators"
```

---

### Task 4: Add shared thermodynamics and deterministic planet forcing

**Files:**
- Create: `src/generators/natural/circulation/thermodynamics.rs`
- Create: `src/generators/natural/circulation/fixtures.rs`
- Modify: `src/generators/natural/circulation/mod.rs`
- Modify: `tests/support/circulation.rs`
- Test: `tests/circulation_thermodynamics.rs`

**Interfaces:**
- Consumes: `CubedSphereGrid`, `PlanetForcing`, `CirculationSpec`, shared operators.
- Produces: `CirculationFixture::{AquaPlanet, TwoBasins, EarthLikeHarmonics}`, `build_fixture`, `ThermodynamicState`, `ThermodynamicTendencies`, `thermodynamic_tendencies`, `advance_thermodynamics`.

- [ ] **Step 1: Write forcing symmetry and uniqueness RED tests**

```rust
#[test]
fn aqua_forcing_is_oceanic_axisymmetric_and_seasonal() {
    let grid = CubedSphereGrid::new(12, 6_371_000.0).unwrap();
    let forcing = build_fixture(&grid, CirculationFixture::AquaPlanet).unwrap();
    assert!(forcing.land_fraction().iter().all(|&value| value == 0.0));
    assert_ne!(forcing.equilibrium_air_temperature_c()[0][0],
               forcing.equilibrium_air_temperature_c()[0][6]);
}

#[test]
fn forcing_kinds_produce_distinct_content_hashes() {
    let grid = CubedSphereGrid::new(12, 6_371_000.0).unwrap();
    let a = build_fixture(&grid, CirculationFixture::AquaPlanet).unwrap();
    let b = build_fixture(&grid, CirculationFixture::TwoBasins).unwrap();
    assert_ne!(a.fingerprint(), b.fingerprint());
}
```

- [ ] **Step 2: Run the thermodynamics test and verify RED**

Run: `cargo test --test circulation_thermodynamics -- --nocapture`

Expected: compilation fails because fixtures and thermodynamic state do not exist.

- [ ] **Step 3: Implement analytic fixture fields once**

Compute declination at month midpoint from axial tilt `23.44°`; compute positive daily-mean insolation from latitude and declination; turn it into equilibrium temperature with one shared albedo/lapse-rate formula. `TwoBasins` uses two analytic longitude/latitude ellipses and one Gaussian great-circle mountain belt. `EarthLikeHarmonics` uses a fixed sum of low-order trigonometric harmonics, never random noise. All three constructors finish by calling the same `PlanetForcing::new`.

- [ ] **Step 4: Implement one thermodynamic tendency kernel**

The kernel must:

```text
dT_air/dt     = relaxation(T_eq - T_air) - lapse_rate * max(elevation, 0)
dT_surface/dt = relaxation_by_heat_capacity(T_surface_eq - T_surface)
dq/dt         = conservative_advection + evaporation - condensation
precipitation = max(q - q_saturation(T_air), 0) / condensation_timescale
```

Use one documented Tetens saturation-vapor approximation, clamp only to validated physical bounds, and use the shared conservative transport operator for temperature and humidity. Both solvers call this function; no solver module may contain a second radiation, humidity, or precipitation formula.

- [ ] **Step 5: Test finite bounds and conservation, run, and commit**

Test that zero velocity with equilibrium state gives near-zero tendencies, ocean moisture source exceeds dry-land source, warmer air has greater saturation humidity, one transport step preserves non-condensed total moisture, and repeated fixture builds have equal fingerprints.

Extend the shared test support with `uniform_fixture(n) -> (CubedSphereGrid, PlanetForcing, CirculationSpec)`. It must build its forcing through the public validated constructor and derive all sizes from the grid; Tasks 5 and 6 import it instead of defining separate fixtures.

Run: `cargo test --test circulation_thermodynamics -- --nocapture`

Expected: all thermodynamic tests pass.

Commit:

```powershell
git add src/generators/natural/circulation/thermodynamics.rs src/generators/natural/circulation/fixtures.rs src/generators/natural/circulation/mod.rs tests/support/circulation.rs tests/circulation_thermodynamics.rs
git commit -m "feat: add shared planetary circulation forcing"
```

---

### Task 5: Implement the balanced steady solver

**Files:**
- Create: `src/generators/natural/circulation/steady.rs`
- Modify: `src/generators/natural/circulation/mod.rs`
- Test: `tests/circulation_steady.rs`

**Interfaces:**
- Consumes: shared grid, forcing, spec, operators, and thermodynamics.
- Produces: `CirculationSolver` trait, `BalancedSteadySolver`, `CirculationSolveError`.

- [ ] **Step 1: Write zero-forcing and deterministic RED tests**

```rust
mod support;

use support::circulation::{magnitude, uniform_fixture};

#[test]
fn balanced_solver_returns_zero_flow_for_uniform_equilibrium() {
    let (grid, forcing, spec) = uniform_fixture(8);
    let first = BalancedSteadySolver.solve(&grid, &forcing, &spec).unwrap();
    let second = BalancedSteadySolver.solve(&grid, &forcing, &spec).unwrap();
    assert_eq!(serde_json::to_vec(&first).unwrap(), serde_json::to_vec(&second).unwrap());
    assert!(first.monthly_wind_m_s().iter().flatten()
        .all(|u| magnitude(*u) < 1.0e-4));
    assert_eq!(first.solver_id(), CirculationSolverId::BalancedSteadyV1);
}
```

- [ ] **Step 2: Run the steady test and verify RED**

Run: `cargo test --test circulation_steady -- --nocapture`

Expected: compilation fails because `BalancedSteadySolver` and `CirculationSolver` do not exist.

- [ ] **Step 3: Define the strategy-only solver interface**

```rust
pub trait CirculationSolver {
    fn id(&self) -> CirculationSolverId;
    fn solve(
        &self,
        grid: &CubedSphereGrid,
        forcing: &PlanetForcing,
        spec: &CirculationSpec,
    ) -> Result<CirculationSnapshot, CirculationSolveError>;
}
```

The trait owns no physics defaults. Validate `spec`, grid fingerprint, forcing fingerprint, and cell count before allocating state.

- [ ] **Step 4: Implement bounded steady balance iterations**

For each month, start from the previous month state. Each iteration must call the shared gradient, local `(rI + fJ)` balance inversion, shared divergence, pseudo-time layer-height correction, linear air–ocean stress, tangent projection, and shared thermodynamic step in that order. Compute one area-weighted normalized residual from height, velocity, temperature, and humidity. Stop at tolerance or return `NotConverged` with the observed residual after `max_steady_iterations`.

Use the same state-to-snapshot function later consumed by the transient solver; place it in `circulation/mod.rs` as crate-private `snapshot_from_monthly_state`.

- [ ] **Step 5: Add forced-planet validation, run, and commit**

Test all three fixtures at `n=8`: fields are finite and tangent, circulation is nonzero under seasonal forcing, ocean current is zero on dry cells, precipitation is nonnegative, mass error is within `1e-5`, and `CirculationSnapshot::validate()` passes.

Run: `cargo test --test circulation_steady -- --nocapture`

Expected: all steady solver tests pass.

Commit:

```powershell
git add src/generators/natural/circulation/steady.rs src/generators/natural/circulation/mod.rs tests/circulation_steady.rs
git commit -m "feat: add balanced spherical circulation solver"
```

---

### Task 6: Implement the transient shallow-water solver

**Files:**
- Create: `src/generators/natural/circulation/transient.rs`
- Modify: `src/generators/natural/circulation/mod.rs`
- Test: `tests/circulation_transient.rs`

**Interfaces:**
- Consumes: the same contracts and shared modules as Task 5; optional validated steady snapshot as initial state.
- Produces: `TransientShallowWaterSolver::cold_start()` and `TransientShallowWaterSolver::warm_start(&CirculationSnapshot)`.

- [ ] **Step 1: Write time-step and zero-equilibrium RED tests**

```rust
mod support;

use support::circulation::{magnitude, uniform_fixture};

#[test]
fn transient_solver_uses_a_valid_quantized_cfl_step() {
    let (grid, forcing, spec) = uniform_fixture(8);
    let solver = TransientShallowWaterSolver::cold_start();
    let dt = solver.time_step_seconds(&grid, &spec).unwrap();
    assert!(dt >= 1 && dt % 60 == 0);
    assert!(solver.cfl(&grid, &spec, dt) <= f64::from(spec.cfl_limit));
    let snapshot = solver.solve(&grid, &forcing, &spec).unwrap();
    assert!(snapshot.monthly_wind_m_s().iter().flatten()
        .all(|u| magnitude(*u) < 1.0e-3));
}
```

- [ ] **Step 2: Run the transient test and verify RED**

Run: `cargo test --test circulation_transient -- --nocapture`

Expected: compilation fails because `TransientShallowWaterSolver` does not exist.

- [ ] **Step 3: Implement deterministic CFL selection and tendency evaluation**

Compute the maximum wave speed as the maximum of `sqrt(g' H)` for atmosphere and ocean. Choose `floor(cfl_limit * minimum_center_distance / max_wave_speed)` seconds, round down to a whole minute, and reject a result below 60 seconds. One `evaluate_tendencies` function must call shared gradient, divergence, Coriolis, drag, linear wind stress, and thermodynamics without mutating input state.

- [ ] **Step 4: Implement Heun/RK2 annual cycling and convergence**

Advance predictor and corrected states with identical tendency evaluation. Use twelve 30-day climatological months, accumulate arithmetic monthly means, and compare same-month state between completed years with the shared area-weighted residual. Stop on tolerance or return `NotConverged` at `max_formation_years`. Warm start must reject a different grid/forcing/spec fingerprint and must not modify the supplied snapshot.

- [ ] **Step 5: Test cold/warm behavior, run, and commit**

Test all fixtures at `n=8`; require finite tangent fields, closed-coast currents, mass error within `1e-5`, deterministic cold runs, warm-start step count no greater than cold-start step count, and explicit rejection of a mismatched steady snapshot.

Run: `cargo test --test circulation_transient -- --nocapture`

Expected: all transient solver tests pass.

Commit:

```powershell
git add src/generators/natural/circulation/transient.rs src/generators/natural/circulation/mod.rs tests/circulation_transient.rs
git commit -m "feat: add transient shallow water solver"
```

---

### Task 7: Add immutable scientific comparison metrics

**Files:**
- Create: `src/generators/natural/circulation/comparison.rs`
- Modify: `src/generators/natural/circulation/mod.rs`
- Modify: `tests/support/circulation.rs`
- Test: `tests/circulation_comparison.rs`

**Interfaces:**
- Consumes: two immutable snapshots plus their shared grid/forcing/spec fingerprints.
- Produces: `compare_snapshots`, `VectorAgreement`, `ScalarAgreement`, `MonthlyAgreement`, `FixtureComparison`, `ComparisonReport`, `ComparisonError`, `WysiwygEligibility`.

- [ ] **Step 1: Write exact artificial-field RED tests**

```rust
mod support;

use support::circulation::{artificial_snapshot, mismatched_snapshots};

#[test]
fn identical_fields_have_unit_correlation_zero_error_and_full_direction_agreement() {
    let (grid, snapshot) = artificial_snapshot();
    let report = compare_snapshots(&grid, &snapshot, &snapshot).unwrap();
    for month in &report.monthly {
        assert!((month.wind.vector_correlation - 1.0).abs() < 1.0e-12);
        assert_eq!(month.wind.normalized_rmse, 0.0);
        assert_eq!(month.wind.direction_agreement, 1.0);
        assert_eq!(month.air_temperature.rmse, 0.0);
    }
}

#[test]
fn comparison_rejects_different_forcing_fingerprints() {
    let (grid, first, second) = mismatched_snapshots();
    assert!(compare_snapshots(&grid, &first, &second).is_err());
}
```

Extend shared test support with `artificial_snapshot()` and `mismatched_snapshots()`. Build both exclusively through public validated contracts; mismatch exactly one fingerprint per negative case so each rejection has an unambiguous cause.

- [ ] **Step 2: Run the comparison test and verify RED**

Run: `cargo test --test circulation_comparison -- --nocapture`

Expected: compilation fails because comparison types do not exist.

- [ ] **Step 3: Implement area-weighted vector and scalar statistics**

Implement compensated `f64` sums. Vector correlation and normalized RMSE must use the design formulas. Direction agreement must ignore cells below `0.1 m/s` wind or `0.01 m/s` current in either snapshot and count area within 45°. Scalar metrics must include correlation, RMSE, area-weighted bias, and global total relative bias.

- [ ] **Step 4: Encode the published WYSIWYG thresholds once**

`WysiwygEligibility::evaluate` must use exactly: wind correlation `0.95`, wind nRMSE `0.20`, wind direction `0.90`; current correlation `0.90`, current nRMSE `0.30`, current direction `0.85`; temperature correlation `0.98`, absolute bias `0.5 °C`; precipitation total bias `0.02`, correlation `0.95`. Return failed metric names and observed values; do not collapse failures into a boolean only.

- [ ] **Step 5: Compare real solver outputs, run, and commit**

At `n=8`, run both solvers for each fixture and assert the report is finite, has twelve months, contains solver residuals and memory values, and serializes deterministically. Do not assert the eligibility outcome yet.

Run: `cargo test --test circulation_comparison -- --nocapture`

Expected: all comparison tests pass.

Commit:

```powershell
git add src/generators/natural/circulation/comparison.rs src/generators/natural/circulation/mod.rs tests/support/circulation.rs tests/circulation_comparison.rs
git commit -m "feat: compare spherical circulation solvers"
```

---

### Task 8: Build the Release measurement tool

**Files:**
- Create: `src/bin/circulation_compare.rs`
- Create: `tests/circulation_performance.rs`
- Modify: `src/generators/natural/circulation/comparison.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: fixture builders, both solvers, `compare_snapshots`.
- Produces: `circulation_compare` CLI JSON/text output and one ignored Release smoke test.

- [ ] **Step 1: Write argument and report RED tests**

Expose a library function:

```rust
pub fn run_comparison_suite(
    resolutions: &[u16],
    fixtures: &[CirculationFixture],
    measured_samples: usize,
) -> Result<ComparisonSuiteReport, ComparisonError>;
```

`ComparisonSuiteReport`, `ComparisonCaseReport`, and `TimingSummary` live beside the comparison metrics and are the sole serializable measurement schema used by both the library entry point and CLI.

Test that empty resolutions, resolution `65`, or zero samples fail; a single `n=4` aqua sample returns one record with separate grid, forcing, steady, transient-cold, transient-warm, validation, and comparison durations.

- [ ] **Step 2: Run the performance test and verify RED**

Run: `cargo test --test circulation_performance -- --include-ignored --nocapture`

Expected: compilation fails because `run_comparison_suite` does not exist.

- [ ] **Step 3: Implement measurement without contaminating numerical state**

For each case, construct grid/forcing once, run two unreported warmups, then collect requested samples with `Instant` around only the named operation. Sort nanoseconds and report median and maximum. Use `std::hint::black_box` on snapshots. Compute dense bytes from actual slice lengths and element sizes, not process RSS. Never store duration in a `CirculationSnapshot` or fingerprint.

- [ ] **Step 4: Implement the dependency-free CLI**

Accept exactly:

```text
--resolutions 12,24,32
--samples 9
--json target/circulation-comparison.json
```

Unknown flags, malformed integers, empty lists, and unwritable output return a nonzero exit with one concise error. Always print a text table; write pretty JSON only when `--json` is supplied. Add an explicit `[[bin]]` entry named `circulation_compare` in `Cargo.toml`.

- [ ] **Step 5: Run smoke measurements and commit**

Run:

```powershell
cargo test --release --test circulation_performance -- --ignored --nocapture
cargo run --release --bin circulation_compare -- --resolutions 4 --samples 1 --json target/circulation-comparison-smoke.json
```

Expected: test passes; CLI exits 0 and emits valid JSON containing both solver IDs and all timing categories.

Commit:

```powershell
git add Cargo.toml src/bin/circulation_compare.rs src/generators/natural/circulation/comparison.rs tests/circulation_performance.rs
git commit -m "perf: measure spherical circulation solvers"
```

---

### Task 9: Run full evidence gates and record the decision data

**Files:**
- Modify: `docs/superpowers/specs/2026-08-03-spherical-circulation-solver-comparison-design.md`
- Create: `docs/superpowers/specs/2026-08-03-spherical-circulation-solver-comparison-results.md`
- Modify only if required by a witnessed failure: circulation source/test files from Tasks 1–8.

**Interfaces:**
- Consumes: complete implementation and `target/circulation-comparison.json`.
- Produces: verified native/WASM code and a human-readable results document with no automatic production integration.

- [ ] **Step 1: Run focused and full native tests**

Run:

```powershell
cargo test --test circulation_contracts --test circulation_grid --test circulation_operators --test circulation_thermodynamics --test circulation_steady --test circulation_transient --test circulation_comparison
cargo test --all-targets --all-features
```

Expected: every non-ignored test passes; only explicitly documented existing ignored tests remain ignored.

- [ ] **Step 2: Run formatting, lint, and platform gates**

Run:

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-features --lib --target wasm32-unknown-unknown
trunk build
```

Expected: all commands exit 0 with no warnings promoted to errors.

- [ ] **Step 3: Produce the full Release comparison evidence**

Run:

```powershell
cargo run --release --bin circulation_compare -- --resolutions 12,24,32 --samples 9 --json target/circulation-comparison.json
```

Expected: all three fixtures and resolutions finish, all snapshots pass physical validation, and JSON contains cold/warm timings, memory, residuals, and monthly metrics. If a solver returns structured non-convergence, preserve the failing report, write a failing regression test for the observed case, correct only shared physics or solver strategy, and rerun the full command.

- [ ] **Step 4: Write the results document from measured values**

The document must include these exact sections with numeric tables copied from the JSON:

```markdown
# Spherical Circulation Solver Comparison Results

## Environment and commands
## Geometry and conservation validation
## n=12, n=24, and n=32 performance
## Cold start versus steady warm start
## Monthly wind agreement
## Monthly ocean-current agreement
## Temperature and precipitation agreement
## WYSIWYG eligibility failures, if any
## Evidence-based recommendation
## Questions reserved for the production-integration decision
```

Do not describe either solver as production-selected; the user makes the final choice after reviewing raw evidence.

- [ ] **Step 5: Verify diff, commit, and hand off the evidence**

Run:

```powershell
git diff --check
git status --short
git diff --stat HEAD~1
```

Commit:

```powershell
git add docs/superpowers/specs/2026-08-03-spherical-circulation-solver-comparison-design.md docs/superpowers/specs/2026-08-03-spherical-circulation-solver-comparison-results.md
git commit -m "docs: report spherical circulation comparison"
```

Expected handoff: report exact measured timings and consistency metrics, explain which thresholds passed or failed, and ask the user to choose the single production solver or authorize a targeted optimization iteration.
