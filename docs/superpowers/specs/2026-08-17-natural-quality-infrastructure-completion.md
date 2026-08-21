# Natural Quality Infrastructure P0 Completion Record

Date: 2026-08-17  
Branch: `feat/spherical-presentation`  
Scope: P0 only — measurement and baseline infrastructure for the complete natural-world program

## Outcome

P0 publishes a versioned, surface-bound `NaturalQualityReport` as the final seventeenth stage of the formal spherical foundation graph. It records nine deterministic scientific metrics without reading renderer, palette, camera, or other presentation state. Threshold failures are retained as evidence and do not veto generation during P0.

The fixed 17-seed V4 corpus exposes five failures for every seed and no unavailable metrics. This is the intended negative baseline for P1–P10, not a claim that V4 terrain is acceptable.

## Stable contracts

- Report schema: `NaturalQualityReport` V1.
- Artifact key: `world.natural-quality`.
- Stage: `natural.spherical-quality`, version 1, namespace `sekai.core`.
- Protocol: `sekai.spherical-natural-quality.v1`.
- Metric IDs are versioned, sorted, unique, and independent of display state.
- Available metrics require a finite value, a nonzero sample count, and explicit bound/status agreement.
- Empty weighted denominators are `Unavailable` with a reason; an empty Jaccard comparison is never reported as 1.0.
- Report deserialization rejects wrong schemas, unknown fields, contradictions, duplicates, and more than 4,096 metrics.
- Weighted aggregation uses deterministic Neumaier summation in caller order.

The formal quality stage depends only on:

1. resolved world formation;
2. spherical hydro-erosion;
3. spherical relief;
4. relief specification;
5. authoritative spherical surface;
6. spherical tectonics.

It does not depend on UI or renderer artifacts. Cache tests prove scientific changes invalidate it while palette changes do not.

## Fixed V4 scenario

- Seeds, in JSON report order: `42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97`.
- Profile: Draft.
- Requested cells: 20,000; resolved cells: 20,252.
- Radius: 6,371,000 m.
- Formation: Continents.
- Initial plates: 12.
- Initial continental-crust fraction: 0.38.
- Target land fraction: 0.38.

## Aggregate evidence

All values below come from the freshly regenerated `target/natural-quality/v4-baseline.json`.

| Metric | Min | Median | Max | Samples | Pass | Fail | Unavailable |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `hydrology.outlet-area-coverage.v1` | 1.0 | 1.0 | 1.0 | 130,199 | 17 | 0 | 0 |
| `hydrology.river-segment-count.v1` | 762.0 | 869.0 | 1,107.0 | 17 | 17 | 0 | 0 |
| `quality.non-finite-value-count.v1` | 0.0 | 0.0 | 0.0 | 31,096,048 | 17 | 0 | 0 |
| `relief.actual-land-area-fraction.v1` | 0.376929747651527 | 0.3777643714494374 | 0.3782569221657179 | 344,284 | 0 | 17 | 0 |
| `relief.land-crust-jaccard.v1` | 0.4226080689290792 | 0.5751423447164766 | 0.680515814921646 | 130,234 | 0 | 17 | 0 |
| `relief.oceanic-emergent-area-fraction.v1` | 0.1624623182862301 | 0.2053390841740834 | 0.2596260130540789 | 271,699 | 0 | 17 | 0 |
| `relief.requested-land-area-fraction.v1` | 0.3799999952316284 | 0.3799999952316284 | 0.3799999952316284 | 17 | 17 | 0 | 0 |
| `tectonics.continental-area-fraction.v1` | 0.15968292121120983 | 0.21779003504342462 | 0.2577798516831244 | 344,284 | 0 | 17 | 0 |
| `tectonics.continental-retention.v1` | 0.4202182189867538 | 0.5731316783587617 | 0.6783680392574613 | 344,284 | 0 | 17 | 0 |

Across 153 seed-metric results, 68 pass, 85 fail, and 0 are unavailable. Every aggregate sample count equals the sum of its 17 per-seed reports.

## Generated evidence

- `target/natural-quality/v4-baseline.json`: 78,750 bytes; BLAKE3 `4c1a0a8dfe0d41a45bb4f4e4ff36beb888167424db513e948bce53c5a1cac083`.
- `target/natural-quality/v4-metrics.csv`: 11,796 bytes; BLAKE3 `a763d5b4bd5c176794c3a08e5e66bc00953d93ab72e3ae8862df2124a61bee3f`.
- CSV ordering: `(metric_id, seed)`.
- The writer rendered both byte buffers twice before replacement and asserted identical hashes.
- Both files live under the already ignored `target/` tree; neither is committed.

The fresh 17-seed Release run completed in 26.622122 seconds. A separate 20,252-cell Release performance run measured the full spherical graph at 1,599.024 ms and the quality stage at 1.827 ms, below the historical full-graph limit of 1,772.734 ms. The quality stage initially repeated deep upstream validation and measured about 227 ms; splitting the public checked evaluator from the already-validated stage core removed that redundant work without changing the quality artifact hash.

## Verification evidence

The following commands were run in the linked worktree. Each exited 0 unless an individual test is explicitly described as ignored.

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --test natural_quality_contracts -- --nocapture
cargo test --test natural_quality_stage -- --nocapture
cargo test --test natural_quality_baseline -- --nocapture
cargo check --target wasm32-unknown-unknown --workspace --all-features
cargo test --release --test natural_quality_baseline -- --ignored --nocapture
$env:SEKAI_SPHERICAL_BASELINE_MS='1418.187'; cargo test --release --test spherical_natural_graph_performance -- --ignored --nocapture
git diff --check
```

Observed focused results:

- Quality contracts: 5 passed, 0 failed.
- Quality evaluator/stage: 4 passed, 0 failed.
- Baseline isolation/determinism: 2 passed, 0 failed, 1 expected ignored writer.
- Release baseline writer: 1 passed, 0 failed.
- Release graph performance and five-seed land compliance: 2 passed, 0 failed.
- Full target/all-feature Clippy: no warnings with `-D warnings`.
- Native and `wasm32-unknown-unknown` checks: successful.

## Verification environment

- OS: Microsoft Windows NT 10.0.22631.0, x64.
- Process architecture: x64.
- CPU identifier: Intel64 Family 6 Model 183 Stepping 1, GenuineIntel.
- Logical processors visible to the process: 32.
- PowerShell: 5.1.22621.6133.
- Rust: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, host `x86_64-pc-windows-msvc`, LLVM 22.1.6.
- Cargo: `cargo 1.97.1 (c980f4866 2026-06-30)`.
- Release profile: repository `opt-level = 2`.

Runtime measurements are machine observations, not serialized baseline inputs; the JSON and CSV remain machine- and Git-independent.

## Known V4 limitations retained as evidence

1. Evolved continental area is only 15.97%–25.78%, far below the 30%–45% gate and the requested initial 38%.
2. Continental retention is only 0.420–0.678; V4 loses too much continental material through evolution/projection.
3. Final post-erosion land is 37.69%–37.83%, so all seeds miss the one-maximum-cell target tolerance even though constructional relief was quantile-forced to approximately 38%.
4. Land/continental-crust Jaccard is 0.423–0.681, below the 0.75 gate for every seed.
5. Oceanic emergent area is 16.25%–25.96%, above the 10% gate for every seed.
6. P0 measures the current V4 pipeline; it deliberately does not repair tectonics, terrain formation, circulation, ecology, or rendering.
7. The current preliminary analytic wind field and experimental circulation work are not promoted by P0. Locked C0–C5 atmosphere/ocean architecture remains scheduled for P4, P6, and P7.
8. No Gleba visual benchmark is claimed at P0. That gate remains P10 after scientific and presentation stages are complete.

## Commit chain

- `699ada0` — complete natural-world pipeline design.
- `8738d34` — P0 implementation plan.
- `5581e69` — versioned quality report contract.
- `5e51052` — deterministic quality metric builder.
- `c3a2170` — pure spherical quality evaluator.
- `b8b877c` — formal quality artifact and stage.
- `42da9ba` — reproducible V4 baseline writer.
- `c69c5ec` — isolated test-support loading for warning-free all-target builds.

## P1 handoff

P1 receives these fixed inputs:

- `NaturalQualityReport` V1 and the nine P0 metric identities;
- the formal `NaturalQualityArtifact` stage output;
- the exact 17-seed Continents/Draft corpus;
- deterministic JSON/CSV rendering and aggregate semantics;
- negative V4 evidence that later phases must improve without silently changing thresholds.

P1 may now add quality profiles, Standard/High authoritative surfaces, and conservative scalar/vector/extensive remapping. It must preserve the Draft geometry hashes where specified, keep report schema compatibility, add versioned metrics rather than repurpose existing IDs, and rerun this corpus after every scientific phase.
