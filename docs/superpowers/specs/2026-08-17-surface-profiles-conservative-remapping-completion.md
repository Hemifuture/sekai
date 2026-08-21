# Surface Profiles and Conservative Remapping P1 Completion Record

Date: 2026-08-17
Branch: `feat/spherical-presentation`
Scope: P1 of the complete natural-world program

## Outcome

P1 is complete. The project now has fixed Draft, Standard, and High natural
quality profiles; atomically constructed authoritative/control surface bundles;
a deterministic conservative spherical overlap map; typed intensive,
extensive, tangent-vector, and categorical remapping; cooperative cancellation;
and eight versioned spatial quality metrics.

P1 deliberately does not change tectonics V4, relief morphology, climate,
ocean circulation, ecology, rendering, or the current application default. The
unchanged five-failure V4 terrain baseline remains the input to P2.

## Frozen P2 handoff

`ProfileSurfaceBundle` publishes only after all of these components validate
together:

1. `NaturalResolutionPlan` V1;
2. the authoritative spherical surface;
3. the transient tectonic-control surface;
4. the exact control-to-authoritative `ConservativeSurfaceMap` V1;
5. the eight-metric P1 `NaturalQualityReport`;
6. the monotonic `BuildCancellation` contract.

The control surface is a work grid, not a second world identity. P2 must evolve
crust material on that grid and use the retained conservative map to publish on
the authoritative surface. Nearest-neighbour replacement and presentation-grid
sampling are forbidden.

## Deterministic product evidence

The fresh Release evidence writer produced the following exact values.

| Profile | Authoritative cells | Control cells | Authoritative fingerprint | Control fingerprint | Overlaps | Balance iterations |
| --- | ---: | ---: | --- | --- | ---: | ---: |
| Draft | 20,252 | 4,842 | `0d09df7aa131d120490202741b0fd3184919ea9681f16537a14f81f0e5806f2e` | `beaf400d8d12d84480cb67d1e921b5d923e4f334c5501a94dabe9c918fec2030` | 44,352 | 1 |
| Standard | 79,212 | 20,252 | `4247616b841cc415e4647576d4c445b501f053afc9da4bf33255fda1e9c09e84` | `0d09df7aa131d120490202741b0fd3184919ea9681f16537a14f81f0e5806f2e` | 178,452 | 1 |
| High | 198,812 | 20,252 | `31fab191de45f3a55744413684ca89c65176e58332a507dbd0a7d58d2696f97c` | `0d09df7aa131d120490202741b0fd3184919ea9681f16537a14f81f0e5806f2e` | 344,192 | 3 |

The Draft authoritative fingerprint is unchanged from P0 and the pre-P1
spherical foundation.

### Conservative closure

| Profile | Source margin max | Target margin max | Max geometric adjustment |
| --- | ---: | ---: | ---: |
| Draft | `2.6886416701870756e-16` | `2.9465827419711246e-13` | `3.5011851371651417e-13` |
| Standard | `3.1759774931377225e-16` | `6.661434465124948e-13` | `1.4025664488925751e-12` |
| High | `2.1675012304254222e-16` | `8.322442708495139e-13` | `4.071777876767359e-12` |

Every source/target margin is below the public `1e-10` limit. Construction
balanced to the stricter internal threshold and never used a nearest-neighbour
fallback.

### P1 metric values

| Metric | Draft | Standard | High | Bound |
| --- | ---: | ---: | ---: | ---: |
| `spatial.closed-sphere-area-relative-error.v1` | 0 | 0 | 0 | `<= 1e-10` |
| `spatial.shared-edge-flux-cancellation-max.v1` | 0 | 0 | 0 | `<= 1e-12` |
| `remap.constant-scalar-max-error.v1` | 0 | 0 | 0 | `<= 0` |
| `remap.extensive-relative-error.v1` | 0 | 0 | 0 | `<= 1e-6` |
| `remap.source-margin-max-relative-error.v1` | `2.6886416701870756e-16` | `3.1759774931377225e-16` | `2.1675012304254222e-16` | `<= 1e-10` |
| `remap.target-margin-max-relative-error.v1` | `2.9465827419711246e-13` | `6.661434465124948e-13` | `8.322442708495139e-13` | `<= 1e-10` |
| `remap.solid-body-direction-agreement.v1` | `0.9998948003414571` | `0.9999709024134105` | `0.9999424594782147` | `>= 0.999` |
| `remap.category-ambiguity-area-fraction.v1` | 0 | 0 | `0.00012337789144163783` | `[0, 1]` evidence |

All 24 profile-metric results are `Pass`. Separate focused tests also cover
signed extensive fields, both map directions, identity maps, exact constant
quantization, target tangency, lower-category ties, malformed input, and
cancellation during overlap construction.

The shared-edge metric checks the canonical one-edge/two-owner finite-volume
convention: the second owner receives the exact opposite of the single computed
edge flux. It is an operator/topology invariant, not evidence that P4 ocean or
atmosphere dynamics already exist. Likewise, the categorical fixture measures
remap ambiguity and is not an ecological-quality claim.

## Storage and retained payload observations

Exact JSON byte counts were streamed through `serde_json` without retaining the
encoded buffers. Retained payload values are deterministic lower bounds from
public record/slice sizes; they are not peak RSS measurements.

| Profile | Exact JSON bytes | Retained payload lower bound |
| --- | ---: | ---: |
| Draft | 45,381,179 | 15,861,289 |
| Standard | 182,250,346 | 62,991,209 |
| High | 397,408,460 | 136,487,529 |

The deterministic evidence file is
`target/natural-quality/p1/evidence.json`: 9,743 bytes, BLAKE3
`4de0429a600ee5a0d4a892110ff9979dc599c2a59be7a17fc072d9a400363139`.
It and all runtime evidence remain under the ignored `target/` tree.

## Release performance and cancellation observations

The final Release timing run used the repository `opt-level = 2` profile.
Durations are observations on this machine and are not semantic inputs or CI
wall-clock assertions.

| Profile | Authoritative | Control | Map | Quality | Component sum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Draft | 96.4972 ms | 22.7308 ms | 227.8410 ms | 89.0663 ms | 436.1353 ms |
| Standard | 397.0622 ms | 97.2191 ms | 928.1350 ms | 376.9297 ms | 1,799.3460 ms |
| High | 1,027.0050 ms | 96.1638 ms | 2,126.2625 ms | 864.9741 ms | 4,114.4054 ms |

A High background bundle cancelled during active construction returned no
bundle 1.694 ms after the cancellation request. Engine tests also prove that a
cancelled attempt does not cache its current output, expose partial artifacts,
or invalidate a previously published successful outcome. A never-cancelled
token preserves exact legacy RNG draws, diagnostics, artifact bytes, and result
hashes.

`target/natural-quality/p1/performance.json` is 1,380 bytes from the final run,
with BLAKE3
`e5d00a674aaef918e6bddf52de602533e995d9c793478ed0eff0f3bbf8578c7c`.

## Unchanged P0/V4 negative baseline

The complete 17-seed Release baseline was regenerated after P1:

- elapsed: 26.415426 seconds;
- every seed still has exactly five failed V4 metrics;
- JSON: 78,750 bytes, BLAKE3
  `4c1a0a8dfe0d41a45bb4f4e4ff36beb888167424db513e948bce53c5a1cac083`;
- CSV: 11,796 bytes, BLAKE3
  `a763d5b4bd5c176794c3a08e5e66bc00953d93ab72e3ae8862df2124a61bee3f`.

This confirms that P1 added infrastructure without silently changing V4
tectonic or relief behavior.

## Algorithm-conformance review

The implementation follows the locked P1 design:

- profile targets and resolved counts are exact and strictly decoded;
- High accepts a 200,000-cell request and resolves to 198,812 cells without
  raising the actual allocation ceiling;
- overlap geometry is deterministic spherical convex-polygon clipping over
  generating-site KD candidates and expanding adjacency rings;
- uncovered fine cells fail instead of falling back to nearest neighbours;
- row/column balancing is canonical, bounded to 96 iterations, and preserves
  raw geometric-adjustment evidence;
- intensive, extensive, tangent-vector, and categorical fields use their
  separately specified semantics;
- surface, map, engine, and bundle construction poll monotonic cancellation at
  bounded intervals and publish atomically;
- Draft repetition is byte-identical and Standard/High identities are frozen by
  the generated evidence.

The final direct code review found no unresolved Critical or Important issue.

## Verification evidence

The following commands were run fresh in the isolated worktree and exited 0:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --test natural_quality_profiles -- --nocapture
cargo test --test conservative_surface_map_contracts -- --nocapture
cargo test --test conservative_surface_map_generation -- --nocapture
cargo test --test conservative_surface_field_remap -- --nocapture
cargo test --test build_cancellation -- --nocapture
cargo test --test profile_surface_bundle -- --nocapture
cargo check --target wasm32-unknown-unknown --workspace --all-features
cargo test --release --test profile_surface_performance -- --ignored --nocapture
cargo test --release --test profile_surface_evidence -- --ignored --nocapture
cargo test --release --test natural_quality_baseline -- --ignored --nocapture
git diff --check
```

Focused results were 4 profile tests, 4 map-contract tests, 4 map-generation
tests with one expected ignored measurement, 8 field-remap tests, 6 cancellation
tests, and 2 bundle tests, all with zero failures. Both Release P1 evidence tests
and the Release P0 baseline writer passed. Native all-target/all-feature,
warning-free Clippy, and `wasm32-unknown-unknown` checks passed.

## Verification environment

- OS: Microsoft Windows 11 Pro 10.0.22631, x64.
- CPU: Intel Core i9-14900KF; 32 logical processors.
- Rust: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, LLVM 22.1.6.
- Cargo: `cargo 1.97.1 (c980f4866 2026-06-30)`.
- Host: `x86_64-pc-windows-msvc`.

## Known limitations and next phase

1. P1 is spatial infrastructure. It does not improve the visibly weak V4
   continents, mountains, drainage, or biome appearance.
2. High exact JSON persistence is roughly 397 MB, so High remains a cancellable
   background/export profile.
3. Retained-payload figures are lower bounds, not allocator-aware peak memory.
4. Global wind and ocean circulation remain locked for P4, P6, and P7; they were
   not omitted or replaced by the current preliminary climate.
5. Gleba visual comparison remains P10, after P2-P9 scientific and presentation
   stages are complete.

P2 may now implement evolved tectonics V5 against this frozen handoff.

## P1 commit chain

- `4498c52` - locked P1 design and implementation plan.
- `900fb6c` - fixed semantic quality profiles.
- `012bb25` - conservative map contract.
- `0fb983e` - spherical overlap construction and balancing.
- `4fc0a7b` - scientific field remapping.
- `173379c` - atomic cooperative cancellation.
- `c10e281` - atomic profile-surface bundle and P1 metrics.
- final P1 evidence/completion commit - this record and Release evidence gates.
