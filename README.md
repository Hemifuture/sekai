# Sekai world generator

A procedural world generator inspired by Azgaar's World Generator that runs on top of `egui`/`eframe`. The app produces layered GPU textures for terrain, tectonics, erosion, rivers, moisture, and temperature so each simulation stage can be inspected independently or composited together.

## Features
- **Layered data model**: Elevation, tectonic collision energy, thermal erosion, flow-accumulation rivers, moisture diffusion, and temperature lapse rates are tracked separately.
- **GPU-backed rendering**: Each layer is uploaded as an `egui` texture and can be blended together in a composite view with per-layer opacity controls.
- **Physically motivated simulation**: Plate centers and velocities shape the crust; thermal erosion relaxes steep slopes; rainfall accumulates along downhill paths to carve rivers; moisture considers latitude, elevation, and proximity to water; temperature falls with altitude and latitude.
- **Interactive regeneration**: Adjust sea level, rainfall, erosion strength, plate counts, and iteration counts; randomize seeds; regenerate instantly.
- **Cross-platform**: Runs natively or in the browser via WASM.

## Running locally

### Native
```bash
cargo run --release
```

### Web (wasm)
Install [trunk](https://trunkrs.dev/) if needed, then:
```bash
trunk serve --release
```
Open the printed local URL in your browser.

## Controls
- **Seed**: Drag or randomize to change the world state.
- **Sea level**: Raises or lowers coastlines and affects moisture.
- **Rainfall**: Amount of precipitation used when accumulating river flow.
- **Erosion strength / iterations**: Governs the number of thermal erosion passes applied to the terrain.
- **Plate count**: Number of tectonic plate seeds used to sculpt height and collision energy.
- **Composite view**: Toggle layered blending on/off; adjust opacity per layer; click *Preview* to isolate any single layer.
- **Keyboard**: `Ctrl+R` regenerates with the current parameters.

## Architecture overview
- `src/world.rs` contains the simulation pipeline and data storage for each named layer. Deterministic generation is backed by `rand_chacha` so the same seed always yields identical maps.
- `src/rendering.rs` holds the palette system and texture uploader. Layers are colorized and uploaded to GPU textures, then blended for a composite output.
- `src/app.rs` wires UI controls to regeneration and texture updates. Texture uploads are triggered lazily when inputs change to keep the interactive loop responsive.

## Development
- Run all tests with `cargo test`.
- The erosion, river flow, and determinism rules are exercised through unit tests in `src/world.rs`.
- For more detailed implementation notes and troubleshooting tips, read `DEV_DOC.md`.
