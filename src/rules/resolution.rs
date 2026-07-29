use serde::{Deserialize, Serialize};

use super::{RuleContentHash, RulePackId, RuleVersion};

/// One stable audit reference to a rule pack that participated in resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRulePackRef {
    pack_id: RulePackId,
    version: RuleVersion,
    content_hash: RuleContentHash,
}

impl ResolvedRulePackRef {
    pub(crate) const fn new(
        pack_id: RulePackId,
        version: RuleVersion,
        content_hash: RuleContentHash,
    ) -> Self {
        Self {
            pack_id,
            version,
            content_hash,
        }
    }

    /// Returns the participating pack ID.
    pub const fn pack_id(&self) -> &RulePackId {
        &self.pack_id
    }

    /// Returns the exact participating pack version.
    pub const fn version(&self) -> RuleVersion {
        self.version
    }

    /// Returns the exact participating semantic content hash.
    pub const fn content_hash(&self) -> RuleContentHash {
        self.content_hash
    }
}
