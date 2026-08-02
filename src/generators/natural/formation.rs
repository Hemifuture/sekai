use rand::RngCore;
use thiserror::Error;

use crate::engine::StageRng;
use crate::world::natural::{
    ResolvedWorldFormation, ResolvedWorldFormationPreset, WorldFormationPreset, WorldFormationSpec,
    WorldFormationSpecError, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};

/// Deterministic resolution of an author-facing formation request.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorldFormationGenerator;

impl WorldFormationGenerator {
    /// Resolves a named request directly or selects one weighted concrete preset.
    pub fn resolve(
        spec: &WorldFormationSpec,
        rng: &mut StageRng,
    ) -> Result<ResolvedWorldFormation, WorldFormationGenerationError> {
        spec.validate()?;
        let resolved = match spec.preset {
            WorldFormationPreset::Random => resolve_weighted_roll(rng.next_u32() % 100),
            WorldFormationPreset::Continents => ResolvedWorldFormationPreset::Continents,
            WorldFormationPreset::Archipelago => ResolvedWorldFormationPreset::Archipelago,
            WorldFormationPreset::Supercontinent => ResolvedWorldFormationPreset::Supercontinent,
            WorldFormationPreset::GreatIsland => ResolvedWorldFormationPreset::GreatIsland,
            WorldFormationPreset::VolcanicIslands => ResolvedWorldFormationPreset::VolcanicIslands,
        };
        ResolvedWorldFormation::new(RESOLVED_WORLD_FORMATION_SCHEMA_V1, spec.preset, resolved)
            .map_err(WorldFormationGenerationError::InvalidFormation)
    }
}

fn resolve_weighted_roll(roll: u32) -> ResolvedWorldFormationPreset {
    debug_assert!(roll < 100);
    match roll {
        0..=39 => ResolvedWorldFormationPreset::Continents,
        40..=64 => ResolvedWorldFormationPreset::Archipelago,
        65..=74 => ResolvedWorldFormationPreset::Supercontinent,
        75..=89 => ResolvedWorldFormationPreset::GreatIsland,
        90..=99 => ResolvedWorldFormationPreset::VolcanicIslands,
        _ => unreachable!("a modulo-100 roll is always inside the weighted table"),
    }
}

/// Errors returned while resolving a world-formation request.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldFormationGenerationError {
    /// The requested specification is invalid.
    #[error("invalid world-formation specification: {0}")]
    InvalidSpec(#[from] WorldFormationSpecError),
    /// The resolved result failed its immutable domain contract.
    #[error("invalid resolved world formation: {0}")]
    InvalidFormation(WorldFormationSpecError),
}

#[cfg(test)]
mod tests {
    use super::resolve_weighted_roll;
    use crate::world::natural::ResolvedWorldFormationPreset;

    #[test]
    fn weighted_roll_boundaries_select_the_documented_profiles() {
        let cases = [
            (0, ResolvedWorldFormationPreset::Continents),
            (39, ResolvedWorldFormationPreset::Continents),
            (40, ResolvedWorldFormationPreset::Archipelago),
            (64, ResolvedWorldFormationPreset::Archipelago),
            (65, ResolvedWorldFormationPreset::Supercontinent),
            (74, ResolvedWorldFormationPreset::Supercontinent),
            (75, ResolvedWorldFormationPreset::GreatIsland),
            (89, ResolvedWorldFormationPreset::GreatIsland),
            (90, ResolvedWorldFormationPreset::VolcanicIslands),
            (99, ResolvedWorldFormationPreset::VolcanicIslands),
        ];
        for (roll, expected) in cases {
            assert_eq!(resolve_weighted_roll(roll), expected);
        }
    }
}
