use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::sync::Arc;

use serde::Serialize;
use thiserror::Error;

use crate::engine::diagnostics::is_valid_identifier;

const INVALID_VALIDATION_CODE: &str = "engine.invalid-artifact-validation-code";

/// A stable key identifying one typed build artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactKey(&'static str);

impl ArtifactKey {
    /// Creates an artifact key for later graph validation.
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    /// Returns the key's static string value.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// A semantic BLAKE3 hash of one validated artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Creates a content hash from bytes computed inside the engine.
    pub(crate) const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the hash bytes without copying them.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// A deterministic validation failure emitted by an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct ArtifactValidationError {
    code: &'static str,
    message: String,
}

impl ArtifactValidationError {
    /// Creates a validation failure with a stable code and readable message.
    ///
    /// Invalid developer-supplied codes are replaced with an engine-owned stable
    /// code while the rejected code is retained in the message.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        if is_valid_identifier(code) {
            Self { code, message }
        } else {
            Self {
                code: INVALID_VALIDATION_CODE,
                message: format!("invalid validation code `{code}`: {message}"),
            }
        }
    }

    /// Returns the stable machine-readable validation code.
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the human-readable validation message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Errors returned while storing or reading typed artifacts.
#[derive(Debug, Error)]
pub enum ArtifactError {
    /// Artifact validation failed before hashing or storage.
    #[error("artifact `{artifact_key:?}` failed validation: {source}")]
    Validation {
        /// The key of the rejected artifact.
        artifact_key: ArtifactKey,
        /// The artifact's deterministic validation failure.
        #[source]
        source: ArtifactValidationError,
    },
    /// Validated artifact serialization failed while streaming its hash.
    #[error("artifact `{artifact_key:?}` could not be serialized: {source}")]
    Serialization {
        /// The key of the artifact that could not be serialized.
        artifact_key: ArtifactKey,
        /// The JSON serialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// No artifact was stored under the requested type's key.
    #[error("artifact `{artifact_key:?}` is missing")]
    Missing {
        /// The missing artifact key.
        artifact_key: ArtifactKey,
    },
    /// The stored value under a key has a different concrete Rust type.
    #[error("artifact `{artifact_key:?}` has a different concrete type")]
    TypeMismatch {
        /// The key whose stored value had the wrong type.
        artifact_key: ArtifactKey,
    },
    /// An artifact key was inserted more than once.
    #[error("artifact `{artifact_key:?}` was inserted more than once")]
    Duplicate {
        /// The duplicated artifact key.
        artifact_key: ArtifactKey,
    },
}

/// A serializable, validated value published by a generation stage.
pub trait Artifact: Serialize + Send + Sync + 'static {
    /// The globally stable key used for dependency declarations and lookup.
    const KEY: ArtifactKey;

    /// Validates all semantic invariants before hashing or publication.
    ///
    /// Implementations must reject non-finite numbers and use deterministic
    /// collection types for serialized maps.
    fn validate(&self) -> Result<(), ArtifactValidationError>;
}

#[derive(Clone, Copy)]
pub(crate) struct ArtifactType {
    key: ArtifactKey,
    type_id: TypeId,
}

impl ArtifactType {
    pub(crate) fn of<T: Artifact>() -> Self {
        Self {
            key: T::KEY,
            type_id: TypeId::of::<T>(),
        }
    }

    pub(crate) const fn key(self) -> ArtifactKey {
        self.key
    }

    pub(crate) fn hash_in(self, artifacts: &BuildArtifacts) -> Result<ContentHash, ArtifactError> {
        let stored = artifacts
            .entries
            .get(&self.key)
            .ok_or(ArtifactError::Missing {
                artifact_key: self.key,
            })?;
        self.validate_stored(stored)?;
        Ok(stored.hash)
    }

    pub(crate) fn publish_into(
        self,
        stored: StoredArtifact,
        artifacts: &mut BuildArtifacts,
    ) -> Result<(), ArtifactError> {
        self.validate_stored(&stored)?;
        if artifacts.entries.contains_key(&self.key) {
            return Err(ArtifactError::Duplicate {
                artifact_key: self.key,
            });
        }
        artifacts.entries.insert(self.key, stored);
        Ok(())
    }

    fn validate_stored(self, stored: &StoredArtifact) -> Result<(), ArtifactError> {
        if stored.key() != self.key {
            return Err(ArtifactError::Missing {
                artifact_key: self.key,
            });
        }
        stored.validate_type_id(self.type_id)
    }
}

#[derive(Clone)]
pub(crate) struct StoredArtifact {
    key: ArtifactKey,
    value: Arc<dyn Any + Send + Sync>,
    hash: ContentHash,
}

impl StoredArtifact {
    pub(crate) fn new<T: Artifact>(value: T) -> Result<Self, ArtifactError> {
        value
            .validate()
            .map_err(|source| ArtifactError::Validation {
                artifact_key: T::KEY,
                source,
            })?;
        let hash = stream_hash(&value).map_err(|source| ArtifactError::Serialization {
            artifact_key: T::KEY,
            source,
        })?;
        Ok(Self {
            key: T::KEY,
            value: Arc::new(value),
            hash,
        })
    }

    const fn key(&self) -> ArtifactKey {
        self.key
    }

    fn validate_type_id(&self, expected: TypeId) -> Result<(), ArtifactError> {
        if self.value.as_ref().type_id() != expected {
            return Err(ArtifactError::TypeMismatch {
                artifact_key: self.key,
            });
        }
        Ok(())
    }
}

/// Typed artifacts and semantic hashes published during one build.
#[derive(Default)]
pub struct BuildArtifacts {
    entries: BTreeMap<ArtifactKey, StoredArtifact>,
}

impl fmt::Debug for BuildArtifacts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuildArtifacts")
            .field("artifact_keys", &self.entries.keys())
            .finish()
    }
}

impl BuildArtifacts {
    /// Returns an owned typed handle to a stored artifact.
    pub fn get<T: Artifact>(&self) -> Result<Arc<T>, ArtifactError> {
        let stored = self.entries.get(&T::KEY).ok_or(ArtifactError::Missing {
            artifact_key: T::KEY,
        })?;
        Arc::downcast::<T>(Arc::clone(&stored.value)).map_err(|_| ArtifactError::TypeMismatch {
            artifact_key: T::KEY,
        })
    }

    /// Returns the semantic content hash for a stored typed artifact.
    pub fn hash<T: Artifact>(&self) -> Result<ContentHash, ArtifactError> {
        ArtifactType::of::<T>().hash_in(self)
    }

    pub(crate) fn dependency_view(
        &self,
        dependencies: &[ArtifactKey],
    ) -> Result<Self, ArtifactError> {
        let mut entries = BTreeMap::new();
        for dependency in dependencies {
            let stored = self.entries.get(dependency).ok_or(ArtifactError::Missing {
                artifact_key: *dependency,
            })?;
            entries.insert(*dependency, stored.clone());
        }
        Ok(Self { entries })
    }

    pub(crate) fn insert<T: Artifact>(&mut self, value: T) -> Result<(), ArtifactError> {
        ArtifactType::of::<T>().publish_into(StoredArtifact::new(value)?, self)
    }

    pub(crate) fn keys(&self) -> impl ExactSizeIterator<Item = ArtifactKey> + '_ {
        self.entries.keys().copied()
    }
}

fn stream_hash<T: Serialize>(value: &T) -> Result<ContentHash, serde_json::Error> {
    let mut hasher = blake3::Hasher::new();
    serde_json::to_writer(HasherWriter(&mut hasher), value)?;
    Ok(ContentHash::new(*hasher.finalize().as_bytes()))
}

struct HasherWriter<'a>(&'a mut blake3::Hasher);

impl Write for HasherWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::ser::Error as _;
    use serde::{Serialize, Serializer};

    use super::{Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts};

    #[derive(Debug)]
    struct InvalidArtifact;

    impl Serialize for InvalidArtifact {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(S::Error::custom(
                "serialization must not run before validation",
            ))
        }
    }

    impl Artifact for InvalidArtifact {
        const KEY: ArtifactKey = ArtifactKey::new("test.invalid");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Err(ArtifactValidationError::new(
                "test.non-finite",
                "artifact contains a non-finite value",
            ))
        }
    }

    #[derive(Debug, Serialize)]
    struct MapArtifact {
        values: BTreeMap<String, u32>,
    }

    impl Artifact for MapArtifact {
        const KEY: ArtifactKey = ArtifactKey::new("test.map");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[derive(Debug, Serialize)]
    struct ScalarArtifact(u32);

    impl Artifact for ScalarArtifact {
        const KEY: ArtifactKey = ArtifactKey::new("test.scalar");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[derive(Debug, Serialize)]
    struct SameKeyArtifact(u32);

    impl Artifact for SameKeyArtifact {
        const KEY: ArtifactKey = ScalarArtifact::KEY;

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct SerializationFailureArtifact;

    impl Serialize for SerializationFailureArtifact {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(S::Error::custom("intentional serialization failure"))
        }
    }

    impl Artifact for SerializationFailureArtifact {
        const KEY: ArtifactKey = ArtifactKey::new("test.serialization-failure");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[test]
    fn invalid_artifact_is_rejected_before_serialization_or_storage() {
        let mut artifacts = BuildArtifacts::default();

        let error = artifacts.insert(InvalidArtifact).unwrap_err();

        assert!(matches!(
            error,
            ArtifactError::Validation { artifact_key, .. }
                if artifact_key == InvalidArtifact::KEY
        ));
        assert!(matches!(
            artifacts.get::<InvalidArtifact>(),
            Err(ArtifactError::Missing { artifact_key })
                if artifact_key == InvalidArtifact::KEY
        ));
    }

    #[test]
    fn deterministic_map_order_produces_the_same_content_hash() {
        let mut forward = BTreeMap::new();
        forward.insert("a".to_owned(), 1);
        forward.insert("b".to_owned(), 2);

        let mut reverse = BTreeMap::new();
        reverse.insert("b".to_owned(), 2);
        reverse.insert("a".to_owned(), 1);

        let mut first = BuildArtifacts::default();
        first.insert(MapArtifact { values: forward }).unwrap();
        let mut second = BuildArtifacts::default();
        second.insert(MapArtifact { values: reverse }).unwrap();

        assert_eq!(
            first.hash::<MapArtifact>().unwrap(),
            second.hash::<MapArtifact>().unwrap()
        );
    }

    #[test]
    fn typed_get_returns_the_stored_arc_value() {
        let mut artifacts = BuildArtifacts::default();
        artifacts.insert(ScalarArtifact(42)).unwrap();

        let value = artifacts.get::<ScalarArtifact>().unwrap();

        assert_eq!(value.0, 42);
    }

    #[test]
    fn dependency_view_reuses_the_stored_arc() {
        let mut artifacts = BuildArtifacts::default();
        artifacts.insert(ScalarArtifact(42)).unwrap();
        let original = artifacts.get::<ScalarArtifact>().unwrap();

        let view = artifacts.dependency_view(&[ScalarArtifact::KEY]).unwrap();
        let restricted = view.get::<ScalarArtifact>().unwrap();

        assert!(std::sync::Arc::ptr_eq(&original, &restricted));
    }

    #[test]
    fn serialization_failure_after_validation_leaves_store_unchanged() {
        let mut artifacts = BuildArtifacts::default();

        let error = artifacts.insert(SerializationFailureArtifact).unwrap_err();

        assert!(matches!(
            error,
            ArtifactError::Serialization { artifact_key, .. }
                if artifact_key == SerializationFailureArtifact::KEY
        ));
        assert!(matches!(
            artifacts.get::<SerializationFailureArtifact>(),
            Err(ArtifactError::Missing { artifact_key })
                if artifact_key == SerializationFailureArtifact::KEY
        ));
    }

    #[test]
    fn duplicate_insertion_leaves_the_original_value_intact() {
        let mut artifacts = BuildArtifacts::default();
        artifacts.insert(ScalarArtifact(1)).unwrap();

        let error = artifacts.insert(ScalarArtifact(2)).unwrap_err();

        assert!(matches!(
            error,
            ArtifactError::Duplicate { artifact_key }
                if artifact_key == ScalarArtifact::KEY
        ));
        assert_eq!(artifacts.get::<ScalarArtifact>().unwrap().0, 1);
    }

    #[test]
    fn typed_access_rejects_a_same_key_value_of_another_type() {
        let mut artifacts = BuildArtifacts::default();
        artifacts.insert(SameKeyArtifact(7)).unwrap();

        assert!(matches!(
            artifacts.get::<ScalarArtifact>(),
            Err(ArtifactError::TypeMismatch { artifact_key })
                if artifact_key == ScalarArtifact::KEY
        ));
        assert!(matches!(
            artifacts.hash::<ScalarArtifact>(),
            Err(ArtifactError::TypeMismatch { artifact_key })
                if artifact_key == ScalarArtifact::KEY
        ));
    }

    #[test]
    fn invalid_validation_error_codes_are_replaced_with_a_stable_code() {
        let error = ArtifactValidationError::new("Bad Code", "invalid value");

        assert_eq!(error.code(), "engine.invalid-artifact-validation-code");
        assert!(error.message().contains("Bad Code"));
    }
}
