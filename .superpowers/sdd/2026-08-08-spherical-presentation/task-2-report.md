# Task 2 report: prepare spherical field layers

## Implementation summary

- Added one geometry-free `PreparedFieldLayers` packet with a source identity, selected cell fill, optional edge/vector overlay, diagnostic mask, two palettes, independent revisions, and a diagnostics flag.
- Added distinct `PreparedEdgeField` and `PreparedVectorField` payloads. Vector preparation preserves east/north pairs, precomputes finite `hypot` magnitudes, resolves schema/data/manual ranges over magnitudes, and uses the sequential palette for vector fields.
- Extracted scalar/category packing into a private domain-aware helper. The legacy public `prepare_cell_field` API and `PreparedCellField` shape remain unchanged; edge preparation never reports edge values as cells.
- Added reconciliation and update functions that retain compatible state, select the document-preferred fill (or first compatible fill), clear invalid overlays/entities, and preserve unchanged `Arc`s/revisions.
- Added a private app-document wrapper so the spherical document contributes source identity, cardinalities, diagnostics, and preferred ranges without making `SphericalNaturalFieldDocument` public.

## Files changed

- `src/view/field_layers.rs`
- `src/view/palette.rs`
- `src/view/mod.rs`
- `src/app/field_document.rs`
- `src/app/spherical_natural_display.rs`
- `tests/spherical_field_layers.rs`

## RED evidence

1. `cargo test --test spherical_field_layers -- --nocapture`
   - Failed as expected with `unresolved import sekai::view::PreparedOverlayKind` before the packet API existed.
2. `cargo test --lib spherical_natural_display::tests::spherical_document_binds_layer_preparation_to_its_own_source_and_cardinality -- --nocapture`
   - Failed as expected with unresolved `prepare_spherical_document_layers` before the app-bound wrapper existed.
3. `cargo test --lib spherical_natural_display::tests::edge_preparation_reports_field_ids_for_bad_cardinality_and_channels -- --nocapture`
   - Failed as expected because a cell-fill supplied to edge preparation was checked for cardinality before being rejected as an unsupported spherical channel.

## GREEN and verification evidence

- `cargo test --test spherical_field_layers -- --nocapture` — 5 passed.
- `cargo test --lib spherical_natural_display::tests::complete_spherical_catalog_prepares_fill_edge_and_vector_layers -- --nocapture` — 1 passed; verifies the exact 36 fields and 32/2/2 channel contract, vector preservation/range/palette, both edge forms, and packet pointer sharing.
- `cargo test --lib spherical_natural_display::tests::spherical_layer_updates_replace_only_changed_payloads -- --nocapture` — 1 passed; verifies diagnostics toggles and fill changes retain untouched shared allocations/revisions.
- `cargo test --lib spherical_natural_display::tests::spherical_document_binds_layer_preparation_to_its_own_source_and_cardinality -- --nocapture` — 1 passed.
- `cargo test --lib spherical_natural_display::tests::edge_preparation_reports_field_ids_for_bad_cardinality_and_channels -- --nocapture` — 1 passed after the channel-first correction.
- `cargo test --lib app::field_document -- --nocapture` — 4 passed.
- `cargo test --test natural_display_golden -- --nocapture` — 2 passed, 1 ignored (intentional golden regeneration test).
- `cargo test` — completed with exit code 0; library, integration, binary, and doc test suites passed.
- `cargo fmt --check` and `git diff --check` — passed.

## Self-review

- Checked the packet owns prepared selected data only; the full catalog remains borrowed from the document during preparation and the document retains all authoritative arrays.
- Confirmed edge values use `PreparedEdgeField`, `FieldDomain::Edges`, and edge cardinality diagnostics.
- Confirmed legacy planar preparation remains on its original public API and all adjacent planar/golden tests pass.
- Confirmed public classifier and packet types remain renderer-neutral, while private document assertions stay in the private app test module.

## Concerns

None. Git reports the repository's existing LF-to-CRLF checkout advisory for touched Rust files; it does not alter the logical diff or verification results.

## Fix Round 1

### Findings addressed

- Both spherical preparation paths now stage cloned display state and revision clocks. They publish those candidates only after all validation, payload preparation, palette work, and revision issuance succeeds.
- Updates now derive a small prepared-state key and prepare only components invalidated by fill, overlay, range, diagnostic-scope, or palette changes. Test-only counters prove a fill update skips overlay construction and an overlay update skips fill construction.
- Fill palettes retain the fill-compatible override, while overlays resolve their own schema palette. Category edges use categorical colors and vector overlays always use the sequential family.
- Updates validate incoming diagnostics even when the existing diagnostic allocation can be reused, preventing invalid changed diagnostics from bypassing atomic failure handling.

### Files changed

- `src/view/field_layers.rs`
- `src/view/mod.rs`
- `src/app/spherical_natural_display.rs`

### RED evidence

- `cargo test --lib spherical_natural_display::tests -- --nocapture` — 14 passed, 2 failed before the fix:
  - `failed_spherical_preparation_preserves_state_and_revision_clock` showed reconciliation had changed caller state after a later diagnostics error.
  - `overlay_palette_uses_its_own_schema_not_the_fill_override` showed a category edge overlay received the diverging fill palette.
- After the first staging/palette implementation, the new update regression failed because unchanged diagnostics skipped validation. This isolated the remaining bypass and led to the lightweight validation step.

### GREEN and verification evidence

- `cargo test --lib spherical_natural_display::tests -- --nocapture` — 18 passed, including preparation/update atomicity, palette independence, and preparation-counter assertions.
- `cargo test --test spherical_field_layers -- --nocapture` — 5 passed.
- `cargo test --lib app::field_document -- --nocapture` — 4 passed.
- `cargo test` — exit code 0; 249 library tests plus integration, binary, and doc-test suites passed.
- `cargo fmt --check` and `git diff --check` — passed.

### Self-review

- Failure paths never assign staged state or clocks to their callers.
- The update key separates fill, overlay, diagnostics, fill palette, and overlay palette invalidation; display-only flags perform no large payload preparation.
- Overlay palette resolution intentionally does not read the fill override, preserving category/vector compatibility.
- Test instrumentation is `cfg(test)` and thread-local, so it does not exist in product builds.

## Fix Round 2

### Finding addressed

- `update_spherical_field_layers` now fingerprints the complete incoming diagnostic content. A valid changed diagnostic slice rebuilds only the diagnostic mask and advances only its revision; an identical slice reuses the existing mask and revision.

### Files changed

- `src/view/field_layers.rs`
- `src/app/spherical_natural_display.rs`

### RED evidence

- `cargo test --lib spherical_natural_display::tests::spherical_updates_refresh_only_changed_diagnostics -- --nocapture` — failed before the fix because a valid warning at cell zero left the reused mask value at `0` rather than the expected warning value `2`.

### GREEN and verification evidence

- `cargo test --lib spherical_natural_display::tests::spherical_updates_refresh_only_changed_diagnostics -- --nocapture` — 1 passed.
- `cargo test --lib spherical_natural_display::tests -- --nocapture` — 19 passed.
- `cargo test --test spherical_field_layers -- --nocapture` — 5 passed.
- `cargo test --lib app::field_document -- --nocapture` — 4 passed.
- `cargo test` — exit code 0; 250 library tests plus integration, binary, and doc-test suites passed.
- `cargo fmt --check` and `git diff --check` — passed.

### Self-review

- The diagnostics fingerprint includes severity, code, optional field identity, optional cell identity, and message in slice order.
- It is calculated after bounds validation on updates, preserving staged atomic failure behavior.
- Only the private packet metadata stores the compact fingerprint; it does not retain or copy document diagnostics.
- The regression asserts changed diagnostics replace only the mask/revision and that a second identical input reuses both.
