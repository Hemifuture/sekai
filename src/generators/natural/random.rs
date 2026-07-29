#![cfg_attr(not(test), allow(dead_code))]

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::engine::StageRng;

pub(super) const PLATE_SEEDS_LABEL: &str = "plate-seeds-v1";
pub(super) const PLATE_MOTION_LABEL: &str = "plate-motion-v1";
pub(super) const CRUST_SEEDS_LABEL: &str = "crust-seeds-v1";
pub(super) const CRUST_SHAPE_LABEL: &str = "crust-shape-v1";
pub(super) const CRUST_THICKNESS_LABEL: &str = "crust-thickness-v1";

pub(super) struct LabeledSubstreams {
    root: [u8; 32],
}

impl LabeledSubstreams {
    pub(super) fn capture(stage_rng: &mut StageRng) -> Self {
        let mut root = [0_u8; 32];
        stage_rng.fill_bytes(&mut root);
        Self { root }
    }

    pub(super) fn stream(&self, label: &'static str) -> ChaCha8Rng {
        debug_assert!(
            !label.is_empty()
                && label.is_ascii()
                && label.bytes().all(|byte| byte.is_ascii_graphic()),
            "natural RNG labels must be non-empty printable ASCII constants"
        );
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai-natural-substream-v1\0");
        hasher.update(&self.root);
        hasher.update(&(label.len() as u64).to_le_bytes());
        hasher.update(label.as_bytes());
        ChaCha8Rng::from_seed(*hasher.finalize().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use rand::RngCore;

    use super::{LabeledSubstreams, CRUST_SEEDS_LABEL, PLATE_SEEDS_LABEL};
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::world::RootSeed;

    fn stage_rng() -> StageRng {
        StageRng::from_seed(derive_stage_seed(
            RootSeed::new(71),
            StageIdentity::new("natural-test", 1, "sekai.test"),
        ))
    }

    #[test]
    fn labeled_substreams_repeat_exactly() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let mut first = streams.stream(PLATE_SEEDS_LABEL);
        let mut second = streams.stream(PLATE_SEEDS_LABEL);

        assert_eq!(
            (0..8).map(|_| first.next_u64()).collect::<Vec<_>>(),
            (0..8).map(|_| second.next_u64()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn consuming_one_label_does_not_perturb_another() {
        let streams = LabeledSubstreams::capture(&mut stage_rng());
        let mut plate = streams.stream(PLATE_SEEDS_LABEL);
        let mut crust_after_plate = streams.stream(CRUST_SEEDS_LABEL);
        for _ in 0..100 {
            plate.next_u64();
        }

        let pristine = LabeledSubstreams::capture(&mut stage_rng());
        let mut pristine_crust = pristine.stream(CRUST_SEEDS_LABEL);
        assert_eq!(
            (0..8)
                .map(|_| crust_after_plate.next_u64())
                .collect::<Vec<_>>(),
            (0..8)
                .map(|_| pristine_crust.next_u64())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn capture_consumes_exactly_one_32_byte_seed() {
        let mut actual = stage_rng();
        LabeledSubstreams::capture(&mut actual);

        let mut expected = stage_rng();
        let mut seed = [0_u8; 32];
        expected.fill_bytes(&mut seed);

        assert_eq!(actual.next_u64(), expected.next_u64());
    }
}
