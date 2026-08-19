# Formation UI Delivery Plan (2026-08-19)

## Context

The 2026-08-19 terrain audit established that the app canvas still runs the
legacy spherical foundation chain whose evolution destroys continental area
(author 30% → ~13%) and whose land-fraction sea-level solve turns mid-ocean
ridge flanks into land (65.6% of land on oceanic crust). The completed
P2v5→P3→P4→P5 chain is healthy (land on oceanic crust 0.67%) but has no UI
entry point, and the presentation layer compounds the damage (outlier-driven
symmetric elevation range on a beige-midpoint diverging palette; red=ocean
categorical colors).

Per `AGENTS.md`: algorithm work is accepted only when it reaches the UI and
the user personally verifies it there. Every task below therefore ends with a
UI-visible state and names its user verification step.

## Tasks

- [x] Task 1 — AGENTS.md acceptance discipline (commit 97eeed1).
- [ ] Task 2 — Presentation fixes usable by both chains:
      new `Hypsometric` palette hint/table anchored at sea level for surface
      elevation, semantic `LandOcean` palette (ocean blue / land tan),
      percentile-based elevation display radius replacing the single-outlier
      symmetric range; refresh field-display goldens.
      Verify: regenerate in-app; elevation fill reads as a map (blue depths,
      green→brown land), 海陆分类 shows blue ocean / tan land.
- [ ] Task 3 — Formation field document: registry subset + payload bindings
      for the P5 product (final/primary elevation, land/ocean, formation
      components, sediment, hydrology, circulation summaries) as
      `SphericalFormationFieldDocument` implementing the existing
      field-document traits; document-level tests.
      Verify: document unit tests; catalog materializes every registered field.
- [ ] Task 4 — App wiring: spherical canvas builds `surface_formation_graph()`
      (profile surface + resolved-input externals from the author panel),
      candidate/publication carry a document enum, panels read the evolved
      material budget + formation sea level, slice label updated.
      Verify: user regenerates in-app and sees the P5 world on map + globe.
- [ ] Task 5 — Async build with progress + cancellation so the ~12 s Draft
      solve does not freeze the UI.
      Verify: user clicks rebuild; UI stays responsive with progress; cancel
      works.
- [ ] Task 6 — Full gates (fmt, clippy, wasm check, workspace tests incl.
      golden refresh) + scripted UI drive evidence + written user acceptance
      steps.
      Verify: gates green; user walks the acceptance steps personally.

## Non-goals (tracked, not in this plan)

- Legacy-chain retirement or land-target clamping (audit option C).
- P6 topics: supercontinent breakup dispersal, coastal plains, river polyline
  overlay, display-level detail noise, P4 circulation warm start for the
  Standard 90 s gate.
