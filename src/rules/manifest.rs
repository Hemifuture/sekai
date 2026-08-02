use std::collections::BTreeSet;
use std::io::{self, Write};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    CapabilityContribution, CapabilityId, ConstraintError, CoreSchemaRange, RuleContentHash,
    RuleItemId, RulePackId, RulePackKind, RuleVersion, RuleVersionRequirement,
};

/// The supported serialized rule-pack schema.
pub const RULE_PACK_SCHEMA_V1: u16 = 1;
/// The maximum number of declared pack dependencies in one V1 pack.
pub const MAX_RULE_PACK_DEPENDENCIES: usize = 32;
/// The maximum number of consumed capabilities in one V1 pack.
pub const MAX_RULE_PACK_CAPABILITY_REQUIREMENTS: usize = 32;
/// The maximum number of typed contributions in one V1 pack.
pub const MAX_RULE_PACK_CONTRIBUTIONS: usize = 256;

/// Errors returned while constructing or revalidating one data-only rule pack.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RulePackError {
    /// A rule pack uses an unsupported serialized schema.
    #[error("unsupported rule-pack schema {found}; supported schema is {RULE_PACK_SCHEMA_V1}")]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
    },
    /// A rule pack exceeds its dependency budget.
    #[error("rule-pack dependency count {found} exceeds V1 limit {MAX_RULE_PACK_DEPENDENCIES}")]
    TooManyDependencies {
        /// The rejected dependency count.
        found: usize,
    },
    /// A dependency pack ID occurs more than once.
    #[error("duplicate rule-pack dependency {pack_id:?}")]
    DuplicateDependency {
        /// The repeated pack ID.
        pack_id: RulePackId,
    },
    /// A rule pack exceeds its consumed-capability budget.
    #[error(
        "capability requirement count {found} exceeds V1 limit {MAX_RULE_PACK_CAPABILITY_REQUIREMENTS}"
    )]
    TooManyCapabilityRequirements {
        /// The rejected requirement count.
        found: usize,
    },
    /// A consumed capability occurs more than once.
    #[error("duplicate consumed capability {capability_id:?}")]
    DuplicateConsumedCapability {
        /// The repeated capability ID.
        capability_id: CapabilityId,
    },
    /// A rule pack exceeds its typed-contribution budget.
    #[error("rule contribution count {found} exceeds V1 limit {MAX_RULE_PACK_CONTRIBUTIONS}")]
    TooManyContributions {
        /// The rejected contribution count.
        found: usize,
    },
    /// A local rule item occurs more than once.
    #[error("duplicate local rule item {item_id:?}")]
    DuplicateRuleItem {
        /// The repeated local item ID.
        item_id: RuleItemId,
    },
    /// A pack submitted more than one contribution to a closed unique capability.
    #[error("duplicate unique contribution for capability {capability_id:?}")]
    DuplicateUniqueContribution {
        /// The duplicated unique capability.
        capability_id: CapabilityId,
    },
    /// A typed contribution payload failed its own validation.
    #[error(transparent)]
    InvalidContribution(#[from] ConstraintError),
    /// Stable content serialization unexpectedly failed.
    #[error("rule-pack content could not be hashed: {message}")]
    HashSerialization {
        /// The serialization failure text.
        message: String,
    },
    /// A serialized pack's declared hash does not match its canonical content.
    #[error("declared rule content hash does not match canonical pack content")]
    ContentHashMismatch {
        /// The hash stored in the serialized manifest.
        declared: RuleContentHash,
        /// The hash recomputed from canonical semantic content.
        computed: RuleContentHash,
    },
    /// A serialized manifest's declared provider list differs from its contributions.
    #[error("manifest capability providers do not match typed contributions")]
    ManifestProvidesMismatch,
    /// A private stored collection is no longer in canonical order.
    #[error("stored rule pack field {field} is not canonical")]
    NonCanonical {
        /// The non-canonical field name.
        field: &'static str,
    },
}

/// One exact-major, minimum-version dependency on another rule pack.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RulePackDependency {
    pack_id: RulePackId,
    version_requirement: RuleVersionRequirement,
}

impl RulePackDependency {
    /// Creates a dependency from already-validated identity and version contracts.
    pub const fn new(pack_id: RulePackId, version_requirement: RuleVersionRequirement) -> Self {
        Self {
            pack_id,
            version_requirement,
        }
    }

    /// Returns the required pack ID.
    pub const fn pack_id(&self) -> &RulePackId {
        &self.pack_id
    }

    /// Returns the compatible minimum version.
    pub const fn version_requirement(&self) -> RuleVersionRequirement {
        self.version_requirement
    }
}

/// Immutable identity and dependency metadata declared by one rule pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulePackManifest {
    schema_version: u16,
    id: RulePackId,
    version: RuleVersion,
    kind: RulePackKind,
    core_schema: CoreSchemaRange,
    dependencies: Vec<RulePackDependency>,
    provides: Vec<CapabilityId>,
    consumes: Vec<CapabilityId>,
    content_hash: RuleContentHash,
}

impl RulePackManifest {
    /// Returns the rule-pack schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the stable pack ID.
    pub const fn id(&self) -> &RulePackId {
        &self.id
    }

    /// Returns the semantic pack version.
    pub const fn version(&self) -> RuleVersion {
        self.version
    }

    /// Returns the permission class.
    pub const fn kind(&self) -> RulePackKind {
        self.kind
    }

    /// Returns the compatible core-schema range.
    pub const fn core_schema(&self) -> CoreSchemaRange {
        self.core_schema
    }

    /// Returns pack dependencies in stable pack-ID order.
    pub fn dependencies(&self) -> &[RulePackDependency] {
        &self.dependencies
    }

    /// Returns provided capabilities in stable ID order.
    pub fn provides(&self) -> &[CapabilityId] {
        &self.provides
    }

    /// Returns consumed capabilities in stable ID order.
    pub fn consumes(&self) -> &[CapabilityId] {
        &self.consumes
    }

    /// Returns the deterministic semantic content identity.
    pub const fn content_hash(&self) -> RuleContentHash {
        self.content_hash
    }
}

impl<'de> Deserialize<'de> for RulePackManifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RulePackManifestWire::deserialize(deserializer)?;
        if wire.schema_version != RULE_PACK_SCHEMA_V1 {
            return Err(D::Error::custom(RulePackError::UnsupportedSchema {
                found: wire.schema_version,
            }));
        }
        let dependencies = canonical_dependencies(wire.dependencies).map_err(D::Error::custom)?;
        let consumes = canonical_consumes(wire.consumes).map_err(D::Error::custom)?;
        let mut provides = wire.provides;
        provides.sort();
        if provides.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(D::Error::custom(RulePackError::ManifestProvidesMismatch));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            id: wire.id,
            version: wire.version,
            kind: wire.kind,
            core_schema: wire.core_schema,
            dependencies,
            provides,
            consumes,
            content_hash: wire.content_hash,
        })
    }
}

/// One validated data-only rule pack and its closed typed contributions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulePack {
    manifest: RulePackManifest,
    contributions: Vec<CapabilityContribution>,
}

#[derive(Deserialize)]
struct RulePackWire {
    manifest: RulePackManifestWire,
    contributions: Vec<CapabilityContribution>,
}

#[derive(Deserialize)]
struct RulePackManifestWire {
    schema_version: u16,
    id: RulePackId,
    version: RuleVersion,
    kind: RulePackKind,
    core_schema: CoreSchemaRange,
    dependencies: Vec<RulePackDependency>,
    provides: Vec<CapabilityId>,
    consumes: Vec<CapabilityId>,
    content_hash: RuleContentHash,
}

#[derive(Serialize)]
struct RulePackContentFrame<'a> {
    schema_version: u16,
    id: &'a RulePackId,
    version: RuleVersion,
    kind: RulePackKind,
    core_schema: CoreSchemaRange,
    dependencies: &'a [RulePackDependency],
    consumes: &'a [CapabilityId],
    contributions: &'a [CapabilityContribution],
}

impl RulePack {
    /// Creates a canonical pack and computes its deterministic content hash.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: RulePackId,
        version: RuleVersion,
        kind: RulePackKind,
        core_schema: CoreSchemaRange,
        dependencies: Vec<RulePackDependency>,
        consumes: Vec<CapabilityId>,
        contributions: Vec<CapabilityContribution>,
    ) -> Result<Self, RulePackError> {
        Self::new_with_schema(
            RULE_PACK_SCHEMA_V1,
            id,
            version,
            kind,
            core_schema,
            dependencies,
            consumes,
            contributions,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_schema(
        schema_version: u16,
        id: RulePackId,
        version: RuleVersion,
        kind: RulePackKind,
        core_schema: CoreSchemaRange,
        dependencies: Vec<RulePackDependency>,
        consumes: Vec<CapabilityId>,
        contributions: Vec<CapabilityContribution>,
    ) -> Result<Self, RulePackError> {
        if schema_version != RULE_PACK_SCHEMA_V1 {
            return Err(RulePackError::UnsupportedSchema {
                found: schema_version,
            });
        }
        let dependencies = canonical_dependencies(dependencies)?;
        let consumes = canonical_consumes(consumes)?;
        let contributions = canonical_contributions(contributions)?;
        let provides = provided_capabilities(&contributions);
        let content_hash = hash_content(&RulePackContentFrame {
            schema_version,
            id: &id,
            version,
            kind,
            core_schema,
            dependencies: &dependencies,
            consumes: &consumes,
            contributions: &contributions,
        })?;
        Ok(Self {
            manifest: RulePackManifest {
                schema_version,
                id,
                version,
                kind,
                core_schema,
                dependencies,
                provides,
                consumes,
                content_hash,
            },
            contributions,
        })
    }

    /// Returns the immutable manifest.
    pub const fn manifest(&self) -> &RulePackManifest {
        &self.manifest
    }

    /// Returns typed contributions in canonical order.
    pub fn contributions(&self) -> &[CapabilityContribution] {
        &self.contributions
    }

    /// Revalidates canonical collections, payloads, providers, and content hash.
    pub fn validate(&self) -> Result<(), RulePackError> {
        if self.manifest.schema_version != RULE_PACK_SCHEMA_V1 {
            return Err(RulePackError::UnsupportedSchema {
                found: self.manifest.schema_version,
            });
        }
        if canonical_dependencies(self.manifest.dependencies.clone())? != self.manifest.dependencies
        {
            return Err(RulePackError::NonCanonical {
                field: "dependencies",
            });
        }
        if canonical_consumes(self.manifest.consumes.clone())? != self.manifest.consumes {
            return Err(RulePackError::NonCanonical { field: "consumes" });
        }
        if canonical_contributions(self.contributions.clone())? != self.contributions {
            return Err(RulePackError::NonCanonical {
                field: "contributions",
            });
        }
        if provided_capabilities(&self.contributions) != self.manifest.provides {
            return Err(RulePackError::ManifestProvidesMismatch);
        }
        let computed = hash_content(&RulePackContentFrame {
            schema_version: self.manifest.schema_version,
            id: &self.manifest.id,
            version: self.manifest.version,
            kind: self.manifest.kind,
            core_schema: self.manifest.core_schema,
            dependencies: &self.manifest.dependencies,
            consumes: &self.manifest.consumes,
            contributions: &self.contributions,
        })?;
        if computed != self.manifest.content_hash {
            return Err(RulePackError::ContentHashMismatch {
                declared: self.manifest.content_hash,
                computed,
            });
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for RulePack {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RulePackWire::deserialize(deserializer)?;
        let declared_hash = wire.manifest.content_hash;
        let mut declared_provides = wire.manifest.provides;
        declared_provides.sort();
        if declared_provides.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(D::Error::custom(RulePackError::ManifestProvidesMismatch));
        }
        let pack = Self::new_with_schema(
            wire.manifest.schema_version,
            wire.manifest.id,
            wire.manifest.version,
            wire.manifest.kind,
            wire.manifest.core_schema,
            wire.manifest.dependencies,
            wire.manifest.consumes,
            wire.contributions,
        )
        .map_err(D::Error::custom)?;
        if declared_provides != pack.manifest.provides {
            return Err(D::Error::custom(RulePackError::ManifestProvidesMismatch));
        }
        if declared_hash != pack.manifest.content_hash {
            return Err(D::Error::custom(RulePackError::ContentHashMismatch {
                declared: declared_hash,
                computed: pack.manifest.content_hash,
            }));
        }
        Ok(pack)
    }
}

fn canonical_dependencies(
    mut dependencies: Vec<RulePackDependency>,
) -> Result<Vec<RulePackDependency>, RulePackError> {
    if dependencies.len() > MAX_RULE_PACK_DEPENDENCIES {
        return Err(RulePackError::TooManyDependencies {
            found: dependencies.len(),
        });
    }
    dependencies.sort_by(|left, right| left.pack_id().cmp(right.pack_id()));
    if let Some(pack_id) = dependencies.windows(2).find_map(|pair| {
        (pair[0].pack_id() == pair[1].pack_id()).then(|| pair[0].pack_id().clone())
    }) {
        return Err(RulePackError::DuplicateDependency { pack_id });
    }
    Ok(dependencies)
}

fn canonical_consumes(mut consumes: Vec<CapabilityId>) -> Result<Vec<CapabilityId>, RulePackError> {
    if consumes.len() > MAX_RULE_PACK_CAPABILITY_REQUIREMENTS {
        return Err(RulePackError::TooManyCapabilityRequirements {
            found: consumes.len(),
        });
    }
    consumes.sort();
    if let Some(capability_id) = consumes
        .windows(2)
        .find_map(|pair| (pair[0] == pair[1]).then(|| pair[0].clone()))
    {
        return Err(RulePackError::DuplicateConsumedCapability { capability_id });
    }
    Ok(consumes)
}

fn canonical_contributions(
    mut contributions: Vec<CapabilityContribution>,
) -> Result<Vec<CapabilityContribution>, RulePackError> {
    if contributions.len() > MAX_RULE_PACK_CONTRIBUTIONS {
        return Err(RulePackError::TooManyContributions {
            found: contributions.len(),
        });
    }
    for contribution in &contributions {
        contribution.validate()?;
    }
    contributions.sort();

    let mut item_ids = BTreeSet::new();
    let mut unique_contributions = BTreeSet::new();
    for contribution in &contributions {
        if let Some(item_id) = contribution.rule_item_id() {
            if !item_ids.insert(item_id.clone()) {
                return Err(RulePackError::DuplicateRuleItem {
                    item_id: item_id.clone(),
                });
            }
        }
        if matches!(
            contribution,
            CapabilityContribution::TectonicModel(_)
                | CapabilityContribution::GeologicModel(_)
                | CapabilityContribution::ClimateModel(_)
                | CapabilityContribution::HydroErosionModel(_)
        ) {
            let capability_id = contribution.capability_id();
            if !unique_contributions.insert(capability_id.clone()) {
                return Err(RulePackError::DuplicateUniqueContribution { capability_id });
            }
        }
    }
    Ok(contributions)
}

fn provided_capabilities(contributions: &[CapabilityContribution]) -> Vec<CapabilityId> {
    contributions
        .iter()
        .map(CapabilityContribution::capability_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn hash_content<T: Serialize>(value: &T) -> Result<RuleContentHash, RulePackError> {
    let mut hasher = blake3::Hasher::new();
    serde_json::to_writer(HasherWriter(&mut hasher), value).map_err(|error| {
        RulePackError::HashSerialization {
            message: error.to_string(),
        }
    })?;
    Ok(RuleContentHash::from_bytes(*hasher.finalize().as_bytes()))
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
