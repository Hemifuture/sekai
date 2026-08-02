# Natural Map Credibility Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace artificial continent-separator trenches with formation-owned ocean corridors, localize the field observer into Chinese without changing stable keys, and make present-slice tectonic relief use physically relevant motion components plus deterministic correlated detail.

**Architecture:** Crust morphology remains in `generators::natural::tectonics`; Relief removes all reconstruction of continent components and only interprets formal tectonic/mantle inputs. Chinese strings live in a concrete `ui::field` catalog with fallback to schema keys. Boundary classification remains deterministic kinematics, while a Relief-only labeled substream modulates already-classified tectonic effects without changing their sign or kind.

**Tech Stack:** Rust 2021, Cargo, egui/eframe, deterministic `StageRng`/ChaCha substreams, graph-distance fields over `SpatialSnapshot`, golden PNG integration tests, native/WASM/Trunk gates.

## Global Constraints

- Do not introduce history events, years, time integration, spherical tectonics, or a periodic seam.
- `generate_plates` must not consume world-formation data; changing presets at one algorithm version must preserve plate records, ownership, and velocities.
- `world`, `engine`, and generators must not depend on `ui`, `view`, egui, or GPU code.
- UI localization must not change `FieldId`, `FieldDisplayMetadata::label_key`, category keys, build hashes, or serialized world data.
- Every stochastic change uses a named deterministic substream; iteration order and display state must not affect results.
- Continental area remains within one maximum-cell-area error of the requested fraction.
- All closed boundary cells and the existing east/west ocean band remain ocean in formal and current surfaces.
- Keep `TectonicSnapshot`, `ReliefSnapshot`, and natural field schema structures unchanged.
- Use strict TDD: write each behavior test, observe the expected failure, then write production code.
- Do not blindly refresh goldens; inspect every changed reference image.

---

### Task 1: Move continent separation into crust formation

**Files:**
- Modify: `src/generators/natural/tectonics.rs:233-548`
- Modify: `src/generators/natural/relief.rs:1-210`
- Modify: `tests/tectonic_generation.rs`
- Modify: `tests/relief_generation.rs`

**Interfaces:**
- Consumes: existing `NaturalTopologyIndex`, multi-source distance/ownership, smoothed crust-shape noise, and `CrustFormationProfile`.
- Produces: `ownership_divider_distance(topology, owners) -> Vec<u64>` and profile-owned `corridor_half_width_steps: u64`; no new public artifact or field.
- Preserves: `TectonicGenerator::generate`, `generate_plates`, `ReliefGenerator::generate`, and all snapshot constructors.

- [ ] **Step 1: Add a failing Relief ownership test**

In `tests/relief_generation.rs`, add a test-only constructor that reuses the plates and boundaries from `custom_tectonics`, but assigns continental crust to columns `0..=1` and `6..=7`, oceanic crust to columns `2..=5`, and thicknesses `35.0 km` / `7.0 km` respectively. Add this behavior test:

```rust
#[test]
fn ocean_basin_base_depends_on_crust_transition_distance_not_component_ownership() {
    let spatial = regular_grid();
    let tectonic = separated_continental_components(&spatial);
    let relief = generate_relief(&spatial, &tectonic, 7);

    // Columns 3 and 4 are one graph step from equal oceanic transition cells.
    // margin=-2400, interior=-4430, smoothstep(1/4)=0.15625.
    let expected = -2_400.0 + (-4_430.0 + 2_400.0) * 0.15625;
    for column in [3, 4] {
        let found = relief
            .crust_base_elevation_m()
            .get(cell_at(1, column).raw() as usize)
            .unwrap();
        assert!((found - expected).abs() <= 0.01, "column {column}: {found}");
    }
}
```

The test fixture must construct a real validated `TectonicSnapshot` with cloned plates, plate field, boundary records, and segments; do not mock `ReliefGenerator`.

- [ ] **Step 2: Run the Relief test and verify the expected red result**

Run:

```powershell
cargo test --test relief_generation ocean_basin_base_depends_on_crust_transition_distance_not_component_ownership -- --exact
```

Expected: FAIL because the current component separator forces at least one center ocean cell to `-2400 m` instead of the generic transition-distance value.

- [ ] **Step 3: Add a failing minimum-corridor-width test**

In `tests/tectonic_generation.rs`, add a BFS helper that labels all continental components, then measures the minimum number of intervening ocean cells between different components. Add:

```rust
#[test]
fn recommended_continents_keep_multiple_ocean_cell_layers_between_components() {
    for seed in [42, 1_024, 14971025413948366848] {
        let snapshot = generate_quality(
            seed,
            ResolvedWorldFormationPreset::Continents,
            0.38,
        );
        let minimum = minimum_ocean_layers_between_continental_components(
            quality_spatial_fixture(),
            &snapshot,
        );
        assert!(minimum >= 3, "seed {seed}: only {minimum} ocean layers");
    }
}
```

The BFS starts each continental component at distance zero, traverses only oceanic cells, and returns the ocean-cell distance when it first touches another component. Expectations are literal and independent of the production corridor helper.

- [ ] **Step 4: Run the corridor test and verify the expected red result**

Run:

```powershell
cargo test --test tectonic_generation recommended_continents_keep_multiple_ocean_cell_layers_between_components -- --exact
```

Expected: FAIL for at least one seed because `ownership_dividers` currently reserves only the divider cells.

- [ ] **Step 5: Implement variable-width formation corridors**

In `CrustFormationProfile`, replace `hard_corridor: bool` with:

```rust
corridor_half_width_steps: u64,
```

Use exact profile values:

```rust
Continents => 2,
Archipelago => 1,
Supercontinent => 0,
GreatIsland => 0,
VolcanicIslands => 1,
```

Replace `ownership_dividers` with a helper that turns divider cells into sources and calls `multi_source_distance`:

```rust
fn ownership_divider_distance(
    topology: &NaturalTopologyIndex,
    owners: &[u32],
) -> Vec<u64> {
    let sources = topology
        .arcs()
        .iter()
        .enumerate()
        .filter_map(|(index, arcs)| {
            arcs.iter()
                .any(|arc| owners[arc.neighbor.raw() as usize] != owners[index])
                .then_some(CellId::from_raw(index as u32))
        })
        .collect::<Vec<_>>();
    multi_source_distance(topology, &sources, None)
}
```

For multi-nucleus profiles, try every corridor width from `base` down to `0`, then disabled. For a positive option, add one local step where the already-smoothed `shape_noise[index] > 0`; for option zero reserve only exact divider cells. A cell is excluded when:

```rust
divider_distance[index]
    <= typical_cost_u64.saturating_mul(local_half_width_steps)
```

Keep nucleus cells eligible, retain the existing ocean-frame fallback order, and accept the first candidate set large enough for the requested continental area.

- [ ] **Step 6: Delete the Relief component separator**

Remove:

```rust
const OCEAN_COMPONENT_SEPARATOR_BASE_M: f32 = -2_400.0;
apply_oceanic_component_separators(...);
fn apply_oceanic_component_separators(...);
```

Also remove the now-unused `VecDeque` import. Do not add a replacement Relief mask or component lookup.

Set the generic oceanic transition baseline to `-2400 m`, independently of continent ownership. Add `non_volcanic_ocean_corridor_stays_submerged_under_ridge_uplift` to prove an ordinary ridge cannot turn the corridor into a land bridge without volcanic input.

- [ ] **Step 7: Run focused tests and make them green**

Run:

```powershell
cargo test --test relief_generation
cargo test --test tectonic_generation
cargo test --test natural_display_golden quality_across_fixed_seed_set --release -- --exact --nocapture
```

Expected: the two new tests pass; all existing relief, crust-area, morphology, plate-orthogonality, and closed-frame assertions remain green. If the quality matrix reports a recommended seed using fewer than three ocean layers, adjust only profile half-widths/ordered fallback—not Relief.

- [ ] **Step 8: Commit the formation/Relief ownership fix**

```powershell
git add src/generators/natural/tectonics.rs src/generators/natural/relief.rs tests/tectonic_generation.rs tests/relief_generation.rs
git commit -m "fix: naturalize ocean corridors between continents"
```

---

### Task 2: Resolve natural-field labels at the UI boundary

**Files:**
- Create: `src/ui/field/localization.rs`
- Modify: `src/ui/field/mod.rs`
- Modify: `src/ui/field/controls.rs`
- Modify: `src/ui/field/inspector.rs`

**Interfaces:**
- Consumes: stable `FieldSchema::display.label_key`, category localization keys, `FieldDomain`, `FieldValueType`, and `PaletteId`.
- Produces: concrete UI-only helpers `localized_field_key`, `localized_domain`, `localized_value_type`, and `localized_palette`.
- Fallback: unknown keys return their complete original text; no world/schema mutation.

- [ ] **Step 1: Add failing localization contract tests**

Declare `mod localization;` in `src/ui/field/mod.rs`, import its helpers into the existing `controls.rs` test module, and add:

```rust
#[test]
fn formal_natural_field_and_category_keys_have_chinese_labels() {
    assert_eq!(
        localized_field_key("field.sekai.core.natural.surface_elevation_m"),
        "当前地表高程"
    );
    assert_eq!(
        localized_field_key("field.sekai.core.natural.boundary_kind.subduction"),
        "俯冲"
    );
    assert_eq!(
        localized_field_key("field.sekai.core.natural.plate_id.plate-03"),
        "板块 03"
    );
    assert_eq!(
        localized_field_key("field.sekai.core.natural.strahler_stream_order.order-004"),
        "4 级河流"
    );
}

#[test]
fn unknown_extension_label_keys_remain_inspectable() {
    assert_eq!(localized_field_key("field.example.magic_flux"), "field.example.magic_flux");
}
```

Add a registry coverage test that iterates `natural_field_registry(12)` and asserts every top-level label key resolves to a string different from its key. This catches missing labels when formal fields are added.

- [ ] **Step 2: Run the localization tests and verify red**

Run:

```powershell
cargo test --lib formal_natural_field_and_category_keys_have_chinese_labels
```

Expected: compile failure because `src/ui/field/localization.rs` and the helpers do not exist.

- [ ] **Step 3: Implement the concrete Chinese resolver**

Create `src/ui/field/localization.rs` with:

```rust
use std::borrow::Cow;

pub(super) fn localized_field_key(key: &str) -> Cow<'_, str> {
    const PREFIX: &str = "field.sekai.core.natural.";
    let Some(tail) = key.strip_prefix(PREFIX) else {
        return Cow::Borrowed(key);
    };
    let label = match tail {
        "annual_local_runoff_mm" => "本地年径流量",
        "bedrock_kind" => "基岩类型",
        "boundary_kind" => "构造边界类型",
        "boundary_strength" => "构造边界强度",
        "crust_base_elevation_m" => "地壳基准高程",
        "crust_kind" => "地壳类型",
        "crust_thickness_km" => "地壳厚度",
        "drainage_area_km2" => "汇水面积",
        "elevation_m" => "构造地形高程",
        "erosion_resistance" => "抗侵蚀性",
        "fluvial_erosion_depth_m" => "河流侵蚀深度",
        "fracture_intensity" => "裂隙强度",
        "geothermal_potential" => "地热潜力",
        "lake_depth_m" => "湖泊深度",
        "land_ocean" => "海陆分类",
        "latitude_degrees" => "纬度",
        "mantle_heat_flow_mw_m2" => "地幔热流",
        "maritime_influence" => "海洋影响度",
        "mean_annual_discharge_m3_s" => "多年平均流量",
        "metallic_mineral_potential" => "金属矿产潜力",
        "plate_id" => "板块编号",
        "plate_velocity" => "板块速度",
        "preliminary_annual_precipitation_mm" => "初步年降水量",
        "preliminary_mean_air_temperature_c" => "初步年均气温",
        "preliminary_prevailing_wind_m_s" => "初步盛行风",
        "preliminary_temperature_seasonality_c" => "初步气温季节性",
        "regional_offset_m" => "区域起伏",
        "relative_permeability" => "相对渗透率",
        "sediment_deposition_thickness_m" => "沉积厚度",
        "sedimentary_basin_potential" => "沉积盆地潜力",
        "strahler_stream_order" => "斯特拉勒河级",
        "surface_elevation_m" => "当前地表高程",
        "surface_water_kind" => "地表水类型",
        "tectonic_offset_m" => "构造地貌偏移",
        "volcanic_influence" => "火山影响度",
        "volcanic_offset_m" => "火山地貌偏移",
        "crust_kind.oceanic" => "海洋地壳",
        "crust_kind.continental" => "大陆地壳",
        "boundary_kind.none" => "无构造事件",
        "boundary_kind.weak" => "弱边界",
        "boundary_kind.continental_collision" => "大陆碰撞",
        "boundary_kind.subduction" => "俯冲",
        "boundary_kind.continental_rift" => "大陆裂谷",
        "boundary_kind.oceanic_ridge" => "洋中脊",
        "boundary_kind.transform" => "走滑边界",
        "land_ocean.ocean" => "海洋",
        "land_ocean.land" => "陆地",
        "bedrock_kind.oceanic_mafic" => "海洋镁铁质岩",
        "bedrock_kind.continental_crystalline" => "大陆结晶岩",
        "bedrock_kind.sedimentary" => "沉积岩",
        "bedrock_kind.metamorphic" => "变质岩",
        "bedrock_kind.volcanic" => "火山岩",
        "surface_water_kind.dry_land" => "旱地",
        "surface_water_kind.ocean" => "海洋",
        "surface_water_kind.lake" => "湖泊",
        "strahler_stream_order.none" => "无河道",
        _ => return dynamic_or_fallback(key, tail),
    };
    Cow::Borrowed(label)
}
```

`dynamic_or_fallback` parses only the exact `plate_id.plate-NN` and `strahler_stream_order.order-NNN` suffix forms, returning `板块 NN` and an integer `N 级河流`; malformed or unknown keys return `key` unchanged.

Add direct match helpers for:

```rust
localized_domain(FieldDomain::Cells) == "单元格"
localized_domain(FieldDomain::Edges) == "边"
localized_value_type(FieldValueType::ScalarF32) == "标量"
localized_value_type(FieldValueType::CategoryU32) == "分类"
localized_value_type(FieldValueType::Vector2F32) == "二维向量"
localized_palette(PaletteId::Sequential) == "顺序"
localized_palette(PaletteId::Diverging) == "发散"
localized_palette(PaletteId::Categorical) == "分类"
```

Cover the remaining `FieldDomain` and `FieldValueType` enum variants exhaustively with concise Chinese names.

- [ ] **Step 4: Wire controls and inspector to the resolver**

In `controls.rs`:

- `field_label` returns `localized_field_key(schema.display.label_key()).into_owned()`.
- Replace visible `Schema`/`Data` with `字段定义`/`数据范围`.
- Replace `Schema 默认` with `字段默认`.
- Replace `palette_label` with `localized_palette`.

In `inspector.rs`:

- Display `localized_field_key(schema.display.label_key())` as the strong title.
- Keep the existing `namespace.name@version` monospace line unchanged.
- Resolve every category label key before drawing it.
- Use `localized_domain` and `localized_value_type` instead of `Debug` enum output.
- Replace visible `Schema 范围` and `显示范围` with `字段定义范围` and `当前显示范围`.

- [ ] **Step 5: Run localization/UI tests and make them green**

Run:

```powershell
cargo test --lib ui::field
cargo test --test natural_field_views
```

Expected: all mappings, unknown-key fallback, catalog coverage, and existing non-mutation UI tests pass.

- [ ] **Step 6: Commit the UI-only localization**

```powershell
git add src/ui/field/localization.rs src/ui/field/mod.rs src/ui/field/controls.rs src/ui/field/inspector.rs
git commit -m "feat: localize natural field observer labels"
```

---

### Task 3: Use projected boundary strength and correlated tectonic detail

**Files:**
- Modify: `src/generators/natural/tectonics.rs:718-802`
- Modify: `src/generators/natural/random.rs`
- Modify: `src/generators/natural/relief.rs:308-438`
- Modify: `tests/relief_generation.rs`

**Interfaces:**
- Consumes: two constant plate velocities, quantized local boundary normal, existing boundary kind rules, and `LabeledSubstreams`.
- Produces: motion-component-derived `BoundaryRecord::strength` and Relief-only `relief-tectonic-detail-v1` modulation.
- Preserves: boundary category values, subduction polarity rules, public fields, and signed relief meaning.

- [ ] **Step 1: Add a failing three-plate oblique-motion test**

Inside `src/generators/natural/tectonics.rs`'s existing `#[cfg(test)]` module, add:

```rust
#[test]
fn oblique_middle_plate_classifies_opposite_sides_without_inflating_normal_strength() {
    let left = PlateId::from_raw(0);
    let middle = PlateId::from_raw(1);
    let right = PlateId::from_raw(2);
    let normal = [1_000, 0];
    let crust = [CrustKind::Continental, CrustKind::Continental];
    let thickness = [35.0, 35.0];

    let pure_left = classify_kinematics(
        [left, middle],
        [velocity(0, 0), velocity(-30, 0)],
        normal,
        crust,
        thickness,
    );
    let oblique_left = classify_kinematics(
        [left, middle],
        [velocity(0, 0), velocity(-30, 40)],
        normal,
        crust,
        thickness,
    );
    let oblique_right = classify_kinematics(
        [middle, right],
        [velocity(-30, 40), velocity(0, 0)],
        normal,
        crust,
        thickness,
    );

    assert_eq!(oblique_left.kind, BoundaryKind::ContinentalCollision);
    assert_eq!(oblique_right.kind, BoundaryKind::ContinentalRift);
    assert!((oblique_left.strength - pure_left.strength).abs() <= f32::EPSILON);
}
```

- [ ] **Step 2: Run the three-plate test and verify red**

Run:

```powershell
cargo test --lib oblique_middle_plate_classifies_opposite_sides_without_inflating_normal_strength
```

Expected: classification assertions pass, but the strength assertion fails because current strength uses total relative speed and includes the `40 mm/year` tangential component.

- [ ] **Step 3: Implement decomposed motion strengths**

In `classify_kinematics`, retain integer dot products for kind selection, then calculate:

```rust
let speed = (speed_squared as f32).sqrt();
let normal_speed = if normal_squared == 0 {
    0.0
} else {
    projection.unsigned_abs() as f32 / (normal_squared as f32).sqrt()
};
let tangent_speed = (speed * speed - normal_speed * normal_speed)
    .max(0.0)
    .sqrt();
let maximum_relative_speed =
    f32::from(MAX_PLATE_VELOCITY_MM_PER_YEAR) * 2.0 * 2.0_f32.sqrt();
```

Weak boundaries use `speed / maximum_relative_speed`; Transform uses `tangent_speed / maximum_relative_speed`; convergent/divergent kinds use `normal_speed / maximum_relative_speed`. Clamp every result to `0.0..=1.0`. Keep the current integer `16%` normal-component threshold and projection sign.

- [ ] **Step 4: Run tectonic boundary tests and make them green**

Run:

```powershell
cargo test --lib generators::natural::tectonics::tests
cargo test --test tectonic_boundaries
```

Expected: the new three-plate test and all existing collision/subduction/rift/ridge/transform contracts pass.

- [ ] **Step 5: Add a failing Relief-seed detail test**

In `tests/relief_generation.rs`, add:

```rust
#[test]
fn tectonic_relief_detail_is_seeded_repeatable_and_sign_preserving() {
    let spatial = regular_grid();
    let tectonic = custom_tectonics(&spatial, BoundaryKind::ContinentalCollision);
    let first = generate_relief(&spatial, &tectonic, 7);
    let repeated = generate_relief(&spatial, &tectonic, 7);
    let changed = generate_relief(&spatial, &tectonic, 8);

    assert_eq!(first.tectonic_offset_m(), repeated.tectonic_offset_m());
    assert_ne!(first.tectonic_offset_m(), changed.tectonic_offset_m());
    assert!(first
        .tectonic_offset_m()
        .values()
        .iter()
        .zip(changed.tectonic_offset_m().values())
        .all(|(&a, &b)| a == 0.0 || b == 0.0 || a.is_sign_positive() == b.is_sign_positive()));
}
```

- [ ] **Step 6: Run the detail test and verify red**

Run:

```powershell
cargo test --test relief_generation tectonic_relief_detail_is_seeded_repeatable_and_sign_preserving -- --exact
```

Expected: `assert_ne!` fails because `tectonic_offset_m` currently ignores the Relief random stream.

- [ ] **Step 7: Implement the independent correlated detail substream**

In `src/generators/natural/random.rs`, add:

```rust
pub(super) const RELIEF_TECTONIC_DETAIL_LABEL: &str = "relief-tectonic-detail-v1";
```

Pass `&LabeledSubstreams` into `synthesize_tectonic_offset`. After all event classes are accumulated and before range clamping:

```rust
let mut detail_rng = streams.stream(RELIEF_TECTONIC_DETAIL_LABEL);
let detail = diffuse(topology, random_noise(topology.arcs().len(), &mut detail_rng), 3);
for (value, detail) in result.iter_mut().zip(detail) {
    if *value != 0.0 {
        let normalized = (detail as f32 / REGIONAL_NOISE_SCALE as f32).clamp(-1.0, 1.0);
        let multiplier = (1.0 + normalized * 0.25).clamp(0.75, 1.25);
        *value *= multiplier;
    }
    *value = value.clamp(TECTONIC_OFFSET_MIN_M, TECTONIC_OFFSET_MAX_M);
}
```

Reuse the existing private `random_noise` and `diffuse`; do not add another generic noise abstraction or consume `RELIEF_REGIONAL_LABEL`.

- [ ] **Step 8: Run focused Relief and invariant tests**

Run:

```powershell
cargo test --test relief_generation
cargo test --test relief_contracts
cargo test --test natural_stage_graph
```

Expected: repeatability, sign, component identity, field bounds, and graph contracts pass.

- [ ] **Step 9: Commit projected kinematics and detail**

```powershell
git add src/generators/natural/tectonics.rs src/generators/natural/random.rs src/generators/natural/relief.rs tests/relief_generation.rs
git commit -m "feat: refine present-slice tectonic relief"
```

---

### Task 4: Version, regressions, goldens, and actual application review

**Files:**
- Modify: `src/generators/natural/stage.rs`
- Modify: version assertions and `StageIdentity` fixtures under `tests/`
- Modify: affected files under `tests/golden/natural-foundation/`
- Modify only if required by measured quality: `tests/natural_display_golden.rs`

**Interfaces:**
- Produces: final cache-safe `TectonicStage@3` and `ReliefStage@5` semantics.
- Preserves: snapshot schema versions and the public natural-field registry.

- [ ] **Step 1: Add failing stage-version assertions**

Update the explicit assertions in `tests/natural_stage_graph.rs` to:

```rust
assert_eq!(TectonicStage.version(), 3);
assert_eq!(ReliefStage.version(), 5);
```

Update test-only `StageIdentity::new("natural.tectonics", 2, ...)` and `StageIdentity::new("natural.relief", 4, ...)` fixtures to versions 3 and 5 only where they intentionally reproduce production stage streams.

- [ ] **Step 2: Run the graph test and verify red**

Run:

```powershell
cargo test --test natural_stage_graph complete_natural_graph_publishes_physical_artifacts_with_exact_stage_metadata -- --exact
```

Expected: FAIL because production stages still report versions 2 and 4.

- [ ] **Step 3: Bump production stage versions**

Change only:

```rust
TectonicStage::version() -> 3
ReliefStage::version() -> 5
```

Do not change `TECTONIC_SNAPSHOT_SCHEMA_V1`, `RELIEF_SCHEMA_V2`, field IDs, or artifact IDs.

- [ ] **Step 4: Run complete Debug tests before touching goldens**

Run:

```powershell
cargo test --workspace --all-targets --no-fail-fast
```

Expected: all behavioral tests pass; only reviewed golden comparisons may fail because algorithm outputs changed. Investigate any non-golden failure before continuing.

- [ ] **Step 5: Regenerate and inspect affected natural goldens**

Use the repository's existing golden-update mechanism from `tests/natural_display_golden.rs`/`src/bin/generate_screenshots.rs`; do not create a second generator. Review at least:

```text
plate.png
crust.png
elevation.png
current-surface.png
surface-water.png
bedrock.png
fracture-related/geologic views whose upstream relief changed
```

Reject any image with ellipse-composed continents, edge land, fixed-width Y trenches, uniform tectonic rings, missing data, or palette corruption. Record the exact changed golden list with `git diff --stat`.

- [ ] **Step 6: Run full Release and platform quality gates**

Run each command separately and require exit code zero:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --release --no-fail-fast
cargo check --all-features --lib --target wasm32-unknown-unknown
trunk build
git diff --check
```

For the WASM and Trunk commands, retain the repository's required:

```powershell
$env:RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
$env:RUSTDOCFLAGS='--cfg getrandom_backend="wasm_js"'
```

Run the 20,000-cell named-preset quality matrix with `--release --nocapture` and confirm continental area, component profiles, plate orthogonality, boundary ocean, land components, volcanism ordering, and performance budgets.

- [ ] **Step 7: Build and launch the feature Release executable**

Verify the exact currently running Sekai process path before stopping only that process. Build:

```powershell
cargo build --release --bin sekai
```

Launch the worktree's exact `target/release/sekai.exe` with a hidden helper shell and visible application window. Do not stop unrelated worktree executables or desktop apps.

- [ ] **Step 8: Perform actual UI and visual acceptance with computer-use**

Inspect the running Release app at the current seed and at least two additional seeds. Check:

1. `地壳类型` — separated multi-cell oceanic corridors.
2. `地壳基准高程` — continuous continental-shelf-to-basin gradients without the former Y skeleton.
3. `构造地貌偏移` — causal bands with non-uniform along-line amplitude.
4. `当前地表高程` and `地表水类型` — several continents remain separated after erosion.
5. `板块编号` — coherent plate regions and preset independence.
6. Field list, inspector, and category legend — Chinese user labels with stable technical ID retained only in the inspector.

Capture screenshot paths and compare the same seed before/after where available. If a visual defect remains, return to the owning task and add a failing automated regression before changing code.

- [ ] **Step 9: Commit final versions and reviewed goldens**

```powershell
git add src/generators/natural/stage.rs tests tests/golden/natural-foundation
git commit -m "test: review natural map credibility outputs"
```

- [ ] **Step 10: Final clean-state audit**

Run:

```powershell
git status --short
git log --oneline --decorate -8
git diff main...HEAD --check
```

Expected: clean worktree, intentional commits only, no changes in unrelated legacy worktrees, and the feature Release app left running for user inspection.
