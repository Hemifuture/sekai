use std::fmt;
use std::marker::PhantomData;

use serde::de::{DeserializeSeed, Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

/// Deserializes a sequence without ever retaining more than `MAX` elements.
pub(crate) fn deserialize_bounded_vec<'de, D, T, const MAX: usize>(
    deserializer: D,
) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    BoundedVecSeed::new(MAX).deserialize(deserializer)
}

/// A vector that consumes a caller's remaining aggregate allocation budget.
pub(crate) struct BoundedVecSeed<T> {
    maximum: usize,
    marker: PhantomData<T>,
}
impl<T> BoundedVecSeed<T> {
    pub(crate) fn new(maximum: usize) -> Self {
        Self {
            maximum,
            marker: PhantomData,
        }
    }
}
impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for BoundedVecSeed<T> {
    type Value = Vec<T>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, T> Visitor<'de> for BoundedVecSeed<T>
where
    T: Deserialize<'de>,
{
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "a sequence with at most {} elements",
            self.maximum
        )
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        if let Some(length) = sequence.size_hint() {
            if length > self.maximum {
                return Err(A::Error::invalid_length(length, &self));
            }
        }
        let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(self.maximum));
        while values.len() < self.maximum {
            let Some(value) = sequence.next_element()? else {
                return Ok(values);
            };
            values.push(value);
        }
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(A::Error::invalid_length(self.maximum + 1, &self));
        }
        Ok(values)
    }
}
