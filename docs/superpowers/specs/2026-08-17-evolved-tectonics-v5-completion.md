# Evolved Tectonics V5 P2 Completion Record

Date: 2026-08-17
Branch: `feat/spherical-presentation`
Scope: P2 of the complete natural-world program

## Outcome

P2 is complete. Sekai now has a separately versioned conservative tectonic
candidate, `sekai.core/natural.evolved-tectonics@5`, which publishes
`world.evolved-tectonics` only after its material, lineage, forcing, remap, and
quality contracts validate together. The frozen V4 stage and artifact remain
available and byte-identical.

V5 evolves on the P1 tectonic-control surface and publishes onto the exact P1
authoritative surface through the retained conservative overlap map. It does
not use presentation geometry, a nearest-cell extensive fallback, or a
renderer-time scientific calculation. The current product UI remains on the
legacy complete graph until P3-P9 provide a mutually consistent downstream
world and P10 performs the atomic product switch.

## Algorithm provenance and conformance

The declared reference remains Cortial, Peytavie, Galin, and Guerin,
*Procedural Tectonic Planets*, Computer Graphics Forum 38(2), 2019. V5 retains
the reference model's rigid plate rotations, 2 Myr step, 128-step bounded
evolution, polarity-aware oceanic subduction, terrane collision, divergent
spreading with age-zero crust, bounded rifting, displacement-driven resampling,
and crustal relaxation semantics.

The following behavior is deliberately Sekai-specific and is not represented
as a literal equation from the paper:

- extensive continental/oceanic reference-area and volume tracers;
- dense conservative material resampling and exact material ledgers;
- anisotropic graph-distance plate domains;
- deterministic oversized-plate mechanical fragmentation;
- P1 conservative control-to-authority publication;
- separate present-day uplift, subsidence, shortening, distance, and event-age
  forcing fields;
- bounded pure-shear continental extension;
- strict validation, quality gates, cancellation, and atomic publication.

This is therefore a conservative, testable procedural extension, not a mantle
convection solver or predictive geodynamic simulation. Randomness is confined
to labeled deterministic streams used for bounded plate-domain fabric, initial
state, and rift events. It is not post-hoc height noise used to decorate a
failed result.

Direct review confirmed the locked process order, material equations,
lineage equation, forcing polarity, conservative publication, stable iteration
order, and cancellation boundaries. No Critical or Important finding remains.

## Frozen P3 handoff

The complete artifact contains:

1. the exact `NaturalResolutionPlan`;
2. a strict V3 compatibility/current-state tectonic snapshot;
3. four authoritative extensive crust-material fields;
4. five authoritative present-day forcing fields;
5. recomputed control and authority material budgets;
6. an exact never-reused lineage ledger;
7. a 13-metric per-world quality report.

P3 must derive density, substrate, isostasy, primary relief, water-volume sea
level, and the physical coast from these facts. It must not infer material from
the compatibility category or reuse the P2 diagnostic renderer. P4, P6, and P7
remain responsible for the already locked global winds, atmosphere layers,
surface currents, thermocline/deep-ocean coupling, and final water/climate
variability.

## Deterministic product evidence

The Release corpus used Earth radius, Draft profile, `Continents`, the fixed 17
seeds, 20,252 authoritative cells, 4,842 control cells, and 44,352 conservative
overlaps. The exact surface fingerprints are:

- authoritative: `0d09df7aa131d120490202741b0fd3184919ea9681f16537a14f81f0e5806f2e`;
- control: `beaf400d8d12d84480cb67d1e921b5d923e4f334c5501a94dabe9c918fec2030`.

The deterministic evidence files are under `target/natural-quality/p2`:

- `evidence.json`: 112,200 bytes, BLAKE3
  `d6af0f68da189291d46e0af36d4f6875bd73671f2d12b28a56c0f16e47ebce97`;
- `metrics.csv`: 25,500 bytes, BLAKE3
  `f8cbc4c506970f020afbd4450bd0152121f6c49ebb74b9ab170957f65802150a`.

Both hashes repeated exactly in a second fresh Release run. Seed 42 repeated
byte-for-byte within the writer as an additional in-process determinism check.

### Corpus gates

| Metric | Value | Samples | Bound | Result |
| --- | ---: | ---: | ---: | --- |
| Collision causality fraction | 1.000000 | 351 | `>= 0.80` | Pass |
| Median continental material fraction | 0.4369821542 | 17 | `0.30..=0.45` | Pass |
| Ocean age-depth Spearman | 0.9547085011 | 196,337 | `>= 0.70` | Pass |
| Near-120-degree triple-junction fraction | 0.130000 | 600 | `<= 0.35` | Pass |
| Subduction causality fraction | 0.9598747017 | 6,704 | `>= 0.80` | Pass |
| Transform/convergent median uplift ratio | 0.000000 | 17,858 | `<= 0.50` | Pass |

Corpus rank correlation and uplift medians are recomputed from their original
cell/edge samples. Fractions are recombined by contributing sample count; the
implementation does not average per-seed ratios or hide unavailable samples.

### Per-world hard gates

Across all 17 seeds:

- continental-area retention is `1.149999999999` for every seed, within the
  locked `0.75..=1.15` bound;
- maximum plate area ranges from `0.2720549801` to `0.3709023039`, below 0.45;
- control material relative error ranges from
  `3.8195876945e-16` to `2.6742392165e-15`, below `1e-9`;
- authority material relative error, lineage closure error, and non-finite
  value count are exactly zero for every seed;
- final live plate counts range from 5 to 12, with exact lineage accounting;
- each seed records 2-6 mechanical fragmentations and 154-311 terrane
  transfers.

The identical 1.15 retention values show that the explicit global pure-shear
extension safety budget is active in this 256 Myr corpus. This is an honest
calibration saturation, not emergent prediction. It is accepted by the locked
P2 contract because material and volume remain explicit and conservative; P3
must compute physical freeboard and land independently, and any future change
to this bound requires a versioned design amendment and new before/after
evidence.

### Per-seed artifact freezes

| Seed | Plates | JSON bytes | BLAKE3 |
| ---: | ---: | ---: | --- |
| 42 | 12 | 7,616,184 | `285e2c42ec414292c7309d0726eda5843e267d78e0b8b684ee7a5b2399d552e5` |
| 3 | 11 | 7,603,243 | `8822f349becfcfc18f61b5fef519543df9618d595cbde3db786c515fa8f0b25f` |
| 7 | 9 | 7,474,939 | `f2c03a3eae354bc1476bbfc63d342e0ea027ca6b8a80acdc6590d1ce92a29b5c` |
| 11 | 12 | 7,574,614 | `d0e9c9c871e44c15e25e6b45bb61754b3e3c50ed479f32e13da76163aa52a6ce` |
| 19 | 8 | 7,548,264 | `934d3b1f8412fabd813d7d106d18226122523e5c5bee63595dd8132534024aff` |
| 23 | 10 | 7,530,854 | `6a8206ef4a04dc17bf787c77bbfcc161a1aed57c9306488334948e9014caa708` |
| 29 | 8 | 7,441,768 | `a121cb668143372c1e7582e51cf98674375a70842eba6307851da489baf708ec` |
| 31 | 7 | 7,424,888 | `791eedbc9a9a99aeb8d487000c66348c53c7f27b3981a3b68046bb6b14171f30` |
| 43 | 6 | 7,399,227 | `c107eb8425224a476dfc4b2a6f85fddbf164053878ebe809edad4445b7ceb524` |
| 47 | 11 | 7,577,839 | `a1d47639515f01677bff8962d879c9c880b8a842f740135a4e538abd1641dfc7` |
| 59 | 11 | 7,566,991 | `76356ddca1347c769a96c48d61746018b4ffc74514da7ce452112acff6f2a05e` |
| 61 | 10 | 7,513,613 | `6683b58960c9a9f5df6040ae94ce608638a0b528b80376c2909aa17b4b7aba18` |
| 71 | 7 | 7,476,526 | `817ba4f10a4d965f08dab059a40f883fc90c0a77d47e356f42271fdcf7705074` |
| 73 | 5 | 7,490,747 | `d5440d69a33aeb6ccabb45829e7ae3c64b94cab2626c6a2c0fe41123f675a7f9` |
| 83 | 6 | 7,638,488 | `f8745960a4708a5f7c029cd67ccebf363a6325b4048434afa57c170560362574` |
| 89 | 9 | 7,561,421 | `017aa4fc27dfd617084f8d0bbd652352d88dcfae29648f9d9d90058512bf79cf` |
| 97 | 6 | 7,443,892 | `043da58eab8f450f82ba6c937c111c0998cf8300648dcd3255e34d6b437c0270` |

## Coast compatibility gate

The temporary read-only P2 harness ran the legacy relief generator against the
V5 plate boundaries. Buffered coast/plate overlap has median
`0.2403156412` and maximum `0.2728243464`, below the `0.35` P2 handoff limit.
This is not a production dependency or evidence that final relief is complete;
P3 must replace and re-run the gate against its physical primary relief.

## Diagnostic atlas review

The ignored Release atlas writer produced 17 sheets with eight rows: plate
owner, continental material fraction, ocean age, uplift, subsidence,
shortening, boundary distance, and accumulated tectonic elevation. Each row
contains a 512x256 equirectangular view and a fixed oblique globe.

The first centroid-splat diagnostic renderer exposed lace-like polar holes.
That was an atlas rasterization artifact, not missing world cells. It was
replaced with deterministic bounded raster Voronoi filling, renderer identity
`bounded-raster-voronoi-v1`, and all 17 sheets were regenerated. Direct visual
inspection of seeds 42, 43, and 83 confirmed full polar coverage, map/globe
agreement, coherent material/age domains, localized forcing, and no severe
diagnostic artifact. Seed 42 is 812,239 bytes with BLAKE3
`6847d6125d5b6cee5a07c43484dd6eb979b108fce29864936e98870949a3ad69`.

The atlas remains scientific diagnostic evidence, not the P9 natural renderer.

## Performance and cancellation observations

The final Release measurement used the repository `opt-level = 2` profile.
Timings are observations on this machine, not deterministic inputs.

| Profile | Authority | Control | Overlaps | Bundle | V5 generation/cancellation |
| --- | ---: | ---: | ---: | ---: | ---: |
| Draft | 20,252 | 4,842 | 44,352 | 532.3484 ms | generation 625.344 ms |
| Standard | 79,212 | 20,252 | 178,452 | 2.1902223 s | cancellation 358.373 ms |
| High | 198,812 | 20,252 | 344,192 | 5.0119407 s | cancellation 847.414 ms |

Draft serialized to 7,616,184 bytes. Standard and High were intentionally
cancelled during active generation and returned the typed cancellation error
within the two-second gate without publishing partial artifacts. The latest
timing file is 942 bytes with run-specific BLAKE3
`3db2a277a6f0154d270ce726c18548cbeb8bb42c7d70fc3102f68c0a45c4dd6d`.

## Frozen V4 and presentation compatibility

The P0 17-seed V4 baseline regenerated after P2 with exactly the same hashes:

- JSON: `4c1a0a8dfe0d41a45bb4f4e4ff36beb888167424db513e948bce53c5a1cac083`;
- CSV: `a763d5b4bd5c176794c3a08e5e66bc00953d93ab72e3ae8862df2124a61bee3f`.

Every V4 seed still reports its five known failures, proving V5 did not
silently alter the legacy algorithm.

The full-suite review also caught two stale presentation freezes introduced
when `natural.spherical-quality` became a formal stage. The surface and 12 of
16 GPU goldens remained byte-identical. Because vector glyph LOD is correctly
keyed to the complete presentation source, the changed build-result hash
changed only the deterministic glyph IDs and four vector-frame hashes. CPU
semantics passed before those four hashes were accepted. The complete Vulkan
and OpenGL golden suites then passed 5/5 on the audited RTX 4080 SUPER.

## Verification evidence

The following gates were run fresh in the isolated worktree and exited 0:

```powershell
cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --no-fail-fast --quiet
$env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test --test spherical_presentation_gpu -- --nocapture
$env:WGPU_BACKEND='gl'; $env:SEKAI_REQUIRE_SPHERICAL_GPU='1'; cargo test --test spherical_presentation_gpu -- --nocapture
cargo test --release --test evolved_tectonic_evidence write_evolved_tectonic_evidence -- --ignored --nocapture
cargo test --release --test evolved_tectonic_quality legacy_relief_coast_overlap_remains_below_the_p2_handoff_limit -- --ignored --nocapture
cargo test --release --test evolved_tectonic_performance measure_evolved_tectonic_profiles -- --ignored --nocapture
cargo test --release --test evolved_tectonic_atlas render_evolved_tectonic_atlas -- --ignored --nocapture
cargo test --release --test natural_quality_baseline write_v4_natural_quality_baseline -- --ignored --nocapture
cargo check --all-features --lib --target wasm32-unknown-unknown
git diff --check
```

The final all-target/all-feature matrix completed in 282.4 seconds. Its library
target passed 443 tests with one intentional ignore; every integration and
benchmark target completed without failure.

## Known limits before P3-P10

P2 supplies stronger causal tectonic fields, but it cannot by itself look like
a finished natural planet. It has no density-aware isostatic substrate,
water-volume sea level, final mountain/volcano/shelf construction, erosion and
drainage convergence, global coupled atmosphere-ocean solution, cryosphere,
soil/ecology, terrain detail pyramid, or natural material/lighting renderer.
Judging V5 through the current V4 relief and diagnostic palette would therefore
measure downstream omissions, not the quality of the new causes.

The next gate is P3 substrate and primary relief. Gleba comparison remains
deferred until every P0-P9 scientific and visual contract passes, exactly as
locked by the global design.
