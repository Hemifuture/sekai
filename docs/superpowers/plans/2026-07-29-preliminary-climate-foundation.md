# Preliminary Climate Foundation Implementation Plan

> Execute in this isolated worktree with test-first commits. The user has authorized routine design decisions, self-review, merge, and push. Stop only for a genuine product-direction conflict.

**Goal:** Add a deterministic, bounded, monthly preliminary-climate slice that supplies future hydrology with typed temperature, precipitation, and wind while preserving current-slice, single-writer, and module-boundary constraints.

**Architecture:** Add a fixed-point `ClimateSpec`, a unique trusted climate-model capability, full rule audit plus minimal projection, a pure low-resolution monthly climate generator, a validated `PreliminaryClimateSnapshot`, one production stage, formal annual-summary fields, and atomic application display integration. Climate reads only resolved climate input, spatial topology, and relief.

**Tech stack:** Rust 2021, serde, thiserror, existing stage engine/cache, egui/wgpu field display, PNG reference rasterizer, native and wasm32 builds.

**Design:** `docs/superpowers/specs/2026-07-29-preliminary-climate-foundation-design.md`

---

## Global constraints

- Do not import `src/terrain` into the formal natural pipeline.
- Do not create history dates, weather events, final-climate fields, hydrology placeholders, or empty extension traits.
- Every authoritative climate field has exactly one writer.
- All public serialized types validate on construction and deserialization.
- Use fixed iteration bounds and stable traversal/tie-breaking.
- Keep `world` independent of engine/rules/generators/app/UI.
- Keep climate generation independent of app/UI/GPU and unrelated natural domains.
- Preserve unrelated user work and the existing `field-display-system` worktree.
- Run the named failing test before each implementation step.
- Commit only after the focused test set passes.

## Task 1: Define the fixed-point climate specification

**Files**

- Create: `src/world/natural/climate_spec.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/climate_spec.rs`

**Red tests**

- Default values exactly match the design.
- Valid boundary values pass.
- Unsupported schema, reversed/too-small latitude span, out-of-range latitude, tilt, temperature offset, and moisture scale fail with precise variants.
- JSON deserialization revalidates the contract.
- Conversion accessors return degrees, Celsius, and scale without exposing mutable float state.

**Command**

```powershell
cargo test --test climate_spec
```

**Implementation**

- Add constants and `ClimateSpec`.
- Store public authoring values as fixed-point integers.
- Add `ClimateSpecError`.
- Implement custom `Deserialize` that validates.
- Re-export only the stable public contract.

**Commit**

```text
feat: define preliminary climate spec
```

## Task 2: Register the trusted climate model capability

**Files**

- Modify: `src/rules/capability.rs`
- Modify: `src/rules/builtin.rs`
- Modify: `src/rules/mod.rs`
- Modify: `tests/rule_capabilities.rs`
- Modify: `tests/builtin_rules.rs`
- Modify: `tests/rule_manifests.rs`

**Red tests**

- Stable ID is `sekai.core.natural.climate-model@1`.
- Descriptor is `UniqueRequired`, `WorldLaw`, author-disallowed.
- `ClimateModel::SeasonalEnergyMoistureV1` is a closed typed contribution.
- Earthlike pack provides tectonic, geologic, and climate models in canonical order.
- Duplicate climate contributions are rejected as duplicate unique capability contributions.

**Command**

```powershell
cargo test --test rule_capabilities --test builtin_rules --test rule_manifests
```

**Implementation**

- Add capability ID constructor.
- Add `ClimateModel`.
- Extend `CapabilityContribution` matching, validation, item IDs, and uniqueness.
- Register descriptor and contribution in built-ins.
- Update exhaustive matches without wildcard escape hatches.

**Commit**

```text
feat: register preliminary climate world law
```

## Task 3: Resolve climate rules into a complete audit

**Files**

- Create: `src/rules/climate.rs`
- Modify: `src/rules/mod.rs`
- Create: `tests/rule_climate_resolution.rs`

**Red tests**

- Resolution records canonical participating pack identities, model, and exact spec.
- Missing and multiple climate models fail.
- Invalid base/resolved specs fail.
- Duplicate serialized pack identities and unsupported audit schema fail on deserialize.
- Tectonic constraints and geologic contributions are ignored by climate resolution without being dropped from the overall pack audit.

**Command**

```powershell
cargo test --test rule_climate_resolution
```

**Implementation**

- Mirror the established geologic audit boundary, with climate-specific error codes and types.
- Keep the resolver pure and engine-independent.

**Commit**

```text
feat: resolve preliminary climate rules
```

## Task 4: Add engine transport and minimal climate projection

**Files**

- Create: `src/generators/natural/climate_rule_input.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/rule_climate_stage.rs`
- Modify: `tests/rule_stage_graph.rs`

**Red tests**

- Artifact keys are exact and stable.
- Rule stage dependencies are exactly climate spec plus pack set.
- Projection depends only on climate audit.
- Audit-only pack changes can change the audit hash while preserving the projected-input hash.
- Invalid serialized spec/audit/input cannot publish.
- Neither stage consumes spatial, relief, geology, UI, or author tectonic constraints directly.

**Command**

```powershell
cargo test --test rule_climate_stage --test rule_stage_graph
```

**Implementation**

- Add `ClimateSpecArtifact`.
- Add `ClimateRuleResolutionArtifact`.
- Add `ResolvedClimateInput` and artifact.
- Add resolution and projection stages with typed `StageInputs`.
- Reuse only generic rule-stage error adapters; do not couple to tectonic/geologic internals.

**Commit**

```text
feat: project preliminary climate input
```

## Task 5: Define monthly preliminary-climate contracts

**Files**

- Create: `src/world/natural/climate.rs`
- Modify: `src/world/natural/mod.rs`
- Create: `tests/climate_contracts.rs`

**Red tests**

- Monthly scalar/vector fields accept exactly finite dense data.
- Snapshot rejects unsupported schema, length mismatch, non-finite values, range violations, and summary identity mismatches.
- Snapshot validates against spatial and relief cell counts.
- JSON deserialization revalidates every invariant.
- Per-cell/per-month accessors use stable zero-based month indices and reject month 12.
- Annual mean, total, seasonality, and vector mean identities use the documented tolerance.

**Command**

```powershell
cargo test --test climate_contracts
```

**Implementation**

- Add `CLIMATE_MONTH_COUNT = 12`.
- Add `MonthlyScalarField`, `MonthlyVectorField`, and `PreliminaryClimateSnapshot`.
- Keep arrays private; expose immutable slices and typed accessors.
- Add range constants and a single exhaustive validation error enum.

**Commit**

```text
feat: define preliminary climate snapshot
```

## Task 6: Implement the bounded monthly climate generator

**Files**

- Create: `src/generators/natural/climate.rs`
- Modify: `src/generators/natural/mod.rs`
- Create: `tests/climate_generation.rs`

**Red tests**

- Same inputs produce byte-identical output.
- Grid budget remains within 16..=4096 for minimum, default, and maximum world cell budgets.
- Latitude mapping hits configured south/north limits within cell-site bounds.
- Northern and southern seasonal phases are opposite.
- High latitude is colder than low latitude after comparing similar elevations.
- Higher terrain is colder than nearby lower terrain.
- Ocean/high-maritime cells have lower temperature seasonality than interior land.
- Precipitation is finite, non-degenerate, and responds to moisture scale.
- A synthetic ridge fixture produces a wetter windward side and drier leeward side.
- All-land and all-ocean fixtures take explicit safe paths.
- Generator never mutates relief and never reads geology.

**Command**

```powershell
cargo test --test climate_generation
```

**Implementation sequence**

1. Add deterministic climate-grid sizing.
2. Aggregate area-weighted relief and land fraction.
3. Fill empty bins by stable four-neighbor wavefront.
4. Calculate latitude, monthly solar geometry, and smooth circulation bands.
5. Calculate maritime distance/influence.
6. Run fixed-bound synchronous water-vapor transport.
7. Project monthly fields back to cells and derive summaries.
8. Construct only through the validated snapshot.

**Commit**

```text
feat: generate preliminary monthly climate
```

## Task 7: Publish the preliminary climate stage

**Files**

- Create: `src/generators/natural/climate_stage.rs`
- Modify: `src/generators/natural/mod.rs`
- Modify: `src/generators/natural/stage.rs`
- Create: `tests/climate_stage.rs`
- Modify: `tests/natural_stage_graph.rs`
- Modify: `tests/foundation_build.rs`

**Red tests**

- Artifact key is `world.preliminary-climate`.
- Stage ID is `natural.preliminary-climate`, version 1.
- Dependencies are exactly resolved climate input, spatial, and relief.
- Complete graph contains 12 stages and 6 external artifacts.
- Second identical build reports 12 cache hits.
- Climate-spec change preserves all upstream artifact hashes and reruns only 3 climate stages.
- Climate output changes when relief changes.
- Geology-only artifacts are not a declared climate dependency.
- Invalid cross-artifact input publishes nothing and does not poison prior valid cache entries.

**Command**

```powershell
cargo test --test climate_stage --test natural_stage_graph --test foundation_build
```

**Implementation**

- Add artifact, typed inputs, stage, and error mapping.
- Register rule, projection, and climate stages after their real dependencies.
- Preserve the public graph function name for compatibility.

**Commit**

```text
feat: publish preliminary climate stage
```

## Task 8: Register climate fields and zero-copy views

**Files**

- Modify: `src/world/natural/fields.rs`
- Modify: `src/world/natural/mod.rs`
- Modify: `tests/field_contracts.rs`
- Modify: `tests/natural_field_views.rs`

**Red tests**

- Registry grows from 21 to 27 schemas.
- IDs, units, ranges, palettes, decimal places, domains, and dependencies exactly match the design.
- Five scalar climate fields are cell-fillable.
- Prevailing wind remains inspectable but not cell-fillable in display V1.
- Annual arrays are borrowed from the snapshot without copies.
- Monthly arrays are not incorrectly flattened into unrelated field IDs.

**Command**

```powershell
cargo test --test field_contracts --test natural_field_views
```

**Implementation**

- Add six stable ID constructors.
- Add schemas in dependency-coherent order.
- Keep the registry generic and free of generator logic.

**Commit**

```text
feat: expose preliminary climate fields
```

## Task 9: Atomically integrate climate into the application document

**Files**

- Modify: `src/app.rs`
- Modify: `src/app/natural_display.rs`
- Modify: tests inside those modules
- Modify: `tests/field_display_integration.rs` if the public fixture requires it

**Red tests**

- External-artifact builder includes default climate spec.
- Rebuild retrieves and validates preliminary climate before publishing.
- `NaturalFieldDocument` owns all six artifacts.
- Climate payload pointers equal authoritative annual arrays.
- Existing selection remains stable across rebuilds.
- Default field remains elevation.
- Header states that preliminary climate is part of the current slice.
- Failed climate validation leaves the prior document and display packet untouched.

**Command**

```powershell
cargo test app:: --lib
cargo test --test field_display_integration
```

**Implementation**

- Add `ClimateSpec` to app-owned generation inputs without adding climate controls.
- Add artifact retrieval and document validation.
- Extend payload list only; keep rendering and palette logic unchanged.

**Commit**

```text
feat: integrate preliminary climate display
```

## Task 10: Add climate quality, goldens, performance, and CI coverage

**Files**

- Modify: `tests/natural_display_golden.rs`
- Add reviewed PNGs under `tests/golden/natural-foundation/`
- Modify: `src/bin/generate_screenshots.rs`
- Modify: `tests/natural_performance.rs`
- Modify: `.github/workflows/rust.yml`

**Red tests**

- Multi-seed suite validates climate snapshot and physical quality gates.
- Goldens include mean temperature, annual precipitation, maritime influence, and temperature seasonality.
- Golden generation remains opt-in.
- Performance output includes climate-stage time and climate dense bytes.
- CI runs focused climate generation and climate stage tests explicitly.

**Commands**

```powershell
cargo test --test natural_display_golden quality_across_fixed_seed_set -- --nocapture
$env:SEKAI_UPDATE_NATURAL_GOLDENS='1'; cargo test --test natural_display_golden regenerate_natural_goldens -- --ignored --nocapture
Remove-Item Env:SEKAI_UPDATE_NATURAL_GOLDENS
cargo test --test natural_display_golden reviewed_natural_goldens_match
cargo test --release --test natural_performance profile_default_natural_foundation -- --ignored --nocapture
```

**Review**

- Open every new PNG at original resolution.
- Reject horizontal banding without topographic/coastal modulation, single-color fields, salt-and-pepper noise, or unexplained compact ellipses.

**Commit**

```text
test: verify preliminary climate quality
```

## Task 11: Run architectural and platform verification

**Boundary scans**

```powershell
rg -n "crate::(app|ui|gpu|terrain|rules|engine|generators)" src/world
rg -n "crate::(app|ui|gpu|terrain)" src/generators/natural/climate.rs src/generators/natural/climate_stage.rs
rg -n "crate::generators" src/rules
rg -n "history|timeline|event_year|weather_event" src/world/natural src/generators/natural src/rules
```

Expected: no new forbidden imports or history/time-event framework.

**Full gates**

```powershell
cargo test --workspace --all-targets
cargo test --workspace --all-targets --release
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets --all-features
$env:RUSTFLAGS='--cfg getrandom_backend="wasm_js"'; cargo check --workspace --all-features --lib --target wasm32-unknown-unknown
Remove-Item Env:RUSTFLAGS
trunk build
git diff --check
```

**Desktop acceptance**

1. Build release desktop binary in the feature worktree.
2. Stop only the previously verified Sekai process whose executable path is the main-worktree binary.
3. Launch the feature-worktree binary visibly.
4. Inspect accessibility state and screenshots.
5. Switch among mean temperature, annual precipitation, maritime influence, and seasonality.
6. Verify seed and stage summary remain unchanged on field-only switches.
7. Trigger a new-seed rebuild.
8. Verify seed/statistics change, all climate fields remain valid, and no error is shown.

## Task 12: Commit, merge, push, and relaunch main

**Pre-merge**

```powershell
git status --short --branch
git log --oneline --decorate main..HEAD
git fetch origin main
git rev-list --left-right --count main...origin/main
```

**Merge**

```powershell
git -C <main-worktree> merge --no-ff feature/climate-foundation -m "merge: preliminary climate foundation"
```

**Post-merge focused gates**

```powershell
cargo test --test climate_stage
cargo test --test natural_display_golden
cargo check --workspace --all-targets --all-features
```

**Push and verify**

```powershell
git push origin main
git fetch origin main
git rev-parse main
git rev-parse origin/main
```

Remove only the clean, exact climate worktree and merged feature branch. Do not touch `.worktrees/field-display-system`.

Build `target/release/sekai.exe` from merged main, launch it visibly, verify its exact executable path, inspect one climate field, and leave it running for the user.

## Completion evidence

Report:

- merge commit and remote equality;
- stage/external/field counts;
- focused and full verification results;
- default-world climate performance and dense-byte figures;
- desktop field-switch/rebuild observations;
- exact running main executable and PID;
- any remaining issue that truly requires a product-direction decision.
