# Task 7 Report: Independent 2D/3D GPU Fill Pipelines

## Implementation summary

- Added a new `gpu::spherical` renderer beside the legacy `gpu::field` path. It owns separate projected-map and unit-globe vertex/index buffers, render pipelines, and 96-byte camera uniforms, while both modes bind the same packed fill values, diagnostic mask, and combined palette buffers.
- Added `SphericalGpuPacket`, which retains independent map/globe geometry revisions and exactly one shared `Arc<PreparedFieldLayers>`. Its free-form constructor remains crate-private so only source-bound application preparation can assemble production packets.
- Added checked, revision-aware `prepare_packet`. Source identity, cell cardinality, geometry/index validity, palette bounds, checked byte arithmetic, `u32` counts, `u64` counters, and relevant `wgpu::Limits` are validated before any installed state or successful-upload counter changes. Changed resources are prepared independently; a source change forces one complete replacement.
- Added fixed-uniform frame preparation and generation tokens. Camera, viewport, and mode changes write only the selected 96-byte uniform. A stale egui callback cannot paint a newer callback's prepared frame.
- Added `SphericalPaintCallback` using `egui_wgpu::CallbackResources` and the independent spherical renderer.
- Added one WGSL value/diagnostic color path shared by `vs_map` and `vs_globe`, with one `fs_fill`. Globe uses CCW front faces, back-face culling, WebGPU's `[0, w]` clip-depth convention, and unlit fill colors. The shader has no elevation, height, lighting, edge, or vector input/pass.
- Kept source-bound packet and offscreen tests inside the new module, as required by the task's testing resolution. The public integration target covers source-independent layout-facing counters, modes, typed errors, and callback-trait contracts.

## Files changed

- `src/gpu/spherical/mod.rs`: public spherical GPU exports.
- `src/gpu/spherical/renderer.rs`: packet, typed errors, independent pipelines/resources, validation, revision/upload accounting, uniform generation, painting, GPU fixtures, and offscreen tests.
- `src/gpu/spherical/callback.rs`: egui-wgpu prepare/paint bridge with stale-frame suppression.
- `assets/shaders/spherical_field.wgsl`: shared fill decode, palette sampling, diagnostic overlay, two vertex entry points, and one fill fragment entry point.
- `src/gpu/mod.rs`: exposed only the new spherical GPU module publicly and retained legacy GPU modules crate-private.
- `src/lib.rs`: exposed the documented `gpu` namespace so the new public contracts are reachable.
- `tests/spherical_presentation_gpu.rs`: source-independent public mode/counter/error/callback contracts.
- `.superpowers/sdd/2026-08-08-spherical-presentation/task-7-report.md`: this report.

## RED/GREEN evidence

### Initial public API RED

The public contract tests were created before the module and interfaces:

```powershell
cargo test --test spherical_presentation_gpu -- --nocapture
```

Result: exit 1 as expected. Rust reported `E0432` for missing `gpu::spherical` imports and `E0603` because the existing `gpu` namespace was private.

After the minimum public module, modes, counters, and typed errors were introduced, the same command returned exit 0. Further compile-first cycles used the same target: tests first failed for missing `GpuMapVertex`/`GpuGlobeVertex`/uniform layout, then for missing `SphericalGpuPacket`, and then for missing renderer preparation/counter/error contracts. Each went GREEN only after its minimum implementation.

### Source-bound renderer RED/GREEN

The source-bound tests were kept in `src/gpu/spherical/renderer.rs` because `SphericalPresentationSource` deliberately has no public free-form constructor:

```powershell
cargo test --lib gpu::spherical -- --nocapture
```

Incremental RED results included missing packet/renderer APIs and, for frame ownership, a compile failure for missing `is_frame_current`. After implementation, the final result was exit 0: 9 passed, 0 failed, 264 filtered out.

### Shader semantic mutation RED/GREEN

The offscreen suite was run with three temporary negative mutations; none remain:

```powershell
cargo test --lib gpu::spherical::renderer::tests::offscreen_map_and_unlit_globe_scalar_and_category_match_cpu_colors -- --exact --nocapture
```

- Multiplying globe output by `0.5` produced RED through an RGBA mismatch against CPU colors. Restoring the shared unlit color path returned GREEN.

```powershell
cargo test --lib gpu::spherical::renderer::tests::offscreen_globe_culls_back_facing_cell_triangles -- --exact --nocapture
```

- Disabling globe back-face culling produced RED: the intentionally back-facing triangle wrote pixels. Restoring `FrontFace::Ccw` plus `Face::Back` returned GREEN.
- During this cycle the globe transform was corrected from OpenGL-style `[-w, w]` depth to WebGPU `[0, w]` depth; this ensures the test exercises culling rather than accidental clip rejection.

```powershell
cargo test --lib gpu::spherical::renderer::tests::offscreen_diagnostic_overlay_replaces_fill_color_in_both_modes -- --exact --nocapture
```

- Bypassing the diagnostic overlay produced RED through an expected-color mismatch. Restoring the single shared overlay helper returned GREEN.

## Exact final verification commands and results

```powershell
cargo test --lib gpu::spherical -- --nocapture
# exit 0: 9 passed, 0 failed, 264 filtered out

cargo test --test spherical_presentation_gpu -- --nocapture
# exit 0: 3 passed, 0 failed

cargo test --lib gpu::field::renderer::tests::offscreen_scalar_and_category_match_cpu_reference -- --exact --nocapture
# exit 0: 1 passed, 0 failed; explicit optional skip because no fallback adapter was available

cargo clippy --test spherical_presentation_gpu -- -D warnings
# exit 0, no warnings

cargo fmt --all -- --check
# exit 0

git diff --check
# exit 0; only Git's existing LF-to-CRLF working-copy notices were printed

cargo test
# exit 0 in 142.5 seconds: 273 library tests (272 passed, 1 ignored), then every binary,
# integration, and doc-test target completed with no failures
```

An earlier full-suite invocation used a 120-second command timeout and was terminated at roughly 122 seconds, which caused a broken-pipe test artifact rather than a code failure. It was rerun with a 600-second command allowance after the final edits; the successful 142.5-second result above is the completion evidence.

## Adapter and shader-validation status

- The new spherical helper first requests a fallback adapter and then requests any available adapter. In this environment the spherical tests obtained an adapter; they did not print the skip notice.
- The adapter compiled the WGSL, created both map/globe pipelines and bind groups, rendered scalar/category/diagnostic cases, and completed texture readback comparisons. Shader compilation or validation errors therefore could not be converted into a silent success.
- The legacy exact offscreen test requests only a fallback adapter. None was available, so it printed the repository-established message `skipping optional field-display GPU test: no fallback adapter is available` and returned successfully.

## Upload-counter evidence

The static-frame test printed the exact successful counters after installing the immutable packet:

```text
SphericalUploadCounters { map_geometry: 1, globe_geometry: 1, fill_field: 1, diagnostics: 1, palettes: 1, uniforms: 0, uploaded_bytes: 27152 }
```

After a map frame, a globe frame, and a rotated/zoomed globe frame:

```text
SphericalUploadCounters { map_geometry: 1, globe_geometry: 1, fill_field: 1, diagnostics: 1, palettes: 1, uniforms: 3, uploaded_bytes: 27440 }
```

Thus all five large-resource counters stayed exactly unchanged while uniforms increased by three and uploaded bytes increased by exactly `3 * 96 = 288`. Re-preparing the identical packet before those frames also left every counter unchanged. Generation assertions prove only the most recently prepared callback frame is paintable.

## Atomic rejection evidence

Three GPU-backed tests first install and render a valid packet, snapshot its source, counters, and complete offscreen RGBA byte buffer, submit one rejected candidate, and render again without re-preparing:

- Mixed source identity is rejected as `SourceMismatch { resource: "projected map" }`.
- A short fill layer is rejected as `CardinalityMismatch { resource: "fill field", ... }`.
- A genuinely changed map revision submitted against 64-byte test limits is rejected as `BufferLimitExceeded`.

For every rejection, the installed source and every counter are byte-for-byte/equality unchanged, and the entire post-rejection RGBA output equals the pre-rejection baseline. Candidate limits, checked sizes/counts, combined palette, counter increments, replacement buffers, and replacement bind groups are all completed before queue writes and installed-state swaps.

## Self-review

- Checked the task brief item-by-item: map/globe geometry, pipelines, and uniforms are independent; one field/diagnostic/palette packet is shared; revisions are cached independently; only fixed uniforms change per frame; CCW/back-cull and unlit CPU-equivalent colors are tested; diagnostic semantics are shared; and no Task 8 edge/vector behavior was added.
- Confirmed shader bindings and Rust layouts align: map vertex 12 bytes, globe vertex 16 bytes, uniform 96 bytes; shader uses exactly one fill buffer, one diagnostic buffer, one palette buffer, and one mode-specific uniform.
- Confirmed globe position is sourced only from `PreparedGlobeMesh::vertices().position()`. Searches of the spherical Rust/WGSL path show no elevation, height, illumination, or lighting input.
- Confirmed free-form packet construction is crate-private and public tests do not bypass source identity.
- Reviewed callback resource ownership. An initial review identified that multiple callbacks could otherwise paint the last shared uniform; the generation-token RED/GREEN cycle resolved this, and follow-up review confirmed stale callbacks now skip painting.
- The review suggestion to move source-bound GPU fixtures into the public integration test was intentionally not applied because the task explicitly requires these tests to remain in the module when construction is crate-private. The suggestion to expose a free-form packet constructor was likewise not applied for the same source-boundary requirement.
- Strengthened the review's atomicity concern beyond counters/source by adding full before/after GPU readback equality to all three rejection tests.

## Concerns

- No Task 7 blocker remains.
- Task 9 still needs to provide the application-owned source-bound packet factory/registration path; that is why direct public free-form packet construction is intentionally unavailable here.
- Task 8 owns edge/vector passes; this commit deliberately renders fills and diagnostic overlays only.
- The new offscreen suite exercised a real available adapter here, but adapter/backend diversity remains dependent on CI and developer machines.

## Fix Round 1: O(1) immutable-packet fast path

### Review finding and root cause

The first review found that flat large-upload counters did not imply constant-time static frames. `SphericalPaintCallback::prepare` correctly calls `prepare_packet` on every frame, but `prepare_packet_with_limits` called `validate_packet` before deciding whether anything changed. `validate_packet` iterates both meshes' cell IDs, indices, and positions, so the unchanged path was O(cell count).

The fix adds an installed immutable-packet key containing the source, all five renderer-relevant revisions, and retained `Arc` clones for the projected map, globe mesh, and shared field layers. An O(1) return occurs only when every scalar identity/revision and every `Arc::ptr_eq` comparison matches. Retaining the three Arcs keeps their allocations alive, so allocator address reuse cannot create an ABA false match. First installs and every different-Arc or changed-revision candidate still enter full validation before any queue write, resource swap, or successful counter change.

### Fix Round 1 RED

Test-only thread-local instrumentation counts every full validation and every existing per-element cell-ID, index, and position visit. The static-frame regression calls the same sequence as callback preparation: `prepare_packet`, then one fixed uniform for map, globe, and rotated/zoomed globe frames.

```powershell
cargo test --lib gpu::spherical::renderer::tests::static_frames_and_camera_or_mode_changes_upload_only_fixed_uniforms -- --exact --nocapture
```

Before the fast path, the command exited 1 at the intended assertion:

```text
left:  ScanCounts { full_validations: 4, cell_ids: 5952, indices: 5952, positions: 5952 }
right: ScanCounts { full_validations: 1, cell_ids: 1488, indices: 1488, positions: 1488 }
```

This reproduced exactly three unnecessary full validations and three extra complete geometry scans. The immutable field/value/palette data had no separate CPU element-validation loop; the new return is before packet validation, upload planning, vertex conversion, palette assembly, and buffer-size work, so the exact-match path cannot traverse those payloads either.

### Fix Round 1 GREEN and safety guard

After installing the exact key, the same test returned exit 0. The final focused suite printed:

```text
validation work after first install: ScanCounts { full_validations: 1, cell_ids: 1488, indices: 1488, positions: 1488 }
validation work after static frames: ScanCounts { full_validations: 1, cell_ids: 1488, indices: 1488, positions: 1488 }
```

Therefore the first installation performs exactly one full validation, while repeated map/globe/rotated preparations add zero full validations and zero element visits. The associated upload evidence remained:

```text
after immutable upload: SphericalUploadCounters { map_geometry: 1, globe_geometry: 1, fill_field: 1, diagnostics: 1, palettes: 1, uniforms: 0, uploaded_bytes: 27152 }
after camera/mode frames: SphericalUploadCounters { map_geometry: 1, globe_geometry: 1, fill_field: 1, diagnostics: 1, palettes: 1, uniforms: 3, uploaded_bytes: 27440 }
```

The source-rejection regression now constructs a distinct malformed packet whose map and globe Arcs differ but whose map/globe revisions exactly equal the installed packet's revisions. It asserts both pointer differences and revision equalities. Preparation increments `full_validations` by one, rejects `SourceMismatch` before element traversal, preserves every upload counter/source, and leaves the full offscreen RGBA result unchanged. This proves reused revisions on a different allocation cannot enter the fast path.

### Fix Round 1 final verification

```powershell
cargo test --lib gpu::spherical -- --nocapture
# exit 0: 9 passed, 0 failed, 264 filtered out; real adapter/offscreen execution

cargo test --test spherical_presentation_gpu -- --nocapture
# exit 0: 3 passed, 0 failed

cargo test --lib gpu::field::renderer::tests::offscreen_scalar_and_category_match_cpu_reference -- --exact --nocapture
# exit 0: 1 passed, 0 failed; legacy emitted the established no-fallback-adapter skip

cargo clippy --lib --tests -- -D warnings
# exit 0, no warnings

cargo fmt --all -- --check
# exit 0

git diff --check
# exit 0; only Git's LF-to-CRLF working-copy notice was printed

cargo test
# exit 0 in 155.0 seconds: 273 library tests (272 passed, 1 ignored), then every binary,
# integration, and doc-test target completed with no failures
```

### Fix Round 1 self-review and concerns

- The fast path precedes every O(n) scan and allocation, but only exact immutable component identities can reach it.
- The renderer retains the identity Arcs rather than raw addresses, preventing allocation-address reuse while a key is installed.
- A different allocation with reused revisions remains subject to source/cardinality/geometry/byte/limit validation and the original atomic replacement rules.
- No public API, WGSL binding, shader behavior, Task 8 overlay pass, or Task 9 packet-factory boundary changed.
- No Fix Round 1 blocker remains. The two review observations about crate-local source-bound tests and final-ledger cleanup remain outside this fix finding, as directed.
