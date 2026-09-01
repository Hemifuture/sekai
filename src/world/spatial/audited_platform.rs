//! Detection of the audited floating-point platform.
//!
//! The exact frozen identities in this repository — surface fingerprints,
//! matrix hashes, whole-graph goldens, endstate-contract corpora — are
//! measured on one audited platform. Transcendental functions round
//! differently across math libraries and instruction dispatch (Windows ucrt,
//! glibc, CPU feature levels), so the same seeds legitimately build slightly
//! different worlds elsewhere. Exact-identity assertions therefore key on
//! this canary, the same policy that keys exact GPU goldens to the audited
//! adapter; on other platforms the science and consistency checks still run
//! and the exact comparisons are skipped with a notice.

/// Canary of the audited platform's transcendental rounding, measured on the
/// audited machine (x86_64 Windows, ucrt): a Blake3 hash over the exact bit
/// patterns of a battery of libm evaluations.
pub const AUDITED_FLOAT_PLATFORM_CANARY: [u8; 32] = [
    0x80, 0xda, 0x27, 0x81, 0x66, 0x52, 0x76, 0xcf, 0xaa, 0xbb, 0x11, 0xc3, 0xeb, 0x10, 0x68, 0x8d,
    0x2e, 0x90, 0x7a, 0xb1, 0xbc, 0x74, 0x00, 0x03, 0x29, 0x12, 0x0f, 0x7c, 0x6d, 0x2d, 0x9b, 0x1e,
];

/// Hashes the bit patterns of transcendental evaluations that the surface
/// and stage builders lean on (sin/cos/atan2/asin/exp/ln/powf/cbrt/tan) over
/// arguments spanning small angles to large magnitudes.
pub fn float_platform_canary() -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    let mut feed = |value: f64| {
        hasher.update(&value.to_bits().to_le_bytes());
    };
    for index in 0..512 {
        let x = (f64::from(index) - 255.5) * 0.017_453_292_519_943_295;
        let y = f64::from(index).mul_add(0.618_033_988_749_894_8, 1.0e-6);
        feed(x.sin());
        feed(x.cos());
        feed(x.tan());
        feed((x * 0.1).asin());
        feed((x * 0.1).acos());
        feed(y.atan2(1.0 + x.abs()));
        feed(y.ln());
        feed(x.exp());
        feed(y.sqrt());
        feed(y.cbrt());
        feed(y.powf(1.5 + x * 0.01));
    }
    *hasher.finalize().as_bytes()
}

/// True on the platform whose libm rounding matches the audited canary.
pub fn audited_float_platform() -> bool {
    float_platform_canary() == AUDITED_FLOAT_PLATFORM_CANARY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_platform_canary_prints_and_is_stable() {
        let first = float_platform_canary();
        assert_eq!(first, float_platform_canary());
        println!(
            "float_platform_canary={}",
            blake3::Hash::from(first).to_hex()
        );
    }
}
