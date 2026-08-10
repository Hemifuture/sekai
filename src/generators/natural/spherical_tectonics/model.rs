//! Shared transient-model constants for evolved spherical tectonics.
//!
//! Presets select only a coherent-noise spectrum and bounded integer process
//! multipliers. They do not branch the process model or prescribe the final
//! number or shape of continents.

#![cfg_attr(not(test), allow(dead_code))]

use crate::generators::natural::fractal::FractalProfile;
use crate::world::natural::ResolvedWorldFormationPreset;

#[derive(Debug, Clone, Copy)]
pub(in crate::generators::natural) struct FormationTectonicRecipe {
    pub(in crate::generators::natural) initial_crust_profile: FractalProfile,
    pub(in crate::generators::natural) base_scale_rad: f64,
    pub(in crate::generators::natural) rift_rate_permille: u16,
    pub(in crate::generators::natural) subduction_gain_permille: u16,
    pub(in crate::generators::natural) island_arc_gain_permille: u16,
}

impl FormationTectonicRecipe {
    pub(in crate::generators::natural) const fn for_preset(
        preset: ResolvedWorldFormationPreset,
    ) -> Self {
        use ResolvedWorldFormationPreset::{
            Archipelago, Continents, GreatIsland, Supercontinent, VolcanicIslands,
        };

        match preset {
            Continents => Self::new(4, 1.8, 0.75, 1_000, 1_000, 1_000),
            Archipelago => Self::new(5, 3.4, 0.40, 1_250, 950, 1_150),
            Supercontinent => Self::new(3, 1.1, 1.15, 700, 1_050, 850),
            GreatIsland => Self::new(4, 1.5, 0.90, 850, 1_000, 950),
            VolcanicIslands => Self::new(5, 4.2, 0.32, 1_100, 1_200, 1_500),
        }
    }

    const fn new(
        octaves: usize,
        frequency: f64,
        base_scale_rad: f64,
        rift_rate_permille: u16,
        subduction_gain_permille: u16,
        island_arc_gain_permille: u16,
    ) -> Self {
        Self {
            initial_crust_profile: FractalProfile {
                octaves,
                frequency,
                lacunarity: 2.03,
                persistence: 0.5,
            },
            base_scale_rad,
            rift_rate_permille,
            subduction_gain_permille,
            island_arc_gain_permille,
        }
    }
}
