//! Canonical byte feeds shared by world fingerprints.

/// Feeds `values` to `hasher` as the concatenation of each value's little-endian IEEE-754 bits.
///
/// The digest is identical to calling `hasher.update(&value.to_bits().to_le_bytes())` once per
/// value: BLAKE3 hashes the byte stream however it is split into updates. Handing it whole
/// slices lets it hash many chunks at once with SIMD instead of paying one call per four bytes.
///
/// # Arguments
///
/// * `hasher` - The fingerprint being accumulated.
/// * `values` - The values to append, in order.
pub(crate) fn update_f32_le(hasher: &mut blake3::Hasher, values: &[f32]) {
    // On little-endian targets an `f32` slice already is its little-endian byte sequence, so
    // `bytemuck` reinterprets it without copying.
    #[cfg(target_endian = "little")]
    hasher.update(bytemuck::cast_slice(values));
    #[cfg(target_endian = "big")]
    for value in values {
        hasher.update(&value.to_bits().to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_feed_matches_one_update_per_value() {
        let values = [0.0_f32, -0.0, 1.5, f32::MIN_POSITIVE, -3.25e7, f32::MAX];
        let mut per_value = blake3::Hasher::new();
        for value in values {
            per_value.update(&value.to_bits().to_le_bytes());
        }
        let mut bulk = blake3::Hasher::new();
        update_f32_le(&mut bulk, &values);

        assert_eq!(bulk.finalize(), per_value.finalize());
    }
}
