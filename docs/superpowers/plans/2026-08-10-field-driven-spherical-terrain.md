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

- [ ] **Step 1: Write field and random-stream RED tests**

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

- [ ] **Step 2: Capture the untouched Release baseline**

Before any production edit, run from commit f00466ce's current behavior:

    cargo test --release --test spherical_natural_graph_performance -- --ignored --nocapture

Record the exact 20,252-cell full-graph duration, persistent bytes, peak working-set delta, cell count, plate count, command, machine/backend, and baseline commit in Execution Evidence. This is the denominator for Task 8's 1.25-times budget.

- [ ] **Step 3: Run RED**

Run:

    cargo test --lib generators::natural::morphology::field -- --nocapture
    cargo test --lib generators::natural::random::tests::spherical_morphology_substreams_are_pairwise_orthogonal -- --nocapture

Expected: compilation fails because morphology, the field types, sampler, recipes, and V2 labels do not exist.

- [ ] **Step 4: Extract the shared 3D coherent-noise core**

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

- [ ] **Step 5: Implement sampling, normalization, resolution filtering, and quantization**

Use point = cell.centroid.components(). For each retained band, sample CoherentNoise3d at point / angular_scale_rad, apply Smooth or Ridged shape, and combine integer milli-weights. Drop a band when its angular scale is less than four times the median equivalent cell angular diameter.

Normalize using cell.area weights:

    mean = sum(value_i * area_i) / sum(area_i)
    variance = sum((value_i - mean)^2 * area_i) / sum(area_i)
    normalized = clamp((value_i - mean) / sqrt(variance), -clamp_sigma, clamp_sigma)
    quantized = round(normalized / clamp_sigma * i16::MAX)

Reject empty surfaces, invalid recipes, non-finite samples, zero variance, and cardinality mismatch with typed MorphologyFieldError variants.

- [ ] **Step 6: Run GREEN and legacy adjacency**

Run:

    cargo test --lib generators::natural::morphology::field -- --nocapture
    cargo test --lib generators::natural::relief_noise -- --nocapture
    cargo test --lib generators::natural::random -- --nocapture
    cargo test --test natural_display_golden -- --nocapture

Expected: all pass; planar and existing spherical relief noise results remain unchanged.

- [ ] **Step 7: Commit**

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

- [ ] **Step 1: Write metric and arrival RED tests**

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

- [ ] **Step 2: Run RED**

Run:

    cargo test --lib generators::natural::morphology::metric -- --nocapture
    cargo test --lib generators::natural::morphology::arrival -- --nocapture

Expected: compilation fails on missing metric and arrival modules/types.

- [ ] **Step 3: Implement PositiveEdgeMetric**

For each EdgeId, read its two owners and base traversal length. Compute resistance as the endpoint mean in -1..1. Compute fabric slope as absolute endpoint difference divided by traversal length; normalize all slopes by twice their length-weighted RMS and clamp to 0..1.

Use fixed-point multiplication for:

    multiplier = clamp(1 + 0.45 * resistance + 0.35 * fabric_crossing, 0.45, 2.20)
    cost = max(1, round(base_cost * multiplier))

Store one cost per EdgeId. Validate field and edge cardinality before allocation.

- [ ] **Step 4: Implement the one arrival heap**

Use a min-heap ordering encoded through reverse Ord. Compare entries by total cost, then owner, then CellId. Initialize sources after subtracting their minimum initial cost so all values are non-negative. Reject duplicate source cells, duplicate owner IDs, out-of-range cells, empty sources, and addition overflow.

Replace topology.rs internal propagate body with a call to the unified core using the base metric. Preserve current GraphAssignment public(super) shape and exact tie ordering.

- [ ] **Step 5: Run GREEN, topology, and planar golden tests**

Run:

    cargo test --lib generators::natural::morphology -- --nocapture
    cargo test --lib generators::natural::topology -- --nocapture
    cargo test --test legacy_planar_boundary -- --nocapture
    cargo test --test natural_display_golden -- --nocapture

Expected: all pass and the frozen planar hashes do not change.

- [ ] **Step 6: Perform mutation checks**

Temporarily remove the fabric term and confirm fabric_and_resistance_change_routes_but_keep_positive_symmetric_costs fails. Restore it.

Temporarily reverse the owner tie-break and confirm legacy_multi_source_helpers_keep_their_exact_outputs fails. Restore it.

- [ ] **Step 7: Commit**

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

- [ ] **Step 1: Write target, seed, shape, and calibration RED tests**

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

- [ ] **Step 2: Run RED**

Run:

    cargo test --lib generators::natural::spherical_tectonics::plates -- --nocapture

Expected: compilation fails because the plates submodule and PlatePartition contract do not exist.

- [ ] **Step 3: Implement target weights and non-uniform seed placement**

Generate a stable rank profile from 0.55 to 1.90 for plate_count >= 8, apply bounded ±20% random perturbations, shuffle the target values using the same target-area stream, and renormalize to the existing total area-weight quantization.

Place targets largest first. For each candidate cell calculate:

    separation_score =
        min_j(metric_distance(candidate, seed_j) / (sqrt(target_i) + sqrt(target_j)))
        + seed_preference(candidate) * 0.12

Keep candidates satisfying the minimum separation, sort the best 5% by score descending and CellId ascending, and use the placement stream to choose within that band. Map selected positions back to stable PlateId order.

- [ ] **Step 4: Implement field metric and six-round bias calibration**

Sample the fixed plate resistance and fabric recipes from their dedicated streams. Build PositiveEdgeMetric once. Reuse one ArrivalWorkspace for all rounds.

Let S be median nearest-seed metric distance. Update each signed bias by:

    error_i = (actual_i - target_i) / target_i
    delta_i = clamp(0.35 * S * error_i, -0.12 * S, 0.12 * S)
    bias_i = clamp(bias_i + delta_i, -0.60 * S, 0.60 * S)

Shift all signed biases by the common minimum before constructing ArrivalSource. Reject a round when a seed loses itself, a plate becomes empty, or connectivity validation fails. Stop after six rounds or two improvements below 0.005. Return the valid round with the lowest maximum relative error.

- [ ] **Step 5: Run GREEN and existing spherical motion tests**

Run:

    cargo test --lib generators::natural::spherical_tectonics::plates -- --nocapture
    cargo test --test spherical_tectonic_generation spherical_rotations_are_repeatable_bounded_connected_and_locally_separated -- --nocapture

Expected: plate unit tests pass. The existing integration test may still use the old production partition until Task 5, but it must compile.

- [ ] **Step 6: Mutation checks**

Set resistance and fabric coefficients to zero and confirm field_driven_partition_is_connected_and_not_uniform_voronoi fails. Restore.

Disable best-round retention and confirm six_bias_rounds_keep_the_best_valid_area_fit fails on the fixed seed. Restore.

- [ ] **Step 7: Commit**

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

- [ ] **Step 1: Write generic area-mask RED tests**

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
    fn coast_rebalance_never_removes_a_protected_articulation_cell() {
        let mask = build_narrow_neck_fixture();
        assert!(mask.is_selected(NECK_CELL));
        assert_eq!(connected_major_components(&mask), EXPECTED_COMPONENTS);
    }

- [ ] **Step 2: Run area RED**

Run:

    cargo test --lib generators::natural::morphology::area -- --nocapture

Expected: compilation fails because area.rs and its types do not exist.

- [ ] **Step 3: Implement area growth and cleanup**

Use one max-heap keyed by score descending, component ID, then CellId. A cell can enter a protected component only from an already selected neighbor of that component. Grow protected budgets first, then island seeds.

Label selected and unselected components with iterative queues. Remove undersized unprotected selected components. Fill enclosed unselected components below maximum_hole_weight. Before each coast-shrink batch, compute articulation points for each protected component with iterative Tarjan discovery/low-link arrays; only remove non-articulation shore cells. Recompute after each batch, and stop at the closest achievable area prefix.

- [ ] **Step 4: Write crust RED tests**

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

- [ ] **Step 5: Run crust RED**

Run:

    cargo test --lib generators::natural::spherical_tectonics::crust -- --nocapture

Expected: compilation fails because crust.rs, recipes, lobe kernels, and CrustMorphology do not exist.

- [ ] **Step 6: Implement preset fields, static lobe clusters, and thickness**

Implement the exact recipe and preset tables from the design spec. Derive local plate-interior preference from distance to plate boundary divided by target equivalent radius and clamp to 0..1.

Use Wendland C2:

    if q < 1.0 {
        (1.0 - q).powi(4) * (4.0 * q + 1.0)
    } else {
        0.0
    }

Place lobe centers along neighbors with minimum absolute fabric-field change. Choose separated island local maxima from the final affinity. Convert selected mask to CrustKindField.

Generate thickness from CRUST_THICKNESS_FIELD_LABEL. Continental thickness adds normalized distance-to-coast modulation; oceanic thickness uses only its independent meso field. Clamp to existing continental and oceanic min/max constants.

- [ ] **Step 7: Run GREEN and formation adjacency**

Run:

    cargo test --lib generators::natural::morphology::area -- --nocapture
    cargo test --lib generators::natural::spherical_tectonics::crust -- --nocapture
    cargo test --test spherical_tectonic_generation every_formation_preset_uses_global_spherical_area_and_plate_independent_crust -- --nocapture

Expected: new unit tests pass. The existing integration test still compiles; its old exact plate-independence assertion is replaced during Task 5.

- [ ] **Step 8: Mutation checks**

Replace affinity with pure nearest-anchor distance and confirm continent_field_is_related_to_but_not_equal_to_plate_ownership or the radial-variation oracle fails. Restore.

Reuse affinity as thickness and confirm thickness_uses_an_independent_field_and_stays_in_physical_ranges fails. Restore.

- [ ] **Step 9: Commit**

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

- [ ] **Step 1: Write orchestration RED tests**

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

- [ ] **Step 2: Run RED**

Run:

    cargo test --test spherical_tectonic_generation every_formation_preset_uses_global_area_and_soft_plate_coupling -- --nocapture
    cargo test --lib generators::natural::spherical_tectonics::tests::facade_keeps_domain_modules_orthogonal -- --nocapture

Expected: old crust is exactly independent of plate count and the facade still owns all logic.

- [ ] **Step 3: Move existing motion and boundary logic without behavior edits**

Move EULER_POLES, assign_plate_rotations, rotation candidates, velocity helpers, and their tests to motion.rs.

Move BoundaryEventDraft, classify_and_aggregate_boundaries, aggregation compatibility, and their tests to boundaries.rs.

Keep visibility pub(super) only where spherical_tectonics.rs needs it. Do not create a generic tectonic policy trait.

- [ ] **Step 4: Replace the spherical orchestration**

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

- [ ] **Step 5: Bump only SphericalTectonicStage to version 2**

Change:

    fn version(&self) -> u32 {
        2
    }

Update direct test StageIdentity values that intentionally reproduce the production spherical tectonic stream. Do not change planar TectonicStage, mantle, or relief versions.

Update graph invalidation expectations: an unchanged second build hits all stages; changing tectonic input misses spherical tectonics and downstream stages; surface and resolved formation remain independently cacheable.

- [ ] **Step 6: Run GREEN focused and adjacent suites**

Run:

    cargo test --lib generators::natural::spherical_tectonics -- --nocapture
    cargo test --test spherical_tectonic_generation -- --nocapture
    cargo test --test spherical_tectonic_contracts -- --nocapture
    cargo test --test spherical_tectonic_mantle_stage -- --nocapture
    cargo test --test spherical_natural_stage_graph -- --nocapture
    cargo test --test legacy_planar_boundary -- --nocapture
    cargo test --test natural_display_golden -- --nocapture

Expected: all pass; only sphere hashes/fixtures intentionally change.

- [ ] **Step 7: Verify output/source/atomic boundaries**

Add assertions that same cell count on a different SurfaceRef is rejected, every output array shares exact sphere cardinality, failed morphology does not publish a SphericalTectonicArtifact, and no morphology intermediate implements Artifact or Serialize.

- [ ] **Step 8: Commit**

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

- [ ] **Step 1: Write end-to-end preliminary-height RED**

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

- [ ] **Step 2: Run RED**

Run:

    cargo test --test spherical_field_driven_relief -- --nocapture

Expected: the new test target is missing. After adding only the test, at least one macro morphology assertion fails against the old production path or the new helpers are unresolved.

- [ ] **Step 3: Keep one existing causal height implementation**

Do not add or modify a second height generator. Production remains `ReliefGenerator::generate_spherical`, consuming the new `SphericalTectonicSnapshot` through its existing crust-base, boundary-kinematic, mantle/hotspot, regional, and safety-reconciliation terms. Any failing causal assertion must be corrected in Tasks 3–5; Task 6 does not authorize spherical relief formula, schema, physical-bound, or support-scale changes.

Implement `assert_no_serialized_time_axis<T>()` as a test-only helper that serializes `T` to `serde_json::Value`, recursively visits all object keys, and rejects `history`, `timeline`, `time_slices`, and `previous_state`. Implement the geometry assertion by saving the surface's exact vertex/centroid bit patterns before generation and comparing them afterward.

- [ ] **Step 4: Update deterministic sphere matrices**

Run each matrix once with deliberately impossible expected hashes to capture changed values only after all semantic assertions pass:

    cargo test --test spherical_natural_matrix -- --nocapture
    cargo test --test spherical_relief_geology_matrix -- --nocapture

Replace expected hashes with the reported stable values, rerun twice, and require identical output. Do not alter planar hashes.

- [ ] **Step 5: Run GREEN and relief adjacency**

Run:

    cargo test --test spherical_field_driven_relief -- --nocapture
    cargo test --test spherical_relief_generation -- --nocapture
    cargo test --test spherical_relief_contracts -- --nocapture
    cargo test --test spherical_relief_geology_matrix -- --nocapture
    cargo test --test spherical_natural_matrix -- --nocapture

Expected: all pass, the preliminary heightmap is current-state, explainable, bounded, and source-bound.

- [ ] **Step 6: Mutation checks**

Set tectonic_offset to zero before final reconciliation and confirm convergent_boundaries_have_positive_uplift_signal or subduction_has_arc_above_trench_signal fails. Restore.

Make crust base ignore CrustKind and confirm continental_interior_median_above_oceanic_median fails. Restore.

- [ ] **Step 7: Commit**

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

- [ ] **Step 1: Write the multi-seed morphology RED**

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

- [ ] **Step 2: Run RED**

Run:

    cargo test --test spherical_morphology_quality -- --nocapture

Expected: the old uniform Voronoi/current crust baseline fails area diversity, perimeter, radial variation, or overlap. On the new implementation, any failure identifies a bounded recipe/calibration defect rather than authorizing threshold deletion.

- [ ] **Step 3: Implement exact metric definitions**

Use true cell areas and shared boundary arc lengths. For equal-area-circle perimeter, solve the spherical cap radius from:

    area = 2 * PI * R^2 * (1 - cos(radius_angle))
    circle_perimeter = 2 * PI * R * sin(radius_angle)

For aspect ratio, project cell centroids to the area-centroid tangent plane and use the square roots of the two weighted covariance eigenvalues.

Define major land as at least 10% of all continental area. Define plate/coast overlap using one median-cell angular-diameter buffer.

- [ ] **Step 4: Add Release resolution-invariance RED**

Add an ignored test:

    #[test]
    #[ignore = "release-only 5k/20k morphology resolution gate"]
    fn field_morphology_is_resolution_invariant() {
        let coarse = generate(42, 5_000, Continents);
        let fine = generate(42, 20_000, Continents);
        assert!(perimeter_stat_difference(&coarse, &fine) <= 0.15);
        assert!(optimally_matched_owner_agreement(&coarse, &fine) >= 0.65);
        assert!(continental_mask_jaccard(&coarse, &fine) >= 0.75);
    }

Run:

    cargo test --release --test spherical_morphology_quality field_morphology_is_resolution_invariant -- --ignored --nocapture

Expected: old cell-scaled crust noise fails at least the mask or perimeter gate.

- [ ] **Step 5: Tune recipes only inside the frozen design bounds**

Tune FieldRecipe weights, seed preference coefficient, lobe support radii, and bias damping only within the ranges stated in the design spec. Do not weaken a gate merely to accept a known round/equal partition. Record each changed constant and the seed that motivated it in the Execution Evidence section.

- [ ] **Step 6: Update GPU goldens after semantic GREEN**

First run:

    $env:SEKAI_REQUIRE_SPHERICAL_GPU='1'
    cargo test --test spherical_presentation_gpu complete_spherical_offscreen_rgba8_goldens_keep_cpu_semantic_oracles -- --nocapture

Expected: semantic oracles pass and audited exact hashes report intentional changes for plate/crust/elevation-related cases.

Set affected expected hashes to the reported values only after checking source/cardinality/scalar/category/edge/vector semantics. Run twice on Vulkan and once with:

    $env:WGPU_BACKEND='gl'

Unknown adapters remain semantic-only-unaudited. Do not add their hashes to the exact allowlist.

- [ ] **Step 7: Run visual acceptance**

Launch the Release app with seed 42 and inspect plate ownership, crust kind, crust thickness, elevation, and boundary kind in Equal Earth and globe views. Then inspect seeds 1 through 11 at 20k cells.

Reject and fix:

- near-equal round plate tiling;
- circular continental blobs;
- one-cell coastline noise;
- pole concentration or antimeridian seams;
- continent boundaries identical to plate boundaries;
- height displacement of the unit globe.

Capture the accepted seed/field/view list and observed morphology in Execution Evidence. Dynamic arrows must remain annotations and must not change science data.

- [ ] **Step 8: Run GREEN**

Run:

    cargo test --test spherical_morphology_quality -- --nocapture
    cargo test --release --test spherical_morphology_quality field_morphology_is_resolution_invariant -- --ignored --nocapture
    cargo test --test spherical_tectonic_generation -- --nocapture
    $env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test --test spherical_presentation_gpu -- --nocapture

Expected: all pass.

- [ ] **Step 9: Commit**

    git add tests/spherical_morphology_quality.rs tests/spherical_tectonic_generation.rs tests/spherical_presentation_gpu.rs src/generators/natural/morphology src/generators/natural/spherical_tectonics
    git commit -m "test: gate spherical morphology quality"

Only include production files if bounded tuning changed them.

---

### Task 8: Lock Performance, Compatibility, and Whole-Graph Acceptance

**Files:**

- Modify: tests/spherical_natural_graph_performance.rs
- Modify: docs/superpowers/plans/2026-08-10-field-driven-spherical-terrain.md

**Interfaces:**

- Consumes the formal spherical_natural_foundation_graph and existing product default inputs.
- Produces release timing/memory evidence and final verification evidence only.

- [ ] **Step 1: Add per-stage morphology timing and memory RED**

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

- [ ] **Step 2: Run Release RED or confirm budget**

Run:

    cargo test --release --test spherical_natural_graph_performance -- --ignored --nocapture

Expected: compilation fails until evidence fields are added, then the new implementation must satisfy the budgets. A budget failure requires profiling and algorithmic correction; do not delete the gate.

- [ ] **Step 3: Optimize only measured hotspots**

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

- [ ] **Step 4: Run focused compatibility and source-identity gates**

Run:

    cargo test --test legacy_planar_boundary -- --nocapture
    cargo test --test natural_display_golden -- --nocapture
    cargo test --test spherical_natural_stage_graph -- --nocapture
    cargo test --test spherical_natural_matrix -- --nocapture
    cargo test --test spherical_field_driven_relief -- --nocapture
    cargo test --test spherical_morphology_quality -- --nocapture

Expected: all pass; planar hashes remain frozen, sphere stage version/cache hashes are intentionally updated.

- [ ] **Step 5: Run engineering gates**

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

- [ ] **Step 6: Perform final boundary audit**

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

- [ ] **Step 7: Append Execution Evidence**

Append exact commits, RED failures, mutation failures, focused counts, new sphere hashes, Release timings, memory, visual seed review, GPU adapter/backend, wasm result, and full-suite elapsed times to this plan. Do not claim an unobserved result.

- [ ] **Step 8: Commit final acceptance**

    git add tests/spherical_natural_graph_performance.rs docs/superpowers/plans/2026-08-10-field-driven-spherical-terrain.md
    git commit -m "test: lock field-driven spherical terrain"

- [ ] **Step 9: Verify clean completion**

Run:

    git status --short
    git log -10 --oneline

Expected: tracked worktree clean, the design and plan commits are present, all implementation commits are ordered by task, and no ignored report is accidentally tracked.

---

## Execution Evidence

Observed evidence is appended here during execution: exact RED/GREEN commands, mutation failures, hashes, timings, visual review results, and commit IDs. Every required behavior is specified above.
