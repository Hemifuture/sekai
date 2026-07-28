//! Deterministic, domain-neutral engine services.

mod random;

pub use random::{derive_entity_seed, derive_stage_seed, StageIdentity, StageRng, StageSeed};
