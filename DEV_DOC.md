# Developer notes

This document summarizes how the simulation and renderer are stitched together so new contributors can extend or debug the generator quickly.

## Layered simulation pipeline (`src/world.rs`)
1. **Tectonic plates**: Randomized plate origins and velocity vectors seed the base elevation. The distance to the two closest plates shapes crust thickness and collision energy writes into the `Tectonics` layer.
2. **Elevation sculpting**: Base height is adjusted by plate elevation and collision intensity, yielding a raw `Elevation` layer.
3. **Thermal erosion**: `thermal_erosion` relaxes slopes that exceed a talus threshold (`TALUS_SLOPE`). The number of passes scales with `erosion_strength * iterations`.
4. **River flow**: `accumulate_flow` orders cells from high to low, pushing rainfall downhill to produce flow accumulation that becomes the `Rivers` layer.
5. **Moisture**: Combines latitude humidity, proximity to sea (`sea_level`), and river bonus to build the `Moisture` layer.
6. **Temperature**: Declines with latitude and elevation (lapse rate), normalized into the `Temperature` layer.
7. **Erosion map**: Difference between the pre/post erosion heights becomes the `Erosion` layer.

All layers are normalized to `[0, 1]` for predictable palettes. `GenerationParameters` and `World::generate` are deterministic for a given seed (`ChaCha8Rng`).

## Rendering and GPU composition (`src/rendering.rs`)
- `Palette` defines ordered color stops. `default_palettes` provides tuned defaults for each `LayerKind`.
- `LayerRenderer::upload_layer` converts layer data into `egui::ColorImage` and uploads it as a texture.
- `LayerRenderer::upload_composite` blends visible layers on the CPU, then uploads a dedicated composite texture. Each layer receives an alpha multiplier; river/tectonic palettes use embedded transparency to avoid overpowering elevation.

Although blending is performed on the CPU for clarity, all views are rendered by the GPU through `egui` textures, satisfying layered GPU rendering without custom shaders.

### Performance notes
- Neighborhood lookups reuse a fixed offset table (`NEIGHBOR_OFFSETS`) to avoid per-cell allocations when simulating erosion or routing flow.
- Flow ordering uses `total_cmp` to remain stable even if floating-point noise appears, preventing panics from `NaN` comparisons.

## App/UI glue (`src/app.rs`)
- Controls live in the left panel. Changing any generation parameter triggers `regenerate` to rebuild the `World` and mark textures dirty.
- `ensure_textures` lazily rebuilds per-layer and composite textures when marked dirty to keep the render loop light.
- `Ctrl+R` is wired for quick regeneration in both native and web builds.

## Testing
- Run `cargo test` to exercise core physics helpers:
  - `erosion_reduces_peaks` validates thermal erosion.
  - `rivers_flow_downhill` ensures downhill flow accumulation.
  - `world_generation_is_deterministic` guards deterministic seeds.

## Extending the generator
- Add new layers by extending `LayerKind`, generating values in `World::generate`, and defining a default palette in `default_palettes`.
- When adding new generation parameters, expose sliders in `SekaiApp` and propagate them into the relevant calculations.
- For alternate render styles, adjust palette stops or provide a custom blend strategy inside `upload_composite`.
