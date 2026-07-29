# Hydro-Erosion Foundation Implementation Plan

> Execute in this isolated worktree with test-first commits. Routine design decisions, self-review, merge, and push are authorized. Stop only for a genuine product-direction or system-boundary conflict.

**Goal:** Publish a deterministic current-slice surface and hydrology foundation with monthly runoff/discharge, a validated drainage DAG, lakes, basins, directed river segments, bounded fluvial erosion, and conserved sediment.

**Architecture:** Add a fixed-point hydro-erosion spec, one trusted model capability, full rule audit plus minimal projection, independent pure hydrology and erosion solvers, a fixed two-pass orchestrator, separate validated surface/hydrology contracts, one atomic production artifact, formal display fields, and atomic application integration.

**Tech stack:** Rust 2021, serde, thiserror, existing stage engine/cache, `BinaryHeap`, egui/wgpu field display, PNG reference rasterizer, native and `wasm32-unknown-unknown`.

**Design:** `docs/superpowers/specs/2026-07-29-hydro-erosion-foundation-design.md`

---

## Global constraints

- Do not import `src/terrain` into the formal natural pipeline.
- Do not create dates, flood events, erosion years, final-climate fields, groundwater placeholders, or empty extension traits.
- `ReliefSnapshot` remains immutable constructional relief; only the new stage writes current surface elevation.
- The public stage graph remains acyclic; both hydrology passes stay inside one bounded generator.
- `world` remains independent of engine, rules, generators, app, UI, GPU, and legacy terrain.
- Hydrology reads only spatial topology, surface elevation, sea level, relative permeability, preliminary monthly precipitation, and resolved controls.
- Erosion reads only initial hydrology, constructional elevation, erosion resistance, and resolved controls.
- All public serialized types revalidate on deserialization.
- All classifications use stable traversal and quantized thresholds.
- Preserve unrelated user work and the `field-display-system` worktree.
- Run the named failing test before implementation in every task.
- Commit only after the focused test set passes.

## Task 1: Define the fixed-point hydro-erosion specification

**Files**

- Create: `src/world/natural/hydro_erosion_spec.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/hydro_erosion_spec.rs`

**Red tests**

- Defaults exactly match the design.
- Boundary values pass.
- Unsupported schema, river threshold outside `1..=1_000_000` deci-m³/s, erosion strength above 2000‰, and lake depth outside `1..=10_000` cm fail with precise variants.
- JSON deserialization revalidates.
- Accessors return m³/s, normalized strength, and meters without mutable float state.

**Command**

```powershell
cargo test --test hydro_erosion_spec
```

**Implementation**

- Add constants, `HydroErosionSpec`, and `HydroErosionSpecError`.
- Store every author-facing value as a fixed-point integer.
- Implement custom `Deserialize`.
- Re-export only the stable contract.

**Commit**

```text
feat: define hydro erosion spec
```

## Task 2: Register the trusted hydro-erosion model capability

**Files**

- Modify: `src/rules/capability.rs`
- Modify: `src/rules/builtin.rs`
- Modify: `src/rules/mod.rs`
- Modify: `tests/rule_capabilities.rs`
- Modify: `tests/builtin_rules.rs`
- Modify: `tests/rule_manifests.rs`

**Red tests**

- Stable ID is `sekai.core.natural.hydro-erosion-model@1`.
- Descriptor is `UniqueRequired`, `WorldLaw`, and author-disallowed.
- `HydroErosionModel::PriorityFloodStreamPowerV1` is closed and typed.
- Earthlike contributes tectonic, geologic, climate, and hydro-erosion models in canonical order.
- Duplicate hydro-erosion contributions fail as duplicate unique capability content.

**Command**

```powershell
cargo test --test rule_capabilities --test builtin_rules --test rule_manifests
```

**Implementation**

- Add the stable capability ID constructor and enum.
- Extend all exhaustive `CapabilityContribution` matches.
- Register the descriptor and Earthlike contribution.
- Do not add wildcard matches that could hide future capabilities.

**Commit**

```text
feat: register hydro erosion world law
```

## Task 3: Resolve hydro-erosion rules into a complete audit

**Files**

- Create: `src/rules/hydro_erosion.rs`
- Modify: `src/rules/mod.rs`
- Create: `tests/rule_hydro_erosion_resolution.rs`
- Modify exhaustive matches in existing tectonic, geologic, and climate resolvers.

**Red tests**

- Resolution stores canonical participating pack identities, exact spec, and chosen model.
- Missing and multiple models fail.
- Invalid base/resolved specs fail.
- Duplicate serialized pack identities and unsupported audit schema fail during deserialization.
- Unrelated model and tectonic-control contributions are ignored without disappearing from overall pack audit.

**Command**

```powershell
cargo test --test rule_hydro_erosion_resolution --test rule_tectonic_resolution --test rule_geologic_resolution --test rule_climate_resolution
```

**Implementation**

- Mirror the established full-audit resolver shape without sharing mutable state.
- Add `HydroErosionRuleResolution`, `HydroErosionRuleAudit`, and precise errors.
- Keep `rules` independent of generators/engine/app.

**Commit**

```text
feat: resolve hydro erosion rules
```

## Task 4: Project the complete audit into minimal generator input

**Files**

- Create: `src/generators/natural/hydro_erosion_rule_input.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/rule_hydro_erosion_stage.rs`
- Modify: `tests/rule_stage_graph.rs`

**Red tests**

- Stable artifacts:
  - `natural.hydro-erosion-spec`
  - `rules.hydro-erosion-resolution`
  - `natural.resolved-hydro-erosion-input`
- Stable stages:
  - `rules.hydro-erosion-resolution`
  - `natural.resolve-hydro-erosion-input`
- Stage versions and namespace are exact.
- Rule stage depends only on spec, rule packs, and author constraints.
- Projection stage depends only on resolution.
- Projected input contains spec, model, and audit identity but no raw pack set.
- Invalid cross-artifact combinations fail.

**Command**

```powershell
cargo test --test rule_hydro_erosion_stage --test rule_stage_graph
```

**Implementation**

- Add external, resolution, and resolved-input artifacts.
- Add the two pure stage adapters.
- Validate the projected input on construction and artifact validation.

**Commit**

```text
feat: project hydro erosion input
```

## Task 5: Add stable hydrology identifiers and the surface-process contract

**Files**

- Modify: `src/world/ids.rs`
- Modify: `src/world/mod.rs`
- Create: `src/world/natural/surface_process.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/surface_process_contracts.rs`
- Modify: `tests/world_primitives.rs`

**Red tests**

- `DrainageBasinId`, `LakeId`, and `RiverSegmentId` are typed `u32` IDs with serde round trips.
- Valid surface fields construct and round trip.
- Unsupported schema, wrong lengths, NaN/infinity, negative depths, range overflow, non-finite sediment volume, and negative export fail.
- `validate_against` with spatial and relief enforces:
  - dense alignment;
  - `surface = constructional - erosion + deposition` within 5 cm;
  - eroded volume equals deposited volume plus export within a relative tolerance.
- Invalid JSON cannot bypass validation.

**Command**

```powershell
cargo test --test world_primitives --test surface_process_contracts
```

**Implementation**

- Add the three IDs through the existing macro.
- Add `SurfaceProcessSnapshot` and `SurfaceProcessValidationError`.
- Reuse `ElevationField` for current surface.
- Store throughput and export as `f64`; all displayable arrays remain `f32`.

**Commit**

```text
feat: define surface process snapshot
```

## Task 6: Define the formal hydrology contract

**Files**

- Create: `src/world/natural/hydrology.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/hydrology_contracts.rs`

**Red tests**

- `SurfaceWaterField` accepts only dry/ocean/lake raw values and exposes zero-copy `u32`.
- `StrahlerOrderField` accepts bounded orders and exposes zero-copy `u32`.
- Valid monthly fields, receivers, basins, lakes, and river segments construct and round trip.
- Wrong lengths, non-finite/range errors, bad IDs, self-receivers, cycles, non-contiguous IDs, duplicate lake cells, inconsistent water kinds, invalid segment direction, bad summary identities, and nonriver nonzero order fail precisely.
- `validate_against_spatial` requires every receiver and river segment to follow a real adjacency.
- Downstream area and flow dominate each direct upstream contribution within tolerance.
- Invalid JSON is rejected.

**Command**

```powershell
cargo test --test hydrology_contracts
```

**Implementation**

- Add typed fields, enums, records, snapshot, accessors, and validation errors.
- Use `Vec<[f32; 12]>` for monthly arrays.
- Store drainage area as `f32` for display zero-copy; retain basin aggregate area as `f64`.
- Validate cycles in linear time without recursion.

**Commit**

```text
feat: define hydrology snapshot
```

## Task 7: Define the atomic composite contract

**Files**

- Create: `src/world/natural/hydro_erosion.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/hydro_erosion_contracts.rs`

**Red tests**

- Valid surface and hydrology sub-snapshots combine and round trip.
- Schema and cell-count mismatches fail.
- Cross-validation checks exact spatial, relief, geology, and preliminary-climate alignment.
- Ocean classification uses the formal sea level and current surface.
- Runoff is zero over ocean.
- Invalid receiver, surface identity, and external cardinality cannot be hidden by a valid sibling snapshot.

**Command**

```powershell
cargo test --test hydro_erosion_contracts
```

**Implementation**

- Add `HydroErosionSnapshot` and a narrow composite error.
- Delegate self-contained work to each sub-snapshot.
- Keep cross-domain validation here rather than importing geology/climate into the two smaller contracts.

**Commit**

```text
feat: define hydro erosion snapshot
```

## Task 8: Implement deterministic Priority-Flood and water accumulation

**Files**

- Create: `src/generators/natural/hydrology.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/hydrology_generation.rs`

**Red tests**

- Same inputs produce byte-identical snapshots.
- A synthetic bowl produces one lake, stable depth, and stable outlet.
- A ridge produces separate terminal basins.
- A flat drains without a cycle.
- All-ocean and all-land worlds are defined.
- Every nonterminal receiver is a real neighbor and has a strictly earlier drainage rank.
- Higher permeability lowers runoff and discharge.
- Higher monthly precipitation raises the corresponding monthly discharge.
- Downstream monthly water volume equals local plus direct-upstream volume within tolerance.
- Strahler order follows the thresholded DAG.
- Lake interiors do not publish fake channel segments; a real lake outflow publishes one `LakeOutlet`.

**Command**

```powershell
cargo test --test hydrology_generation
```

**Implementation**

- Quantize input elevation to centimeters.
- Use a deterministic min-heap key `(height_cm, CellId)`.
- Record flood rank, then select receivers with stable steepest-descent/flat rules.
- Accumulate area and 12 monthly volumes in reverse receiver rank.
- Derive runoff only from precipitation and relative permeability.
- Label terminals/basins, connected lakes, lake outlets, Strahler order, and directed segments.
- Avoid hash maps and per-month topology copies in hot paths.

**Commit**

```text
feat: solve deterministic hydrology
```

## Task 9: Implement bounded erosion, sediment, and two-pass orchestration

**Files**

- Create: `src/generators/natural/erosion.rs`
- Create: `src/generators/natural/hydro_erosion.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/hydro_erosion_generation.rs`

**Red tests**

- Same inputs produce byte-identical composite snapshots.
- Zero erosion strength preserves constructional elevation exactly and exports no sediment.
- Softer rock incises deeper than resistant rock under equal flow and slope.
- More discharge or slope increases incision before the hard cap.
- Flat/no-flow cells do not incise.
- Low-energy cells retain more sediment than high-energy cells.
- Global eroded volume equals deposited plus exported volume.
- Surface identity holds after centimeter quantization and safety clamping.
- Final hydrology is recomputed from current surface, not reused from the first pass.
- The generator does not inspect unrelated geologic potentials or climate wind/temperature.

**Command**

```powershell
cargo test --test hydro_erosion_generation
```

**Implementation**

- Add a pure `FluvialErosionGenerator`.
- Compute bounded stream-power response from first-pass discharge, filled-surface slope, and resistance.
- Route sediment in stable upstream-to-downstream order with bounded local capacity.
- Adjust stored components after clamping so the surface identity remains true.
- Add `HydroErosionGenerator` that runs hydrology, erosion, then final hydrology exactly once each.
- Validate the composite before returning.

**Commit**

```text
feat: generate hydro eroded landscape
```

## Task 10: Publish the atomic stage and extend the production graph

**Files**

- Create: `src/generators/natural/hydro_erosion_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/stage.rs`
- Create: `tests/hydro_erosion_stage.rs`
- Modify: `tests/natural_stage_graph.rs`
- Modify: `tests/geologic_stage.rs`
- Modify: `tests/climate_stage.rs`

**Red tests**

- Artifact key is `world.hydro-erosion`.
- Stage ID/version/namespace are exact.
- Dependencies are exactly resolved hydro input, spatial, relief, geology, and preliminary climate.
- Production graph has 15 stages and 7 externals.
- A repeated build gets all 15 cache hits.
- Changing only hydro spec reruns exactly 3 stages.
- Changing climate spec reruns climate resolution/projection/generation plus hydro stages.
- Changing geologic spec reruns geology resolution/projection/generation plus hydro stages.
- Invalid cross-artifact cache data cannot poison a valid result.

**Command**

```powershell
cargo test --test hydro_erosion_stage --test natural_stage_graph --test geologic_stage --test climate_stage
```

**Implementation**

- Add `HydroErosionArtifact` with validated artifact boundary.
- Add a thin stage adapter.
- Register external and three stages in dependency order.
- Update existing exact graph/cache expectations.

**Commit**

```text
feat: publish hydro erosion stage
```

## Task 11: Expose formal fields without leaking display concerns

**Files**

- Modify: `src/world/natural/fields.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `src/app/natural_display.rs`
- Modify: `tests/natural_field_views.rs`

**Red tests**

- Registry contains exactly 36 stable fields.
- Nine new IDs, kinds, domains, units, ranges, decimal hints, palettes, and dependencies match the design.
- Surface-water and Strahler categories are complete.
- All nine payloads borrow formal arrays without copying.
- Current surface uses an explicit sea-level-symmetric preferred range.
- Constructional elevation remains available and unchanged.
- No receiver IDs, flood ranks, display-normalized discharge, or monthly arrays masquerade as public V1 fill fields.

**Command**

```powershell
cargo test --test natural_field_views
```

**Implementation**

- Register nine schemas in stable ID order.
- Extend `NaturalFieldDocument` to own the composite artifact.
- Add borrowed payload adapters.
- Prefer current surface for a newly created natural document.
- Do not implement river-line GPU rendering in this task.

**Commit**

```text
feat: expose hydro erosion fields
```

## Task 12: Integrate the current surface atomically into the application

**Files**

- Modify: `src/app.rs`
- Modify: `src/app/natural_display.rs`
- Extend in-module app/document tests.

**Red tests**

- Default externals include `HydroErosionSpec`.
- Successful rebuild retrieves and cross-validates the composite.
- Document publication owns spatial, tectonic, mantle, relief, geology, climate, and hydro-erosion artifacts.
- Current-surface selection survives rebuild.
- Failed builds preserve the previous document, display packet, selected field, revision clock, and hydro artifact.
- Header text includes water/hydrology without implying history or final climate.
- Default field is `surface_elevation_m`.

**Command**

```powershell
cargo test --lib app::
```

**Implementation**

- Add the seventh external.
- Retrieve and validate the new artifact before document construction.
- Preserve the existing atomic candidate-then-publish path.
- Update only application copy and status text.

**Commit**

```text
feat: integrate hydro erosion display
```

## Task 13: Add quality gates, reviewed goldens, performance, and CI

**Files**

- Modify: `tests/natural_display_golden.rs`
- Add reviewed PNGs under `tests/golden/natural-foundation/`.
- Modify: `tests/natural_performance.rs`
- Modify: `.github/workflows/rust.yml`

**Red tests**

- Eight fixed seeds satisfy receiver reachability, nonzero drainage, branching river, lake, erosion, sediment, and no-speckle gates.
- Reviewed goldens cover:
  - current surface;
  - surface water;
  - Strahler order;
  - erosion depth;
  - deposition thickness.
- Goldens use truthful schema/data/manual ranges, never a display-only world field.
- 20,000-cell release stage time is at most 350 ms.
- Added dense memory is at most 8 MiB.
- CI explicitly names rule, generation, and stage integration tests.

**Commands**

```powershell
cargo test --test natural_display_golden
cargo test --release --test natural_performance -- --nocapture
```

**Implementation**

- Add deterministic metrics before tuning visual output.
- Tune only compiled model constants, never inject cosmetic noise.
- Regenerate goldens through the ignored reviewed command.
- Inspect every new PNG at original resolution before committing.

**Commit**

```text
test: verify hydro erosion quality
```

## Task 14: Verify, self-review, merge, push, and continue

**Architecture scans**

```powershell
rg -n "crate::(app|ui|gpu|terrain|engine|rules|generators)" src/world
rg -n "crate::(app|ui|gpu|terrain)" src/generators/natural/hydrology.rs src/generators/natural/erosion.rs src/generators/natural/hydro_erosion.rs
rg -n "history|timeline|event_year|flood_event|erosion_year" src/world/natural src/generators/natural src/rules
```

**Full verification**

```powershell
cargo test --workspace --all-targets
cargo test --workspace --all-targets --release
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets --all-features
cargo fmt --all -- --check
git diff --check
$env:RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
cargo check --workspace --all-features --lib --target wasm32-unknown-unknown
Remove-Item Env:RUSTFLAGS
trunk build
cargo build --release --bin sekai
```

**Desktop acceptance**

- Stop only the exact currently running main executable after verifying its path.
- Start the exact release executable from this feature worktree.
- Inspect current surface, surface water, Strahler order, erosion, deposition, runoff, and discharge.
- Confirm maps are causal, irregular, and free of ellipse/noise artifacts.
- Trigger a new-seed rebuild.
- Confirm seed/statistics change, selected hydro field survives, and no error is published.

**Self-review**

- Inspect `main...HEAD`, all production `expect`/`unwrap`, module imports, stage dependencies, serialized schema changes, and repository cleanliness.
- Fetch `origin/main`; integrate any legitimate upstream changes without force.
- Re-run focused stage/golden/check gates after merge.

**Finish**

- Merge with `--no-ff`.
- Push `main`.
- Fetch and verify local/remote hashes are identical.
- Stop only the exact feature executable.
- Remove only the clean `hydro-erosion-foundation` worktree and merged feature branch.
- Rebuild and start the exact main executable.
- Inspect one hydro field in the main build and leave it running.
- If no major direction decision has appeared, update the plan and continue to the next approved causal slice.

---

## Completion criteria

- `main` and `origin/main` contain the same merge commit.
- Production graph is 15 stages / 7 externals.
- Registry is 36 fields.
- All formal snapshots validate on construction and deserialization.
- Every receiver is adjacent and the drainage graph is acyclic.
- Monthly water and sediment conserve within explicit tolerance.
- Current surface has a single writer and constructional relief remains immutable.
- No legacy terrain, history, UI, or GPU dependency crosses into formal generation.
- All debug/release/clippy/fmt/WASM/Trunk/performance/golden/desktop gates pass.
- The running application is the exact main-branch release executable.
