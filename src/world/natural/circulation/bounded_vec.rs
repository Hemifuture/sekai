use std::{fmt, marker::PhantomData};

use serde::{
    de::{IgnoredAny, SeqAccess, Visitor},
    Deserialize, Deserializer,
};

/// A wire-only vector that rejects oversized sequences while they are streamed.
pub(super) struct BoundedVec<T, const MAX_ITEMS: usize, const ITEMS_PER_ELEMENT: usize>(Vec<T>);

impl<T, const MAX_ITEMS: usize, const ITEMS_PER_ELEMENT: usize>
    BoundedVec<T, MAX_ITEMS, ITEMS_PER_ELEMENT>
{
    pub(super) fn into_vec(self) -> Vec<T> {
        self.0
    }
}

impl<'de, T, const MAX_ITEMS: usize, const ITEMS_PER_ELEMENT: usize> Deserialize<'de>
    for BoundedVec<T, MAX_ITEMS, ITEMS_PER_ELEMENT>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(BoundedVecVisitor::<T, MAX_ITEMS, ITEMS_PER_ELEMENT>(
            PhantomData,
        ))
    }
}

struct BoundedVecVisitor<T, const MAX_ITEMS: usize, const ITEMS_PER_ELEMENT: usize>(PhantomData<T>);

impl<'de, T, const MAX_ITEMS: usize, const ITEMS_PER_ELEMENT: usize> Visitor<'de>
    for BoundedVecVisitor<T, MAX_ITEMS, ITEMS_PER_ELEMENT>
where
    T: Deserialize<'de>,
{
    type Value = BoundedVec<T, MAX_ITEMS, ITEMS_PER_ELEMENT>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "a sequence containing at most {MAX_ITEMS} logical items"
        )
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        assert!(
            ITEMS_PER_ELEMENT > 0,
            "bounded sequence item weight must be nonzero"
        );
        assert_eq!(
            MAX_ITEMS % ITEMS_PER_ELEMENT,
            0,
            "bounded sequence limit must be divisible by its item weight"
        );
        let max_elements = MAX_ITEMS / ITEMS_PER_ELEMENT;
        let initial_capacity = sequence.size_hint().unwrap_or(0).min(max_elements);
        let mut values = Vec::with_capacity(initial_capacity);

        loop {
            if values.len() == max_elements {
                if sequence.next_element::<IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::custom(format_args!(
                        "bounded sequence exceeds maximum item count {MAX_ITEMS}"
                    )));
                }
                break;
            }

            let Some(value) = sequence.next_element()? else {
                break;
            };
            if values.len() == values.capacity() {
                let remaining = max_elements - values.len();
                let additional = values.len().max(1).min(remaining);
                values.reserve_exact(additional);
            }
            values.push(value);
        }

        Ok(BoundedVec(values))
    }
}
