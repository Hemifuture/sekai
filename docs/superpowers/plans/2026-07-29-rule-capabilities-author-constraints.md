# Rule Capabilities and Author Constraints Implementation Plan

> **For Codex:** Execute this plan task by task with TDD. Do not introduce a generic scripting or arbitrary-value API. Keep every commit independently formatted, linted, and testable.

**Goal:** Add a deterministic, data-only rule-pack system and typed author constraints, resolve them into audited tectonic decisions, project the minimum natural-generation input, and route the default application through that path without changing the existing natural output.

**Architecture:** Pure rule contracts live in `src/rules/**` and depend only on `world`. Engine transport and stage composition live in `src/generators/natural/rule_input.rs`. A full audit artifact is projected into a minimal model/spec artifact before `TectonicStage`, so audit-only changes do not invalidate tectonics or relief. The default app supplies one built-in earthlike world-law pack and an empty author-constraint set.

**Tech Stack:** Rust 1.85, serde/serde_json with float round-trip, thiserror, BLAKE3, the existing typed artifact/stage/cache engine, the existing natural generator, egui/eframe/wgpu only at the app/view boundary, native and `wasm32-unknown-unknown`.

**Design:** `docs/superpowers/specs/2026-07-29-rule-capabilities-author-constraints-design.md`

## Global Constraints

- Keep the single-package layout.
- Follow red-green-refactor for every behavioral change.
- `src/rules/**` may import only `crate::world`, serde, serde_json, thiserror, BLAKE3, and standard-library data structures.
- `src/rules/**` must not import engine, generators, app, UI, view, GPU, egui, eframe, wgpu, old `terrain`, file APIs, network APIs, wall-clock time, or random APIs.
- Rule data is a closed typed set. Do not use `serde_json::Value`, `Any`, callbacks, trait objects supplied by packs, arbitrary maps, scripts, or raw byte payloads.
- Input Vec order must not affect serialization, hashes, dependency order, conflict order, decisions, or downstream random streams.
- Use stable vectors and ordered collections for externally observable order.
- Use bounded integer weights and checked integer score accumulation.
- Keep `TectonicSnapshot` and `ReliefSnapshot` schemas unchanged.
- Keep existing natural field IDs and golden images unchanged.
- `TectonicStage` must not see rule-pack IDs, author IDs, manifests, or adoption records.
- Candidate application builds publish document, display packet, rule summary, and revision clock atomically.
- Do not add rule editing UI, magic content, physical forcing, project storage, or history in this slice.
- Public contracts and error variants require concise rustdoc.
- No delegated/sub-agent review is authorized for this session; use a fresh local self-review.
- Each implementation task ends with focused tests, `cargo fmt --all -- --check`, relevant Clippy, and an intentional commit.

## Target File Map

### New pure rule modules

- `src/rules/mod.rs` — public exports and module boundary.
- `src/rules/ids.rs` — IDs, versions, compatibility range, content hash.
- `src/rules/constraints.rs` — strength, typed tectonic clauses, sources, author collections.
- `src/rules/capability.rs` — descriptors, permissions, cardinality, typed contribution enum.
- `src/rules/manifest.rs` — manifest, rule pack, stable content hashing, structural budgets.
- `src/rules/registry.rs` — pack-set dependency and capability resolution.
- `src/rules/tectonics.rs` — deterministic tectonic-control solver and audit records.
- `src/rules/builtin.rs` — core capability registry and earthlike built-in pack.

### New engine adapter

- `src/generators/natural/rule_input.rs` — external artifacts, full resolution stage, minimal projection stage.

### Existing files modified

- `src/lib.rs` — exports `rules`.
- `src/world/natural/spec.rs` — gives `TectonicActivity` stable ordering.
- `src/generators/natural/mod.rs` — exports rule-input artifacts and stages.
- `src/generators/natural/stage.rs` — makes tectonics consume the minimal projected input and extends the graph.
- `src/app.rs` — supplies rule inputs, extracts a summary, and publishes it atomically.
- `.github/workflows/rust.yml` — adds focused rule integration coverage.

### New tests

- `tests/rule_ids.rs`
- `tests/author_constraints.rs`
- `tests/rule_capabilities.rs`
- `tests/rule_manifests.rs`
- `tests/rule_pack_resolution.rs`
- `tests/rule_tectonic_resolution.rs`
- `tests/builtin_rules.rs`
- `tests/rule_stage_graph.rs`

### Existing tests updated

- `tests/natural_stage_graph.rs`
- `tests/natural_field_views.rs`
- `tests/natural_display_golden.rs`
- `tests/natural_performance.rs`
- app unit tests in `src/app.rs`
- natural display unit fixtures in `src/app/natural_display.rs`

## Task 1: Define Stable Rule Identities and Versions

**Files:**

- Create `src/rules/mod.rs`
- Create `src/rules/ids.rs`
- Modify `src/lib.rs`
- Create `tests/rule_ids.rs`

**Produces:**

- `RulePackId`
- `CapabilityId`
- `RuleItemId`
- `RuleVersion`
- `RuleVersionRequirement`
- `CoreSchemaRange`
- `RuleContentHash`
- bounded identifier validation and serde revalidation

- [ ] **Step 1: Write failing identity tests**

Cover:

- valid minimum and representative IDs;
- empty, overlong, uppercase, whitespace, slash, bad leading/trailing punctuation;
- capability version zero;
- rule version major zero;
- version compatibility at exact, newer minor/patch, wrong major, and older version;
- schema range zero/reversed/current inclusion;
- deterministic JSON round-trip;
- invalid private state rejected during deserialization;
- content-hash byte access and JSON round-trip.

Run:

```powershell
cargo test --test rule_ids
```

Expected: FAIL because `sekai::rules` does not exist.

- [ ] **Step 2: Implement the smallest validated contracts**

Implementation notes:

- store owned strings privately;
- expose borrowed accessors;
- custom `Deserialize` must call public constructors;
- use one private identifier validator inside `rules::ids`;
- `CapabilityId` stores namespace, name, and non-zero version;
- `RuleContentHash` accepts only already-computed `[u8; 32]` inside the rule module.

- [ ] **Step 3: Export the rules boundary**

Add documented `pub mod rules;` in `src/lib.rs` and narrow re-exports in `src/rules/mod.rs`.

- [ ] **Step 4: Verify**

```powershell
cargo test --test rule_ids
cargo fmt --all -- --check
cargo clippy --test rule_ids -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/lib.rs src/rules/mod.rs src/rules/ids.rs tests/rule_ids.rs
git commit -m "feat: define stable rule identities"
```

## Task 2: Define Typed Constraint Contracts

**Files:**

- Create `src/rules/constraints.rs`
- Modify `src/rules/mod.rs`
- Modify `src/world/natural/spec.rs`
- Create `tests/author_constraints.rs`

**Produces:**

- `ConstraintStrength`
- `InclusiveU16Range`
- `ActivitySet`
- `TectonicConstraintClause`
- `ConstraintSource`
- `RuleTectonicConstraint`
- `AuthorConstraint`
- `AuthorConstraints`

- [ ] **Step 1: Write failing contract tests**

Cover:

- hard constraints contain no meaningless weight;
- soft/hint weights accept 1 and 1000, reject 0;
- plate ranges stay inside `MIN_PLATE_COUNT..=MAX_PLATE_COUNT`;
- fraction ranges stay inside the supported permille domain;
- reversed ranges fail;
- activity sets normalize input order and reject empty/duplicates after malicious deserialization;
- clauses report their stable target;
- rule item IDs are unique within a pack contribution list;
- author IDs are sorted and duplicate IDs fail;
- author-constraint set enforces schema and 4096-entry budget;
- all types round-trip and revalidate.

Run:

```powershell
cargo test --test author_constraints
```

Expected: FAIL because the contracts are absent.

- [ ] **Step 2: Give activity values stable order**

Add `PartialOrd` and `Ord` to `TectonicActivity`. Do not change variants or serialized names.

- [ ] **Step 3: Implement validated constraint types**

Implementation notes:

- ranges use private fields and typed constructors;
- fraction ranges are integer permille;
- `ActivitySet` stores sorted unique values;
- `ConstraintSource::RulePack` contains both pack and item IDs;
- `AuthorConstraints::new` sorts by `AuthorObjectId`;
- empty constraints are the default;
- custom deserialization reuses constructors.

- [ ] **Step 4: Verify**

```powershell
cargo test --test author_constraints
cargo test --test natural_spec
cargo fmt --all -- --check
cargo clippy --test author_constraints -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/rules src/world/natural/spec.rs tests/author_constraints.rs
git commit -m "feat: define typed author constraints"
```

## Task 3: Define Capabilities and Closed Contributions

**Files:**

- Create `src/rules/capability.rs`
- Modify `src/rules/mod.rs`
- Create `tests/rule_capabilities.rs`

**Produces:**

- `RulePackKind`
- `CapabilityCardinality`
- `CapabilityDescriptor`
- `TectonicModel`
- `CapabilityContribution`
- `CapabilityRegistry` and builder

- [ ] **Step 1: Write failing capability tests**

Cover:

- descriptor IDs are unique;
- empty registry is valid but immutable after build;
- duplicate descriptors fail atomically;
- `WorldLaw` satisfies an ordinary minimum, ordinary does not satisfy world-law minimum;
- author permission is explicit;
- each contribution returns the exact capability ID;
- a tectonic-model contribution cannot masquerade as controls;
- registry iteration is stable;
- registry deserialization revalidates and rejects duplicates.

Run:

```powershell
cargo test --test rule_capabilities
```

Expected: FAIL.

- [ ] **Step 2: Implement descriptors and registry**

Use a builder/frozen-registry pattern analogous to field schemas, but do not reuse `FieldRegistry` because capabilities have different invariants.

- [ ] **Step 3: Implement the closed contribution enum**

Only:

- `TectonicModel(TectonicModel::CurrentSliceV1)`
- `TectonicConstraint(RuleTectonicConstraint)`

Contribution ordering must be stable by capability, then local item identity/payload.

- [ ] **Step 4: Verify**

```powershell
cargo test --test rule_capabilities
cargo fmt --all -- --check
cargo clippy --test rule_capabilities -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/rules tests/rule_capabilities.rs
git commit -m "feat: register typed rule capabilities"
```

## Task 4: Build Validated Rule Packs and Content Hashes

**Files:**

- Create `src/rules/manifest.rs`
- Modify `src/rules/mod.rs`
- Create `tests/rule_manifests.rs`

**Produces:**

- `RulePackDependency`
- `RulePackManifest`
- `RulePack`
- stable content frame and hash verification
- per-pack safety budgets

- [ ] **Step 1: Write failing manifest tests**

Cover:

- dependencies and consumed capabilities normalize input order;
- contributions normalize input order;
- `provides` is derived from contributions and sorted unique;
- duplicate dependency, consume, or local rule item fails;
- dependency/consume/contribution budgets fail at the boundary;
- identical semantic content has identical BLAKE3 hash;
- changing one semantic value changes the hash;
- JSON round-trip preserves identity;
- tampering with content while retaining a hash is rejected;
- tampering with the hash is rejected;
- non-semantic input Vec order does not alter bytes or hash.

Run:

```powershell
cargo test --test rule_manifests
```

Expected: FAIL.

- [ ] **Step 2: Implement pack construction**

Implementation notes:

- validate and sort before hashing;
- stream serde JSON into BLAKE3 rather than building an unbounded string;
- exclude only the hash field from the content frame;
- manifest fields stay private and read-only;
- custom `Deserialize` reconstructs and compares the declared hash;
- do not add signature or filesystem concepts.

- [ ] **Step 3: Verify**

```powershell
cargo test --test rule_manifests
cargo fmt --all -- --check
cargo clippy --test rule_manifests -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add src/rules tests/rule_manifests.rs
git commit -m "feat: validate rule pack manifests"
```

## Task 5: Resolve Pack Dependencies Deterministically

**Files:**

- Create `src/rules/registry.rs`
- Modify `src/rules/mod.rs`
- Create `tests/rule_pack_resolution.rs`

**Produces:**

- `RulePackSet`
- `ResolvedRulePackSet`
- stable dependency order
- dependency and set-level errors

- [ ] **Step 1: Write failing dependency tests**

Cover:

- input pack order normalizes by ID;
- duplicate pack ID fails;
- 64-pack boundary and overflow;
- missing dependency;
- incompatible dependency version;
- direct self-dependency;
- two-node and longer cycles;
- cycle error names the stable minimum member;
- independent packs sort by ID;
- dependency always precedes consumer;
- reverse input produces identical resolved order and serialized set;
- malicious deserialization revalidates.

Run:

```powershell
cargo test --test rule_pack_resolution dependency
```

Expected: FAIL.

- [ ] **Step 2: Implement set construction and stable Kahn ordering**

Use `BTreeMap`/`BTreeSet`, bounded vectors, and no recursive DFS.

- [ ] **Step 3: Verify the dependency subset**

```powershell
cargo test --test rule_pack_resolution dependency
cargo fmt --all -- --check
cargo clippy --test rule_pack_resolution -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add src/rules tests/rule_pack_resolution.rs
git commit -m "feat: resolve rule pack dependencies"
```

## Task 6: Enforce Capability Permissions and Cardinality

**Files:**

- Modify `src/rules/registry.rs`
- Modify `tests/rule_pack_resolution.rs`

**Produces:**

- capability validation in `RulePackSet::resolve`
- typed provider lookup in `ResolvedRulePackSet`

- [ ] **Step 1: Add failing capability-resolution tests**

Cover:

- unknown capability contribution;
- unknown consumed capability;
- ordinary pack providing world-law capability;
- world-law pack providing ordinary capability;
- missing consumed capability;
- required unique capability missing;
- unique capability with two providers;
- merge capability with several providers;
- exact provider list sorted by pack ID;
- contribution/payload capability mismatch cannot enter a validated pack;
- capability registry input order does not alter resolution.

Run:

```powershell
cargo test --test rule_pack_resolution capability
```

Expected: FAIL.

- [ ] **Step 2: Implement capability checks**

Run checks after dependency validation and before exposing a resolved set. Return typed IDs in every error.

- [ ] **Step 3: Verify**

```powershell
cargo test --test rule_pack_resolution
cargo fmt --all -- --check
cargo clippy --test rule_pack_resolution -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add src/rules/registry.rs tests/rule_pack_resolution.rs
git commit -m "feat: enforce rule capability contracts"
```

## Task 7: Resolve Tectonic Constraints and Produce Audit Records

**Files:**

- Create `src/rules/tectonics.rs`
- Modify `src/rules/mod.rs`
- Create `tests/rule_tectonic_resolution.rs`

**Produces:**

- `TectonicRuleResolver`
- `TectonicRuleResolution`
- `ResolvedRulePackRef`
- `ConstraintAdoption`
- hard/soft/hint solver

- [ ] **Step 1: Write failing no-op and hard-constraint tests**

Cover:

- no controls preserves all base spec values exactly, including arbitrary valid `f32` bits;
- one hard range narrows plate count;
- overlapping hard ranges intersect;
- disjoint hard ranges fail;
- hard activity sets intersect;
- conflict sources are sorted and include every hard source on the target;
- rule and author hard constraints share the same solver;
- invalid base spec fails before resolution.

Run:

```powershell
cargo test --test rule_tectonic_resolution hard
```

Expected: FAIL.

- [ ] **Step 2: Implement finite candidate domains and hard filtering**

Do not use RNG or floats for candidate feasibility.

- [ ] **Step 3: Add failing soft/hint tests**

Cover:

- soft constraints choose minimum weighted penalty;
- a hint cannot defeat a soft preference;
- hint breaks a soft tie;
- base value breaks a soft/hint tie;
- stable candidate order breaks a complete tie;
- checked scores remain inside bounds at maximum constraint count;
- fraction output is quantized only when constrained;
- input constraint order does not affect the decision.

Run:

```powershell
cargo test --test rule_tectonic_resolution preference
```

Expected: FAIL.

- [ ] **Step 4: Implement lexicographic scoring**

Score tuple:

```text
(soft_penalty, hint_penalty, base_distance, stable_candidate)
```

Use checked `u64` multiply/add and return a typed overflow error.

- [ ] **Step 5: Add adoption and serialization tests**

Cover:

- hard records are satisfied;
- satisfied and compromised soft/hint records are correct;
- adoption order is stable by source and target;
- resolved packs retain ID/version/hash in dependency order;
- resolution JSON round-trips and revalidates;
- invalid serialized audit state is rejected.

- [ ] **Step 6: Verify**

```powershell
cargo test --test rule_tectonic_resolution
cargo fmt --all -- --check
cargo clippy --test rule_tectonic_resolution -- -D warnings
```

Expected: PASS.

- [ ] **Step 7: Commit**

```powershell
git add src/rules tests/rule_tectonic_resolution.rs
git commit -m "feat: resolve typed tectonic constraints"
```

## Task 8: Add Built-In Capabilities and Earthlike World Law

**Files:**

- Create `src/rules/builtin.rs`
- Modify `src/rules/mod.rs`
- Create `tests/builtin_rules.rs`

**Produces:**

- `tectonic_model_capability_id`
- `tectonic_controls_capability_id`
- `core_capability_registry`
- `earthlike_rule_pack`
- `default_rule_pack_set`

- [ ] **Step 1: Write failing built-in tests**

Cover:

- exact stable IDs and versions;
- model capability is required unique/world-law/no-author;
- controls capability is merge/ordinary/author-allowed;
- earthlike pack is world-law and provides exactly `CurrentSliceV1`;
- default set contains exactly earthlike;
- default set resolves;
- an ordinary replacement model fails permission;
- a second world-law model fails unique cardinality;
- built-ins have deterministic JSON and content hash.

Run:

```powershell
cargo test --test builtin_rules
```

Expected: FAIL.

- [ ] **Step 2: Implement built-ins from validated constructors**

Return `Result` from public factory functions; do not hide construction failure behind an application panic.

- [ ] **Step 3: Verify**

```powershell
cargo test --test builtin_rules
cargo fmt --all -- --check
cargo clippy --test builtin_rules -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add src/rules tests/builtin_rules.rs
git commit -m "feat: add earthlike world law rules"
```

## Task 9: Add Rule Resolution Artifacts and Stage

**Files:**

- Create `src/generators/natural/rule_input.rs`
- Modify `src/generators/natural/mod.rs`
- Modify `src/generators/natural/stage.rs`
- Create `tests/rule_stage_graph.rs`

**Produces:**

- `RulePackSetArtifact`
- `AuthorConstraintsArtifact`
- `TectonicRuleResolutionArtifact`
- `RuleTectonicResolutionStage`

- [ ] **Step 1: Write failing artifact tests**

Cover:

- exact stable artifact keys;
- artifact JSON round-trip;
- artifact boundary revalidates malformed pack sets, author sets, and resolutions;
- stage ID/version/namespace;
- exact stage dependencies;
- default stage resolution preserves the base spec and chooses `CurrentSliceV1`;
- hard conflicts return a stable rule-specific stage error code;
- no output is published on failure.

Run:

```powershell
cargo test --test rule_stage_graph resolution
```

Expected: FAIL.

- [ ] **Step 2: Implement artifact wrappers**

Each wrapper owns one already-validated pure rule contract and delegates `Artifact::validate` to it.

- [ ] **Step 3: Implement the resolution stage**

The stage:

- ignores its `StageRng`;
- constructs the built-in capability registry;
- resolves the pack set;
- runs `TectonicRuleResolver`;
- publishes one full audit artifact;
- maps errors to stable codes without erasing readable IDs.

- [ ] **Step 4: Verify**

```powershell
cargo test --test rule_stage_graph resolution
cargo fmt --all -- --check
cargo clippy --test rule_stage_graph -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/generators/natural src/rules tests/rule_stage_graph.rs
git commit -m "feat: publish tectonic rule resolution"
```

## Task 10: Project Minimal Input and Rewire the Natural Graph

**Files:**

- Modify `src/generators/natural/rule_input.rs`
- Modify `src/generators/natural/mod.rs`
- Modify `src/generators/natural/stage.rs`
- Modify `tests/rule_stage_graph.rs`
- Modify `tests/natural_stage_graph.rs`
- Modify `tests/natural_field_views.rs`
- Modify `tests/natural_display_golden.rs`
- Modify `tests/natural_performance.rs`
- Modify natural display fixtures in `src/app/natural_display.rs`

**Produces:**

- `ResolvedTectonicInput`
- `ResolvedTectonicInputArtifact`
- `ResolvedTectonicInputStage`
- a graph in which `TectonicStage` only sees spatial + projected input

- [ ] **Step 1: Write failing projection tests**

Cover:

- projected input contains only model and final spec;
- projected artifact key is stable;
- projection stage depends only on the full resolution;
- model/spec round-trip and validation;
- audit source/version/hash changes do not change projected hash when model/spec are equal.

Run:

```powershell
cargo test --test rule_stage_graph projection
```

Expected: FAIL.

- [ ] **Step 2: Implement projection**

Keep the projection type in the engine adapter module, not `world`. It is a generation input artifact, not a world snapshot.

- [ ] **Step 3: Write failing graph-contract tests**

Update expectations:

- complete graph has exact external inputs: planar spec, base tectonic spec, rule pack set, author constraints;
- exact stage order is deterministic;
- `TectonicStageInputs` no longer accepts `TectonicSpecArtifact`;
- missing any new external artifact fails graph input validation;
- default complete graph still emits identical spatial, tectonic, and relief artifacts.

- [ ] **Step 4: Rewire `TectonicStage` and the graph**

Dispatch only on `TectonicModel::CurrentSliceV1`. Keep the model match exhaustive.

- [ ] **Step 5: Update all natural fixtures**

Every formal natural graph fixture supplies:

```text
default_rule_pack_set()
AuthorConstraints::default()
```

Do not introduce a hidden fallback inside BuildEngine or the graph.

- [ ] **Step 6: Verify**

```powershell
cargo test --test rule_stage_graph
cargo test --test natural_stage_graph
cargo test --test natural_field_views
cargo test --test natural_display_golden
cargo test --test natural_performance --no-run
cargo test --lib app::natural_display
cargo fmt --all -- --check
cargo clippy --test rule_stage_graph --test natural_stage_graph -- -D warnings
```

Expected: PASS; existing natural golden bytes unchanged.

- [ ] **Step 7: Commit**

```powershell
git add src/generators/natural src/app/natural_display.rs tests
git commit -m "feat: route natural generation through rule input"
```

## Task 11: Prove Cache Orthogonality and Failure Boundaries

**Files:**

- Modify `tests/rule_stage_graph.rs`
- Modify `tests/natural_stage_graph.rs`

- [ ] **Step 1: Add failing cache tests**

Scenario A:

1. Build default inputs.
2. Add a valid satisfied rule constraint whose presence changes the audit artifact but not model/spec.
3. Rebuild with the same cache.

Expected:

- spatial can hit;
- rule resolution misses;
- projection misses because audit changed;
- projected output hash is unchanged;
- tectonics and relief hit;
- final result hash remains unchanged if build-result semantics include only stage outputs that are unchanged; if the audit artifact intentionally changes the global result hash, assert tectonic/relief hashes stay unchanged instead.

Scenario B:

1. Change a hard constraint so the projected plate count changes.

Expected:

- rule resolution and projection miss;
- tectonics and relief miss;
- spatial hits.

Scenario C:

1. Submit disjoint author and pack hard constraints.

Expected:

- build fails with the rule conflict code;
- neither resolution, projection, tectonic, nor relief artifact publishes;
- valid earlier cache entries stay reusable;
- a subsequent valid build succeeds.

- [ ] **Step 2: Implement only evidence-driven fixes**

If the tests reveal that full audit data leaks into the tectonic dependency hash, fix the projection boundary. Do not special-case cache keys in the scheduler.

- [ ] **Step 3: Verify**

```powershell
cargo test --test rule_stage_graph cache
cargo test --test natural_stage_graph
cargo fmt --all -- --check
cargo clippy --test rule_stage_graph -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add tests/rule_stage_graph.rs tests/natural_stage_graph.rs src/generators/natural
git commit -m "test: verify rule input cache boundaries"
```

## Task 12: Integrate the Default Application Atomically

**Files:**

- Modify `src/app.rs`

**Produces:**

- exact new external input composition
- atomically published `RuleBuildSummary`
- read-only rule summary in the side panel

- [ ] **Step 1: Add failing app composition tests**

Cover:

- external artifact set is exactly four types;
- default rule set is earthlike and author constraints are empty;
- build candidate extracts full rule resolution;
- default resolution summary is 1 pack, 0 author constraints, 0 compromised constraints;
- source scan confirms app does not construct `ResolvedTectonicInputArtifact` directly;
- source scan confirms app does not call `TectonicGenerator`;
- a rule-resolution failure retains document, display packet, revision clock, and prior summary.

Run:

```powershell
cargo test --lib app::natural_app_tests
```

Expected: FAIL.

- [ ] **Step 2: Add a private runtime summary**

`RuleBuildSummary` contains only counts needed by UI. It is skipped from persistence and initialized empty until a successful build.

- [ ] **Step 3: Supply validated rule inputs**

`build_natural_external_artifacts` constructs:

- planar space artifact;
- base tectonic spec artifact;
- default rule pack set artifact;
- empty author constraints artifact.

Factory failures become `NaturalWorldBuildError` variants.

- [ ] **Step 4: Publish summary with the candidate**

Extract `TectonicRuleResolutionArtifact` from the outcome and create the summary before preparing the display packet. Replace all candidate-owned state together only after every step succeeds.

- [ ] **Step 5: Render a read-only status line**

Show a compact Chinese summary near cell/plate/segment counts. Do not add editors or toggles.

- [ ] **Step 6: Verify**

```powershell
cargo test --lib app::natural_app_tests
cargo test --test natural_display_golden
cargo fmt --all -- --check
cargo clippy --lib -- -D warnings
```

Expected: PASS.

- [ ] **Step 7: Commit**

```powershell
git add src/app.rs
git commit -m "feat: compose built-in rules in the app"
```

## Task 13: Add Cross-Seed and Platform Rule Gates

**Files:**

- Modify `.github/workflows/rust.yml`
- Modify `tests/rule_tectonic_resolution.rs`
- Modify `tests/rule_stage_graph.rs`
- Modify only if a discovered defect requires a focused source fix

- [ ] **Step 1: Add deterministic property coverage**

Across fixed base specs, reversed input orders, and representative constraint combinations, assert:

- all output specs validate;
- hard constraints are satisfied;
- identical semantic input gives identical serialized resolution and hashes;
- rule resolution consumes no randomness;
- adding a no-effect constraint does not alter spatial/tectonic/relief hashes;
- changing root seed changes natural output but not resolved rule input;
- pack load order never changes results.

- [ ] **Step 2: Add focused CI commands**

Add one CI step that runs:

```text
cargo test --test rule_pack_resolution --test rule_tectonic_resolution --test rule_stage_graph
```

Do not remove existing natural or GPU gates.

- [ ] **Step 3: Run a static boundary scan**

```powershell
rg -n "crate::(engine|generators|app|ui|view|gpu|terrain)|egui|eframe|wgpu|HashMap|thread_rng|rand::|SystemTime|Instant|std::fs|std::net|serde_json::Value|Any" src/rules
```

Expected: no hits.

Also run:

```powershell
rg -n "RulePack|AuthorConstraint|RuleResolution" src/generators/natural/tectonics.rs src/world/natural
```

Expected:

- no rule concepts in the tectonic algorithm;
- no rule dependency in world natural contracts.

- [ ] **Step 4: Verify focused suites**

```powershell
cargo test --test rule_ids
cargo test --test author_constraints
cargo test --test rule_capabilities
cargo test --test rule_manifests
cargo test --test rule_pack_resolution
cargo test --test rule_tectonic_resolution
cargo test --test builtin_rules
cargo test --test rule_stage_graph
cargo test --test natural_stage_graph
cargo test --test natural_display_golden
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add .github/workflows/rust.yml tests src/rules src/generators/natural
git commit -m "test: verify rule capability integration"
```

## Task 14: Run Final Verification, Review, Merge, and Publish

**Files:**

- All files changed by this plan.

- [ ] **Step 1: Read completion skills**

Read fully:

```text
superpowers:verification-before-completion
superpowers:finishing-a-development-branch
```

- [ ] **Step 2: Run the complete native gates**

```powershell
cargo test --workspace --all-targets
cargo test --workspace --all-targets --release
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets --all-features
```

Expected: PASS with only explicitly documented ignored regeneration, performance, and extreme-size tests.

- [ ] **Step 3: Run wasm and Trunk**

```powershell
$previousRustflags = $env:RUSTFLAGS
$previousRustdocflags = $env:RUSTDOCFLAGS
$env:RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
$env:RUSTDOCFLAGS='--cfg getrandom_backend="wasm_js"'
cargo check --workspace --all-features --lib --target wasm32-unknown-unknown
$wasmExit = $LASTEXITCODE
trunk build
$trunkExit = $LASTEXITCODE
if ($null -eq $previousRustflags) { Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue } else { $env:RUSTFLAGS = $previousRustflags }
if ($null -eq $previousRustdocflags) { Remove-Item Env:RUSTDOCFLAGS -ErrorAction SilentlyContinue } else { $env:RUSTDOCFLAGS = $previousRustdocflags }
if ($wasmExit -ne 0) { exit $wasmExit }
if ($trunkExit -ne 0) { exit $trunkExit }
```

Expected: PASS.

- [ ] **Step 4: Review scope and architecture**

```powershell
git diff main...HEAD --check
git diff main...HEAD --stat
git log --oneline main..HEAD
git status --short
```

Manually verify:

- no unrelated user file changed;
- no history/time model;
- no arbitrary rule payload or executable code;
- no rules-to-engine dependency;
- no world-to-rules dependency;
- no rule data inside natural snapshots;
- no app-side rule merging;
- no audit metadata in the tectonic cache projection;
- existing natural goldens unchanged;
- design and implementation agree.

- [ ] **Step 5: Run the actual release application**

Build and inspect:

```powershell
cargo build --release --bin sekai
```

Verify:

- application opens and remains responsive;
- natural visual is unchanged;
- rule summary shows one active pack and no author constraints;
- changing/rebuilding the base tectonic controls still works;
- field switching and inspection still work;
- no blank map or persistence regression.

- [ ] **Step 6: Merge according to prior user authorization**

The user already authorized direct merge and push for this project. Confirm `main` and `origin/main` have not moved unexpectedly, then merge without destructive reset.

On merged `main`, run:

```powershell
cargo test --test rule_stage_graph
cargo test --test natural_display_golden
cargo check --all-targets
```

Expected: PASS.

- [ ] **Step 7: Push and verify remote identity**

```powershell
git push origin main
git fetch origin main
git status --short --branch
git rev-parse HEAD
git rev-parse origin/main
```

Expected: clean main and identical revisions.

- [ ] **Step 8: Clean only the owned worktree**

After merged smoke tests pass:

- remove `.worktrees/rule-capabilities-author-constraints`;
- prune worktree metadata;
- delete the merged feature branch;
- do not touch `.worktrees/field-display-system`.

- [ ] **Step 9: Launch the merged release application**

Leave the merged app running for user inspection unless the environment requires it to stop.

## Completion Gate

This plan is complete only when:

- rule IDs, versions, manifests, hashes, dependencies, and budgets validate;
- capability permissions, consumption, and cardinality validate;
- earthlike world law uniquely selects `CurrentSliceV1`;
- rule and author tectonic constraints share one typed solver;
- hard/soft/hint semantics pass fixed tests;
- hard conflicts stop publication with stable source information;
- audit and minimal input artifacts are separate;
- no-effect audit changes preserve tectonic/relief cache hits;
- `TectonicStage` sees only spatial and projected input;
- app uses the formal rule path and publishes its summary atomically;
- existing natural goldens remain unchanged;
- native, release, fmt, Clippy, wasm, Trunk, and actual-app checks pass;
- commits are merged and pushed to `main`;
- no unresolved direction decision remains inside this slice.
