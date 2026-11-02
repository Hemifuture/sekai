// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas src/gen/params.ts and src/ui/genesisSliders.ts; see NOTICE.md.

/// Author controls from the pinned Civ Atlas generator, independent of Sekai climate.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainParams {
    /// Deterministic 32-bit random seed.
    pub seed: u32,
    /// Requested sphere cell count (the sampled count is approximate).
    pub cells: usize,
    /// Target land fraction, before inland-sea treatment.
    pub land_fraction: f64,
    /// Number of tectonic plates.
    pub plates: usize,
    /// Orogeny intensity, an author prior.
    pub mountains: f64,
    /// Private initialization temperature offset in degrees Celsius.
    pub temperature: f64,
    /// Private initialization precipitation multiplier.
    pub rainfall: f64,
    /// Main continent count; zero selects the upstream seeded choice.
    pub continents: usize,
    /// Continent gathering strength.
    pub gather: f64,
    /// Coast and plate fragmentation multiplier.
    pub rugged: f64,
    /// Plate hardness multiplier.
    pub hardness: f64,
    /// Hemisphere bias, south negative and north positive.
    pub north: f64,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self {
            seed: 1,
            cells: 36000,
            land_fraction: 0.33,
            plates: 30,
            mountains: 1.0,
            temperature: 0.0,
            rainfall: 1.0,
            continents: 0,
            gather: 0.0,
            rugged: 1.0,
            hardness: 1.0,
            north: 0.0,
        }
    }
}
impl TerrainParams {
    /// Check all controls against the pinned source UI domain before allocating a mesh.
    ///
    /// # Errors
    /// Returns `TerrainError::InvalidParameter` for non-finite or unsupported controls.
    pub fn validate(&self) -> Result<(), crate::TerrainError> {
        use crate::TerrainError::InvalidParameter;
        // genesisSliders.ts bounds and ParamField.tsx CONTINENT_CHOICES.
        if !(12000..=200000).contains(&self.cells) {
            return Err(InvalidParameter { name: "cells" });
        }
        if !(8..=60).contains(&self.plates) {
            return Err(InvalidParameter { name: "plates" });
        }
        if self.continents > 6 || (self.continents > 0 && self.continents > self.continent_cap()) {
            return Err(InvalidParameter { name: "continents" });
        }
        for (name, value, lo, hi) in [
            ("land_fraction", self.land_fraction, 0.12, 0.6),
            ("mountains", self.mountains, 0.2, 2.0),
            ("temperature", self.temperature, -12.0, 12.0),
            ("rainfall", self.rainfall, 0.4, 1.8),
            ("gather", self.gather, 0.0, 1.0),
            ("rugged", self.rugged, 0.2, 2.0),
            ("hardness", self.hardness, 0.4, 2.5),
            ("north", self.north, -1.0, 1.0),
        ] {
            if !value.is_finite() || !(lo..=hi).contains(&value) {
                return Err(InvalidParameter { name });
            }
        }
        Ok(())
    }
    /// Return the largest explicit main-continent count allowed by these author controls.
    fn continent_cap(&self) -> usize {
        1.max((self.plates / 3).min(
            ((self.plates as f64 * (self.land_fraction * 1.3).clamp(0.08, 0.92)) / 0.7).floor()
                as usize,
        ))
    }
}
