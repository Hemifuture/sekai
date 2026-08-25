use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use thiserror::Error;

use crate::engine::artifact::{ArtifactKey, ContentHash, StoredArtifact};
use crate::engine::diagnostics::Diagnostic;
use crate::engine::random::{StageIdentity, StageSeed};

const DEFAULT_MAX_ENTRIES: usize = 32;

/// A semantic BLAKE3 key for one versioned stage invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StageCacheKey([u8; 32]);

impl StageCacheKey {
    /// Computes a cache key from stage metadata, its seed, and checked dependency hashes.
    ///
    /// # Errors
    ///
    /// Returns [`StageCacheError::FrameLengthOverflow`] when any framed string length or
    /// the dependency count does not fit in the V1 frame's `u32` field.
    pub fn new(
        identity: StageIdentity,
        output: ArtifactKey,
        stage_seed: StageSeed,
        dependencies: &[(ArtifactKey, ContentHash)],
    ) -> Result<Self, StageCacheError> {
        checked_length_u32(identity.namespace().len())?;
        checked_length_u32(identity.id().len())?;
        checked_length_u32(output.as_str().len())?;
        checked_length_u32(dependencies.len())?;
        for (artifact_key, _) in dependencies {
            checked_length_u32(artifact_key.as_str().len())?;
        }

        let mut dependencies = dependencies.to_vec();
        dependencies.sort_unstable_by_key(|(artifact_key, _)| *artifact_key);

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai-stage-cache-v1\0");
        update_length_prefixed(&mut hasher, identity.namespace())?;
        update_length_prefixed(&mut hasher, identity.id())?;
        hasher.update(&identity.version().to_le_bytes());
        update_length_prefixed(&mut hasher, output.as_str())?;
        hasher.update(&stage_seed.into_bytes());
        hasher.update(&checked_length_u32(dependencies.len())?.to_le_bytes());
        for (artifact_key, content_hash) in dependencies {
            update_length_prefixed(&mut hasher, artifact_key.as_str())?;
            hasher.update(content_hash.as_bytes());
        }

        Ok(Self(*hasher.finalize().as_bytes()))
    }

    /// Returns the cache-key hash bytes without copying them.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Errors returned while configuring an in-memory stage cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum StageCacheError {
    /// A bounded cache cannot be configured with no storage slots.
    #[error("stage cache capacity must be greater than zero")]
    ZeroCapacity,
    /// A cache frame string length or dependency count does not fit in its V1 `u32` field.
    #[error("stage cache frame length or dependency count exceeds u32::MAX")]
    FrameLengthOverflow,
}

/// A deterministic, process-local, bounded FIFO cache of validated stage outputs.
#[derive(Clone)]
pub struct MemoryStageCache {
    entries: BTreeMap<StageCacheKey, CachedSuccessfulStage>,
    insertion_order: VecDeque<StageCacheKey>,
    max_entries: usize,
}

impl fmt::Debug for MemoryStageCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryStageCache")
            .field("len", &self.len())
            .field("max_entries", &self.max_entries)
            .finish_non_exhaustive()
    }
}

impl Default for MemoryStageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStageCache {
    /// Creates an empty cache with the default capacity of 32 entries.
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            insertion_order: VecDeque::new(),
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }

    /// Creates an empty cache with an explicit non-zero entry capacity.
    pub fn with_max_entries(max_entries: usize) -> Result<Self, StageCacheError> {
        if max_entries == 0 {
            return Err(StageCacheError::ZeroCapacity);
        }
        Ok(Self {
            entries: BTreeMap::new(),
            insertion_order: VecDeque::new(),
            max_entries,
        })
    }

    /// Returns the number of entries currently retained.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the cache currently retains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the configured maximum number of retained entries.
    pub const fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Returns whether a cache key is currently retained without exposing its erased value.
    pub fn contains(&self, key: &StageCacheKey) -> bool {
        self.entries.contains_key(key)
    }

    pub(crate) fn get(&self, key: &StageCacheKey) -> Option<(StoredArtifact, Vec<Diagnostic>)> {
        self.entries
            .get(key)
            .map(|entry| (entry.output.clone(), entry.diagnostics.clone()))
    }

    pub(crate) fn insert(
        &mut self,
        key: StageCacheKey,
        output: StoredArtifact,
        diagnostics: Vec<Diagnostic>,
    ) {
        let entry = CachedSuccessfulStage {
            output,
            diagnostics,
        };
        if let Some(existing) = self.entries.get_mut(&key) {
            *existing = entry;
            return;
        }

        while self.entries.len() >= self.max_entries {
            let oldest = self
                .insertion_order
                .pop_front()
                .expect("every cache entry must have one FIFO queue key");
            self.entries.remove(&oldest);
        }
        self.entries.insert(key, entry);
        self.insertion_order.push_back(key);
    }
}

/// Internal cache payload for one successful stage invocation.
///
/// Diagnostics are operational reporting data; they do not participate in the
/// cache key or any semantic artifact/result hash.
#[derive(Clone)]
struct CachedSuccessfulStage {
    output: StoredArtifact,
    diagnostics: Vec<Diagnostic>,
}

fn update_length_prefixed(hasher: &mut blake3::Hasher, value: &str) -> Result<(), StageCacheError> {
    hasher.update(&checked_length_u32(value.len())?.to_le_bytes());
    hasher.update(value.as_bytes());
    Ok(())
}

fn checked_length_u32(length: usize) -> Result<u32, StageCacheError> {
    u32::try_from(length).map_err(|_| StageCacheError::FrameLengthOverflow)
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::{checked_length_u32, MemoryStageCache, StageCacheError, StageCacheKey};
    use crate::engine::artifact::{Artifact, ArtifactKey, ArtifactValidationError, StoredArtifact};

    #[derive(Debug, Serialize)]
    struct CachedArtifact(u32);

    impl Artifact for CachedArtifact {
        const KEY: ArtifactKey = ArtifactKey::new("test.cached");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    fn key(byte: u8) -> StageCacheKey {
        StageCacheKey([byte; 32])
    }

    #[test]
    fn duplicate_insertion_replaces_in_place_without_duplicating_fifo_keys() {
        let mut cache = MemoryStageCache::with_max_entries(2).unwrap();
        cache.insert(
            key(1),
            StoredArtifact::new(CachedArtifact(1)).unwrap(),
            Vec::new(),
        );
        cache.insert(
            key(1),
            StoredArtifact::new(CachedArtifact(2)).unwrap(),
            Vec::new(),
        );

        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache.insertion_order.iter().copied().collect::<Vec<_>>(),
            vec![key(1)]
        );

        cache.insert(
            key(2),
            StoredArtifact::new(CachedArtifact(2)).unwrap(),
            Vec::new(),
        );
        cache.insert(
            key(3),
            StoredArtifact::new(CachedArtifact(3)).unwrap(),
            Vec::new(),
        );

        assert!(!cache.contains(&key(1)));
        assert!(cache.contains(&key(2)));
        assert!(cache.contains(&key(3)));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn oversized_frame_length_is_rejected_without_panicking() {
        assert_eq!(
            checked_length_u32(usize::MAX),
            Err(StageCacheError::FrameLengthOverflow)
        );
    }
}
