//! Exact upstream-oracle fixtures catch JS overflow, UTF-16, simplex ordering and f32 stores.

use crate::noise::{fbm3, ridged3, tileable_fbm, Noise3};
use crate::random::{keyed, round24, sub_seed, Mulberry32};

include!("../reference/primitives.rs");

#[test]
fn random_stream_matches_upstream_wrapping_arithmetic() {
    for &(seed, expected) in STREAMS {
        let mut rng = Mulberry32::new(seed);
        for &bits in expected {
            assert_eq!(rng.next_f64().to_bits(), bits, "seed {seed}");
        }
    }
}

#[test]
fn substreams_match_upstream_utf16_code_units() {
    for &(seed, salt, expected) in SUBSEEDS {
        assert_eq!(sub_seed(seed, salt), expected, "{salt}");
    }
}

#[test]
fn keyed_random_matches_upstream_signed_keys() {
    for &(seed, [a, b, c], bits) in KEYED {
        assert_eq!(keyed(seed, a, b, c).to_bits(), bits);
    }
}

#[test]
fn rounding_matches_upstream_splitter_including_extreme_values() {
    for &(input, output) in ROUNDED {
        assert_eq!(round24(f64::from_bits(input)).to_bits(), output);
    }
    assert!(round24(f64::NAN).is_nan());
}

#[test]
fn simplex_and_fractals_match_pinned_upstream() {
    for &(seed, point, expected) in NOISE {
        let noise = Noise3::new(seed);
        assert_eq!(
            noise.sample(point[0], point[1], point[2]).to_bits(),
            expected[0]
        );
        assert_eq!(fbm3(&noise, point, 4, 0.6, 2.0).to_bits(), expected[1]);
        assert_eq!(ridged3(&noise, point, 5, 0.5).to_bits(), expected[2]);
    }
}

#[test]
fn tileable_raster_noise_matches_upstream_float32_stores() {
    let actual = tileable_fbm(42, 8, 2, 2, 0.5);
    assert_eq!(actual.len(), TILE.len());
    assert!(actual.iter().all(|value| value.is_finite()));
    assert!(actual.iter().any(|&value| value != 0.0));
    // The independent oracle canary isolates libm variation from translation mistakes.
    // This pure crate cannot depend on the consuming Sekai crate's platform canary.
    if !TRIG_CANARY.iter().all(|&(angle, cos, sin)| {
        let angle = std::hint::black_box(f64::from_bits(angle));
        (angle.cos() as f32).to_bits() == cos && (angle.sin() as f32).to_bits() == sin
    }) {
        eprintln!("Skipping exact tile fixture: host trigonometry differs from the Node oracle");
        return;
    }
    for (actual, &bits) in actual.iter().zip(TILE) {
        assert_eq!(actual.to_bits(), bits);
    }
}
