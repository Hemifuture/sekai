use rand::RngCore;

use crate::engine::cancellation::{BuildCancellation, BuildCancellationError};
use crate::world::RootSeed;

/// Identifies a versioned generation stage within an owning namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StageIdentity {
    id: &'static str,
    version: u32,
    namespace: &'static str,
}

impl StageIdentity {
    /// Creates a versioned stage identity in its registered namespace.
    pub const fn new(id: &'static str, version: u32, namespace: &'static str) -> Self {
        Self {
            id,
            version,
            namespace,
        }
    }

    /// Returns the stable stage identifier.
    pub const fn id(self) -> &'static str {
        self.id
    }

    /// Returns the stage implementation version.
    pub const fn version(self) -> u32 {
        self.version
    }

    /// Returns the namespace owning the stage's deterministic streams.
    pub const fn namespace(self) -> &'static str {
        self.namespace
    }
}

/// A 32-byte deterministic seed for a generation stage or entity stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StageSeed([u8; 32]);

impl StageSeed {
    /// Returns all 32 bytes of this deterministic seed.
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// A private-ChaCha deterministic random stream derived from a stage seed.
pub struct StageRng {
    random: rand_chacha::ChaCha8Rng,
    cancellation: BuildCancellation,
}

impl StageRng {
    /// Creates a deterministic random stream from a full 32-byte stage seed.
    pub fn from_seed(seed: StageSeed) -> Self {
        Self::from_seed_with_cancellation(seed, &BuildCancellation::new())
    }

    pub(crate) fn from_seed_with_cancellation(
        seed: StageSeed,
        cancellation: &BuildCancellation,
    ) -> Self {
        use rand::SeedableRng;

        Self {
            random: rand_chacha::ChaCha8Rng::from_seed(seed.into_bytes()),
            cancellation: cancellation.clone(),
        }
    }

    /// Returns whether the owning build has requested cooperative cancellation.
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// Returns a stable error when the owning build requested cancellation.
    pub fn check_cancelled(&self) -> Result<(), BuildCancellationError> {
        self.cancellation.check_cancelled()
    }
}

impl RngCore for StageRng {
    fn next_u32(&mut self) -> u32 {
        self.random.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.random.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.random.fill_bytes(dest);
    }
}

/// Derives a versioned, namespace-qualified random seed from a world's root seed.
pub fn derive_stage_seed(root_seed: RootSeed, stage: StageIdentity) -> StageSeed {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai-stage-seed-v1\0");
    hasher.update(&root_seed.raw().to_le_bytes());
    update_length_prefixed(&mut hasher, stage.namespace);
    update_length_prefixed(&mut hasher, stage.id);
    hasher.update(&stage.version.to_le_bytes());

    StageSeed(*hasher.finalize().as_bytes())
}

/// Derives an entity-specific random seed in an explicit entity namespace.
pub fn derive_entity_seed(
    stage_seed: StageSeed,
    entity_namespace: &str,
    entity_key: u64,
) -> StageSeed {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai-entity-seed-v1\0");
    hasher.update(&stage_seed.into_bytes());
    update_length_prefixed(&mut hasher, entity_namespace);
    hasher.update(&entity_key.to_le_bytes());

    StageSeed(*hasher.finalize().as_bytes())
}

fn update_length_prefixed(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
