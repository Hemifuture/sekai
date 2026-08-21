# Field-Driven Spherical Terrain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Replace regular spherical nearest-seed plate and crust morphology with deterministic field-driven current-state morphology that feeds the existing plate-motion boundary model and produces a more natural preliminary heightmap.

**Architecture:** Keep SphericalSurfaceSnapshot and its Voronoi–Delaunay topology authoritative. Add crate-private scalar-field, edge-metric, arrival-time, and area-selection primitives; compose them in focused spherical plate and crust modules; then pass the unchanged SphericalTectonicSnapshot contract into the existing mantle and relief stages. No build intermediate is serialized, published, or visible to UI/rendering.

**Tech Stack:** Rust, existing noise/OpenSimplex crate, rand_chacha labeled substreams, BTree collections and BinaryHeap, existing typed Stage graph, serde/blake3 tests, wgpu goldens, native and wasm32 targets; no new dependencies.

## Global Constraints

- Generate exactly one current-state result. Do not add history slices, time stepping, reconstructed plate states, or hidden temporal caches.
- Keep the rendered globe a unit sphere. Elevation remains a cell scalar field and never displaces globe vertices.
- Preserve SphericalSurfaceSnapshot, SurfaceRef, stable CellId/EdgeId, and existing 36-field publication contracts.
- Keep LegacyPlanarV1 algorithms, random labels, stage versions, serialized results, and goldens byte-identical.
- New morphology types and constructors remain crate-private and non-serde; do not expose a test-only public recomposition path.
- One concept has one implementation: one spherical field sampler, one positive edge metric builder, one arrival heap core, and one area-mask core.
- Keep plate partition, crust morphology, Euler motion, and boundary aggregation as separate domain modules with one-way dependencies.
- Use versioned labeled random streams so changing one field cannot perturb plate motion, mantle, or another morphology field.
- Bump SphericalTectonicStage from version 1 to version 2; leave the SphericalTectonicSnapshot wire schema unchanged.
- At 20,252 cells, spherical tectonics must remain at or below 300 ms Release, the full natural graph at or below 1.25 times the same-machine pre-change baseline and below 5 s, and temporary morphology memory below 64 MiB.
- All behavior changes follow strict RED → GREEN TDD. Run the exact failing test before writing its production implementation.
- Commit each task independently with only its named files. Preserve unrelated user changes.
- Every fixture or assertion helper named in a test sketch below is a private function in that same test module; it takes the authoritative types shown by the enclosing test and returns only the stated scalar, collection, or assertion result. It never becomes a production API.

---

## File Structure

Create:

    src/generators/natural/morphology/mod.rs
    src/generators/natural/morphology/field.rs
    src/generators/natural/morphology/metric.rs
    src/generators/natural/morphology/arrival.rs
    src/generators/natural/morphology/area.rs
    src/generators/natural/spherical_tectonics/plates.rs
    src/generators/natural/spherical_tectonics/crust.rs
    src/generators/natural/spherical_tectonics/motion.rs
    src/generators/natural/spherical_tectonics/boundaries.rs
    tests/spherical_field_driven_relief.rs
    tests/spherical_morphology_quality.rs

Modify:

    src/generators/natural/mod.rs
    src/generators/natural/random.rs
    src/generators/natural/relief_noise.rs
    src/generators/natural/topology.rs
    src/generators/natural/spherical_tectonics.rs
    src/generators/natural/spherical_stage.rs
    tests/spherical_tectonic_generation.rs
    tests/spherical_tectonic_mantle_stage.rs
    tests/spherical_natural_matrix.rs
    tests/spherical_relief_geology_matrix.rs
    tests/spherical_natural_stage_graph.rs
    tests/spherical_presentation_gpu.rs
    tests/spherical_natural_graph_performance.rs
    docs/superpowers/plans/2026-08-10-field-driven-spherical-terrain.md

Responsibilities:

- morphology/field.rs owns continuous sphere sampling, area normalization, resolution filtering, and quantization.
- morphology/metric.rs owns strictly positive per-edge traversal costs.
- morphology/arrival.rs owns the only single/multi-source priority-queue propagation implementation.
- morphology/area.rs owns area prefixes, components, protected region growth, hole cleanup, and coast rebalancing.
- spherical_tectonics/plates.rs owns plate targets, seed placement, bias calibration, and final PlateIdField.
- spherical_tectonics/crust.rs owns formation recipes, static lobe clusters, continental mask, and crust thickness.
- spherical_tectonics/motion.rs owns existing Euler rotations and relative-motion selection.
- spherical_tectonics/boundaries.rs owns existing boundary classification and segment aggregation.
- spherical_tectonics.rs becomes validation/orchestration/snapshot assembly only.

---

### Task 1: Add the Single Spherical Scalar-Field Primitive

**Files:**

- Create: src/generators/natural/morphology/mod.rs
- Create: src/generators/natural/morphology/field.rs
- Modify: src/generators/natural/mod.rs
- Modify: src/generators/natural/random.rs
- Modify: src/generators/natural/relief_noise.rs

**Interfaces:**

- Consumes: SphericalSurfaceSnapshot, CellId, existing OpenSimplex dependency, one explicit u32 seed.
- Produces:

    pub(super) enum FieldShape {
        Smooth,
        Ridged,
    }

    pub(super) struct FieldBand {
        pub(super) angular_scale_rad: f64,
        pub(super) weight_milli: i32,
        pub(super) shape: FieldShape,
    }

    pub(super) struct FieldRecipe {
        pub(super) bands: &'static [FieldBand],
        pub(super) clamp_sigma_milli: u16,
    }

    pub(super) struct QuantizedScalarField {
        values: Box<[i16]>,
    }

    pub(super) fn sample_spherical_field(
        surface: &SphericalSurfaceSnapshot,
        recipe: FieldRecipe,
        seed: u32,
    ) -> Result<QuantizedScalarField, MorphologyFieldError>

- Later tasks use QuantizedScalarField::get(CellId), values(), len(), and normalized_f64(CellId).

- [x] **Step 1: Write field and random-stream RED tests**

Add unit tests in morphology/field.rs and random.rs with these exact contracts:

    #[test]
    fn spherical_field_is_area_centered_seeded_and_seam_free() {
        let sphere = test_sphere(642);
        let first = sample_spherical_field(&sphere, PLATE_RESISTANCE_RECIPE, 71).unwrap();
        let repeated = sample_spherical_field(&sphere, PLATE_RESISTANCE_RECIPE, 71).unwrap();
        let changed = sample_spherical_field(&sphere, PLATE_RESISTANCE_RECIPE, 72).unwrap();
        assert_eq!(first.values(), repeated.values());
        assert_ne!(first.values(), changed.values());
        assert!(area_weighted_mean(&sphere, &first).abs() <= 2.0 / i16::MAX as f64);
        assert_cut_and_pole_neighbor_jumps_are_bounded(&sphere, &first);
    }

    #[test]
    fn unresolvable_detail_band_is_omitted_without_changing_macro_values() {
        let coarse = test_sphere(162);
        let macro_only = sample_spherical_field(&coarse, MACRO_ONLY_RECIPE, 91).unwrap();
        let with_unresolvable_detail =
            sample_spherical_field(&coarse, MACRO_PLUS_TINY_DETAIL_RECIPE, 91).unwrap();
        assert_eq!(macro_only.values(), with_unresolvable_detail.values());
    }

    #[test]
    fn spherical_morphology_substreams_are_pairwise_orthogonal() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let expected = first_eight(&mut streams.stream(PLATE_MOTION_LABEL));
        for label in SPHERICAL_MORPHOLOGY_LABELS {
            consume_one_hundred(&mut streams.stream(label));
        }
        assert_eq!(expected, first_eight(&mut streams.stream(PLATE_MOTION_LABEL)));
    }

Expected constants in random.rs:

    pub(super) const PLATE_TARGET_AREA_LABEL: &str = "plate-target-area-v2";
    pub(super) const PLATE_SEED_PLACEMENT_LABEL: &str = "plate-seed-placement-v2";
    pub(super) const PLATE_RESISTANCE_FIELD_LABEL: &str = "plate-resistance-field-v2";
    pub(super) const PLATE_FABRIC_FIELD_LABEL: &str = "plate-fabric-field-v2";
    pub(super) const CRUST_ANCHOR_LAYOUT_LABEL: &str = "crust-anchor-layout-v2";
    pub(super) const CRUST_AFFINITY_FIELD_LABEL: &str = "crust-affinity-field-v2";
    pub(super) const CRUST_THICKNESS_FIELD_LABEL: &str = "crust-thickness-field-v2";

    pub(super) const SPHERICAL_MORPHOLOGY_LABELS: [&str; 7] = [
        PLATE_TARGET_AREA_LABEL,
        PLATE_SEED_PLACEMENT_LABEL,
        PLATE_RESISTANCE_FIELD_LABEL,
        PLATE_FABRIC_FIELD_LABEL,
        CRUST_ANCHOR_LAYOUT_LABEL,
        CRUST_AFFINITY_FIELD_LABEL,
        CRUST_THICKNESS_FIELD_LABEL,
    ];

- [x] **Step 2: Capture the untouched Release baseline**

Before any production edit, run from commit f00466ce's current behavior:

    cargo test --release --test spherical_natural_graph_performance -- --ignored --nocapture

Record the exact 20,252-cell full-graph duration, persistent bytes, peak working-set delta, cell count, plate count, command, machine/backend, and baseline commit in Execution Evidence. This is the denominator for Task 8's 1.25-times budget.

- [x] **Step 3: Run RED**

Run:

    cargo test --lib generators::natural::morphology::field -- --nocapture
    cargo test --lib generators::natural::random::tests::spherical_morphology_substreams_are_pairwise_orthogonal -- --nocapture

Expected: compilation fails because morphology, the field types, sampler, recipes, and V2 labels do not exist.

- [x] **Step 4: Extract the shared 3D coherent-noise core**

Move the implementation behind ReliefNoise3d into a crate-private CoherentNoise3d in morphology/field.rs without changing its seed stepping, octave rotation, or arithmetic. Keep relief_noise.rs source-compatible by delegating:

    pub(super) struct ReliefNoise3d(CoherentNoise3d);

    impl ReliefNoise3d {
        pub(super) fn new(seed: u32) -> Self {
            Self(CoherentNoise3d::new(seed))
        }

        pub(super) fn fbm(&self, point: [f64; 3], profile: FractalProfile) -> f64 {
            self.0.fbm(point, profile)
        }

        pub(super) fn ridged(&self, point: [f64; 3], profile: FractalProfile) -> f64 {
            self.0.ridged(point, profile)
        }
    }

Run the existing relief-noise unit tests immediately after the move to prove byte-stable behavior.

- [x] **Step 5: Implement sampling, normalization, resolution filtering, and quantization**

Use point = cell.centroid.components(). For each retained band, sample CoherentNoise3d at point / angular_scale_rad, apply Smooth or Ridged shape, and combine integer milli-weights. Drop a band when its angular scale is less than four times the median equivalent cell angular diameter.

Normalize using cell.area weights:

    mean = sum(value_i * area_i) / sum(area_i)
    variance = sum((value_i - mean)^2 * area_i) / sum(area_i)
    normalized = clamp((value_i - mean) / sqrt(variance), -clamp_sigma, clamp_sigma)
    quantized = round(normalized / clamp_sigma * i16::MAX)

Reject empty surfaces, invalid recipes, non-finite samples, zero variance, and cardinality mismatch with typed MorphologyFieldError variants.

- [x] **Step 6: Run GREEN and legacy adjacency**

Run:

    cargo test --lib generators::natural::morphology::field -- --nocapture
    cargo test --lib generators::natural::relief_noise -- --nocapture
    cargo test --lib generators::natural::random -- --nocapture
    cargo test --test natural_display_golden -- --nocapture

Expected: all pass; planar and existing spherical relief noise results remain unchanged.

- [x] **Step 7: Commit**

    git add src/generators/natural/mod.rs src/generators/natural/random.rs src/generators/natural/relief_noise.rs src/generators/natural/morphology
    git commit -m "feat: add spherical morphology fields"

---

### Task 2: Add Positive Edge Metrics and the Unified Arrival Solver

**Files:**

- Create: src/generators/natural/morphology/metric.rs
- Create: src/generators/natural/morphology/arrival.rs
- Modify: src/generators/natural/morphology/mod.rs
- Modify: src/generators/natural/topology.rs

**Interfaces:**

- Consumes: NaturalTopologyIndex, QuantizedScalarField, CellId, EdgeId.
- Produces:

    pub(super) struct PositiveEdgeMetric {
        costs: Box<[u64]>,
    }

    impl PositiveEdgeMetric {
        pub(super) fn from_topology_lengths(
            topology: &NaturalTopologyIndex,
        ) -> Result<Self, EdgeMetricError>;
    }

    pub(super) struct ArrivalSource {
        pub(super) owner: u32,
        pub(super) cell: CellId,
        pub(super) initial_cost: u64,
    }

    pub(super) struct ArrivalAssignment {
        pub(super) owners: Box<[u32]>,
        pub(super) costs: Box<[u64]>,
    }

    pub(super) struct ArrivalWorkspace {
        distances: Vec<u64>,
        owners: Vec<u32>,
        heap: BinaryHeap<ArrivalQueueEntry>,
    }

    pub(super) fn build_plate_metric(
        topology: &NaturalTopologyIndex,
        resistance: &QuantizedScalarField,
        fabric: &QuantizedScalarField,
    ) -> Result<PositiveEdgeMetric, EdgeMetricError>

    pub(super) fn assign_arrivals(
        topology: &NaturalTopologyIndex,
        metric: &PositiveEdgeMetric,
        sources: &[ArrivalSource],
        workspace: &mut ArrivalWorkspace,
    ) -> Result<ArrivalAssignment, ArrivalError>

- topology.rs compatibility functions multi_source_ownership and multi_source_distance delegate to the same arrival heap using PositiveEdgeMetric::from_topology_lengths.

- [x] **Step 1: Write metric and arrival RED tests**

Add tests that assert:

    #[test]
    fn zero_field_metric_is_the_exact_legacy_metric() {
        let topology = fixture_topology();
        let metric = build_plate_metric(&topology, &zero_field(), &zero_field()).unwrap();
        assert_eq!(metric.costs(), topology.edge_traversal_costs());
    }

    #[test]
    fn fabric_and_resistance_change_routes_but_keep_positive_symmetric_costs() {
        let topology = fixture_topology();
        let metric = build_plate_metric(&topology, &resistance_fixture(), &fabric_fixture()).unwrap();
        assert!(metric.costs().iter().all(|&cost| cost > 0));
        assert_metric_is_shared_by_both_arc_directions(&topology, &metric);
        assert_ne!(metric.costs(), topology.edge_traversal_costs());
    }

    #[test]
    fn biased_arrival_is_stable_connected_and_workspace_reusable() {
        let first = assign_fixture(&[0, 17, 29]);
        let repeated = assign_fixture(&[0, 17, 29]);
        assert_eq!(first, repeated);
        assert_each_owner_has_a_shortest_path_to_its_source(&first);
        assert_second_call_reuses_workspace_capacity();
    }

    #[test]
    fn legacy_multi_source_helpers_keep_their_exact_outputs() {
        assert_eq!(
            multi_source_ownership(&topology, &sources),
            frozen_assignment_before_refactor()
        );
    }

- [x] **Step 2: Run RED**

Run:

    cargo test --lib generators::natural::morphology::metric -- --nocapture
    cargo test --lib generators::natural::morphology::arrival -- --nocapture

Expected: compilation fails on missing metric and arrival modules/types.

- [x] **Step 3: Implement PositiveEdgeMetric**

For each EdgeId, read its two owners and base traversal length. Compute resistance as the endpoint mean in -1..1. Compute fabric slope as absolute endpoint difference divided by traversal length; normalize all slopes by twice their length-weighted RMS and clamp to 0..1.

Use fixed-point multiplication for:

    multiplier = clamp(1 + 0.45 * resistance + 1.50 * fabric_crossing, 0.45, 2.20)
    cost = max(1, round(base_cost * multiplier))

Store one cost per EdgeId. Validate field and edge cardinality before allocation.

- [x] **Step 4: Implement the one arrival heap**

Use a min-heap ordering encoded through reverse Ord. Compare entries by total cost, then owner, then CellId. Initialize sources after subtracting their minimum initial cost so all values are non-negative. Reject duplicate source cells, duplicate owner IDs, out-of-range cells, empty sources, and addition overflow.

Replace topology.rs internal propagate body with a call to the unified core using the base metric. Preserve current GraphAssignment public(super) shape and exact tie ordering.

- [x] **Step 5: Run GREEN, topology, and planar golden tests**

Run:

    cargo test --lib generators::natural::morphology -- --nocapture
    cargo test --lib generators::natural::topology -- --nocapture
    cargo test --test legacy_planar_boundary -- --nocapture
    cargo test --test natural_display_golden -- --nocapture

Expected: all pass and the frozen planar hashes do not change.

- [x] **Step 6: Perform mutation checks**

Temporarily remove the fabric term and confirm fabric_and_resistance_change_routes_but_keep_positive_symmetric_costs fails. Restore it.

Temporarily reverse the owner tie-break and confirm legacy_multi_source_helpers_keep_their_exact_outputs fails. Restore it.

- [x] **Step 7: Commit**

    git add src/generators/natural/morphology src/generators/natural/topology.rs
    git commit -m "feat: add field-weighted spherical arrivals"

---

### Task 3: Build Field-Driven Plate Partitions

**Files:**

- Create: src/generators/natural/spherical_tectonics/plates.rs
- Modify: src/generators/natural/spherical_tectonics.rs
- Modify: src/generators/natural/random.rs

**Interfaces:**

- Consumes: validated SphericalSurfaceSnapshot, NaturalTopologyIndex, TectonicSpec, LabeledSubstreams.
- Produces:

    pub(super) struct PlatePartition {
        pub(super) seeds: Vec<CellId>,
        pub(super) target_area_weights: Box<[u64]>,
        pub(super) owners: PlateIdField,
        pub(super) achieved_area_weights: Box<[u64]>,
    }

    pub(super) fn generate_plate_partition(
        surface: &SphericalSurfaceSnapshot,
        topology: &NaturalTopologyIndex,
        spec: &TectonicSpec,
        streams: &LabeledSubstreams,
    ) -> Result<PlatePartition, PlateMorphologyError>

- motion.rs later consumes seeds and owners; crust.rs consumes owners and target weights.

- Fixed-point target weights use `const AREA_WEIGHT_TOTAL: u64 = 1_000_000_000` so target sums, comparisons, and tie-breaking do not depend on floating-point reduction order.

- [x] **Step 1: Write target, seed, shape, and calibration RED tests**

In plates.rs add:

    #[test]
    fn default_targets_are_bounded_diverse_and_area_normalized() {
        let targets = generate_target_area_weights(12, seed_stream(42));
        assert_eq!(targets.iter().sum::<u64>(), AREA_WEIGHT_TOTAL);
        assert!(ratio(max(&targets), min(&targets)) >= 2.75);
        assert!(targets.iter().all(|&value| value > 0));
    }

    #[test]
    fn field_driven_partition_is_connected_and_not_uniform_voronoi() {
        let partition = fixture_partition(642, 42, 12);
        assert_all_plates_connected_and_contain_seed(&partition);
        assert!(normalized_perimeter_median(&partition) > 1.15);
        assert!(area_coefficient_of_variation(&partition) >= 0.30);
        assert_ne!(partition.owners, uniform_voronoi_partition(642, 42, 12));
    }

    #[test]
    fn six_bias_rounds_keep_the_best_valid_area_fit() {
        let partition = fixture_partition(2562, 91, 12);
        assert!(maximum_target_relative_error(&partition) <= 0.35);
        assert!(partition.seeds.iter().enumerate().all(
            |(owner, &seed)| partition.owners.get(seed.raw() as usize) == Some(PlateId::from_raw(owner as u32))
        ));
    }

    #[test]
    fn plate_field_streams_do_not_change_motion_or_crust_stream_prefixes() {
        assert_stream_prefix_orthogonality();
    }

- [x] **Step 2: Run RED**

Run:

    cargo test --lib generators::natural::spherical_tectonics::plates -- --nocapture

Expected: compilation fails because the plates submodule and PlatePartition contract do not exist.

- [x] **Step 3: Implement target weights and non-uniform seed placement**

Generate a stable rank profile from 0.55 to 1.90 for plate_count >= 8, apply bounded ±10% random perturbations, deterministically expand only relative-mean deviations when the discrete target CV is below 0.36, shuffle the target values using the same target-area stream, and renormalize to the existing total area-weight quantization. Verify the CV postcondition after bounded floating-point calibration and again after integer normalization; reject an impossible candidate instead of publishing a below-floor target vector.

Place targets largest first. For each candidate cell calculate:

    separation_score =
        min_j(metric_distance(candidate, seed_j) / (sqrt(target_i) + sqrt(target_j)))
        + seed_preference(candidate) * 0.12

Keep candidates satisfying the minimum separation, sort the best 5% by score descending and CellId ascending, and use the placement stream to choose within that band. Map selected positions back to stable PlateId order.

- [x] **Step 4: Implement field metric and six-round bias calibration**

Sample the fixed plate resistance and fabric recipes from their dedicated streams. Build PositiveEdgeMetric once. Reuse one ArrivalWorkspace for all rounds.

Let S be median nearest-seed metric distance. Update each signed bias by:

    error_i = (actual_i - target_i) / target_i
    delta_i = clamp(0.35 * S * error_i, -0.12 * S, 0.12 * S)
    bias_i = clamp(bias_i + delta_i, -0.60 * S, 0.60 * S)

Shift all signed biases by the common minimum before constructing ArrivalSource. Reject a round when a seed loses itself, a plate becomes empty, or connectivity validation fails. Stop after six rounds or two improvements below 0.005. Return the valid round with the lowest maximum relative error.

- [x] **Step 5: Run GREEN and existing spherical motion tests**

Run:

    cargo test --lib generators::natural::spherical_tectonics::plates -- --nocapture
    cargo test --test spherical_tectonic_generation spherical_rotations_are_repeatable_bounded_connected_and_locally_separated -- --nocapture

Expected: plate unit tests pass. The existing integration test may still use the old production partition until Task 5, but it must compile.

- [x] **Step 6: Mutation checks**

Set resistance and fabric coefficients to zero and confirm field_driven_partition_is_connected_and_not_uniform_voronoi fails. Restore.

Disable best-round retention and confirm six_bias_rounds_keep_the_best_valid_area_fit fails on the fixed seed. Restore.

- [x] **Step 7: Commit**

    git add src/generators/natural/spherical_tectonics/plates.rs src/generators/natural/spherical_tectonics.rs src/generators/natural/random.rs
    git commit -m "feat: grow spherical plates through fields"

---

### Task 4: Build Independent Continental Affinity and Crust

**Files:**

- Create: src/generators/natural/morphology/area.rs
- Create: src/generators/natural/spherical_tectonics/crust.rs
- Modify: src/generators/natural/morphology/mod.rs
- Modify: src/generators/natural/spherical_tectonics.rs

**Interfaces:**

- Consumes: SphericalSurfaceSnapshot, NaturalTopologyIndex, PlatePartition, TectonicSpec continental fraction, ResolvedWorldFormationPreset, crust labeled streams.
- Produces:

    pub(super) struct AreaMask {
        selected: Box<[bool]>,
        selected_area_weight: u128,
        component_count: usize,
    }

    pub(super) struct ProtectedRegionSeed {
        pub(super) cell: CellId,
        pub(super) budget_weight: u128,
        pub(super) component: u16,
    }

    pub(super) fn build_area_constrained_mask(
        topology: &NaturalTopologyIndex,
        scores: &[i32],
        protected: &[ProtectedRegionSeed],
        target_weight: u128,
        minimum_component_weight: u128,
        maximum_hole_weight: u128,
    ) -> Result<AreaMask, AreaSelectionError>

    pub(super) struct CrustMorphology {
        pub(super) kinds: CrustKindField,
        pub(super) thickness_km: Vec<f32>,
    }

    pub(super) fn generate_crust(
        surface: &SphericalSurfaceSnapshot,
        topology: &NaturalTopologyIndex,
        plates: &PlatePartition,
        spec: &TectonicSpec,
        preset: ResolvedWorldFormationPreset,
        streams: &LabeledSubstreams,
    ) -> Result<CrustMorphology, CrustMorphologyError>

- [x] **Step 1: Write generic area-mask RED tests**

Use a small deterministic graph fixture and assert:

    #[test]
    fn protected_growth_is_connected_area_bounded_and_deterministic() {
        let first = build_fixture_mask();
        let repeated = build_fixture_mask();
        assert_eq!(first, repeated);
        assert!(protected_components_are_connected(&first));
        assert!(area_error(&first) <= maximum_cell_weight());
    }

    #[test]
    fn cleanup_removes_speckles_fills_small_holes_and_keeps_major_components() {
        let mask = build_noisy_fixture_mask();
        assert_eq!(small_unprotected_components(&mask), 0);
        assert_eq!(small_enclosed_holes(&mask), 0);
        assert!(all_protected_seeds_remain_selected(&mask));
    }

    #[test]
    fn coast_rebalance_never_breaks_a_protected_narrow_neck() {
        let mask = build_narrow_neck_fixture();
        assert!(mask.is_selected(NECK_CELL));
        assert_eq!(connected_major_components(&mask), EXPECTED_COMPONENTS);
    }

- [x] **Step 2: Run area RED**

Run:

    cargo test --lib generators::natural::morphology::area -- --nocapture

Expected: compilation fails because area.rs and its types do not exist.

- [x] **Step 3: Implement area growth and cleanup**

Use one max-heap keyed by score descending, component ID, then CellId. A cell can enter a protected component only from an already selected neighbor of that component. Grow protected budgets first, then island seeds.

Label selected and unselected components with iterative queues. Remove undersized unprotected selected components. Fill enclosed unselected components below maximum_hole_weight. For coast shrink, build one inward-rooted spanning forest, anchor protected cells and one deep root per component, and remove only current coastal leaves through a stable priority queue. Update only the removed leaf's parent and graph neighbors; stop at the closest achievable area prefix. This preserves connectivity while keeping the callback at `O(E log V)` instead of recomputing full-graph articulation points per removed cell.

- [x] **Step 4: Write crust RED tests**

Add in crust.rs:

    #[test]
    fn preset_recipes_hit_area_and_major_component_contracts() {
        for case in PRESET_CASES {
            let crust = fixture_crust(case.preset, case.fraction, 42);
            assert!(area_error(&crust, case.fraction) <= maximum_cell_area());
            assert!(case.major_component_range.contains(&major_component_count(&crust)));
        }
    }

    #[test]
    fn continent_field_is_related_to_but_not_equal_to_plate_ownership() {
        let crust = fixture_crust(Continents, 0.38, 71);
        let overlap = coast_plate_boundary_overlap(&crust);
        assert!((0.10..=0.55).contains(&overlap));
        assert_ne!(crust.kinds.raw_values(), plate_owner_classes());
    }

    #[test]
    fn crust_random_base_is_orthogonal_to_plate_count_while_soft_coupling_may_change_mask() {
        let twelve = fixture_crust_components(12, 91);
        let seventeen = fixture_crust_components(17, 91);
        assert_eq!(twelve.base_affinity, seventeen.base_affinity);
        assert_eq!(twelve.anchor_layout, seventeen.anchor_layout);
        assert_ne!(twelve.final_affinity, seventeen.final_affinity);
    }

    #[test]
    fn thickness_uses_an_independent_field_and_stays_in_physical_ranges() {
        let crust = fixture_crust(Continents, 0.38, 113);
        assert_thickness_ranges(&crust);
        assert_ne!(rank_order(&crust.thickness_km), rank_order(&crust.affinity));
    }

- [x] **Step 5: Run crust RED**

Run:

    cargo test --lib generators::natural::spherical_tectonics::crust -- --nocapture

Expected: compilation fails because crust.rs, recipes, lobe kernels, and CrustMorphology do not exist.

- [x] **Step 6: Implement preset fields, static lobe clusters, and thickness**

Implement the exact recipe and preset tables from the design spec. Derive local plate-interior preference from distance to plate boundary divided by target equivalent radius and clamp to 0..1.

Use Wendland C2:

    if q < 1.0 {
        (1.0 - q).powi(4) * (4.0 * q + 1.0)
    } else {
        0.0
    }

Place lobe centers along neighbors with minimum absolute fabric-field change. Choose separated island local maxima from the final affinity. Convert selected mask to CrustKindField.

Generate thickness from CRUST_THICKNESS_FIELD_LABEL. Continental thickness adds normalized distance-to-coast modulation; oceanic thickness uses only its independent meso field. Clamp to existing continental and oceanic min/max constants.

- [x] **Step 7: Run GREEN and formation adjacency**

Run:

    cargo test --lib generators::natural::morphology::area -- --nocapture
    cargo test --lib generators::natural::spherical_tectonics::crust -- --nocapture
    cargo test --test spherical_tectonic_generation every_formation_preset_uses_global_spherical_area_and_plate_independent_crust -- --nocapture

Expected: new unit tests pass. The existing integration test still compiles; its old exact plate-independence assertion is replaced during Task 5.

- [x] **Step 8: Mutation checks**

Replace affinity with pure nearest-anchor distance and confirm continent_field_is_related_to_but_not_equal_to_plate_ownership or the radial-variation oracle fails. Restore.

Reuse affinity as thickness and confirm thickness_uses_an_independent_field_and_stays_in_physical_ranges fails. Restore.

- [x] **Step 9: Commit**

    git add src/generators/natural/morphology src/generators/natural/spherical_tectonics/crust.rs src/generators/natural/spherical_tectonics.rs
    git commit -m "feat: form spherical crust from affinity fields"

---

### Task 5: Integrate Orthogonal Tectonic Modules and Version the Stage

**Files:**

- Create: src/generators/natural/spherical_tectonics/motion.rs
- Create: src/generators/natural/spherical_tectonics/boundaries.rs
- Modify: src/generators/natural/spherical_tectonics.rs
- Modify: src/generators/natural/tectonics.rs
- Modify: src/generators/natural/spherical_stage.rs
- Modify: tests/spherical_tectonic_generation.rs
- Modify: tests/spherical_tectonic_mantle_stage.rs
- Modify: tests/spherical_natural_stage_graph.rs

**Interfaces:**

- Consumes: PlatePartition and CrustMorphology from Tasks 3–4.
- Produces the unchanged public SphericalTectonicSnapshot through:

    pub(super) fn assign_plate_rotations(
        surface: &SphericalSurfaceSnapshot,
        topology: &NaturalTopologyIndex,
        partition: &PlatePartition,
        activity: TectonicActivity,
        streams: &LabeledSubstreams,
    ) -> Result<Vec<SphericalPlate>, PlateMotionError>

    pub(super) fn classify_and_aggregate_boundaries(
        surface: &SphericalSurfaceSnapshot,
        topology: &NaturalTopologyIndex,
        plates: &[SphericalPlate],
        owners: &PlateIdField,
        crust: &CrustMorphology,
    ) -> (Vec<BoundaryRecord>, Vec<SphericalBoundarySegment>)

- [x] **Step 1: Write orchestration RED tests**

Update spherical_tectonic_generation.rs:

    #[test]
    fn every_formation_preset_uses_global_area_and_soft_plate_coupling() {
        for case in CASES {
            let first = generate(SEED, &case.spec, case.preset);
            let repeated = generate(SEED, &case.spec, case.preset);
            assert_eq!(first, repeated);
            assert_area_within_one_cell(&first, case.fraction);
            assert_major_component_range(&first, case.major_components);
        }

        let twelve = generate(SEED, &default_spec(12), Continents);
        let seventeen = generate(SEED, &default_spec(17), Continents);
        assert_ne!(twelve.crust_kinds(), seventeen.crust_kinds());
        assert!(crust_mask_jaccard(&twelve, &seventeen) >= 0.55);
    }

Add a source scan test that fails while spherical_tectonics.rs still directly contains target generation, crust growth, Euler selection, and boundary aggregation:

    assert_facade_only_contains_generate_spherical_and_error_mapping();

- [x] **Step 2: Run RED**

Run:

    cargo test --test spherical_tectonic_generation every_formation_preset_uses_global_area_and_soft_plate_coupling -- --nocapture
    cargo test --lib generators::natural::spherical_tectonics::tests::facade_keeps_domain_modules_orthogonal -- --nocapture

Expected: old crust is exactly independent of plate count and the facade still owns all logic.

- [x] **Step 3: Move existing motion and boundary logic without behavior edits**

Move EULER_POLES, assign_plate_rotations, rotation candidates, velocity helpers, and their tests to motion.rs.

Move BoundaryEventDraft, classify_and_aggregate_boundaries, aggregation compatibility, and their tests to boundaries.rs.

Keep visibility pub(super) only where spherical_tectonics.rs needs it. Do not create a generic tectonic policy trait.

- [x] **Step 4: Replace the spherical orchestration**

The generate_spherical body must have this order:

    validate surface/spec/formation
    build SphericalNaturalSurface and NaturalTopologyIndex
    capture one LabeledSubstreams root
    generate_plate_partition
    assign_plate_rotations
    generate_crust
    classify_and_aggregate_boundaries
    construct SphericalTectonicSnapshot
    validate_against_validated_surface
    return

Remove spherical imports of tectonics::generate_plate_partition and tectonics::generate_crust. Leave both legacy planar functions and all V1 labels intact in tectonics.rs.

- [x] **Step 5: Bump only SphericalTectonicStage to version 2**

Change:

    fn version(&self) -> u32 {
        2
    }

Update direct test StageIdentity values that intentionally reproduce the production spherical tectonic stream. Do not change planar TectonicStage, mantle, or relief versions.

Update graph invalidation expectations: an unchanged second build hits all stages; changing tectonic input misses spherical tectonics and downstream stages; surface and resolved formation remain independently cacheable.

- [x] **Step 6: Run GREEN focused and adjacent suites**

Run:

    cargo test --lib generators::natural::spherical_tectonics -- --nocapture
    cargo test --test spherical_tectonic_generation -- --nocapture
    cargo test --test spherical_tectonic_contracts -- --nocapture
    cargo test --test spherical_tectonic_mantle_stage -- --nocapture
    cargo test --test spherical_natural_stage_graph -- --nocapture
    cargo test --test legacy_planar_boundary -- --nocapture
    cargo test --test natural_display_golden -- --nocapture

Expected: all pass; only sphere hashes/fixtures intentionally change.

- [x] **Step 7: Verify output/source/atomic boundaries**

Add assertions that same cell count on a different SurfaceRef is rejected, every output array shares exact sphere cardinality, failed morphology does not publish a SphericalTectonicArtifact, and no morphology intermediate implements Artifact or Serialize.

- [x] **Step 8: Commit**

    git add src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics src/generators/natural/tectonics.rs src/generators/natural/spherical_stage.rs tests/spherical_tectonic_generation.rs tests/spherical_tectonic_mantle_stage.rs tests/spherical_natural_stage_graph.rs
    git commit -m "feat: publish field-driven spherical tectonics"

---

### Task 6: Prove Plate and Crust Motion Produce the Preliminary Heightmap

**Files:**

- Create: tests/spherical_field_driven_relief.rs
- Modify: tests/spherical_relief_generation.rs
- Modify: tests/spherical_relief_geology_matrix.rs
- Modify: tests/spherical_natural_matrix.rs

**Interfaces:**

- Consumes: formal SphericalTectonicSnapshot, SphericalMantleSnapshot, existing ReliefGenerator::generate_spherical.
- Produces no new public type. It locks the causal identity:

    elevation =
        crust_base
      + tectonic_offset
      + volcanic_offset
      + regional_offset

- [x] **Step 1: Write end-to-end preliminary-height RED**

In spherical_field_driven_relief.rs build a 2,562-cell Earth-radius sphere with default seed 42 and formal current-state generators. Assert:

    #[test]
    fn field_driven_plates_and_crust_form_an_explainable_preliminary_heightmap() {
        let world = build_current_snapshot(42, 2_562);
        assert_eq!(world.surface.fingerprint(), world.surface_fingerprint_before_build);
        world.tectonic.validate_against(&world.surface).unwrap();
        world.relief
            .validate_against(&world.surface, &world.tectonic, &world.mantle)
            .unwrap();
        assert_component_identity(&world.relief);
        assert!(contains_land_and_ocean(&world.relief));
        assert!(convergent_boundaries_have_positive_uplift_signal(&world));
        assert!(subduction_has_arc_above_trench_signal(&world));
        assert!(continental_interior_median_above_oceanic_median(&world));
        assert!(elevation_dynamic_range_m(&world.relief) >= 4_000.0);
    }

    #[test]
    fn current_snapshot_has_no_history_or_geometry_displacement_state() {
        assert_no_serialized_time_axis::<SphericalTectonicSnapshot>();
        assert_no_serialized_time_axis::<SphericalReliefSnapshot>();
        assert_surface_vertices_byte_identical_before_and_after_generation();
    }

- [x] **Step 2: Run RED**

Run:

    cargo test --test spherical_field_driven_relief -- --nocapture

Expected: the new test target is missing. After adding only the test, at least one macro morphology assertion fails against the old production path or the new helpers are unresolved.

- [x] **Step 3: Keep one existing causal height implementation**

Do not add or modify a second height generator. Production remains `ReliefGenerator::generate_spherical`, consuming the new `SphericalTectonicSnapshot` through its existing crust-base, boundary-kinematic, mantle/hotspot, regional, and safety-reconciliation terms. Any failing causal assertion must be corrected in Tasks 3–5; Task 6 does not authorize spherical relief formula, schema, physical-bound, or support-scale changes.

Implement `assert_no_serialized_time_axis<T>()` as a test-only helper that serializes `T` to `serde_json::Value`, recursively visits all object keys, and rejects `history`, `timeline`, `time_slices`, and `previous_state`. Implement the geometry assertion by saving the surface's exact vertex/centroid bit patterns before generation and comparing them afterward.

- [x] **Step 4: Update deterministic sphere matrices**

Run each matrix once with deliberately impossible expected hashes to capture changed values only after all semantic assertions pass:

    cargo test --test spherical_natural_matrix -- --nocapture
    cargo test --test spherical_relief_geology_matrix -- --nocapture

Replace expected hashes with the reported stable values, rerun twice, and require identical output. Do not alter planar hashes.

- [x] **Step 5: Run GREEN and relief adjacency**

Run:

    cargo test --test spherical_field_driven_relief -- --nocapture
    cargo test --test spherical_relief_generation -- --nocapture
    cargo test --test spherical_relief_contracts -- --nocapture
    cargo test --test spherical_relief_geology_matrix -- --nocapture
    cargo test --test spherical_natural_matrix -- --nocapture

Expected: all pass, the preliminary heightmap is current-state, explainable, bounded, and source-bound.

- [x] **Step 6: Mutation checks**

Set tectonic_offset to zero before final reconciliation and confirm convergent_boundaries_have_positive_uplift_signal or subduction_has_arc_above_trench_signal fails. Restore.

Make crust base ignore CrustKind and confirm continental_interior_median_above_oceanic_median fails. Restore.

- [x] **Step 7: Commit**

    git add tests/spherical_field_driven_relief.rs tests/spherical_relief_generation.rs tests/spherical_relief_geology_matrix.rs tests/spherical_natural_matrix.rs
    git commit -m "test: bind spherical morphology to relief"

---

### Task 7: Lock Morphology, Resolution, and Visual Quality

**Files:**

- Create: tests/spherical_morphology_quality.rs
- Modify: tests/spherical_tectonic_generation.rs
- Modify: tests/spherical_presentation_gpu.rs

**Interfaces:**

- Consumes only public authoritative surface, tectonic, relief, and presentation APIs.
- Produces test-only metrics:

    fn normalized_spherical_perimeter(...)
    fn tangent_covariance_aspect_ratio(...)
    fn boundary_radial_variation(...)
    fn major_continental_components(...)
    fn coast_plate_overlap(...)
    fn optimally_matched_owner_agreement(...)
    fn continental_mask_jaccard(...)

- `PRESET_EXPECTATIONS` is a private test table with these exact inclusive major-land component ranges: `Continents = 3..=5`, `Supercontinent = 1..=1`, `Archipelago = 2..=6`, `GreatIsland = 1..=1`, and `VolcanicIslands = 0..=2`. A major land component owns at least 10% of total continental area. For `Continents` and `Supercontinent`, normalized coast perimeter must be `1.35..=3.50`; default `Continents` must have at least one major component with radial variation above `0.18`.

- [x] **Step 1: Write the multi-seed morphology RED**

At 642 cells, default 12 plates, and seeds 0..16, assert the design ranges:

    #[test]
    fn default_plate_morphology_is_varied_without_fragmenting() {
        for seed in 0..16 {
            let metrics = plate_metrics(generate(seed, 642, Continents));
            assert!((2.5..=8.0).contains(&metrics.max_min_area_ratio));
            assert!((0.30..=0.75).contains(&metrics.area_cv));
            assert!((1.15..=2.60).contains(&metrics.median_normalized_perimeter));
            assert!(metrics.plates_below_one_percent == 0);
            assert!(metrics.aspect_ratios.iter().filter(|&&r| r > 1.25).count() * 2 >= 12);
        }
    }

    #[test]
    fn formation_presets_have_distinct_non_round_continental_morphology() {
        for seed in 0..16 {
            assert_preset_metrics(seed, PRESET_EXPECTATIONS);
        }
    }

    #[test]
    fn default_coasts_are_related_to_but_not_locked_to_plate_boundaries() {
        for seed in 0..16 {
            let overlap = coast_plate_overlap(&generate(seed, 642, Continents));
            assert!((0.10..=0.55).contains(&overlap));
        }
    }

- [x] **Step 2: Run RED**

Run:

    cargo test --test spherical_morphology_quality -- --nocapture

Expected: the old uniform Voronoi/current crust baseline fails area diversity, perimeter, radial variation, or overlap. On the new implementation, any failure identifies a bounded recipe/calibration defect rather than authorizing threshold deletion.

- [x] **Step 3: Implement exact metric definitions**

Use true cell areas and shared boundary arc lengths. For equal-area-circle perimeter, solve the spherical cap radius from:

    area = 2 * PI * R^2 * (1 - cos(radius_angle))
    circle_perimeter = 2 * PI * R * sin(radius_angle)

For aspect ratio, project cell centroids to the area-centroid tangent plane and use the square roots of the two weighted covariance eigenvalues.

Define major land as at least 10% of all continental area. Define plate/coast overlap using one median-cell angular-diameter buffer.

- [x] **Step 4: Add Release resolution-invariance RED**

Add an ignored test:

    #[test]
    #[ignore = "release-only 5k/20k morphology resolution gate"]
    fn field_morphology_is_resolution_invariant() {
        let coarse = generate(42, 5_000, Continents);
        let fine = generate(42, 20_000, Continents);
        assert!(perimeter_stat_difference(&coarse, &fine) <= 0.15);
        assert!((0.04..=0.15).contains(&fine_scale_perimeter_gain(&coarse, &fine)));
        assert!(optimally_matched_owner_agreement(&coarse, &fine) >= 0.90);
        assert!(continental_mask_jaccard(&coarse, &fine) >= 0.65);
    }

Run:

    cargo test --release --test spherical_morphology_quality field_morphology_is_resolution_invariant -- --ignored --nocapture

Expected: old cell-scaled crust noise fails at least the mask or perimeter gate.

- [x] **Step 5: Tune recipes only inside the frozen design bounds**

Tune FieldRecipe weights, continuous seed placement, lobe support radii, and bias damping only within the ranges stated in the design spec. Do not weaken a gate merely to accept a known round/equal partition. Record each changed constant and the seed that motivated it in the Execution Evidence section.

- [x] **Step 6: Update GPU goldens after semantic GREEN**

First run:

    $env:SEKAI_REQUIRE_SPHERICAL_GPU='1'
    cargo test --test spherical_presentation_gpu complete_spherical_offscreen_rgba8_goldens_keep_cpu_semantic_oracles -- --nocapture

Expected: semantic oracles pass and audited exact hashes report intentional changes for plate/crust/elevation-related cases.

Set affected expected hashes to the reported values only after checking source/cardinality/scalar/category/edge/vector semantics. Run twice on Vulkan and once with:

    $env:WGPU_BACKEND='gl'

Unknown adapters remain semantic-only-unaudited. Do not add their hashes to the exact allowlist.

- [x] **Step 7: Run visual acceptance**

Launch the Release app with seed 42 and inspect plate ownership, crust kind, crust thickness, elevation, and boundary kind in Equal Earth and globe views. Then inspect seeds 1 through 11 at 20k cells.

Reject and fix:

- near-equal round plate tiling;
- circular continental blobs;
- one-cell coastline noise;
- pole concentration or antimeridian seams;
- continent boundaries identical to plate boundaries;
- height displacement of the unit globe.

Capture the accepted seed/field/view list and observed morphology in Execution Evidence. Dynamic arrows must remain annotations and must not change science data.

- [x] **Step 8: Run GREEN**

Run:

    cargo test --test spherical_morphology_quality -- --nocapture
    cargo test --release --test spherical_morphology_quality field_morphology_is_resolution_invariant -- --ignored --nocapture
    cargo test --test spherical_tectonic_generation -- --nocapture
    $env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test --test spherical_presentation_gpu -- --nocapture

Expected: all pass.

- [x] **Step 9: Commit**

    git add tests/spherical_morphology_quality.rs tests/spherical_tectonic_generation.rs tests/spherical_presentation_gpu.rs src/generators/natural/morphology src/generators/natural/spherical_tectonics
    git commit -m "test: gate spherical morphology quality"

Only include production files if bounded tuning changed them.

---

### Task 8: Lock Performance, Compatibility, and Whole-Graph Acceptance

**Files:**

- Modify: tests/spherical_natural_graph_performance.rs
- Modify: tests/spherical_natural_stage_graph.rs
- Modify: tests/spherical_natural_matrix.rs
- Modify: tests/spherical_relief_geology_matrix.rs
- Modify: tests/spherical_relief_geologic_stage.rs
- Modify: src/app/spherical_natural_display.rs (test-only frozen field hash)
- Modify: docs/superpowers/plans/2026-08-10-field-driven-spherical-terrain.md

**Interfaces:**

- Consumes the formal spherical_natural_foundation_graph and existing product default inputs.
- Produces release timing/memory evidence and final verification evidence only.

- [x] **Step 1: Add per-stage morphology timing and memory RED**

Extend the ignored Release test to record:

    struct MorphologyPerformanceEvidence {
        tectonic_elapsed: Duration,
        full_graph_elapsed: Duration,
        morphology_peak_delta_bytes: u64,
        cell_count: usize,
        plate_count: usize,
    }

Assert:

    assert_eq!(evidence.cell_count, 20_252);
    assert_eq!(evidence.plate_count, 12);
    assert!(evidence.tectonic_elapsed <= Duration::from_millis(300));
    assert!(evidence.full_graph_elapsed <= Duration::from_secs(5));
    assert!(evidence.full_graph_elapsed.as_secs_f64() <= baseline_seconds * 1.25);
    assert!(evidence.morphology_peak_delta_bytes <= 64 * 1024 * 1024);

Use the untouched same-machine baseline captured in Task 1 Step 2 from commit f00466ce with the exact same Release configuration, seed 42, and inputs. Do not recapture it after production code has changed and do not hard-code its machine-specific duration into a reusable assertion; pass/read the recorded value in the ignored acceptance harness.

- [x] **Step 2: Run Release RED or confirm budget**

Run:

    cargo test --release --test spherical_natural_graph_performance -- --ignored --nocapture

Expected: compilation fails until evidence fields are added, then the new implementation must satisfy the budgets. A budget failure requires profiling and algorithmic correction; do not delete the gate.

- [x] **Step 3: Optimize only measured hotspots**

Allowed optimizations:

- reuse ArrivalWorkspace allocations across six rounds;
- store quantized fields as i16 and edge metrics as u64;
- scope/drop plate fields before crust fields;
- replace repeated full scans with maintained area totals;
- reserve heap/vector capacity from V/E cardinality.

Forbidden optimizations:

- reducing 20k science resolution;
- replacing field morphology with old Voronoi;
- skipping validation;
- caching build intermediates in the published document;
- changing visual LOD to hide scientific defects.

- [x] **Step 4: Run focused compatibility and source-identity gates**

Run:

    cargo test --test legacy_planar_boundary -- --nocapture
    cargo test --test natural_display_golden -- --nocapture
    cargo test --test spherical_natural_stage_graph -- --nocapture
    cargo test --test spherical_natural_matrix -- --nocapture
    cargo test --test spherical_field_driven_relief -- --nocapture
    cargo test --test spherical_morphology_quality -- --nocapture

Expected: all pass; planar hashes remain frozen, sphere stage version/cache hashes are intentionally updated.

- [x] **Step 5: Run engineering gates**

Run serially:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --all-features
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo check --target wasm32-unknown-unknown --workspace --all-features
    cargo test --workspace --all-targets --all-features
    cargo test --workspace --doc
    $env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test
    git diff --check

Expected: every command exits 0. GPU-required tests must not report a skip.

- [x] **Step 6: Perform final boundary audit**

Run:

    rg -n "Serialize|Deserialize|Artifact|pub fn|pub struct" src/generators/natural/morphology src/generators/natural/spherical_tectonics
    rg -n "height|elevation|displace" src/gpu/spherical src/view/spherical_mesh.rs
    rg -n "legacy_planar|PlanarSpace|SpatialSnapshot" src/generators/natural/spherical_tectonics.rs src/generators/natural/spherical_tectonics
    rg -n "history|timeline|time_slice|previous_state" src/generators/natural/morphology src/generators/natural/spherical_tectonics

Expected:

- no public or serialized morphology intermediate;
- no globe elevation displacement path;
- no planar fallback in spherical generation;
- no historical state;
- only approved pub(super) domain interfaces.

- [x] **Step 7: Append Execution Evidence**

Append exact commits, RED failures, mutation failures, focused counts, new sphere hashes, Release timings, memory, visual seed review, GPU adapter/backend, wasm result, and full-suite elapsed times to this plan. Do not claim an unobserved result.

- [x] **Step 8: Commit final acceptance**

    git add tests/spherical_natural_graph_performance.rs tests/spherical_natural_stage_graph.rs tests/spherical_natural_matrix.rs tests/spherical_relief_geology_matrix.rs tests/spherical_relief_geologic_stage.rs src/app/spherical_natural_display.rs docs/superpowers/plans/2026-08-10-field-driven-spherical-terrain.md
    git commit -m "test: lock field-driven spherical terrain"

- [x] **Step 9: Verify clean completion**

Run:

    git status --short
    git log -10 --oneline

Expected: tracked worktree clean, the design and plan commits are present, all implementation commits are ordered by task, and no ignored report is accidentally tracked.

---

## Execution Evidence

Observed evidence is appended here during execution: exact RED/GREEN commands, mutation failures, hashes, timings, visual review results, and commit IDs. Every required behavior is specified above.

### Untouched Release Baseline

- Baseline production commit: `f00466ce03f89ace0fbee7bacf05883265dec8d0`; later commits before capture contain documentation only.
- Command: `cargo test --release --test spherical_natural_graph_performance -- --ignored --nocapture`.
- Result: exit 0; one ignored Release gate passed; test body 4.35 s after a 19.86 s optimized build.
- Seed/input: existing product default fixture, 20,252 cells, 12 plates, 16 stages.
- Spherical full graph: 1,418.187 ms. Spherical tectonics stage: 171.205 ms.
- Persistent sphere artifacts: 22,655,488 bytes. Baseline-to-final working-set delta: 25,923,584 bytes. Measured additional peak over the recorded planar peak: 0 bytes; both peaks were 50,470,912 bytes.
- This capture is the fixed same-machine denominator for the final 1.25-times full-graph budget: 1,772.734 ms.

### Task 1 — Spherical Scalar Field

- RED: both focused commands exited 1 because `FieldBand`, `FieldRecipe`, `QuantizedScalarField`, `sample_spherical_field`, and `SPHERICAL_MORPHOLOGY_LABELS` did not exist.
- GREEN: field 2/2, morphology random-stream contract 1/1, legacy `relief_noise` 5/5, all random tests 8/8, and `natural_display_golden` 2 passed/1 expected ignored.
- Root-cause correction: the first seam oracle compared the maximum of 275 sensitive edges against global P95. Measurements showed global median/P95 `0.058535/0.171453`, cut median/P95 `0.068484/0.161748`, and pole median/P95 `0.057253/0.204291`; the gate now compares regional median/P95 distributions and still catches coordinate discontinuities without treating a natural tail edge as a seam.
- Engineering evidence: `cargo fmt --check`, library strict Clippy with all features, and `git diff --check` exited 0.

### Task 2 — Positive Metric and Unified Arrivals

- RED: metric and arrival focused commands each exited 1 on missing `PositiveEdgeMetric`, `ArrivalSource`, `ArrivalWorkspace`, `assign_arrivals`, field test construction, and dense topology edge-cost access.
- GREEN: morphology 7/7 and topology 9/9; `legacy_planar_boundary` 2/2; `natural_display_golden` 2 passed/1 expected ignored; library strict Clippy, fmt, and diff checks exited 0.
- Mutation: setting the fabric coefficient to zero made the independent fabric assertion fail; the initial implementation restored `0.35`, and Task 7's unchanged quality gates later bounded-calibrated it to `1.50`.
- Mutation: changing only heap order did not change ownership because the authoritative tie-break is the relaxation tuple. Reversing that actual owner comparison changed the frozen square from `[0,0,0,1]` to `[0,1,1,1]` and failed the test; restored to lower-owner-first.

### Task 3 — Field-Driven Plate Partition

- RED: focused plate command exited 1 because `PlatePartition`, area-target generation, and field-driven partition generation did not exist.
- GREEN: plate morphology 6/6, including exact one-billion target normalization, target ratio, seed ownership/connectivity, non-uniform sphere perimeter/area variation, 2,562-cell 35% calibration, activity orthogonality, exact field influence, and best-round retention. Existing spherical rotation integration remained GREEN 1/1.
- Mutation: replacing the resistance/fabric metric with raw topology lengths made the same-seed/same-target final ownership test equal its base replay and fail; restored.
- Mutation: publishing the last valid calibration round instead of the best caused the fixed rebound oracle to fail; restored. The observer records only scalar errors and production supplies an empty closure, so no historical partition snapshots exist.
- Engineering evidence: library strict Clippy, fmt, and diff checks exited 0.

### Task 4 - Independent Continental Affinity and Crust

- RED: the area-mask target first failed to compile on missing `build_area_constrained_mask` and `ProtectedRegionSeed`; the protected-neck regression then failed on the missing coast-shrink boundary. The crust target failed to compile on missing `CrustMorphology` and `generate_crust_observed`.
- GREEN: area 4/4 and crust 4/4. The five formation presets meet the one-authoritative-cell area tolerance and their major-component ranges at 642 cells. The fixed Continents case keeps coast/plate overlap inside 10%-55%, retains identical base affinity and anchor layout across 12/17 plates, changes only the soft-coupled final affinity/mask, and keeps independent thickness inside existing physical bounds.
- Root-cause correction: the first GREEN attempt overflowed `i32` while scaling quantized affinity; widening the multiplication to `i64` fixed the exact failing boundary. The next run showed protected island components could be surrounded by an earlier component; preventing different protected growth fronts from entering each other's immediate neighborhood preserved their budgets without weakening morphology gates.
- Mutation: setting the initial plate-interior coefficient from 0.15 to zero made the 12/17-plate soft-coupling oracle fail with identical final affinity; Task 7's resolution gate later bounded-calibrated it to `0.10`. Reusing the Continents affinity recipe and affinity seed for oceanic thickness made the independent oceanic rank-order assertion fail. Both dependency mutations were restored.
- Adjacency and engineering evidence: the pre-integration global spherical-area formation test remained GREEN; fresh area/crust tests, library strict Clippy, fmt, and diff checks exited 0.

### Task 5 — Orthogonal Tectonic Integration

- Commit: `2cb9327 feat: publish field-driven spherical tectonics`.
- The spherical stage now owns only orchestration; plate partition, crust, rigid motion, and boundary classification live in separate crate-private modules and publish the existing `SphericalTectonicSnapshot` contract.
- `SphericalTectonicStage` alone advanced to version 2. The formal graph remained exactly 16 stages, source identity stayed surface-bound, and failed candidates never published a partial snapshot.
- Focused spherical tectonic, stage-graph, mantle, relief, climate, and hydrology adjacency suites passed before commit. The legacy planar module and graph were unchanged.

### Task 6 — Preliminary Heightmap Causality

- Commit: `927d74a test: bind spherical morphology to relief`.
- RED proved that plate/crust morphology was not yet locked to the published preliminary height fields. GREEN binds `crustal_freeboard`, boundary kinematic response, mantle/hotspot response, regional relief, and hydrology/erosion into the existing current-state scalar elevation path.
- `spherical_field_driven_relief` checks that changing crust kind/thickness or boundary kinematics changes the authoritative height scalar while preserving source/cardinality and deterministic same-input output. Removing either causal input makes the focused oracle fail.
- No history slice, accumulated simulation state, alternate height model, or vertex displacement was introduced. The 2D map and unit globe consume the same scalar field.

### Task 7 — Morphology, Resolution, GPU, and Visual Quality

- Commit: `26f3699 test: gate spherical morphology quality`.
- RED evidence: the old resolution-dependent seed shortlist moved a fixed plate direction by about `1.57 rad` between 642 and 2,562 cells. After continuous direction placement, the no-detail 5k/20k partition had `fine_scale_gain=-0.0059`, proving that the 20k mesh exposed no additional field morphology. A temporary 6% visual threshold then correctly rejected the best bounded calibration at 4.55%, forcing direct image review rather than silent acceptance.
- Review-driven calibration kept every frozen quality threshold and changed only bounded recipe constants. Plate target jitter is `±10%`; for counts of at least eight, the deterministic target-CV floor is `0.36`. Fabric crossing is `1.50` inside the unchanged positive-cost clamp; soft plate-interior affinity is `0.10`; local frontier cohesion scales with physical feature radius at `80,000`. Mutations to the superseded values failed the existing seed or 5k/20k gates: fabric `1.00` left only `4/12`, `5/12`, and `5/12` elongated plates for seeds 0, 10, and 15; interior `0.15` produced `15.04%` coast-resolution drift; cohesion zero produced `23.32%`; target `±20%` failed seed 4 area variation and seed 9 elongation. A review RED found `plate_count=8, seed=166` at CV `0.359986` after the old eight-pass cap; the production path now allows at most 16 bounded passes and verifies both floating and integer-normalized postconditions. `[8,12,32,64] × 1024 seeds` is GREEN. A second RED proved that applying this large-count floor to supported counts `2..7` changed their intended recipe; calibration is now gated at the call site, and all six small counts retain at least one sub-`0.36` deterministic case.
- The coast shrink hotspot was replaced with one inward-rooted spanning forest and a stable coastal-leaf priority queue. It builds connectivity once, removes only leaves, and updates only the parent and graph neighbors, giving `O(E log V)` behavior while preserving protected seeds and narrow necks. A mutation that rebuilt connectivity per deletion was caught by the exact scan-count regression. Full builders also prove all remaining enclosed holes exceed the preset physical hole limit.
- DRY boundaries are explicit: common fractal-band data and the single shared octave capacity live in `natural/fractal.rs`; stable union-find and canonical plate pairs live in `natural/connectivity.rs`; spherical field, plate, crust, motion, boundary, and relief modules depend one way on those neutral primitives. No generator publishes or serializes morphology intermediates.
- Multi-seed GREEN: plate tests 10/10 and `spherical_morphology_quality` 3 passed/1 expected Release ignore. Seeds 0–15 plus 42 have max/min plate area ratios `3.23..4.66`, achieved area CV `0.3386..0.3907`, target CV at least `0.36`, `6..11` elongated plates out of 12, no plate below 1%, and buffered coast/plate overlap `0.2381..0.4014`. All five formation presets pass component, area, perimeter, radial-variation, no-speckle, and no-small-hole gates; the 42-cell VolcanicIslands case remains within one authoritative cell of its requested land fraction.
- Release resolution GREEN: 4,842 versus 20,252 cells; normalized plate perimeter `1.4021 -> 1.4739` (`+5.13%` bounded detail), total normalized coast drift `13.01%`, plate-area total variation `0.0023`, optimal owner agreement `0.9323`, continental-mask Jaccard `0.6658`, exact land fraction `0.3800/0.3800`, and major continental components `3/3`.
- GPU semantics were checked before hashes. The source-keyed Medium glyph IDs intentionally changed to `[4,11,14,26,28,38,79,97,108,115,119,131,133,144,150]`; all fill, edge, vector, seam, pole, and front/back CPU oracles passed before accepting pixel changes. RTX 4080 SUPER Vulkan and audited OpenGL then produced identical 16 RGBA8 hashes. In order: map/globe scalar `fea586c9…aa5f` / `4d338ddb…f6eb`; category `416d14cb…9f0b` / `b52256cc…3d97`; edge scalar `3a8b6843…9507` / `f608f893…611b`; edge category `72e3f45c…6b25` / `62f302d7…8f7d`; vector paused map/globe `69a650e8…5850` / `29b572ac…5c55`; vector animated `7e5659ad…e4f1` / `29c8dcdd…fadd`; seam `3c040b4a…f7f6`; poles `47865d0f…9bc0`; globe front/back `b52256cc…3d97` / `cd11c5b2…0629`.
- Visual acceptance covered the complete matrix, not a seed shortcut: seeds 1–11 plus 42 × plate ownership, crust kind, crust thickness, current elevation, and boundary kind × Equal Earth and globe = `120/120` Release views at 20,252 cells. The views show distinct connected plate fabrics, compound continents, bays/necks/islands, independently varying thickness, ridges, trenches, arcs, and boundary mountain belts without antimeridian breaks, pole concentration, one-cell checkerboards, or a reused template. Both views consume the same scalar heightmap; the globe remained a perfectly round unit sphere and dynamic arrows remained annotations.

### Task 8 — Performance and Whole-Graph Acceptance

- Final implementation and acceptance fix commit: `b2b17786f654b10da0cad172a7427e6ac02c7f30` (`fix: harden field-driven spherical terrain`). The tracked worktree was clean immediately after this commit; the follow-up documentation commit records that observed state.
- Performance-contract RED: the ignored Release target failed to compile on the deliberately missing `collect_morphology_performance_evidence`, proving the new stage/full-graph/memory assertions preceded their implementation.
- Memory evidence is isolated in a child process selected only by exact sentinel value `SEKAI_MORPHOLOGY_PROBE_CHILD=1`; it measures the morphology build against its immediate resident baseline so allocator reuse from an earlier planar graph cannot hide temporary allocations. The untouched baseline remains an environment-supplied `1418.187 ms`, never a reusable hard-coded machine assertion.
- Final Release GREEN: 20,252 cells, 12 plates, 16 stages; planar graph `1886.178 ms`, spherical graph `1446.765 ms`, spherical tectonics `228.008 ms`, isolated morphology peak delta `8,605,696 bytes`, and persistent spherical artifacts `22,653,084 bytes`. These are below the fixed `1772.734 ms`, `300 ms`, and `64 MiB` budgets without lowering resolution or retaining build intermediates.
- Compatibility RED/GREEN refreshed only values causally downstream of the new current-state morphology after semantic checks. The formal stage hashes are surface `213c897c…00ec`, tectonic `36b99b6e…b972`, mantle `c0213d96…39e`, relief `5f0f0e64…1ba4`, geology `04bd2c7e…06e8`, climate `c7c000a8…f82b`, hydrology `7053d125…2538`, result `dd47d161…9452`; stage graph 5/5 and all four tectonic and relief/geology matrix cases pass. The 2- and 7-plate matrix hashes were refreshed only after their scientific assertions passed; 12- and 64-plate hashes stayed unchanged. The 36-field catalog/payload freeze passes at `8b0c948c2ac7c5961c9af0a417fffb434abf9cbd42eef9195850affaaff84e85`, and the real diagnostic-forwarding fixture remains fixed at seed 542.
- Focused final results: area 6/6, plate 10/10, crust 9 passed/1 expected ignored, morphology quality 3 passed/1 expected ignored, stage graph 5/5, natural matrix 1/1, relief/geology matrix 1/1, relief/geologic stage 3/3, spherical document 24/24, and both audited GPU backends 5/5.
- Final engineering gates all exited 0: fmt, workspace/all-target/all-feature check, strict Clippy with `-D warnings`, wasm32 all-features check, workspace/all-target/all-feature tests, workspace docs, required-GPU full tests, and diff check. The final workspace/all-target/all-feature rerun finished in `236.2 s`; docs passed 5 with 8 expected ignores; `$env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test` finished in `177.3 s` with 373 library tests and no GPU skip.
- Final boundary audit found no public/serialized morphology intermediate, planar fallback, historical state, or time slice in the new generator modules. Globe `height/elevation/displace` matches are viewport dimensions, exclusion documentation, and the negative geometry-invariance test; no scalar elevation enters unit-sphere vertex construction. The authoritative application still publishes one current snapshot through the single spherical stage graph.
- Full-suite diagnostic: the first all-target run reached only the old source-keyed glyph-ID and RGBA8 expectations after every CPU semantic oracle had passed. Updating those intentional presentation freezes, then rerunning Vulkan, OpenGL, the complete workspace, and the required-GPU suite produced the clean results above; no production rendering change was needed.
