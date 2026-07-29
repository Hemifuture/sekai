use serde::{Deserialize, Serialize};

macro_rules! define_id {
    ($name:ident, $raw:ty) => {
        #[doc = concat!("A typed identifier for ", stringify!($name), ".")]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[repr(transparent)]
        pub struct $name($raw);

        impl $name {
            /// Creates an identifier from its raw value.
            pub const fn from_raw(raw: $raw) -> Self {
                Self(raw)
            }

            /// Returns the identifier's raw value.
            pub const fn raw(self) -> $raw {
                self.0
            }
        }
    };
}

define_id!(CellId, u32);
define_id!(EdgeId, u32);
define_id!(PlateId, u32);
define_id!(BoundarySegmentId, u32);
define_id!(SpeciesId, u32);
define_id!(CultureId, u32);
define_id!(SettlementId, u32);
define_id!(PolityId, u32);
define_id!(AuthorObjectId, u64);

/// The deterministic seed from which a world is generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct RootSeed(u64);

impl RootSeed {
    /// Creates a root seed from its raw value.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the seed's raw value.
    pub const fn raw(self) -> u64 {
        self.0
    }
}
