use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use super::{CapabilityId, ConstraintError, RuleItemId, RuleTectonicConstraint};

const CORE_NATURAL_NAMESPACE: &str = "sekai.core.natural";
const TECTONIC_MODEL_NAME: &str = "tectonic-model";
const TECTONIC_CONTROLS_NAME: &str = "tectonic-controls";
const CAPABILITY_SCHEMA_V1: u16 = 1;

/// Returns the stable unique tectonic-model capability ID.
pub fn tectonic_model_capability_id() -> CapabilityId {
    CapabilityId::new(
        CORE_NATURAL_NAMESPACE,
        TECTONIC_MODEL_NAME,
        CAPABILITY_SCHEMA_V1,
    )
    .expect("the engine-owned tectonic model capability ID is valid")
}

/// Returns the stable mergeable tectonic-controls capability ID.
pub fn tectonic_controls_capability_id() -> CapabilityId {
    CapabilityId::new(
        CORE_NATURAL_NAMESPACE,
        TECTONIC_CONTROLS_NAME,
        CAPABILITY_SCHEMA_V1,
    )
    .expect("the engine-owned tectonic controls capability ID is valid")
}

/// The permission class assigned to one data-only rule pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RulePackKind {
    /// A content pack limited to ordinary public capabilities.
    Ordinary,
    /// A trusted data pack allowed to select compiled core world-law models.
    WorldLaw,
}

/// How many rule packs may provide one capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CapabilityCardinality {
    /// Exactly one provider is required.
    UniqueRequired,
    /// Zero or one provider is allowed.
    UniqueOptional,
    /// A bounded number of providers may contribute to a typed merger.
    Merge,
}

/// Immutable permission and cardinality metadata for one compiled capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    id: CapabilityId,
    cardinality: CapabilityCardinality,
    minimum_pack_kind: RulePackKind,
    author_allowed: bool,
}

impl CapabilityDescriptor {
    /// Creates one capability descriptor from already-validated identity data.
    pub const fn new(
        id: CapabilityId,
        cardinality: CapabilityCardinality,
        minimum_pack_kind: RulePackKind,
        author_allowed: bool,
    ) -> Self {
        Self {
            id,
            cardinality,
            minimum_pack_kind,
            author_allowed,
        }
    }

    /// Returns the stable capability ID.
    pub const fn id(&self) -> &CapabilityId {
        &self.id
    }

    /// Returns the provider-cardinality contract.
    pub const fn cardinality(&self) -> CapabilityCardinality {
        self.cardinality
    }

    /// Returns the minimum rule-pack permission.
    pub const fn minimum_pack_kind(&self) -> RulePackKind {
        self.minimum_pack_kind
    }

    /// Returns whether authored project objects may contribute directly.
    pub const fn author_allowed(&self) -> bool {
        self.author_allowed
    }

    /// Returns whether a rule-pack permission meets this capability's minimum.
    pub const fn allows_pack_kind(&self, kind: RulePackKind) -> bool {
        match self.minimum_pack_kind {
            RulePackKind::Ordinary => true,
            RulePackKind::WorldLaw => matches!(kind, RulePackKind::WorldLaw),
        }
    }
}

/// Errors returned while registering immutable capability descriptors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CapabilityRegistryError {
    /// A capability descriptor was registered more than once.
    #[error("capability {capability_id:?} is already registered")]
    DuplicateCapability {
        /// The duplicate stable capability ID.
        capability_id: CapabilityId,
    },
}

/// A mutable builder that rejects duplicate capability descriptors.
#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistryBuilder {
    descriptors: BTreeMap<CapabilityId, CapabilityDescriptor>,
}

impl CapabilityRegistryBuilder {
    /// Creates an empty capability registry builder.
    pub const fn new() -> Self {
        Self {
            descriptors: BTreeMap::new(),
        }
    }

    /// Registers one descriptor without replacing an existing entry.
    pub fn register(
        &mut self,
        descriptor: CapabilityDescriptor,
    ) -> Result<(), CapabilityRegistryError> {
        let id = descriptor.id().clone();
        if self.descriptors.contains_key(&id) {
            return Err(CapabilityRegistryError::DuplicateCapability { capability_id: id });
        }
        self.descriptors.insert(id, descriptor);
        Ok(())
    }

    /// Freezes descriptors into stable capability-ID order.
    pub fn build(self) -> CapabilityRegistry {
        CapabilityRegistry {
            descriptors: self.descriptors.into_values().collect(),
        }
    }
}

/// A frozen, deterministically ordered registry of compiled capabilities.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityRegistry {
    descriptors: Vec<CapabilityDescriptor>,
}

impl CapabilityRegistry {
    /// Returns a registered descriptor by stable ID.
    pub fn get(&self, id: &CapabilityId) -> Option<&CapabilityDescriptor> {
        self.descriptors
            .binary_search_by(|descriptor| descriptor.id().cmp(id))
            .ok()
            .map(|index| &self.descriptors[index])
    }

    /// Iterates through descriptors in stable capability-ID order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &CapabilityDescriptor> {
        self.descriptors.iter()
    }

    /// Returns the descriptor count.
    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    /// Returns whether no capabilities are registered.
    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
}

impl Serialize for CapabilityRegistry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.descriptors.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CapabilityRegistry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let descriptors = Vec::<CapabilityDescriptor>::deserialize(deserializer)?;
        let mut builder = CapabilityRegistryBuilder::new();
        for descriptor in descriptors {
            builder.register(descriptor).map_err(D::Error::custom)?;
        }
        Ok(builder.build())
    }
}

/// A trusted compiled tectonic-model implementation selected by world law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TectonicModel {
    /// The deterministic current-slice tectonic synthesizer.
    CurrentSliceV1,
}

/// A closed data contribution accepted by the V1 capability system.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CapabilityContribution {
    /// Selects one trusted compiled tectonic model.
    TectonicModel(TectonicModel),
    /// Adds one typed tectonic control constraint.
    TectonicConstraint(RuleTectonicConstraint),
}

impl CapabilityContribution {
    /// Returns the exact capability receiving this contribution.
    pub fn capability_id(&self) -> CapabilityId {
        match self {
            Self::TectonicModel(_) => tectonic_model_capability_id(),
            Self::TectonicConstraint(_) => tectonic_controls_capability_id(),
        }
    }

    /// Returns a local item ID for merge contributions that require uniqueness.
    pub fn rule_item_id(&self) -> Option<&RuleItemId> {
        match self {
            Self::TectonicModel(_) => None,
            Self::TectonicConstraint(constraint) => Some(constraint.item_id()),
        }
    }

    /// Revalidates the typed contribution payload.
    pub fn validate(&self) -> Result<(), ConstraintError> {
        match self {
            Self::TectonicModel(_) => Ok(()),
            Self::TectonicConstraint(constraint) => constraint.validate(),
        }
    }
}
