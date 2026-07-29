use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

const MAX_IDENTIFIER_BYTES: usize = 128;

/// Errors returned while constructing stable rule identities and versions.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuleIdentityError {
    /// An identifier component violates the supported V1 grammar.
    #[error(
        "{component} must be 1..={MAX_IDENTIFIER_BYTES} lowercase ASCII bytes, use only a-z, 0-9, '-', '_', or '.', and start and end with an alphanumeric byte"
    )]
    InvalidIdentifier {
        /// The role of the rejected component.
        component: &'static str,
    },
    /// Capability version zero is reserved.
    #[error("capability version must be non-zero")]
    ZeroCapabilityVersion,
    /// Rule semantic-version major zero is unsupported by the V1 compatibility rule.
    #[error("rule semantic-version major must be non-zero")]
    ZeroRuleMajor,
    /// A core-schema compatibility range is zero or reversed.
    #[error("core schema range must be non-zero with minimum <= maximum")]
    InvalidCoreSchemaRange,
}

macro_rules! string_id {
    ($name:ident, $component:literal) => {
        #[doc = concat!("A validated stable ", $component, ".")]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[doc = concat!("Creates a validated ", $component, ".")]
            pub fn new(value: impl Into<String>) -> Result<Self, RuleIdentityError> {
                let value = value.into();
                validate_identifier(&value, $component)?;
                Ok(Self(value))
            }

            #[doc = concat!("Returns the ", $component, " text.")]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(D::Error::custom)
            }
        }
    };
}

string_id!(RulePackId, "rule pack identifier");
string_id!(RuleItemId, "rule item identifier");

/// A stable, versioned identifier for one compiled rule capability.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CapabilityId {
    namespace: String,
    name: String,
    version: u16,
}

#[derive(Deserialize)]
struct CapabilityIdWire {
    namespace: String,
    name: String,
    version: u16,
}

impl CapabilityId {
    /// Creates a validated capability identifier.
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        version: u16,
    ) -> Result<Self, RuleIdentityError> {
        let namespace = namespace.into();
        let name = name.into();
        validate_identifier(&namespace, "capability namespace")?;
        validate_identifier(&name, "capability name")?;
        if version == 0 {
            return Err(RuleIdentityError::ZeroCapabilityVersion);
        }
        Ok(Self {
            namespace,
            name,
            version,
        })
    }

    /// Returns the capability namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Returns the capability name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the capability schema version.
    pub const fn version(&self) -> u16 {
        self.version
    }
}

impl<'de> Deserialize<'de> for CapabilityId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CapabilityIdWire::deserialize(deserializer)?;
        Self::new(wire.namespace, wire.name, wire.version).map_err(D::Error::custom)
    }
}

/// A bounded semantic version for one rule pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct RuleVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

#[derive(Deserialize)]
struct RuleVersionWire {
    major: u16,
    minor: u16,
    patch: u16,
}

impl RuleVersion {
    /// Creates a semantic version with a non-zero major component.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Result<Self, RuleIdentityError> {
        if major == 0 {
            return Err(RuleIdentityError::ZeroRuleMajor);
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }

    /// Returns the major component.
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the minor component.
    pub const fn minor(self) -> u16 {
        self.minor
    }

    /// Returns the patch component.
    pub const fn patch(self) -> u16 {
        self.patch
    }
}

impl<'de> Deserialize<'de> for RuleVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RuleVersionWire::deserialize(deserializer)?;
        Self::new(wire.major, wire.minor, wire.patch).map_err(D::Error::custom)
    }
}

/// A V1 rule-version requirement: exact major and an inclusive minimum minor/patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct RuleVersionRequirement {
    major: u16,
    minimum_minor: u16,
    minimum_patch: u16,
}

#[derive(Deserialize)]
struct RuleVersionRequirementWire {
    major: u16,
    minimum_minor: u16,
    minimum_patch: u16,
}

impl RuleVersionRequirement {
    /// Creates a V1 compatibility requirement.
    pub const fn new(
        major: u16,
        minimum_minor: u16,
        minimum_patch: u16,
    ) -> Result<Self, RuleIdentityError> {
        if major == 0 {
            return Err(RuleIdentityError::ZeroRuleMajor);
        }
        Ok(Self {
            major,
            minimum_minor,
            minimum_patch,
        })
    }

    /// Returns whether a version has the required major and minimum minor/patch.
    pub const fn matches(self, version: RuleVersion) -> bool {
        version.major == self.major
            && (version.minor > self.minimum_minor
                || (version.minor == self.minimum_minor && version.patch >= self.minimum_patch))
    }

    /// Returns the required major component.
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the inclusive minimum minor component.
    pub const fn minimum_minor(self) -> u16 {
        self.minimum_minor
    }

    /// Returns the inclusive minimum patch component at the minimum minor.
    pub const fn minimum_patch(self) -> u16 {
        self.minimum_patch
    }
}

impl<'de> Deserialize<'de> for RuleVersionRequirement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RuleVersionRequirementWire::deserialize(deserializer)?;
        Self::new(wire.major, wire.minimum_minor, wire.minimum_patch).map_err(D::Error::custom)
    }
}

/// An inclusive range of supported core world-schema versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CoreSchemaRange {
    minimum: u16,
    maximum: u16,
}

#[derive(Deserialize)]
struct CoreSchemaRangeWire {
    minimum: u16,
    maximum: u16,
}

impl CoreSchemaRange {
    /// Creates a non-zero, ordered core-schema range.
    pub const fn new(minimum: u16, maximum: u16) -> Result<Self, RuleIdentityError> {
        if minimum == 0 || minimum > maximum {
            return Err(RuleIdentityError::InvalidCoreSchemaRange);
        }
        Ok(Self { minimum, maximum })
    }

    /// Returns the inclusive minimum schema version.
    pub const fn minimum(self) -> u16 {
        self.minimum
    }

    /// Returns the inclusive maximum schema version.
    pub const fn maximum(self) -> u16 {
        self.maximum
    }

    /// Returns whether a schema version lies inside the range.
    pub const fn contains(self, version: u16) -> bool {
        version >= self.minimum && version <= self.maximum
    }
}

impl<'de> Deserialize<'de> for CoreSchemaRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CoreSchemaRangeWire::deserialize(deserializer)?;
        Self::new(wire.minimum, wire.maximum).map_err(D::Error::custom)
    }
}

/// A deterministic BLAKE3 content identity for one validated rule pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuleContentHash([u8; 32]);

impl RuleContentHash {
    /// Creates a content identity from already-computed BLAKE3 bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the exact hash bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

fn validate_identifier(value: &str, component: &'static str) -> Result<(), RuleIdentityError> {
    let bytes = value.as_bytes();
    let valid = (1..=MAX_IDENTIFIER_BYTES).contains(&bytes.len())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
        && bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(RuleIdentityError::InvalidIdentifier { component })
    }
}
